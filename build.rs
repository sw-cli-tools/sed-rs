// Copyright (c) 2026 Michael A Wright
// Licensed under the MIT License

use std::path::Path;
use std::process::Command;

fn main() {
    let host = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_HOST={host}");

    let hash = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=GIT_HASH={hash}");

    // Re-run when the commit changes, not only when sources do. Listing any
    // rerun-if-changed replaces Cargo's default (any file in the package),
    // so src and Cargo.toml are listed too, to keep the timestamp current.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=src");
    // Only files that exist: Cargo treats a missing one as always changed.
    if let Some(git_dir) = git(&["rev-parse", "--git-dir"]) {
        let mut watch = vec![format!("{git_dir}/HEAD"), format!("{git_dir}/packed-refs")];
        if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
            watch.push(format!("{git_dir}/{branch}"));
        }
        for path in watch.iter().filter(|p| Path::new(p).exists()) {
            println!("cargo:rerun-if-changed={path}");
        }
    }

    let timestamp = chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S%z")
        .to_string();
    println!("cargo:rustc-env=BUILD_TIMESTAMP={timestamp}");
}

/// Run git and return its trimmed stdout, or None if git fails.
fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
