# CLI Usage

`sed-rs` is a portable sed substitute that provides consistent substitution behavior across Linux and macOS.

## Installation

```bash
cargo install --path .
```

## Synopsis

```
sed-rs -e <expression> [-e <expression> ...]
```

Reads from stdin, writes to stdout. Designed to be a drop-in replacement for `sed` in pipeline/preprocessing contexts.

## Options

| Flag | Description |
|------|-------------|
| `-e`, `--expression` | Script expression (required, repeatable) |
| `-h`, `--help` | Print help |
| `-V`, `--version` | Print version |

As in sed, multiple `-e` expressions are joined with newlines into one script,
so a block or label can span several `-e` arguments.

## Commands

| Command | Description |
|---------|-------------|
| `s/re/repl/flags` | Substitute (see below) |
| `d` | Delete the pattern space; start the next cycle without printing |
| `N` | Append a newline and the next input line to the pattern space. On the last line, print the pattern space and stop (GNU behavior) |
| `{ ... }` | Group commands under one address |
| `:label` | Define a label for `b` |
| `b label` | Branch to `label`; `b` alone branches to the end of the script |
| `# comment` | Ignored to end of line |

Commands are separated by `;` or newlines. A label for `:` or `b` ends at a
newline, `;` or `}`.

## Addresses

A command may be preceded by one address. Add `!` to select the lines that
do *not* match.

| Address | Selects |
|---------|---------|
| `N` | Input line number `N` |
| `$` | The last line |
| `/re/` | Pattern space matching `re` (same regex rules as `s`; `\n` matches an embedded newline) |

Address ranges (`1,5`, `/a/,/b/`) are not supported.

## Substitution Syntax

```
s/pattern/replacement/[flags]
```

Any character can be used as the delimiter (not just `/`):

```
s|pattern|replacement|flags
s#pattern#replacement#flags
```

### Flags

| Flag | Description |
|------|-------------|
| `g` | Replace all matches (not just the first) |
| `i` | Case-insensitive matching |

### Replacement Syntax

| Token | Description |
|-------|-------------|
| `&` | Entire matched text |
| `\1`..`\9` | Capture group back-references |
| `\n` | Newline |
| `\t` | Tab |
| `\&` | Literal `&` |
| `\$` | Literal `$` |
| `\\` | Literal `\` |

### Regex

Patterns use [Rust regex syntax](https://docs.rs/regex/latest/regex/#syntax) internally. Common BRE escapes are automatically converted:

| BRE | Equivalent |
|-----|------------|
| `\{n\}` | `{n}` |
| `\(` `\)` | `(` `)` |
| `\+` `\?` `\|` | `+` `?` `\|` |

This means both BRE-style `[0-9]\{4\}` and ERE-style `[0-9]{4}` work.

## Examples

### Strip timestamps

```bash
my-command | sed-rs -e 's/[0-9]{4}-[0-9]{2}-[0-9]{2}/DATE/g'
```

### Normalize temp paths

```bash
my-command | sed-rs -e 's|/tmp/[^ ]*|<TMPDIR>|g'
```

### Multiple substitutions

Using multiple `-e` flags:

```bash
my-command | sed-rs \
  -e 's/[0-9]{4}-[0-9]{2}-[0-9]{2}/DATE/g' \
  -e 's|/tmp/[^ ]*|<TMPDIR>|g'
```

Using `;`-separated commands in a single expression:

```bash
my-command | sed-rs -e 's/foo/bar/g; s/baz/qux/g'
```

### Normalize PIDs in paths

```bash
my-command | sed-rs -e 's|/tmp/favicon-reg-[0-9]*|/tmp/favicon-reg-PID|g'
```

### Case-insensitive replacement

```bash
echo "Hello HELLO hello" | sed-rs -e 's/hello/hi/gi'
# hi hi hi
```

### Capture groups

```bash
echo "John Smith" | sed-rs -e 's/(\w+) (\w+)/\2, \1/'
# Smith, John
```

### Delete the last line, or lines matching a pattern

```bash
my-command | sed-rs -e '$d'
my-command | sed-rs -e '/^DEBUG/d'
```

### Strip trailing blank lines

The classic sed idiom, which works the same on macOS and Linux:

```bash
my-command | sed-rs -e 's/[[:space:]]*$//' | sed-rs -e ':a' -e '/^\n*$/{$d;N;ba' -e '}'
```

### Join all lines

```bash
printf '1\n2\n3\n' | sed-rs -e ':a' -e 'N' -e '$!ba' -e 's/\n/,/g'
# 1,2,3
```

## Use with reg-rs

`sed-rs` can replace system `sed` in `reg-rs` preprocessing commands for consistent cross-platform behavior:

```bash
reg create my-test -c 'my-command' -P "sed-rs -e 's/[0-9]{4}-[0-9]{2}-[0-9]{2}/DATE/g'"
```
