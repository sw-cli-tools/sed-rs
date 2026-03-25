//! Version information.

const REPOSITORY: &str = "https://github.com/softwarewrighter/sw-cli-tools";
const LICENSE: &str = "MIT";
const COPYRIGHT: &str = "Copyright (c) 2026 Michael A Wright";

/// Print version information.
pub fn print() {
    println!(
        "{} {}\n{}\nLicense: {}\nRepository: {}\n\nBuild Information:\n  Host: {}\n  Commit: {}\n  Timestamp: {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        COPYRIGHT,
        LICENSE,
        REPOSITORY,
        env!("BUILD_HOST"),
        env!("GIT_HASH"),
        env!("BUILD_TIMESTAMP"),
    );
}
