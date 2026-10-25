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
    fn apply(&self, text: &str) -> String {
        if self.global {
            self.pattern
                .replace_all(text, &self.replacement)
                .into_owned()
        } else {
            self.pattern.replace(text, &self.replacement).into_owned()
        }
    }
}

/// Which input a command applies to (single addresses only; no ranges).
#[derive(Debug, Clone)]
enum Addr {
    Line(usize),
    Last,
    Re(Regex),
}

#[derive(Debug, Clone)]
enum Op {
    Subst(SubstCmd),
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
    addr: Option<Addr>,
    negate: bool,
    op: Op,
}

/// A parsed sed script: substitutions plus the addresses, blocks, labels,
/// branches, `N` and `d` needed for multi-line idioms.
#[derive(Debug, Clone)]
pub struct Sed {
    commands: Vec<Cmd>,
}

#[derive(Debug)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

impl Sed {
    /// Parse one or more sed expressions. As in sed, the expressions are
    /// joined with newlines into one script, so a block or label may span
    /// several `-e` arguments. Commands are separated by `;` or newlines.
    pub fn parse(expressions: &[&str]) -> Result<Self, ParseError> {
        let script = expressions.join("\n");
        let mut chars = script.chars().peekable();
        let mut commands: Vec<Cmd> = Vec::new();
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

            let addr = parse_address(&mut chars)?;
            skip_blanks(&mut chars);
            let negate = chars.next_if_eq(&'!').is_some();
            skip_blanks(&mut chars);

            let c = chars
                .next()
                .ok_or_else(|| ParseError("missing command".into()))?;
            let op = match c {
                's' => Op::Subst(parse_subst(&mut chars)?),
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
        Ok(Sed { commands })
    }

    /// Run the script on a single line and return what it prints, without
    /// the trailing newline (an empty string if the line is deleted).
    pub fn apply(&self, line: &str) -> String {
        let mut out = Vec::new();
        let input = std::iter::once(Ok(line.to_string()));
        self.run(input, &mut out)
            .expect("writing to a Vec cannot fail");
        let mut text = String::from_utf8(out).expect("input and script are UTF-8");
        text.pop();
        text
    }

    /// Process a reader line-by-line and write results to a writer.
    pub fn process<R: BufRead, W: Write>(&self, reader: R, writer: W) -> io::Result<()> {
        self.run(reader.lines(), writer)
    }

    /// The sed cycle: read a line into the pattern space, run the script,
    /// print the pattern space (unless `d` ran), repeat.
    fn run<I, W>(&self, lines: I, mut writer: W) -> io::Result<()>
    where
        I: Iterator<Item = io::Result<String>>,
        W: Write,
    {
        let mut lines = lines.peekable();
        let mut line_no = 0;
        while let Some(line) = lines.next() {
            let mut space = line?;
            line_no += 1;
            let mut print = true;
            let mut pc = 0;
            while pc < self.commands.len() {
                let cmd = &self.commands[pc];
                if !cmd.selects(&space, line_no, lines.peek().is_none()) {
                    pc = match cmd.op {
                        Op::Block { end } => end,
                        _ => pc + 1,
                    };
                    continue;
                }
                match &cmd.op {
                    Op::Subst(s) => space = s.apply(&space),
                    Op::Delete => {
                        print = false;
                        break;
                    }
                    Op::AppendNext => match lines.next() {
                        Some(next) => {
                            space.push('\n');
                            space.push_str(&next?);
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
                writeln!(writer, "{}", space)?;
            }
        }
        Ok(())
    }
}

impl Cmd {
    fn selects(&self, space: &str, line_no: usize, is_last: bool) -> bool {
        let hit = match &self.addr {
            None => true,
            Some(Addr::Line(n)) => line_no == *n,
            Some(Addr::Last) => is_last,
            Some(Addr::Re(re)) => re.is_match(space),
        };
        hit != self.negate
    }
}

type Chars<'a> = Peekable<std::str::Chars<'a>>;

fn skip_blanks(chars: &mut Chars<'_>) {
    while chars.next_if(|c| *c == ' ' || *c == '\t').is_some() {}
}

/// An optional address: a line number, `$` (last line) or `/regex/`.
fn parse_address(chars: &mut Chars<'_>) -> Result<Option<Addr>, ParseError> {
    let addr = match chars.peek() {
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
    };
    skip_blanks(chars);
    if chars.peek() == Some(&',') {
        return Err(ParseError("address ranges are not supported".into()));
    }
    Ok(addr)
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
fn parse_subst(chars: &mut Chars<'_>) -> Result<SubstCmd, ParseError> {
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
        flags.push(f);
    }

    let global = flags.contains('g');
    let case_insensitive = flags.contains('i') || flags.contains('I');

    // Convert common BRE escapes to ERE equivalents
    let pattern_str = bre_to_ere(&pattern_str);

    let regex_pattern = if case_insensitive {
        format!("(?i){pattern_str}")
    } else {
        pattern_str
    };

    let pattern =
        Regex::new(&regex_pattern).map_err(|e| ParseError(format!("invalid regex: {e}")))?;

    // Convert sed replacement syntax to regex crate syntax:
    // sed uses \1..\9 for captures; regex crate uses $1..$9
    // sed uses & for whole match; regex crate uses $0
    let replacement = convert_replacement(&replacement);

    Ok(SubstCmd {
        pattern,
        replacement,
        global,
    })
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
        assert_eq!(parse_err(&["1,3d"]), "address ranges are not supported");
        assert_eq!(
            parse_err(&["y/abc/xyz/"]),
            "unsupported command: y/abc/xyz/"
        );
        assert_eq!(parse_err(&[":"]), "\":\" lacks a label");
        assert_eq!(parse_err(&["/x"]), "unterminated address regex");
    }
}
