# CLI Usage

`sed-rs` is a portable sed substitute that provides consistent substitution behavior across Linux and macOS.

## Installation

```bash
cargo install --path .
```

## Synopsis

```
sed-rs [options] -e <expression> [-e <expression> ...] [FILE ...]
```

Reads stdin (or the FILEs, as one stream) and writes to stdout. Designed to be a drop-in replacement for `sed` in pipeline/preprocessing contexts. With `-i` it edits the FILEs in place instead.

## Options

| Flag | Description |
|------|-------------|
| `-e`, `--expression` | Script expression (required, repeatable) |
| `-i`, `--in-place` | Edit FILEs in place (see [In-place editing](#in-place-editing)) |
| `-iSUFFIX`, `--in-place=SUFFIX` | The same, keeping a backup copy at FILE+SUFFIX. The suffix must be attached |
| `-F`, `--fixed-strings` | Make every `s` literal, as if it had the `L` flag. Address regexes stay regexes |
| `--strict` | Exit 1 if any `s` command made no replacement (over all input) |
| `--expect=N` | Exit 1 unless exactly N replacements were made in total |
| `--diff` | Print a unified diff of the change. Without `-i`, a dry run that writes nothing |
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

A command may be preceded by one address or a range of two. Add `!` to
select the lines that do *not* match.

| Address | Selects |
|---------|---------|
| `N` | Input line number `N` |
| `$` | The last line |
| `/re/` | Pattern space matching `re` (same regex rules as `s`; `\n` matches an embedded newline) |

### Ranges

| Range | Selects |
|-------|---------|
| `A,B` | From a line matching `A` through the next line matching `B`. `B` is first tested on the line *after* `A`. If `B` is a line number at or before `A`'s line, only that one line |
| `0,/re/` | From the first line through the first line matching `re`, which may be line 1. This is "first match only" |

A range can start again after it ends: `/a/,/b/d` deletes every `a`...`b`
section. A bare `0` (as in `0d`) is accepted and selects nothing, for
compatibility; `0` as a range start needs a regex end.

```bash
sed-rs -e '10,20s/a/b/'                    # lines 10 to 20
sed-rs -e '/^def: /,/^=$/d'                # from one pattern to the next
sed-rs -e '0,/^## 2026-11-01$/s/$/ (first)/'   # first match only
```

An empty regex is *not* "the last regex" (as it is in GNU sed): `s//X/`
matches the empty string, as it always has in sed-rs. Write the pattern
again: `0,/^## d$/s/^## d$/&\n- new/`.

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
| `i`, `I` | Case-insensitive matching |
| `L` | Literal: the pattern and replacement are plain text (see below) |

Any other flag is an error (before 0.3, unknown flags were silently ignored,
so `s/a/b/p` ran as `s/a/b/`).
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

Because of this conversion `\(` is a group, not a literal parenthesis. For
literal text use `[(]`, `\.`, or the `L` flag.

### Literal substitution (`L`, `--fixed-strings`)

With the `L` flag (or `-F` for every `s`), neither side is a regex:
`( ) [ ] . * + ? | ^ $ &` and `\1` mean themselves. Only these are escapes:

| Escape | Means |
|--------|-------|
| `\n` | Newline |
| `\t` | Tab |
| `\\` | Backslash |
| `\` + delimiter | The delimiter |

```bash
sed-rs -i -e 's/a (b) + c/x/L' file
sed-rs -i -F -e 's|  ." F6 (fn+F6)" cr|  ." Cmd-Enter or F6" cr|' help.f
```

`i` and `g` combine with `L`. `-F` does not affect address regexes, so
`-F -e '/^fn /s/a.b/c/'` still selects lines by regex.

## Input and output

Without FILEs, sed-rs reads stdin. Several FILEs are read as one stream, as
in sed: line numbers keep counting and `$` is the last line of the last
file. `-` means stdin. Output goes to stdout with every line ending in `\n`
(a `\r\n` ending becomes `\n`, and a missing final newline is added). This
is the v0.2 behaviour, and it stays that way.

## In-place editing

```bash
sed-rs -i -e 's/old/new/' a.rs b.rs       # edit, no backup
sed-rs -i.orig -e 's/old/new/' file       # file.orig keeps the original
```

- **All files or none.** The script is parsed, then every FILE is read and
  processed in memory. If anything fails, nothing is written: a parse
  error, an unreadable or non-UTF-8 file, or a `--strict`/`--expect` check.
  Only then are results written.
- **Each file is atomic.** The result goes to a temporary file in the same
  directory, which is then renamed over the original, so a reader never
  sees a half-written file. Across several files the renames happen one
  after another. If a rename fails (rare: e.g. the disk fills), the error
  names the files already changed.
- **Bytes are kept.** Each line is written back with its own ending (`\n`
  or `\r\n`), and a last line without a newline stays that way. A newline
  added by the script (`\n` in a replacement, or `N`) is written as `\r\n`
  in a CRLF file. Permission bits are kept; owner, extended attributes and
  hard links are not, because the rename creates a new file.
- **Unchanged files are not rewritten**, so their modification time stays.
  With a SUFFIX, every FILE is backed up, changed or not, so
  `mv file.orig file` always undoes the edit.
- **Each file is its own stream**: line numbers, `$` and ranges restart for
  each file (like GNU `-s`).
- **Refused**, with exit 1 before anything is written:
  - symbolic links and non-regular files;
  - stdin (`-`);
  - a file listed twice;
  - BSD's `-i ''` (use `-i`);
  - `-ie` (it would mean suffix `e`; write `-i -e`);
  - a suffix containing `/` or `*`.

## Checking an edit: `--strict` and `--expect`

```bash
sed-rs --strict -i -e 's/old/new/' file         # each s must match somewhere
sed-rs --expect=1 -i -F -e 's/exact text/new/' file   # exactly one replacement
```

- `--strict` fails when any `s` command made no replacement. Replacements
  are counted over all the input, so with several files each `s` must
  match in at least one of them. The error names the command and its `-e`:

  ```
  sed-rs: --strict: `s/nothere/x/` (-e 2) made no replacement
  ```
- `--expect=N` fails unless all the `s` commands together made exactly N
  replacements. This catches a pattern that matches too much, as well as
  one that matches nothing.
- With `-i`, a failed check means no file is written. In stream mode the
  output has already been printed, so only the exit status (1) shows the
  failure.
- Without these flags, an `s` that matches nothing is silent, as always,
  because filters often hold substitutions that don't apply to every input.

## Showing the change: `--diff`

```bash
sed-rs --diff -e 's/old/new/' file        # preview: print the diff, write nothing
sed-rs --diff -e 's/old/new/' < file      # the same for stdin (labelled -)
sed-rs -i --diff -e 's/old/new/' file     # edit, then print the diff
```

The diff is unified with 3 lines of context, made with the same
line-ending rules as `-i`, so the preview shows exactly what `-i` would
write. With several FILEs, each one is diffed separately. A file with no
change prints nothing. The diff is meant for people to read; don't store it
as a reference output, since its hunks may differ between versions.

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
