# Compatibility

sed-rs runs inside other people's pipelines, and inside reg-rs
preprocessors whose reference files store its output byte for byte. A
release that changes the output of an existing script silently breaks
their tests. These rules say what may change, and the tests below enforce
them.

## The rules

1. **A script that parses gives the same output bytes and exit status in
   every later release.** A deliberate exception is listed under
   "Behaviour changes" in [CHANGELOG](../CHANGELOG.md), with the old and
   the new output. It ships only in a minor release (0.x) or a major
   release (≥1), never in a patch release.
2. **New features only use input that used to be an error:** new flags,
   FILE operands, and syntax that didn't parse (such as `,` in an address).
   Turning an error into something valid is fine. Changing what a valid
   script does is not.
3. **Anything new that writes to stderr or changes the exit status is
   opt-in** (`--strict`, `--expect`). Without new flags, stderr is empty on
   success.
4. **Library changes only add.** New behaviour comes as new methods or new
   `Options` fields. Existing signatures and meanings don't change. New
   public structs are `#[non_exhaustive]`, so they can grow.
5. **Error message text is not part of the contract.** The exit status (1)
   and the `sed-rs: ` prefix are.

## Frozen behaviours

These differ from GNU sed, but they are kept because users depend on them:

- Stream mode prints every line with `\n`: CRLF becomes LF, and a final
  newline is added. (`-i` and `--diff` keep the bytes instead.)
- The BRE escapes `\(` `\)` `\{ \}` `\+` `\?` `\|` are operators.
- `s//X/` matches the empty string. It does not reuse the last regex.
- `0` alone is a valid address that selects nothing.

## How it is enforced

- **Golden cases:** `tests/compat/cases/*`. These were recorded from the
  v0.2.0 binary by `tests/compat/record.sh` and replayed by
  `tests/compat.rs`. They include:
  - every sed-rs invocation found in the reva-fork and forthlet scripts;
  - the documented examples;
  - byte-level edge cases (CRLF, no final newline, empty input, invalid
    UTF-8);
  - parser edge cases.

  A case may change only if its directory gets a `changed` file naming the
  CHANGELOG entry, with the old output kept as `stdout.v0.2.0`.
- **Differential test:** `tests/differential.rs`. Proptest builds 4000
  random scripts from the v0.2 grammar, with random inputs that include
  `\r`, blank runs and no final newline. For each, it checks that the
  current library and the frozen v0.2.0 source (`tests/support/v0_2.rs`)
  agree on:
  - whether the script parses;
  - the output of `process`;
  - the output of `apply`.
- **CLI tests:** `tests/cli.rs` cover the new flags. They also check that
  an unmatched `s` is still silent without `--strict`.

When adding a feature, add golden cases for any edge of the old behaviour
it comes close to. Record them with the *previous* release's binary:

```bash
git worktree add /tmp/prev v0.3.0 && (cd /tmp/prev && cargo build)
tests/compat/record.sh /tmp/prev/target/debug/sed-rs   # then inspect git diff
```

`record.sh` rewrites every case. Keep the existing ones unchanged: `git
diff tests/compat` must show only additions.
