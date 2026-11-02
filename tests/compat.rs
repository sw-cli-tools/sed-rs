//! Replays the golden cases in tests/compat/cases against the built binary.
//!
//! The cases were recorded from v0.2.0 by tests/compat/record.sh. Every one
//! must still give the same stdout bytes and exit status. A case whose
//! behaviour was changed on purpose holds a `changed` file naming the
//! CHANGELOG entry, the new expected `stdout` and `status`, and the v0.2.0
//! originals as `stdout.v0.2.0` and `status.v0.2.0`.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
fn golden_cases_match_v0_2_0() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compat/cases");
    let mut dirs: Vec<_> = fs::read_dir(&root)
        .expect("tests/compat/cases exists")
        .map(|e| e.unwrap().path())
        .collect();
    dirs.sort();
    assert!(dirs.len() >= 60, "golden cases are missing");

    let mut failures = Vec::new();
    for dir in &dirs {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let args_raw = fs::read(dir.join("args")).unwrap();
        let args: Vec<String> = args_raw
            .split(|b| *b == 0)
            .filter(|a| !a.is_empty())
            .map(|a| String::from_utf8(a.to_vec()).unwrap())
            .collect();
        let stdin = fs::read(dir.join("stdin")).unwrap();
        let want_out = fs::read(dir.join("stdout")).unwrap();
        let want_status: i32 = fs::read_to_string(dir.join("status"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        if dir.join("changed").exists() {
            assert!(
                dir.join("stdout.v0.2.0").exists(),
                "{name}: a changed case keeps its v0.2.0 output"
            );
        }

        let mut child = Command::new(env!("CARGO_BIN_EXE_sed-rs"))
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&stdin).unwrap();
        let out = child.wait_with_output().unwrap();
        let status = out.status.code().unwrap_or(-1);
        if out.stdout != want_out || status != want_status {
            failures.push(format!(
                "{name}: args {args:?}\n  want {:?} (exit {want_status})\n  got  {:?} (exit {status})",
                String::from_utf8_lossy(&want_out),
                String::from_utf8_lossy(&out.stdout),
            ));
        }
        // A successful run of an old script never writes to stderr.
        if want_status == 0 && !out.stderr.is_empty() {
            failures.push(format!(
                "{name}: unexpected stderr {:?}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
