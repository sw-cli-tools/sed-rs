use regex::Regex;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::iter::Peekable;

/// A parsed sed substitution command.
#[derive(Debug, Clone)]
pub struct SubstCmd {
    pattern: Regex,
    replacement: String,
    global: bool,
}

impl SubstCmd {
    /// The substituted text and the number of replacements, or `None` when
    /// nothing matched. Same result as `Regex::replace` / `replace_all`.
    fn apply(&self, text: &str) -> Option<(String, usize)> {
        let mut out = String::new();
        let mut last = 0;
        let mut count = 0;
        for caps in self.pattern.captures_iter(text) {
            let m = caps.get(0).expect("group 0 always matches");
            out.push_str(&text[last..m.start()]);
            caps.expand(&self.replacement, &mut out);
            last = m.end();
            count += 1;
            if !self.global {
                break;
            }
        }
        if count == 0 {
            return None;
        }
        out.push_str(&text[last..]);
        Some((out, count))
    }
}

/// Options for [`Sed::parse_with`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Options {
    /// Treat the pattern and replacement of every `s` command as literal
    /// text, as if each had the `L` flag (the CLI's `--fixed-strings`).
    /// Address regexes stay regexes.
    pub fixed_strings: bool,
}

impl Options {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fixed_strings(mut self, on: bool) -> Self {
        self.fixed_strings = on;
        self
    }
}

/// What a run did, from [`Sed::process_with_stats`] and
/// [`Sed::process_preserving`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stats {
    /// One entry per `s` command, in script order.
    pub substitutions: Vec<SubstStats>,
}

/// The replacements one `s` command made.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SubstStats {
    /// Which `-e` expression the command is in, counting from 1.
    pub expression: usize,
    /// The command as written, with its address: `/x/s/a/b/g`.
    pub source: String,
    /// How many replacements it made.
    pub replacements: usize,
}

impl Stats {
    /// Replacements made by all `s` commands together.
    pub fn total_replacements(&self) -> usize {
        self.substitutions.iter().map(|s| s.replacements).sum()
    }

    /// The `s` commands that made no replacement.
    pub fn unmatched(&self) -> impl Iterator<Item = &SubstStats> {
        self.substitutions.iter().filter(|s| s.replacements == 0)
    }

    /// Add the counts of another run of the same script (e.g. over the next
    /// file).
    pub fn accumulate(&mut self, other: &Stats) {
        if self.substitutions.is_empty() {
            self.substitutions = other.substitutions.clone();
            return;
        }
        for (mine, theirs) in self.substitutions.iter_mut().zip(&other.substitutions) {
            mine.replacements += theirs.replacements;
        }
    }
}

/// A single address.
#[derive(Debug, Clone)]
enum Addr {
    /// `0`, valid only as the start of `0,/re/`.
    Zero,
    Line(usize),
    Last,
    Re(Regex),
}

impl Addr {
    fn matches(&self, space: &str, line_no: usize, is_last: bool) -> bool {
        match self {
            Addr::Zero => false,
            Addr::Line(n) => line_no == *n,
            Addr::Last => is_last,
            Addr::Re(re) => re.is_match(space),
        }
    }
}

/// Which input a command applies to.
#[derive(Debug, Clone)]
enum Address {
    One(Addr),
    /// `start,end`; `slot` indexes the run's range state (active or not).
    Range { start: Addr, end: Addr, slot: usize },
}

#[derive(Debug, Clone)]
enum Op {
    /// `s`, with its index into the script's substitution list.
    Subst(SubstCmd, usize),
    /// `d`: delete the pattern space and start the next cycle.
    Delete,
    /// `N`: append a newline and the next input line to the pattern space.
    AppendNext,
    /// `b`: jump to an instruction index (a label or the end of the script).
    Branch(usize),
    /// `{`: when the address does not match, skip to `end`, the index after `}`.
    Block {
        end: usize,
    },
    /// `:label` and `}` compile to no-ops so jumps have somewhere to land.
    Nop,
}

#[derive(Debug, Clone)]
struct Cmd {
    addr: Option<Address>,
    negate: bool,
    op: Op,
}

#[derive(Debug, Clone)]
struct SubstMeta {
    expression: usize,
    source: String,
}

/// A parsed sed script: substitutions plus the addresses, ranges, blocks,
/// labels, branches, `N` and `d` needed for multi-line idioms.
#[derive(Debug, Clone)]
pub struct Sed {
    commands: Vec<Cmd>,
    substs: Vec<SubstMeta>,
    /// Initial state of each range: `0,/re/` starts active.
    ranges: Vec<bool>,
}

#[derive(Debug)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

/// How an input line ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Eol {
    None,
    Lf,
    CrLf,
}

impl Eol {
    fn bytes(self) -> &'static [u8] {
        match self {
            Eol::None => b"",
            Eol::Lf => b"\n",
            Eol::CrLf => b"\r\n",
        }
    }
}

struct Line {
    text: String,
    eol: Eol,
}

/// Lines as `BufRead::lines` gives them (the stream mode since v0.1):
/// `\n` and `\r\n` are both dropped and every line is printed with `\n`.
fn stream_lines<R: BufRead>(reader: R) -> impl Iterator<Item = io::Result<Line>> {
    reader.lines().map(|l| l.map(|text| Line { text, eol: Eol::Lf }))
}

/// Lines with their terminators kept, so they can be written back exactly.
struct ExactLines<R> {
    reader: R,
}

impl<R: BufRead> Iterator for ExactLines<R> {
    type Item = io::Result<Line>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut buf = Vec::new();
        match self.reader.read_until(b'\n', &mut buf) {
            Ok(0) => None,
            Ok(_) => {
                let eol = if buf.ends_with(b"\r\n") {
                    buf.truncate(buf.len() - 2);
                    Eol::CrLf
                } else if buf.ends_with(b"\n") {
                    buf.pop();
                    Eol::Lf
                } else {
                    Eol::None
                };
                Some(
                    String::from_utf8(buf)
                        .map(|text| Line { text, eol })
                        .map_err(|_| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                "stream did not contain valid UTF-8",
                            )
                        }),
                )
            }
            Err(e) => Some(Err(e)),
        }
    }
}

impl Sed {
    /// Parse one or more sed expressions. As in sed, the expressions are
    /// joined with newlines into one script, so a block or label may span
    /// several `-e` arguments. Commands are separated by `;` or newlines.
    pub fn parse(expressions: &[&str]) -> Result<Self, ParseError> {
        Self::parse_with(expressions, &Options::default())
    }

    /// [`Sed::parse`] with options.
    pub fn parse_with(expressions: &[&str], options: &Options) -> Result<Self, ParseError> {
        let script = expressions.join("\n");
        let mut starts = Vec::with_capacity(expressions.len());
        let mut at = 0;
        for e in expressions {
            starts.push(at);
            at += e.len() + 1;
        }
        let expression_of =
            |offset: usize| starts.iter().rposition(|s| *s <= offset).map_or(1, |i| i + 1);

        let mut chars = script.chars().peekable();
        let mut commands: Vec<Cmd> = Vec::new();
        let mut substs: Vec<SubstMeta> = Vec::new();
        let mut ranges: Vec<bool> = Vec::new();
        let mut labels: HashMap<String, usize> = HashMap::new();
        let mut jumps: Vec<(usize, String)> = Vec::new(); // (command index, label)
        let mut blocks: Vec<usize> = Vec::new(); // indexes of open `{`

        loop {
            while chars.peek().is_some_and(|c| c.is_whitespace() || *c == ';') {
                chars.next();
            }
            let Some(&first) = chars.peek() else { break };
            if first == '#' {
                while chars.next().is_some_and(|c| c != '\n') {}
                continue;
            }

            let start = offset(&script, &chars);
            let addr = parse_address(&mut chars, &mut ranges)?;
            skip_blanks(&mut chars);
            let negate = chars.next_if_eq(&'!').is_some();
            skip_blanks(&mut chars);

            let c = chars
                .next()
                .ok_or_else(|| ParseError("missing command".into()))?;
            let op = match c {
                's' => {
                    let cmd = parse_subst(&mut chars, options.fixed_strings)?;
                    let end = offset(&script, &chars);
                    substs.push(SubstMeta {
                        expression: expression_of(start),
                        source: script[start..end].trim().to_string(),
                    });
                    Op::Subst(cmd, substs.len() - 1)
                }
                'd' => Op::Delete,
                'N' => Op::AppendNext,
                '{' => {
                    blocks.push(commands.len());
                    Op::Block { end: 0 }
                }
                '}' => {
                    if addr.is_some() || negate {
                        return Err(ParseError("`}` doesn't want any addresses".into()));
                    }
                    let open = blocks
                        .pop()
                        .ok_or_else(|| ParseError("unexpected `}`".into()))?;
                    commands[open].op = Op::Block {
                        end: commands.len() + 1,
                    };
                    Op::Nop
                }
                ':' => {
                    if addr.is_some() || negate {
                        return Err(ParseError(": doesn't want any addresses".into()));
                    }
                    let name = read_label(&mut chars);
                    if name.is_empty() {
                        return Err(ParseError("\":\" lacks a label".into()));
                    }
                    labels.insert(name, commands.len());
                    Op::Nop
                }
                'b' => {
                    jumps.push((commands.len(), read_label(&mut chars)));
                    Op::Branch(0)
                }
                _ => {
                    let rest: String = std::iter::once(c).chain(chars.by_ref()).collect();
                    let cmd = rest.lines().next().unwrap_or_default().trim();
                    return Err(ParseError(format!("unsupported command: {cmd}")));
                }
            };
            commands.push(Cmd { addr, negate, op });
        }

        if !blocks.is_empty() {
            return Err(ParseError("unmatched `{`".into()));
        }
        let end = commands.len();
        for (at, name) in jumps {
            let target = if name.is_empty() {
                end
            } else {
                *labels
                    .get(&name)
                    .ok_or_else(|| ParseError(format!("can't find label for jump to `{name}'")))?
            };
            commands[at].op = Op::Branch(target);
        }
        if commands.is_empty() {
            return Err(ParseError("no commands provided".into()));
        }
        Ok(Sed {
            commands,
            substs,
            ranges,
        })
    }

    /// Run the script on a single line and return what it prints, without
    /// the trailing newline (an empty string if the line is deleted).
    pub fn apply(&self, line: &str) -> String {
        let mut out = Vec::new();
        let input = std::iter::once(Ok(Line {
            text: line.to_string(),
            eol: Eol::Lf,
        }));
        self.run(input, &mut out, false)
            .expect("writing to a Vec cannot fail");
        let mut text = String::from_utf8(out).expect("input and script are UTF-8");
        text.pop();
        text
    }

    /// Process a reader line-by-line and write results to a writer.
    ///
    /// Each line is printed with `\n`: a `\r\n` ending becomes `\n`, and a
    /// last line without a newline gets one.
    pub fn process<R: BufRead, W: Write>(&self, reader: R, writer: W) -> io::Result<()> {
        self.run(stream_lines(reader), writer, false).map(|_| ())
    }

    /// [`Sed::process`], also returning how many replacements each `s`
    /// command made.
    pub fn process_with_stats<R: BufRead, W: Write>(
        &self,
        reader: R,
        writer: W,
    ) -> io::Result<Stats> {
        self.run(stream_lines(reader), writer, false)
    }

    /// Like [`Sed::process_with_stats`], but keeps line endings: each line
    /// is written back with the terminator it was read with (`\n`, `\r\n`,
    /// or none on a last line without one). A newline inside the pattern
    /// space (from `N`, or `\n` in a replacement) is written as `\r\n` when
    /// the line it ends up in came from a `\r\n` line. This is what
    /// in-place editing uses: a file changes only where the script
    /// changed it.
    pub fn process_preserving<R: BufRead, W: Write>(
        &self,
        reader: R,
        writer: W,
    ) -> io::Result<Stats> {
        self.run(ExactLines { reader }, writer, true)
    }

    /// The sed cycle: read a line into the pattern space, run the script,
    /// print the pattern space (unless `d` ran), repeat.
    fn run<I, W>(&self, lines: I, mut writer: W, preserve: bool) -> io::Result<Stats>
    where
        I: Iterator<Item = io::Result<Line>>,
        W: Write,
    {
        let mut counts = vec![0usize; self.substs.len()];
        let mut ranges = self.ranges.clone();
        let mut lines = lines.peekable();
        let mut line_no = 0;
        // The line-ending style for embedded newlines (preserve mode only).
        let mut style = Eol::Lf;
        while let Some(line) = lines.next() {
            let Line { text, mut eol } = line?;
            let mut space = text;
            if eol != Eol::None {
                style = eol;
            }
            line_no += 1;
            let mut print = true;
            let mut pc = 0;
            while pc < self.commands.len() {
                let cmd = &self.commands[pc];
                if !cmd.selects(&space, line_no, lines.peek().is_none(), &mut ranges) {
                    pc = match cmd.op {
                        Op::Block { end } => end,
                        _ => pc + 1,
                    };
                    continue;
                }
                match &cmd.op {
                    Op::Subst(s, index) => {
                        if let Some((text, n)) = s.apply(&space) {
                            space = text;
                            counts[*index] += n;
                        }
                    }
                    Op::Delete => {
                        print = false;
                        break;
                    }
                    Op::AppendNext => match lines.next() {
                        Some(next) => {
                            let next = next?;
                            space.push('\n');
                            space.push_str(&next.text);
                            eol = next.eol;
                            if eol != Eol::None {
                                style = eol;
                            }
                            line_no += 1;
                        }
                        // No next line: GNU sed prints the pattern space and exits.
                        None => break,
                    },
                    Op::Branch(target) => {
                        pc = *target;
                        continue;
                    }
                    Op::Block { .. } | Op::Nop => {}
                }
                pc += 1;
            }
            if print {
                if preserve && style == Eol::CrLf {
                    writer.write_all(space.replace('\n', "\r\n").as_bytes())?;
                } else {
                    writer.write_all(space.as_bytes())?;
                }
                writer.write_all(eol.bytes())?;
            }
        }
        Ok(Stats {
            substitutions: self
                .substs
                .iter()
                .zip(counts)
                .map(|(meta, replacements)| SubstStats {
                    expression: meta.expression,
                    source: meta.source.clone(),
                    replacements,
                })
                .collect(),
        })
    }
}

impl Cmd {
    fn selects(&self, space: &str, line_no: usize, is_last: bool, ranges: &mut [bool]) -> bool {
        let hit = match &self.addr {
            None => true,
            Some(Address::One(a)) => a.matches(space, line_no, is_last),
            Some(Address::Range { start, end, slot }) => {
                let active = &mut ranges[*slot];
                if *active {
                    // GNU: a line-number end closes the range once reached
                    // or passed, and a line past it is not selected.
                    match end {
                        Addr::Line(n) => {
                            if line_no >= *n {
                                *active = false;
                            }
                            line_no <= *n
                        }
                        _ => {
                            if end.matches(space, line_no, is_last) {
                                *active = false;
                            }
                            true
                        }
                    }
                } else if start.matches(space, line_no, is_last) {
                    // The end is first tested on the next line; a line
                    // number at or before this one selects just this line.
                    match end {
                        Addr::Line(n) if *n <= line_no => {}
                        _ => *active = true,
                    }
                    true
                } else {
                    false
                }
            }
        };
        hit != self.negate
    }
}

type Chars<'a> = Peekable<std::str::Chars<'a>>;

/// Byte offset of the next unread character of `script`.
fn offset(script: &str, chars: &Chars<'_>) -> usize {
    script.len() - chars.clone().map(char::len_utf8).sum::<usize>()
}

fn skip_blanks(chars: &mut Chars<'_>) {
    while chars.next_if(|c| *c == ' ' || *c == '\t').is_some() {}
}

/// An optional address or range: `A`, or `A,B`, where each is a line number,
/// `$` or `/regex/`, and `0,/regex/` may start at "before line 1".
fn parse_address(
    chars: &mut Chars<'_>,
    ranges: &mut Vec<bool>,
) -> Result<Option<Address>, ParseError> {
    let first = parse_one_address(chars)?;
    skip_blanks(chars);
    if chars.peek() != Some(&',') {
        return Ok(first.map(Address::One));
    }
    chars.next();
    let Some(first) = first else {
        return Err(ParseError("unexpected `,'".into()));
    };
    skip_blanks(chars);
    let end = parse_one_address(chars)?
        .ok_or_else(|| ParseError("unexpected `,'".into()))?;
    let start = match first {
        Addr::Line(0) if matches!(end, Addr::Re(_)) => Addr::Zero,
        Addr::Line(0) => return Err(ParseError("invalid usage of line address 0".into())),
        other => other,
    };
    let slot = ranges.len();
    ranges.push(matches!(start, Addr::Zero));
    skip_blanks(chars);
    Ok(Some(Address::Range { start, end, slot }))
}

/// A line number, `$` (last line) or `/regex/`. A bare `0` is kept as a
/// line number that never matches, as in v0.2.
fn parse_one_address(chars: &mut Chars<'_>) -> Result<Option<Addr>, ParseError> {
    Ok(match chars.peek() {
        Some(c) if c.is_ascii_digit() => {
            let mut n = String::new();
            while let Some(d) = chars.next_if(|c| c.is_ascii_digit()) {
                n.push(d);
            }
            let n = n
                .parse()
                .map_err(|_| ParseError(format!("invalid line number: {n}")))?;
            Some(Addr::Line(n))
        }
        Some('$') => {
            chars.next();
            Some(Addr::Last)
        }
        Some('/') => {
            chars.next();
            let (pattern, closed) = read_delimited(chars, '/');
            if !closed {
                return Err(ParseError("unterminated address regex".into()));
            }
            let re = Regex::new(&bre_to_ere(&pattern))
                .map_err(|e| ParseError(format!("invalid regex: {e}")))?;
            Some(Addr::Re(re))
        }
        _ => None,
    })
}

/// A label for `:` or `b`: up to a newline, `;` or `}` (as GNU sed), trimmed.
fn read_label(chars: &mut Chars<'_>) -> String {
    let mut name = String::new();
    while let Some(c) = chars.next_if(|c| !matches!(c, '\n' | ';' | '}')) {
        name.push(c);
    }
    name.trim().to_string()
}

/// Read up to an unescaped `delim`, stopping before a newline (the end of an
/// `-e` expression). An escaped delimiter becomes the bare character; other
/// escapes are kept. Returns the text and whether `delim` was found.
fn read_delimited(chars: &mut Chars<'_>, delim: char) -> (String, bool) {
    let mut text = String::new();
    while let Some(ch) = chars.next_if(|c| *c != '\n') {
        if ch == '\\' {
            match chars.next() {
                Some(next) if next == delim => text.push(delim),
                Some(next) => {
                    text.push('\\');
                    text.push(next);
                }
                None => text.push('\\'),
            }
        } else if ch == delim {
            return (text, true);
        } else {
            text.push(ch);
        }
    }
    (text, false)
}

/// Parse the rest of a `s/pattern/replacement/flags` command (after the `s`).
/// `fixed` makes the command literal, as if it had the `L` flag.
fn parse_subst(chars: &mut Chars<'_>, fixed: bool) -> Result<SubstCmd, ParseError> {
    let delim = chars
        .next()
        .ok_or_else(|| ParseError("unexpected end of substitution".into()))?;
    if delim == '\n' || delim == '\\' {
        return Err(ParseError(format!("invalid delimiter for s: {delim:?}")));
    }

    let (pattern_str, closed) = read_delimited(chars, delim);
    if !closed {
        return Err(ParseError("incomplete substitution command".into()));
    }
    // Tolerate a missing trailing delimiter, as before.
    let (replacement, _) = read_delimited(chars, delim);
    let mut flags = String::new();
    while let Some(f) = chars.next_if(|c| c.is_alphabetic()) {
        if !matches!(f, 'g' | 'i' | 'I' | 'L') {
            return Err(ParseError(format!("unknown option to `s': {f}")));
        }
        flags.push(f);
    }

    let global = flags.contains('g');
    let case_insensitive = flags.contains('i') || flags.contains('I');
    let literal = fixed || flags.contains('L');

    let pattern_str = if literal {
        regex::escape(&unescape_literal(&pattern_str))
    } else {
        // Convert common BRE escapes to ERE equivalents
        bre_to_ere(&pattern_str)
    };

    let regex_pattern = if case_insensitive {
        format!("(?i){pattern_str}")
    } else {
        pattern_str
    };

    let pattern =
        Regex::new(&regex_pattern).map_err(|e| ParseError(format!("invalid regex: {e}")))?;

    let replacement = if literal {
        unescape_literal(&replacement).replace('$', "$$")
    } else {
        // Convert sed replacement syntax to regex crate syntax:
        // sed uses \1..\9 for captures; regex crate uses $1..$9
        // sed uses & for whole match; regex crate uses $0
        convert_replacement(&replacement)
    };

    Ok(SubstCmd {
        pattern,
        replacement,
        global,
    })
}

/// The text of a literal (`L` / `--fixed-strings`) pattern or replacement:
/// only `\n` (newline), `\t` (tab) and `\\` (backslash) are escapes; the
/// escaped delimiter was already resolved by `read_delimited`. Any other
/// character, including `\`, `&`, `$` and regex syntax, means itself.
fn unescape_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('n') => {
                    chars.next();
                    out.push('\n');
                }
                Some('t') => {
                    chars.next();
                    out.push('\t');
                }
                Some('\\') => {
                    chars.next();
                    out.push('\\');
                }
                _ => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Convert common BRE escapes to ERE: \{n\} → {n}, \( \) → ( ), \+ → +
fn bre_to_ere(pattern: &str) -> String {
    let mut result = String::with_capacity(pattern.len());
    let mut chars = pattern.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some(&'{') => {
                    chars.next();
                    // Collect until \}
                    let mut inner = String::new();
                    while let Some(&ch) = chars.peek() {
                        if ch == '\\' {
                            chars.next();
                            if chars.peek() == Some(&'}') {
                                chars.next();
                                break;
                            } else {
                                inner.push('\\');
                            }
                        } else {
                            inner.push(ch);
                            chars.next();
                        }
                    }
                    result.push('{');
                    result.push_str(&inner);
                    result.push('}');
                }
                Some(&'(') => {
                    chars.next();
                    result.push('(');
                }
                Some(&')') => {
                    chars.next();
                    result.push(')');
                }
                Some(&'+') => {
                    chars.next();
                    result.push('+');
                }
                Some(&'?') => {
                    chars.next();
                    result.push('?');
                }
                Some(&'|') => {
                    chars.next();
                    result.push('|');
                }
                _ => {
                    result.push('\\');
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert sed replacement syntax to regex crate syntax.
/// `\1`..`\9` → `$1`..`$9`, `&` → `$0`, `\&` → literal `&`
fn convert_replacement(repl: &str) -> String {
    let mut result = String::with_capacity(repl.len());
    let mut chars = repl.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some(&d) if d.is_ascii_digit() => {
                    chars.next();
                    result.push('$');
                    result.push(d);
                }
                Some(&'&') => {
                    chars.next();
                    result.push('&'); // literal &
                }
                Some(&'\\') => {
                    chars.next();
                    result.push('\\');
                }
                Some(&'n') => {
                    chars.next();
                    result.push('\n');
                }
                Some(&'t') => {
                    chars.next();
                    result.push('\t');
                }
                Some(&'$') => {
                    chars.next();
                    result.push_str("$$"); // literal $ in regex replacement
                }
                _ => {
                    result.push('\\');
                }
            }
        } else if c == '&' {
            result.push_str("$0");
        } else if c == '$' {
            // Escape literal $ so regex crate doesn't interpret it
            result.push_str("$$");
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(expr: &str, input: &str) -> String {
        let sed = Sed::parse(&[expr]).unwrap();
        sed.apply(input)
    }

    #[test]
    fn basic_substitution() {
        assert_eq!(apply("s/foo/bar/", "foo baz"), "bar baz");
    }

    #[test]
    fn global_flag() {
        assert_eq!(apply("s/o/0/g", "foo boo"), "f00 b00");
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(apply("s/foo/bar/gi", "FOO Foo foo"), "bar bar bar");
    }

    #[test]
    fn alternate_delimiter() {
        assert_eq!(apply("s|/tmp/test|<TMP>|g", "/tmp/test/file"), "<TMP>/file");
    }

    #[test]
    fn bre_repetition() {
        assert_eq!(
            apply(
                r"s/[0-9]\{4\}-[0-9]\{2\}-[0-9]\{2\}/DATE/g",
                "today is 2026-03-25"
            ),
            "today is DATE"
        );
    }

    #[test]
    fn pid_normalization() {
        assert_eq!(
            apply(
                "s|/tmp/favicon-reg-[0-9]*|/tmp/favicon-reg-PID|g",
                "/tmp/favicon-reg-12345"
            ),
            "/tmp/favicon-reg-PID"
        );
    }

    #[test]
    fn multiple_commands_semicolon() {
        let sed = Sed::parse(&["s/foo/bar/g; s/baz/qux/g"]).unwrap();
        assert_eq!(sed.apply("foo baz"), "bar qux");
    }

    #[test]
    fn multiple_expressions() {
        let sed = Sed::parse(&["s/foo/bar/g", "s/baz/qux/g"]).unwrap();
        assert_eq!(sed.apply("foo baz"), "bar qux");
    }

    #[test]
    fn backreference_in_replacement() {
        assert_eq!(
            apply(r"s/\(hello\) \(world\)/\2 \1/", "hello world"),
            "world hello"
        );
    }

    #[test]
    fn ampersand_whole_match() {
        assert_eq!(apply("s/foo/[&]/", "foo bar"), "[foo] bar");
    }

    #[test]
    fn escaped_delimiter() {
        assert_eq!(apply(r"s/a\/b/c/", "a/b"), "c");
    }

    #[test]
    fn process_multiline() {
        let sed = Sed::parse(&["s/x/y/g"]).unwrap();
        let input = b"ax\nbx\n";
        let mut output = Vec::new();
        sed.process(&input[..], &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "ay\nby\n");
    }

    #[test]
    fn path_normalization() {
        let sed = Sed::parse(&[
            r"s|/private/var/[^ ]*|<TMPDIR>|g; s|/var/[^ ]*|<TMPDIR>|g; s|/tmp/[^ ]*|<TMPDIR>|g",
        ])
        .unwrap();
        assert_eq!(
            sed.apply("path: /private/var/folders/xx/123 end"),
            "path: <TMPDIR> end"
        );
        assert_eq!(sed.apply("path: /tmp/foo end"), "path: <TMPDIR> end");
    }

    #[test]
    fn literal_dollar_in_replacement() {
        assert_eq!(apply("s/price/\\$5/", "the price is"), "the $5 is");
    }

    fn run(exprs: &[&str], input: &str) -> String {
        let sed = Sed::parse(exprs).unwrap();
        let mut output = Vec::new();
        sed.process(input.as_bytes(), &mut output).unwrap();
        String::from_utf8(output).unwrap()
    }

    fn parse_err(exprs: &[&str]) -> String {
        Sed::parse(exprs).unwrap_err().0
    }

    #[test]
    fn semicolon_inside_pattern_is_not_a_separator() {
        assert_eq!(apply("s/a;b/c/", "xa;by"), "xcy");
    }

    #[test]
    fn missing_trailing_delimiter_stops_at_expression_end() {
        assert_eq!(apply("s/a/b", "aa"), "ba");
        assert_eq!(run(&["s/a/b", "s/b/c/g"], "ab\n"), "cc\n");
    }

    #[test]
    fn delete_last_line() {
        assert_eq!(run(&["$d"], "a\nb\nc\n"), "a\nb\n");
    }

    #[test]
    fn line_number_and_regex_addresses() {
        assert_eq!(run(&["2d"], "a\nb\nc\n"), "a\nc\n");
        assert_eq!(run(&["/^#/d"], "#x\ny\n#z\n"), "y\n");
        assert_eq!(
            run(&["/keep/!d"], "keep 1\ndrop\nkeep 2\n"),
            "keep 1\nkeep 2\n"
        );
        assert_eq!(run(&["/b/s/./X/"], "ab\ncd\n"), "Xb\ncd\n");
    }

    #[test]
    fn append_next_joins_lines() {
        assert_eq!(run(&["N;s/\\n/+/"], "a\nb\nc\nd\n"), "a+b\nc+d\n");
        // N on the last line prints the pattern space and stops (GNU sed).
        assert_eq!(run(&["N;s/\\n/+/"], "a\nb\nc\n"), "a+b\nc\n");
    }

    #[test]
    fn block_runs_only_when_address_matches() {
        assert_eq!(run(&["/x/{s/x/y/;s/a/b/}"], "ax\naa\n"), "by\naa\n");
    }

    #[test]
    fn label_and_branch_loop() {
        // Join every line into one, a classic label/branch loop.
        assert_eq!(
            run(&[":a", "N", "$!ba", "s/\\n/,/g"], "1\n2\n3\n"),
            "1,2,3\n"
        );
        // `b` with no label jumps to the end of the script.
        assert_eq!(run(&["b;s/a/b/"], "a\n"), "a\n");
    }

    #[test]
    fn strip_trailing_blank_lines() {
        // The norm() pipeline in reva-fork's scripts/doc-check.sh.
        let exprs = [":a", "/^\\n*$/{$d;N;ba", "}"];
        assert_eq!(run(&exprs, "x\n\ny\n\n\n"), "x\n\ny\n");
        assert_eq!(run(&exprs, "\n\nx\n"), "\n\nx\n");
        assert_eq!(run(&exprs, "x\n"), "x\n");
        assert_eq!(run(&exprs, "\n\n"), "");
    }

    #[test]
    fn comments_and_whitespace_between_commands() {
        assert_eq!(run(&["# drop b\n/b/d ;  s/a/A/"], "a\nb\n"), "A\n");
    }

    #[test]
    fn sed_is_clone_send_sync() {
        // docs/library-usage.md promises this: parse once, share across threads.
        fn check<T: Clone + Send + Sync>() {}
        check::<Sed>();
    }

    #[test]
    fn apply_returns_empty_for_deleted_line() {
        assert_eq!(apply("d", "gone"), "");
    }

    #[test]
    fn parse_errors() {
        assert_eq!(parse_err(&["ba"]), "can't find label for jump to `a'");
        assert_eq!(parse_err(&["/x/{s/a/b/"]), "unmatched `{`");
        assert_eq!(parse_err(&["}"]), "unexpected `}`");
        assert_eq!(parse_err(&["0,3d"]), "invalid usage of line address 0");
        assert_eq!(parse_err(&["0d;0,$d"]), "invalid usage of line address 0");
        assert_eq!(parse_err(&["1,d"]), "unexpected `,'");
        assert_eq!(parse_err(&[",2d"]), "unexpected `,'");
        assert_eq!(parse_err(&["s/a/b/p"]), "unknown option to `s': p");
        assert_eq!(parse_err(&["s/a/b/gm"]), "unknown option to `s': m");
        assert_eq!(
            parse_err(&["y/abc/xyz/"]),
            "unsupported command: y/abc/xyz/"
        );
        assert_eq!(parse_err(&[":"]), "\":\" lacks a label");
        assert_eq!(parse_err(&["/x"]), "unterminated address regex");
    }

    #[test]
    fn line_and_last_ranges() {
        assert_eq!(run(&["2,3d"], "1\n2\n3\n4\n"), "1\n4\n");
        assert_eq!(run(&["2,$d"], "1\n2\n3\n"), "1\n");
        assert_eq!(run(&["2,3!d"], "1\n2\n3\n4\n"), "2\n3\n");
        // An end at or before the start selects only the start line.
        assert_eq!(run(&["3,1d"], "1\n2\n3\n4\n"), "1\n2\n4\n");
        assert_eq!(run(&["2 , 3 d"], "1\n2\n3\n4\n"), "1\n4\n");
    }

    #[test]
    fn regex_ranges() {
        assert_eq!(run(&["/a/,/b/d"], "x\na\ny\nb\nz\n"), "x\nz\n");
        // The end is first tested on the line after the start.
        assert_eq!(run(&["/a/,/b/d"], "ab\nc\nb\nd\n"), "d\n");
        // A range can start again after it closes.
        assert_eq!(run(&["/a/,/b/d"], "a\nb\nc\na\nb\nd\n"), "c\nd\n");
        assert_eq!(run(&["2,/^$/s/x/y/"], "x\nx\nx\n\nx\n"), "x\ny\ny\n\nx\n");
        assert_eq!(
            run(&["/a/,/b/{s/^/> /}"], "a\nx\nb\ny\n"),
            "> a\n> x\n> b\ny\n"
        );
    }

    #[test]
    fn zero_range_is_first_match_only() {
        assert_eq!(run(&["0,/x/s/x/y/"], "x\nx\n"), "y\nx\n");
        // 1,/x/ can't end on line 1, so it runs to the second x.
        assert_eq!(run(&["1,/x/s/x/y/"], "x\nx\n"), "y\ny\n");
        // reva-fork's case: add a line under the first header for the day.
        let script = r"0,/^## 2026-11-01$/s/^## 2026-11-01$/&\n\n- new line/";
        assert_eq!(
            run(&[script], "# Changes\n## 2026-11-01\n- a\n## 2026-11-01\n- b\n"),
            "# Changes\n## 2026-11-01\n\n- new line\n- a\n## 2026-11-01\n- b\n"
        );
        // A bare 0 is still accepted and selects nothing, as in v0.2.
        assert_eq!(run(&["0d"], "1\n"), "1\n");
    }

    #[test]
    fn empty_regex_keeps_its_v0_2_meaning() {
        // Decision: `s//x/` is not "reuse the last regex".
        assert_eq!(run(&["/b/s//x/"], "abc\n"), "xabc\n");
    }

    #[test]
    fn literal_flag() {
        assert_eq!(apply("s/a (b) + c/x/L", "q a (b) + c"), "q x");
        assert_eq!(apply("s/a.b/[&]/L", "a.b axb"), "[&] axb");
        assert_eq!(apply("s/$x/\\1$/L", "$x"), "\\1$");
        assert_eq!(apply("s/a.b/x\\ny/gL", "a.b"), "x\ny");
        assert_eq!(apply("s/A.B/x/iL", "a.b aXb"), "x aXb");
        assert_eq!(apply(r"s|\\|/|L", r"a\b"), "a/b");
        assert_eq!(apply(r"s/\(x\)/y/L", r"\(x\)"), "y");
        assert_eq!(apply(r"s/a\/b/c/L", "a/b"), "c");
    }

    #[test]
    fn fixed_strings_option_leaves_addresses_as_regexes() {
        let sed = Sed::parse_with(&["/a.c/s/a.c/X/"], &Options::new().fixed_strings(true)).unwrap();
        assert_eq!(sed.apply("abc a.c"), "abc X");
        assert_eq!(sed.apply("abc"), "abc");
    }

    #[test]
    fn stats_count_replacements_per_command() {
        let sed = Sed::parse(&["s/a/b/g; /x/s/c/d/", "s/zz/y/"]).unwrap();
        let mut out = Vec::new();
        let stats = sed.process_with_stats(&b"aa c\nx c\n"[..], &mut out).unwrap();
        assert_eq!(out, b"bb c\nx d\n");
        let got: Vec<_> = stats
            .substitutions
            .iter()
            .map(|s| (s.expression, s.source.as_str(), s.replacements))
            .collect();
        assert_eq!(
            got,
            [(1, "s/a/b/g", 2), (1, "/x/s/c/d/", 1), (2, "s/zz/y/", 0)]
        );
        assert_eq!(stats.total_replacements(), 3);
        let unmatched: Vec<_> = stats.unmatched().map(|s| s.source.as_str()).collect();
        assert_eq!(unmatched, ["s/zz/y/"]);

        let mut sum = Stats::default();
        sum.accumulate(&stats);
        sum.accumulate(&stats);
        assert_eq!(sum.total_replacements(), 6);
    }

    fn preserve(exprs: &[&str], input: &[u8]) -> Vec<u8> {
        let sed = Sed::parse(exprs).unwrap();
        let mut output = Vec::new();
        sed.process_preserving(input, &mut output).unwrap();
        output
    }

    #[test]
    fn preserving_keeps_line_endings() {
        assert_eq!(preserve(&["s/a/x/"], b"a\r\nb\r\n"), b"x\r\nb\r\n");
        assert_eq!(preserve(&["s/b/y/"], b"a\nb"), b"a\ny");
        assert_eq!(preserve(&["s/q/z/"], b"a\r\nb\nc"), b"a\r\nb\nc");
        assert_eq!(preserve(&["s/a/a\\nnew/"], b"a\r\nb\r\n"), b"a\r\nnew\r\nb\r\n");
        assert_eq!(preserve(&["N;s/\\n/+/"], b"a\r\nb\r\n"), b"a+b\r\n");
        assert_eq!(preserve(&["$d"], b"a\nb"), b"a\n");
        assert_eq!(preserve(&["s/a/b/"], b""), b"");
        // The same script in stream mode normalizes, as it always has.
        let sed = Sed::parse(&["s/a/x/"]).unwrap();
        let mut out = Vec::new();
        sed.process(&b"a\r\nb"[..], &mut out).unwrap();
        assert_eq!(out, b"x\nb\n");
    }
}
