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

Apply all substitution commands to a single string and return the result.

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
| Custom delimiters | `s\|/path\|<P>\|g` | Any character as delimiter |
| Back-references | `s/(\w+) (\w+)/\2 \1/` | Capture group references in replacement |
| Whole match | `s/foo/[&]/` | `&` expands to the entire match |
| BRE escapes | `s/[0-9]\{4\}/YYYY/` | Auto-converted to ERE equivalents |
| Escape sequences | `s/a/b\n/` | `\n` (newline), `\t` (tab) in replacements |

## Error Handling

`ParseError` implements `Display` and `Error`:

```rust
use sed_rs::{Sed, ParseError};

fn try_parse(expr: &str) -> Result<Sed, Box<dyn std::error::Error>> {
    Ok(Sed::parse(&[expr])?)
}
```

## Thread Safety

`Sed` is `Clone + Send + Sync` — parse once, share across threads.
