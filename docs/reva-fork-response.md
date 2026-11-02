# Response to reva-fork's sed-rs asks

Reply to `softwarewrighter/reva-fork/docs/sed-rs-asks.md` (2026-11-01).
All five asks are in **sed-rs 0.3.0**, some of them changed after
negotiation. This file records what was decided, what was built, how it
behaves, and how existing users were protected.

## Quick reference for agents editing files

```bash
# Checked, literal, in-place edit: the safe default for code edits
sed-rs -i -F --expect=1 -e 's|old exact text|new exact text|' file

# Same, keeping a backup (undo: mv file.orig file)
sed-rs -i.orig --strict -e 's/old/new/' file

# Preview first: prints the diff, writes nothing
sed-rs --diff -F -e 's|old|new|' file

# Edit and show what changed
sed-rs -i --diff --strict -e 's/old/new/' file

# Add a line under the FIRST header for the day (repeat the pattern; see ask 2)
sed-rs -i --expect=1 -e '0,/^## 2026-11-01$/s/^## 2026-11-01$/&\n\n- new line/' CHANGES.md

# Mix: the address is a regex, the substitution is literal
sed-rs -i -e '/^: help/,/;$/s/." F6 (fn+F6)"/." Cmd-Enter or F6"/L' help.f
```

If the script is wrong, a file is unreadable, or `--strict`/`--expect`
fails, sed-rs exits 1 and **no file is touched**. The shell pattern
`mv file file.orig && sed-rs … < file.orig > file` is no longer needed.

## Decisions (maintainer, 2026-11-01)

| Question | Decision |
|----------|----------|
| Should `s//repl/` reuse the last regex (POSIX/GNU)? | **No.** `s//X/` keeps its v0.2 meaning (it matches the empty string). |
| Unknown `s` flags: an error now, or warn first? | **An error now.** |
| Literal mode: only an `L` flag, or also a global flag? | **Both:** the `L` flag on `s`, plus a global `-F` / `--fixed-strings`. |
| Offer `--expect=N`? | **Yes.** |

## What was built, ask by ask

### 1. `-i[SUFFIX]`: done, as asked plus safeguards

- `-i` / `--in-place` edits without a backup. `-i.orig` /
  `--in-place=.orig` keeps a backup. The suffix must be attached, as in
  GNU.
- **All files or none.** The script is parsed, then every file is read and
  processed in memory, and the `--strict`/`--expect` checks run. Only after
  all of that succeeds does anything get written. A parse error, a missing
  or non-UTF-8 file, or a failed check leaves every file untouched. This
  directly prevents the empty `CHANGES.md` incident.
- **Atomic per file.** The result is written to a temporary file in the
  same directory, which is fsynced and then renamed over the original.
  - Across several files the renames happen one after another. If a rename
    fails (rare), the error names the files already changed.
- **Bytes kept:**
  - `\n` and `\r\n` endings are kept;
  - a missing final newline stays missing;
  - newlines the script adds are written as `\r\n` in CRLF files;
  - permission bits are kept;
  - owner, extended attributes and hard links are not kept, because the
    rename makes a new file.
- **Unchanged files are not rewritten**, so their mtime stays. With a
  suffix, every file is backed up, so `mv file.orig file` always works.
- **Each file is its own stream:** line numbers, `$` and ranges restart
  for each file.
- **Refused** (exit 1, nothing written):
  - symlinks and non-regular files;
  - `-` (stdin);
  - a file listed twice;
  - BSD `-i ''`;
  - `-ie` (it would mean suffix `e`);
  - a suffix containing `/` or `*`.
- `-e` is still required, so a script is never mistaken for a file name.

### 2. Ranges and "first match only": done, with one change

- `A,B` with line numbers, `$` or regexes, plus `0,/re/`, all with GNU
  semantics:
  - `B` is first tested on the line after `A`;
  - a line-number `B` at or before `A` selects one line;
  - a range can start again after it ends.
- Ranges work with `!` and `{}`.
- **Change from the ask:** `s//…/` does **not** reuse the last regex
  (decided above), so the ask's `0,/^## 2026-11-01$/s//…/` matches the
  empty string. Repeat the pattern instead (`&` saves retyping it in the
  replacement):

  ```bash
  sed-rs -i -e '0,/^## 2026-11-01$/s/^## 2026-11-01$/&\n\n- new line/' CHANGES.md
  ```
- A bare `0` (as in `0d`) is still accepted and selects nothing, as in
  v0.2. `0,5d` is an error, because `0` can only start a range whose end is
  a regex.

### 3. Literal patterns: done (`L` flag and `-F`/`--fixed-strings`)

- `s/a (b) + c/x/L` is literal on both sides: `( ) [ ] . * + ? | ^ $ &`
  and `\1` all mean themselves.
- **Negotiated escape rule.** "Nothing is special" would make `\n`
  impossible, and you rely on `\n` to insert lines. So in literal mode
  `\n`, `\t`, `\\` and an escaped delimiter are still escapes. Any other
  backslash is literal: `\(` is backslash then parenthesis.
- `-F` sets `L` on every `s` but **does not** affect address regexes, so
  `-F -e '/^fn /s/a.b/c/'` still selects lines by regex.
- Unknown flags are now an error. Without that, a typo or an older sed-rs
  would silently run a literal edit as a regex, which is the failure you
  reported.
- **Not changed (objection stands):** the BRE conversion, which makes `\(`
  a group outside literal mode. It is documented, tested and relied on.
  Use `L` instead.

### 4. `--strict`: done; plus `--expect=N`

- `--strict` exits 1 if any `s` made no replacement. Replacements are
  counted over all files together, and the error names the command and its
  `-e`:

  ```
  sed-rs: --strict: `s/nothere/x/` (-e 2) made no replacement
  ```
- `--expect=N` exits 1 unless exactly N replacements were made in total:

  ```
  sed-rs: --expect=1: made 3 replacement(s)
  ```

  It catches a pattern that is too broad, not just one that matches
  nothing. `--expect=1` with `-F` is the tightest form of a code edit.
- With `-i`, a failed check writes nothing. In stream mode the output is
  already printed; only the exit status shows the failure.
- **Declined:** a default warning without `--strict`. Normalising filters
  (reg-rs `-P` chains, forthlet's pipelines, your own `doc-check.sh`) often
  contain substitutions that don't match a given input, and a default
  warning would flood their stderr.

### 5. `--diff`: done

- `sed-rs --diff -e … file` (or `< file`) is a dry run: it prints a unified
  diff (3 lines of context) and writes nothing.
- `sed-rs -i --diff …` writes the files, then prints the diff.
- The preview uses the same byte rules as `-i`, so it shows exactly what
  `-i` would write. Each file is diffed separately. A file with no change
  prints nothing.
- The diff is for people to read. Don't store it in reg-rs references,
  because its hunks may differ between versions of the diff crate
  (`similar` 2).

## How existing users are protected

Existing users are reg-rs preprocessors, reva-fork's `doc-check.sh` and
`parity-examples.sh`, and forthlet's scripts. All of them run sed-rs as
stdin → stdout with `-e`. The rules are in
[compatibility.md](compatibility.md). In short: a script that parsed in
0.2 gives the same bytes, and new behaviour is opt-in or uses what used to
be an error. These checks enforce it:

| Check | What it proves |
|-------|----------------|
| `tests/compat/cases/` (68 cases recorded from the v0.2.0 binary, replayed by `tests/compat.rs`) | Real invocations from reva-fork and forthlet, the documented examples, CRLF, a missing final newline, invalid UTF-8, BRE escapes, `s//X/`, `0d`, errors. Same stdout, same exit status, empty stderr. **66 unchanged**; 2 changed on purpose (below) |
| `tests/differential.rs` (proptest, 4000 cases) | On random v0.2-grammar scripts and inputs, the new library and a frozen copy of v0.2.0 agree on parse, `process` and `apply` output |
| `tests/cli.rs` (14 tests) | Each new flag, the all-or-nothing behaviour, the refusals, and that an unmatched `s` is still silent without `--strict` |
| Library API | Only additions: `parse_with`/`Options`, `process_with_stats`, `process_preserving`, `Stats`/`SubstStats` (all `#[non_exhaustive]`). `parse`, `apply`, `process` and `ParseError` are unchanged |

The only intended behaviour changes are listed in
[CHANGELOG](../CHANGELOG.md):

1. Unknown `s` flags are an error. `s/a/b/p` used to run silently as
   `s/a/b/`. None of the known users' scripts use one.
2. `L` means literal. It used to be ignored.

Stream-mode output is unchanged: CRLF still becomes LF and a final newline
is still added. Only `-i` and `--diff` keep the bytes.

## Not done yet

- Not committed, not tagged, and not installed. To use it now:
  `cargo install --path .` (this replaces the `sed-rs` on PATH that reg-rs
  and the scripts use; the compatibility suite above is why that is safe).
- The repository has no `v0.2.0` tag yet. Tag `edf6b73` as `v0.2.0` before
  tagging `v0.3.0`.
- `cargo semver-checks` is not run in CI yet, because there is no CI. Run
  it before publishing to crates.io.
- Not built, unless someone asks: `addr,+N` and `addr,~N`,
  `--follow-symlinks`, and a byte-preserving stream mode
  (`--preserve-eol`).
