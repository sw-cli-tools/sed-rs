//! sed-rs - A portable subset of sed.

mod version;

use clap::Parser;
use sed_rs::Sed;
use std::io::{self, BufReader};

/// A portable subset of sed for consistent cross-platform behavior.
///
/// Applies sed-style substitution commands to stdin and writes results
/// to stdout. Uses Rust's regex engine for identical behavior on all
/// platforms, with automatic BRE-to-ERE conversion.
///
/// # AI Coding Agent Instructions
///
/// This tool replaces system `sed` in contexts where consistent
/// cross-platform behavior is required — particularly for reg-rs
/// preprocessing (-P) of nondeterministic output.
///
/// ## Usage Patterns
///
/// Strip timestamps from command output:
///     my-command | sed-rs -e 's/[0-9]{4}-[0-9]{2}-[0-9]{2}/DATE/g'
///
/// Normalize temp directory paths (handles macOS and Linux):
///     my-command | sed-rs -e 's|/private/var/[^ ]*|<TMPDIR>|g; s|/tmp/[^ ]*|<TMPDIR>|g'
///
/// Normalize PIDs in file paths:
///     my-command | sed-rs -e 's|/tmp/my-tool-[0-9]*|/tmp/my-tool-PID|g'
///
/// Multiple independent substitutions:
///     my-command | sed-rs -e 's/[0-9]{4}-[0-9]{2}-[0-9]{2}/DATE/g' -e 's/pid=[0-9]+/pid=PID/g'
///
/// Use as a reg-rs preprocessor:
///     reg-rs create my-test -c 'my-command' -P "sed-rs -e 's/[0-9]{4}-[0-9]{2}-[0-9]{2}/DATE/g'"
///
/// ## Substitution Syntax
///
///     s/pattern/replacement/[flags]
///
/// Any character can be used as the delimiter (e.g., s|pat|repl|g).
///
/// Flags: g (global), i (case-insensitive)
///
/// Replacement tokens: & (whole match), \1-\9 (captures), \n (newline), \t (tab)
///
/// BRE escapes (\{n\}, \(\), \+, \?, \|) are auto-converted to ERE equivalents.
///
/// ## Output Format
///
/// stdin lines are read one at a time, each line has all substitutions applied
/// in order, and the result is written to stdout.
///
/// ## Exit Codes
///
///   0  Success
///   1  Error (invalid expression, I/O error)
#[derive(Parser, Debug)]
#[command(name = "sed-rs")]
#[command(disable_version_flag = true)]
#[command(verbatim_doc_comment)]
struct Cli {
    /// Show version information
    #[arg(short = 'V', long)]
    version: bool,

    /// Substitution expression(s) — each may contain `;`-separated commands
    #[arg(short, long = "expression")]
    e: Vec<String>,
}

fn main() {
    let cli = Cli::parse();

    if cli.version {
        version::print();
        return;
    }

    if cli.e.is_empty() {
        eprintln!("sed-rs: no expressions provided (use -e)");
        std::process::exit(1);
    }

    let exprs: Vec<&str> = cli.e.iter().map(|s| s.as_str()).collect();
    let sed = match Sed::parse(&exprs) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("sed-rs: {e}");
            std::process::exit(1);
        }
    };

    let stdin = io::stdin().lock();
    let stdout = io::stdout().lock();
    if let Err(e) = sed.process(BufReader::new(stdin), stdout)
        && e.kind() != io::ErrorKind::BrokenPipe
    {
        eprintln!("sed-rs: {e}");
        std::process::exit(1);
    }
}
