# Changelog

See [docs/compatibility.md](docs/compatibility.md) for what may change
between releases. Behaviour changes are always listed in their own section.

## 0.3.0 (2026-11-01)

### Added

- `-i[SUFFIX]` / `--in-place[=SUFFIX]`: edit FILEs in place.
  - All files or none: nothing is written if any step fails.
  - Each file is replaced atomically, by a rename.
  - Line endings, a missing final newline, and permission bits are kept.
  - Unchanged files are not rewritten.
  - Symlinks, non-regular files, BSD `-i ''` and `-ie` are refused.
- FILE operands. Without `-i` they are read as one stream, and `-` is stdin.
- Address ranges: `A,B`, and `0,/re/` for the first match only.
- The `L` flag on `s`, and `-F` / `--fixed-strings` (`L` on every `s`):
  - the pattern and replacement are literal text;
  - only `\n`, `\t`, `\\` and an escaped delimiter are escapes;
  - address regexes are not affected.
- `--strict`: exit 1 if any `s` made no replacement.
- `--expect=N`: exit 1 unless exactly N replacements were made in total.
- `--diff`: print a unified diff. Without `-i` it is a dry run.
- Library additions:
  - `Sed::parse_with` and `Options`;
  - `Sed::process_with_stats` and `Sed::process_preserving`;
  - the `Stats` and `SubstStats` types.
- Tests:
  - `tests/compat` golden cases, recorded from v0.2.0;
  - `tests/differential.rs`, which compares the library with a frozen
    v0.2.0 copy on random scripts;
  - `tests/cli.rs`.

### Behaviour changes

- **Unknown `s` flags are an error** (`unknown option to `s': p`). Until
  now they were silently ignored:
  - `s/a/b/p` ran as `s/a/b/`, printing once where GNU sed prints twice;
  - `m`, `M` and `e` were dropped the same way.

  Golden case: `tests/compat/cases/flag-p`.
- **`L` now means literal.** Since it was ignored before, `s/a.b/X/gL` was
  a regex substitution until now. Golden case: `flag-L`.
- Error text: `1,3d` no longer fails with "address ranges are not
  supported", because ranges are now valid.

### Unchanged on purpose

- Stream mode (stdin to stdout) output bytes: CRLF becomes LF, and a final
  newline is added.
- The BRE conversion: `\(` is a group.
- `s//X/` matches the empty string. It does not reuse the last regex as GNU
  does.
- A bare `0` address is accepted and selects nothing.
- Exit codes are 0 and 1, and stderr is empty on success.
- An unmatched `s` is silent unless `--strict` is given.

## 0.2.0

- Addresses (`N`, `$`, `/re/`, `!`), blocks, labels and branches, `N`, `d`.

## 0.1.0

- `s///` with `g` and `i`, BRE escape conversion, `-e`, library API.
