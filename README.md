# sed-rs

sed-rs is a portable subset of sed, written in Rust, that behaves the same
on macOS and Linux (and, being plain Rust, should on Windows; untested).

It was created for [reg-rs](https://github.com/sw-cli-tools/reg-rs), the
regression-test runner that compares a command's output against a stored
reference file. Output often contains things that change from run to run,
such as dates, PIDs and temporary paths. reg-rs runs sed-rs as a
preprocessing step to normalize them, so the comparison is deterministic and
a test fails only when the output really changed. Because reference files are
shared between machines, that filter must give the same result on every
platform, which system sed does not.

BSD sed (macOS) and GNU sed (Linux) disagree on regex syntax, on `-i`, and
on multi-line scripts; some GNU idioms crash BSD sed outright. That breaks
shell scripts and test filters that have to run on both. sed-rs implements
the part of sed those scripts actually use, with one regex engine and one
set of rules everywhere. It is meant as a drop-in for `sed` in pipelines
and in [reg-rs](https://github.com/sw-cli-tools/reg-rs) preprocessing, and
as a Rust library.

## Summary

- **Commands:** `s/re/repl/flags` (flags `g`, `i`), `d`, `N`, `{ }`,
  `:label` and `b`.
- **Addresses:** a line number, `$` (the last line) or `/re/`, with `!` to
  negate.
- **Regexes:** Rust [regex](https://docs.rs/regex/latest/regex/#syntax)
  syntax. BRE escapes such as `\(`, `\)` and `\{n\}` are converted
  automatically, so both `[0-9]\{4\}` and `[0-9]{4}` work.
- **Scripts:** commands are separated by `;` or newlines, and several `-e`
  expressions join into one script, so a block or label may span them.
- **Not supported:** address ranges (`1,5`, `/a/,/b/`), in-place editing,
  and other commands (`p`, `y`, `a`/`i`/`c`, the hold space, ...).

```bash
my-command | sed-rs -e 's/[0-9]{4}-[0-9]{2}-[0-9]{2}/DATE/g'   # normalize dates
my-command | sed-rs -e '/^DEBUG/d'                              # drop lines
my-command | sed-rs -e ':a' -e '/^\n*$/{$d;N;ba' -e '}'         # strip trailing blank lines
```

## Build

Requires [Rust](https://rustup.rs) (stable, via rustup).

```bash
cargo build --release          # target/release/sed-rs
cargo test                     # unit tests
cargo install --path .         # install sed-rs on your PATH
sed-rs --help                  # usage, commands and examples
```

## Documentation

- [CLI usage](docs/cli-usage.md): options, commands, addresses, substitution
  syntax and examples.
- [Library usage](docs/library-usage.md): the `sed_rs` crate API
  (`Sed::parse`, `apply`, `process`).

## Copyright

Copyright (c) 2026 Michael A Wright

## License

MIT. See [`LICENSE`](LICENSE) and [`COPYRIGHT`](COPYRIGHT).
