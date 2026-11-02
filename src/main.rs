//! sed-rs - A portable subset of sed.

mod version;

use clap::Parser;
use sed_rs::{Options, Sed, Stats};
use std::collections::HashSet;
use std::fs;
use std::io::{self, BufReader, Write};
use std::path::{Path, PathBuf};

/// A portable subset of sed for consistent cross-platform behavior.
///
/// Applies a sed script to stdin (or FILEs) and writes results to stdout,
/// or edits FILEs in place with -i. Uses Rust's regex engine for identical
/// behavior on all platforms, with automatic BRE-to-ERE conversion.
///
/// # AI Coding Agent Instructions
///
/// This tool replaces system `sed` in contexts where consistent
/// cross-platform behavior is required — particularly for reg-rs
/// preprocessing (-P) of nondeterministic output — and for safe,
/// checked in-place edits of source files.
///
/// ## Usage Patterns
///
/// Strip timestamps from command output:
///     my-command | sed-rs -e 's/[0-9]{4}-[0-9]{2}-[0-9]{2}/DATE/g'
///
/// Normalize temp directory paths (handles macOS and Linux):
///     my-command | sed-rs -e 's|/private/var/[^ ]*|<TMPDIR>|g; s|/tmp/[^ ]*|<TMPDIR>|g'
///
/// Use as a reg-rs preprocessor:
///     reg-rs create my-test -c 'my-command' -P "sed-rs -e 's/[0-9]{4}-[0-9]{2}-[0-9]{2}/DATE/g'"
///
/// Delete the last line, or lines matching a pattern:
///     my-command | sed-rs -e '$d'
///     my-command | sed-rs -e '/^DEBUG/d'
///
/// Strip trailing blank lines (the classic idiom; same on macOS and Linux):
///     my-command | sed-rs -e ':a' -e '/^\n*$/{$d;N;ba' -e '}'
///
/// Edit a file in place, keeping a backup, and fail unless the edit matched:
///     sed-rs -i.orig --strict -e 's/old/new/' file
///
/// Replace exact code text (no regex), exactly once, and show the change:
///     sed-rs -i --fixed-strings --expect=1 --diff -e 's|f(a[0]) + 1|g(a)|' src/x.rs
///
/// Preview an edit without writing anything:
///     sed-rs --diff -e 's/old/new/' file
///
/// Add a line under the first matching header only:
///     sed-rs -i -e '0,/^## 2026-11-01$/s/^## 2026-11-01$/&\n\n- new entry/' CHANGES.md
///
/// ## Commands
///
///     s/re/repl/flags   substitute
///     d                 delete the pattern space, start the next cycle
///     N                 append a newline and the next line (last line: print, stop)
///     { ... }           group commands under one address
///     :label  b label   label and branch (b alone: to the end of the script)
///
/// Separate commands with ; or newlines. Multiple -e expressions are joined
/// with newlines, so a block or label may span several -e arguments.
///
/// ## Addresses
///
///     N  line number     $  last line     /re/  regex     add ! to negate
///     A,B  range from A to B (B first tested on the line after A)
///     0,/re/  from the start to the first line matching re (first match only)
///
/// ## Substitution Syntax
///
///     s/pattern/replacement/[flags]
///
/// Any character can be used as the delimiter (e.g., s|pat|repl|g).
///
/// Flags: g (global), i or I (case-insensitive), L (literal: no regex).
/// Any other flag is an error.
///
/// Replacement tokens: & (whole match), \1-\9 (captures), \n (newline), \t (tab)
///
/// BRE escapes (\{n\}, \(\), \+, \?, \|) are auto-converted to ERE equivalents.
///
/// Literal (L or --fixed-strings): pattern and replacement are plain text;
/// only \n, \t, \\ and an escaped delimiter are escapes. & \1 . ( [ $ mean
/// themselves. Address regexes are still regexes.
///
/// ## Input and Output
///
/// Without FILEs, stdin is read. Several FILEs are read as one stream ($ is
/// the last line of the last file); - means stdin. Each line is printed
/// with \n (a \r\n ending becomes \n).
///
/// With -i, each FILE is a separate stream and is rewritten keeping its
/// line endings and its permissions. All files are processed first; if any
/// step fails (or --strict/--expect fails), no file is changed. A file the
/// script doesn't change is not rewritten. Symlinks are refused.
///
/// ## Exit Codes
///
///   0  Success
///   1  Error (invalid expression, I/O error, --strict or --expect failed)
#[derive(Parser, Debug)]
#[command(name = "sed-rs")]
#[command(disable_version_flag = true)]
#[command(verbatim_doc_comment)]
struct Cli {
    /// Show version information
    #[arg(short = 'V', long)]
    version: bool,

    /// Script expression(s) — each may contain `;`-separated commands
    #[arg(short, long = "expression")]
    e: Vec<String>,

    /// Edit FILEs in place (-i, --in-place); with SUFFIX keep a backup
    /// (-i.orig or --in-place=.orig; the suffix must be attached)
    #[arg(long = "in-place", value_name = "SUFFIX", num_args = 0..=1,
          require_equals = true, default_missing_value = "")]
    in_place: Option<String>,

    /// Make every `s` literal, as if it had the L flag (addresses stay regexes)
    #[arg(short = 'F', long)]
    fixed_strings: bool,

    /// Fail (exit 1, nothing written with -i) if any `s` made no replacement
    #[arg(long)]
    strict: bool,

    /// Fail (exit 1, nothing written with -i) unless exactly N replacements were made in total
    #[arg(long, value_name = "N")]
    expect: Option<usize>,

    /// Print a unified diff of the change; without -i, a dry run that writes nothing
    #[arg(long)]
    diff: bool,

    /// Input files (default: stdin)
    files: Vec<PathBuf>,
}

fn main() {
    let args = match rewrite_short_in_place(std::env::args_os().collect()) {
        Ok(a) => a,
        Err(e) => fail(&e),
    };
    let cli = Cli::parse_from(args);

    if cli.version {
        version::print();
        return;
    }

    if cli.e.is_empty() {
        fail("no expressions provided (use -e)");
    }

    let exprs: Vec<&str> = cli.e.iter().map(|s| s.as_str()).collect();
    let options = Options::new().fixed_strings(cli.fixed_strings);
    let sed = match Sed::parse_with(&exprs, &options) {
        Ok(s) => s,
        Err(e) => fail(&e.to_string()),
    };

    let result = if let Some(suffix) = &cli.in_place {
        edit_in_place(&sed, &cli, suffix)
    } else if cli.diff {
        dry_run_diff(&sed, &cli)
    } else {
        stream(&sed, &cli)
    };
    if let Err(e) = result {
        fail(&e);
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("sed-rs: {msg}");
    std::process::exit(1);
}

/// clap can't express GNU's `-i[SUFFIX]` (an optional value that must be
/// attached), so turn `-i` into `--in-place` and `-iSUFFIX` into
/// `--in-place=SUFFIX` before parsing. Values of `-e` are left alone.
fn rewrite_short_in_place(args: Vec<std::ffi::OsString>) -> Result<Vec<std::ffi::OsString>, String> {
    let mut out = Vec::with_capacity(args.len());
    let mut iter = args.into_iter();
    out.extend(iter.next()); // program name
    while let Some(arg) = iter.next() {
        let Some(s) = arg.to_str() else {
            out.push(arg);
            continue;
        };
        if s == "--" {
            out.push(arg);
            out.extend(iter);
            break;
        }
        if s == "-e" || s == "--expression" {
            out.push(arg);
            out.extend(iter.next());
            continue;
        }
        if s.is_empty() {
            return Err("empty file name (BSD `-i ''` is not supported: use -i, or -i.bak for a backup)".into());
        }
        if let Some(suffix) = s.strip_prefix("-i") {
            if suffix.is_empty() {
                out.push("--in-place".into());
                continue;
            }
            if suffix == "e" || suffix.starts_with('-') {
                return Err(format!(
                    "`{s}` would mean backup suffix `{suffix}`; write `-i -e` (or -i.bak for a backup)"
                ));
            }
            out.push(format!("--in-place={suffix}").into());
            continue;
        }
        out.push(arg);
    }
    Ok(out)
}

/// Check --strict and --expect against the stats of the whole run.
fn check(stats: &Stats, cli: &Cli) -> Result<(), String> {
    let mut problems = Vec::new();
    if cli.strict {
        for s in stats.unmatched() {
            problems.push(format!(
                "--strict: `{}` (-e {}) made no replacement",
                s.source, s.expression
            ));
        }
    }
    if let Some(n) = cli.expect {
        let got = stats.total_replacements();
        if got != n {
            problems.push(format!("--expect={n}: made {got} replacement(s)"));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\nsed-rs: "))
    }
}

fn ignore_broken_pipe(r: io::Result<()>) -> Result<(), String> {
    match r {
        Err(e) if e.kind() != io::ErrorKind::BrokenPipe => Err(e.to_string()),
        _ => Ok(()),
    }
}

/// The original mode: stdin (or FILEs as one stream) to stdout.
fn stream(sed: &Sed, cli: &Cli) -> Result<(), String> {
    let stdout = io::stdout().lock();
    let stats = if cli.files.is_empty() {
        let stdin = io::stdin().lock();
        sed.process_with_stats(BufReader::new(stdin), stdout)
    } else {
        // Read every file first, so a missing one fails before any output.
        let mut all = Vec::new();
        for f in &cli.files {
            let data = read_input(f)?;
            if !all.is_empty() && !all.ends_with(b"\n") {
                all.push(b'\n'); // keep the files' last and first lines apart
            }
            all.extend_from_slice(&data);
        }
        sed.process_with_stats(&all[..], stdout)
    };
    match stats {
        Ok(stats) => check(&stats, cli),
        Err(e) => ignore_broken_pipe(Err(e)),
    }
}

fn read_input(f: &Path) -> Result<Vec<u8>, String> {
    if f.as_os_str() == "-" {
        let mut data = Vec::new();
        io::Read::read_to_end(&mut io::stdin().lock(), &mut data).map_err(|e| e.to_string())?;
        Ok(data)
    } else {
        fs::read(f).map_err(|e| format!("can't read {}: {e}", f.display()))
    }
}

/// Run the script over one input, keeping line endings.
fn transform(sed: &Sed, label: &str, old: &[u8]) -> Result<(Vec<u8>, Stats), String> {
    let mut new = Vec::new();
    let stats = sed
        .process_preserving(old, &mut new)
        .map_err(|e| format!("{label}: {e}"))?;
    Ok((new, stats))
}

fn print_diff(label: &str, old: &[u8], new: &[u8]) -> Result<(), String> {
    if old == new {
        return Ok(());
    }
    let old = String::from_utf8_lossy(old);
    let new = String::from_utf8_lossy(new);
    let diff = similar::TextDiff::from_lines(old.as_ref(), new.as_ref());
    let text = diff
        .unified_diff()
        .context_radius(3)
        .header(label, label)
        .to_string();
    ignore_broken_pipe(io::stdout().lock().write_all(text.as_bytes()))
}

/// --diff without -i: show what -i would do, write nothing.
fn dry_run_diff(sed: &Sed, cli: &Cli) -> Result<(), String> {
    let inputs: Vec<PathBuf> = if cli.files.is_empty() {
        vec![PathBuf::from("-")]
    } else {
        cli.files.clone()
    };
    let mut total = Stats::default();
    let mut results = Vec::new();
    for f in &inputs {
        let label = f.display().to_string();
        let old = read_input(f)?;
        let (new, stats) = transform(sed, &label, &old)?;
        total.accumulate(&stats);
        results.push((label, old, new));
    }
    for (label, old, new) in &results {
        print_diff(label, old, new)?;
    }
    check(&total, cli)
}

struct Edit {
    path: PathBuf,
    old: Vec<u8>,
    new: Vec<u8>,
    perms: fs::Permissions,
    temp: Option<PathBuf>,
}

fn edit_in_place(sed: &Sed, cli: &Cli, suffix: &str) -> Result<(), String> {
    if cli.files.is_empty() {
        return Err("-i needs at least one FILE".into());
    }
    if suffix.contains('/') || suffix.contains('*') {
        return Err(format!("backup suffix `{suffix}` with / or * is not supported"));
    }

    // Phase 1: check every file and compute every result. Nothing is
    // written until all of them succeed.
    let mut seen = HashSet::new();
    let mut edits = Vec::new();
    let mut total = Stats::default();
    for path in &cli.files {
        let shown = path.display();
        if path.as_os_str().is_empty() {
            return Err("empty file name (BSD `-i ''` is not supported: use -i, or -i.bak for a backup)".into());
        }
        if path.as_os_str() == "-" {
            return Err("can't edit stdin in place".into());
        }
        let meta = fs::symlink_metadata(path).map_err(|e| format!("can't read {shown}: {e}"))?;
        if meta.file_type().is_symlink() {
            return Err(format!("{shown} is a symbolic link; not editing it in place"));
        }
        if !meta.is_file() {
            return Err(format!("{shown} is not a regular file"));
        }
        let canon = fs::canonicalize(path).map_err(|e| format!("{shown}: {e}"))?;
        if !seen.insert(canon) {
            return Err(format!("{shown} is listed more than once"));
        }
        let old = fs::read(path).map_err(|e| format!("can't read {shown}: {e}"))?;
        let (new, stats) = transform(sed, &shown.to_string(), &old)?;
        total.accumulate(&stats);
        edits.push(Edit {
            path: path.clone(),
            old,
            new,
            perms: meta.permissions(),
            temp: None,
        });
    }
    check(&total, cli)?;

    // Phase 2: write each changed result to a temporary file beside it.
    for (i, edit) in edits.iter_mut().enumerate() {
        if edit.old == edit.new {
            continue;
        }
        match write_temp(edit, i) {
            Ok(temp) => edit.temp = Some(temp),
            Err(e) => {
                remove_temps(&edits);
                return Err(e);
            }
        }
    }

    // Phase 3: backups (of every file, changed or not), then renames.
    if !suffix.is_empty() {
        for edit in &edits {
            let mut backup = edit.path.clone().into_os_string();
            backup.push(suffix);
            if let Err(e) = fs::copy(&edit.path, &backup) {
                remove_temps(&edits);
                return Err(format!("can't back up {}: {e}", edit.path.display()));
            }
        }
    }
    let mut done = Vec::new();
    for edit in &edits {
        if let Some(temp) = &edit.temp {
            if let Err(e) = fs::rename(temp, &edit.path) {
                remove_temps(&edits);
                let done = if done.is_empty() {
                    "no file was changed".to_string()
                } else {
                    format!("already changed: {}", done.join(", "))
                };
                return Err(format!("can't replace {}: {e} ({done})", edit.path.display()));
            }
            done.push(edit.path.display().to_string());
        }
    }

    if cli.diff {
        for edit in &edits {
            print_diff(&edit.path.display().to_string(), &edit.old, &edit.new)?;
        }
    }
    Ok(())
}

fn write_temp(edit: &Edit, i: usize) -> Result<PathBuf, String> {
    let shown = edit.path.display();
    let dir = match edit.path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let name = edit
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = dir.join(format!(".{name}.sed-rs-{}-{i}.tmp", std::process::id()));
    let write = || -> io::Result<()> {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        f.write_all(&edit.new)?;
        f.sync_all()?;
        fs::set_permissions(&temp, edit.perms.clone())
    };
    write().map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("can't write a temporary file for {shown}: {e}")
    })?;
    Ok(temp)
}

fn remove_temps(edits: &[Edit]) {
    for edit in edits {
        if let Some(temp) = &edit.temp {
            let _ = fs::remove_file(temp);
        }
    }
}
