# Library Usage

The `sed_rs` crate can be imported directly into Rust projects to provide sed substitution functionality without shelling out.

## Add Dependency

```toml
[dependencies]
sed-rs = { path = "../sed-rs" }
```

## API

### `Sed::parse`

Parse one or more sed expressions into a reusable `Sed` instance.

```rust
use sed_rs::Sed;

// Single expression
let sed = Sed::parse(&["s/foo/bar/g"]).unwrap();

// Multiple expressions
let sed = Sed::parse(&[
    "s/[0-9]{4}-[0-9]{2}-[0-9]{2}/DATE/g",
    "s|/tmp/[^ ]*|<TMPDIR>|g",
]).unwrap();

// Semicolon-separated commands in one expression
let sed = Sed::parse(&["s/foo/bar/g; s/baz/qux/g"]).unwrap();
```

Returns `Result<Sed, ParseError>`. A `ParseError` is returned if any expression has invalid syntax or an invalid regex pattern.

### `Sed::apply`

Run the script on a single line and return what it prints, without the
trailing newline. For substitution-only scripts this is the substituted line;
if the script deletes the line (`d`), the result is an empty string.

```rust
let sed = Sed::parse(&["s/[0-9]+/NUM/g"]).unwrap();
let result = sed.apply("port 8080 and pid 12345");
assert_eq!(result, "port NUM and pid NUM");
```

### `Sed::process`

Process a `BufRead` line-by-line and write results to a `Write`.

```rust
use std::io::BufReader;

let sed = Sed::parse(&["s/secret-[a-f0-9]+/secret-REDACTED/g"]).unwrap();

let input = b"token: secret-abc123\nother: secret-def456\n";
let mut output = Vec::new();
sed.process(BufReader::new(&input[..]), &mut output).unwrap();

assert_eq!(
    String::from_utf8(output).unwrap(),
    "token: secret-REDACTED\nother: secret-REDACTED\n"
);
```

`process` prints every line with `\n`: a `\r\n` ending becomes `\n`, and a
last line without a newline gets one. This has not changed since v0.1.

### `Sed::parse_with` and `Options` (0.3)

```rust
use sed_rs::{Options, Sed};

// Every `s` literal, as with the CLI's --fixed-strings.
let sed = Sed::parse_with(&["s/a.b(c)/x/"], &Options::new().fixed_strings(true)).unwrap();
assert_eq!(sed.apply("a.b(c) abbc"), "x abbc");
```

`Options` is `#[non_exhaustive]`, so build it with `Options::new()` and its
setters. More options can then be added without breaking callers.

### `Sed::process_with_stats` (0.3)

The same as `process`, but it also returns how many replacements each `s`
command made:

```rust
let sed = Sed::parse(&["s/a/b/g", "s/zz/y/"]).unwrap();
let mut out = Vec::new();
let stats = sed.process_with_stats(&b"aa\n"[..], &mut out).unwrap();
assert_eq!(stats.total_replacements(), 2);
let unmatched: Vec<_> = stats.unmatched().map(|s| s.source.as_str()).collect();
assert_eq!(unmatched, ["s/zz/y/"]);
```

`Stats::substitutions` holds a `SubstStats` for each `s` command, in script
order, with three fields:
- `expression`: the `-e` index, counting from 1;
- `source`: the command as written;
- `replacements`: how many replacements it made.

`Stats::accumulate` adds up runs over several inputs.

### `Sed::process_preserving` (0.3)

This is what `-i` uses. It is like `process_with_stats`, but the bytes come
out as they went in:
- each line keeps its ending (`\n`, `\r\n`, or none on a last line without
  one);
- a newline added by the script is written as `\r\n` in a CRLF line.

```rust
let sed = Sed::parse(&["s/a/x/"]).unwrap();
let mut out = Vec::new();
sed.process_preserving(&b"a\r\nb"[..], &mut out).unwrap();
assert_eq!(out, b"x\r\nb");
```

## Integration with reg-rs

Instead of shelling out to `sed` for preprocessing, `reg-rs` can use the library directly:

```rust
use sed_rs::Sed;

fn preprocess(sed_expr: &str, input: &str) -> Result<String, sed_rs::ParseError> {
    let sed = Sed::parse(&[sed_expr])?;
    Ok(sed.apply(input))
}

// Per-line processing over captured output
fn preprocess_output(sed_expr: &str, output: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let sed = Sed::parse(&[sed_expr])?;
    let mut result = Vec::new();
    sed.process(std::io::BufReader::new(output), &mut result)?;
    Ok(result)
}
```

## Supported Features

| Feature | Example | Description |
|---------|---------|-------------|
| Basic substitution | `s/foo/bar/` | Replace first match |
| Global flag | `s/foo/bar/g` | Replace all matches |
| Case-insensitive | `s/foo/bar/gi` | Case-insensitive matching |
| Literal | `s/a.b(c)/x/L` | Pattern and replacement are plain text |
| Custom delimiters | `s\|/path\|<P>\|g` | Any character as delimiter |
| Back-references | `s/(\w+) (\w+)/\2 \1/` | Capture group references in replacement |
| Whole match | `s/foo/[&]/` | `&` expands to the entire match |
| BRE escapes | `s/[0-9]\{4\}/YYYY/` | Auto-converted to ERE equivalents |
| Escape sequences | `s/a/b\n/` | `\n` (newline), `\t` (tab) in replacements |
| Addresses | `2d`, `$d`, `/re/d`, `/re/!d` | Line number, last line, regex; `!` negates |
| Ranges | `2,5d`, `/a/,/b/d`, `0,/re/s/x/y/` | From one address to another; `0,/re/` for the first match only |
| Delete | `d` | Delete the pattern space, start the next cycle |
| Next line | `N` | Append the next line to the pattern space |
| Blocks | `/re/{ s/a/b/; s/c/d/ }` | Group commands under one address |
| Labels, branches | `:a` ... `ba` | Loops such as stripping trailing blank lines |

See [cli-usage.md](cli-usage.md) for the exact semantics. Multi-line commands
(`N`, `b`) see the whole input through `process`; `apply` runs the script on
one line.

## Error Handling

`ParseError` implements `Display` and `Error`:

```rust
use sed_rs::{Sed, ParseError};

fn try_parse(expr: &str) -> Result<Sed, Box<dyn std::error::Error>> {
    Ok(Sed::parse(&[expr])?)
}
```

## Thread Safety

`Sed` is `Clone + Send + Sync` -- parse once, share across threads.
