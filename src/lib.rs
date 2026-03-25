use regex::Regex;
use std::io::{self, BufRead, Write};

/// A parsed sed substitution command.
#[derive(Debug, Clone)]
pub struct SubstCmd {
    pattern: Regex,
    replacement: String,
    global: bool,
}

/// A sequence of sed commands to apply to each line.
#[derive(Debug, Clone)]
pub struct Sed {
    commands: Vec<SubstCmd>,
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
    /// Parse one or more sed expressions (each may contain `;`-separated commands).
    pub fn parse(expressions: &[&str]) -> Result<Self, ParseError> {
        let mut commands = Vec::new();
        for expr in expressions {
            let cmds = split_commands(expr);
            for cmd in cmds {
                let cmd = cmd.trim();
                if cmd.is_empty() {
                    continue;
                }
                commands.push(parse_subst(cmd)?);
            }
        }
        if commands.is_empty() {
            return Err(ParseError("no commands provided".into()));
        }
        Ok(Sed { commands })
    }

    /// Apply all substitution commands to a single line.
    pub fn apply(&self, line: &str) -> String {
        let mut result = line.to_string();
        for cmd in &self.commands {
            if cmd.global {
                result = cmd
                    .pattern
                    .replace_all(&result, &cmd.replacement)
                    .into_owned();
            } else {
                result = cmd.pattern.replace(&result, &cmd.replacement).into_owned();
            }
        }
        result
    }

    /// Process a reader line-by-line and write results to a writer.
    pub fn process<R: BufRead, W: Write>(&self, reader: R, mut writer: W) -> io::Result<()> {
        for line in reader.lines() {
            let line = line?;
            let result = self.apply(&line);
            writeln!(writer, "{}", result)?;
        }
        Ok(())
    }
}

/// Split an expression on unescaped `;` that are outside a substitution command.
fn split_commands(expr: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut current = String::new();
    let mut chars = expr.chars().peekable();

    while let Some(c) = chars.next() {
        if c == 's' && current.is_empty() {
            // Start of a substitution command — find delimiter and track it
            current.push(c);
            if let Some(&delim) = chars.peek() {
                current.push(chars.next().unwrap());
                let mut sections = 0; // need to pass 3 delimiters
                while let Some(ch) = chars.next() {
                    current.push(ch);
                    if ch == '\\' {
                        // Escaped character — consume next
                        if let Some(esc) = chars.next() {
                            current.push(esc);
                        }
                    } else if ch == delim {
                        sections += 1;
                        if sections == 2 {
                            // Consume flags
                            while let Some(&fc) = chars.peek() {
                                if fc.is_alphabetic() {
                                    current.push(chars.next().unwrap());
                                } else {
                                    break;
                                }
                            }
                            break;
                        }
                    }
                }
                commands.push(std::mem::take(&mut current));
            }
        } else if c == ';' {
            if !current.trim().is_empty() {
                commands.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        } else {
            current.push(c);
        }
    }
    if !current.trim().is_empty() {
        commands.push(current);
    }
    commands
}

/// Parse a single `s/pattern/replacement/flags` command.
fn parse_subst(cmd: &str) -> Result<SubstCmd, ParseError> {
    let mut chars = cmd.chars();

    match chars.next() {
        Some('s') => {}
        _ => return Err(ParseError(format!("unsupported command: {cmd}"))),
    }

    let delim = chars
        .next()
        .ok_or_else(|| ParseError("unexpected end of substitution".into()))?;

    let (pattern_str, replacement, flags) = parse_s_fields(&mut chars, delim)?;

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

/// Parse the pattern, replacement, and flags from inside a `s` command.
fn parse_s_fields(
    chars: &mut std::str::Chars<'_>,
    delim: char,
) -> Result<(String, String, String), ParseError> {
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                if next == delim {
                    // Escaped delimiter — include literally
                    current.push(delim);
                } else {
                    current.push('\\');
                    current.push(next);
                }
            } else {
                current.push('\\');
            }
        } else if ch == delim {
            parts.push(std::mem::take(&mut current));
            if parts.len() == 2 {
                // Rest is flags
                let flags: String = chars.collect();
                return Ok((parts.remove(0), parts.remove(0), flags));
            }
        } else {
            current.push(ch);
        }
    }

    // Tolerate missing trailing delimiter
    if parts.len() == 1 {
        Ok((parts.remove(0), current, String::new()))
    } else if parts.len() == 2 {
        Ok((parts.remove(0), parts.remove(0), current))
    } else {
        Err(ParseError("incomplete substitution command".into()))
    }
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
}
