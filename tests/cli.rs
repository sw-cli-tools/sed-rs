//! CLI tests for in-place editing, --strict, --expect, --fixed-strings and --diff.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("sed-rs-cli-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        Dir(d)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.0.join(name);
        fs::write(&p, bytes).unwrap();
        p
    }
    fn names(&self) -> Vec<String> {
        let mut v: Vec<_> = fs::read_dir(&self.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sed(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sed-rs"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

fn p(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn in_place_edits_and_keeps_line_endings_and_mode() {
    let d = Dir::new("basic");
    let a = d.file("a.txt", b"old\r\nkeep\r\nold");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&a, fs::Permissions::from_mode(0o751)).unwrap();
    }
    let o = sed(&["-i", "-e", "s/old/new/", p(&a)], b"");
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(o.stdout.is_empty());
    assert_eq!(fs::read(&a).unwrap(), b"new\r\nkeep\r\nnew");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&a).unwrap().permissions().mode() & 0o777, 0o751);
    }
    assert_eq!(d.names(), ["a.txt"], "no temporary file is left behind");
}

#[test]
fn in_place_with_backup_suffix() {
    let d = Dir::new("backup");
    let a = d.file("a", b"x\n");
    let b = d.file("b", b"y\n");
    for flag in ["-i.orig", "--in-place=.orig"] {
        fs::write(&a, b"x\n").unwrap();
        let o = sed(&[flag, "-e", "s/x/X/", p(&a), p(&b)], b"");
        assert!(o.status.success(), "{}", stderr(&o));
        assert_eq!(fs::read(&a).unwrap(), b"X\n");
        assert_eq!(fs::read(d.0.join("a.orig")).unwrap(), b"x\n");
        // Unchanged files are backed up too, so `mv f.orig f` always works.
        assert_eq!(fs::read(d.0.join("b.orig")).unwrap(), b"y\n");
    }
}

#[test]
fn bad_script_leaves_files_untouched() {
    let d = Dir::new("badscript");
    let a = d.file("a", b"x\n");
    let o = sed(&["-i", "-e", "s/x/y/p", p(&a)], b"");
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("unknown option to `s': p"));
    assert_eq!(fs::read(&a).unwrap(), b"x\n");
}

#[test]
fn error_in_one_file_changes_no_file() {
    let d = Dir::new("twophase");
    let a = d.file("a", b"x\n");
    let bad = d.file("bad", b"x\n\xff\n");
    let o = sed(&["-i", "-e", "s/x/y/", p(&a), p(&bad)], b"");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(fs::read(&a).unwrap(), b"x\n");
    let o = sed(&["-i", "-e", "s/x/y/", p(&a), p(&d.0.join("missing"))], b"");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(fs::read(&a).unwrap(), b"x\n");
    assert_eq!(d.names(), ["a", "bad"]);
}

#[test]
fn strict_fails_and_writes_nothing() {
    let d = Dir::new("strict");
    let a = d.file("a", b"old\n");
    let b = d.file("b", b"other\n");
    let o = sed(
        &["--strict", "-i", "-e", "s/old/new/", "-e", "s/nothere/x/", p(&a), p(&b)],
        b"",
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("--strict: `s/nothere/x/` (-e 2) made no replacement"),
        "{}",
        stderr(&o)
    );
    assert_eq!(fs::read(&a).unwrap(), b"old\n");

    // Counted over all files together: each s matched somewhere.
    let o = sed(
        &["--strict", "-i", "-e", "s/old/new/", "-e", "s/other/x/", p(&a), p(&b)],
        b"",
    );
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(fs::read(&a).unwrap(), b"new\n");
    assert_eq!(fs::read(&b).unwrap(), b"x\n");
}

#[test]
fn strict_in_stream_mode_still_prints() {
    let o = sed(&["--strict", "-e", "s/q/z/"], b"a\n");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(o.stdout, b"a\n");
    // Without --strict, an unmatched s is silent, as always.
    let o = sed(&["-e", "s/q/z/"], b"a\n");
    assert!(o.status.success());
    assert!(o.stderr.is_empty());
}

#[test]
fn expect_counts_all_replacements() {
    let d = Dir::new("expect");
    let a = d.file("a", b"x x\nx\n");
    let o = sed(&["--expect=1", "-i", "-e", "s/x/y/g", p(&a)], b"");
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("--expect=1: made 3 replacement(s)"), "{}", stderr(&o));
    assert_eq!(fs::read(&a).unwrap(), b"x x\nx\n");
    let o = sed(&["--expect", "3", "-i", "-e", "s/x/y/g", p(&a)], b"");
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(fs::read(&a).unwrap(), b"y y\ny\n");
}

#[test]
fn fixed_strings_replace_exact_code() {
    let d = Dir::new("fixed");
    let src = b": help\n  .\" F6 (fn+F6)\" cr ;\n";
    let a = d.file("help.f", src);
    let o = sed(
        &["-F", "--expect=1", "-i", "-e", r#"s/  ." F6 (fn+F6)" cr/  ." Cmd-Enter or F6" cr/"#, p(&a)],
        b"",
    );
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(fs::read(&a).unwrap(), b": help\n  .\" Cmd-Enter or F6\" cr ;\n");
    let o = sed(&["--fixed-strings", "-e", "s/a+b/[&]/"], b"a+b aab\n");
    assert_eq!(o.stdout, b"[&] aab\n");
}

#[test]
fn diff_without_in_place_is_a_dry_run() {
    let d = Dir::new("dryrun");
    let a = d.file("a", b"1\nold\n3\n");
    let o = sed(&["--diff", "-e", "s/old/new/", p(&a)], b"");
    assert!(o.status.success(), "{}", stderr(&o));
    let out = String::from_utf8(o.stdout).unwrap();
    assert!(out.contains("-old\n+new\n"), "{out}");
    assert!(out.starts_with(&format!("--- {}\n+++ {}\n", p(&a), p(&a))), "{out}");
    assert_eq!(fs::read(&a).unwrap(), b"1\nold\n3\n");

    let o = sed(&["--diff", "-e", "s/old/new/"], b"old\n");
    let out = String::from_utf8(o.stdout).unwrap();
    assert!(out.starts_with("--- -\n+++ -\n"), "{out}");
    // No change, no diff.
    let o = sed(&["--diff", "-e", "s/q/z/"], b"old\n");
    assert!(o.status.success());
    assert!(o.stdout.is_empty());
}

#[test]
fn diff_with_in_place_writes_and_shows() {
    let d = Dir::new("diffi");
    let a = d.file("a", b"old\n");
    let o = sed(&["-i", "--diff", "-e", "s/old/new/", p(&a)], b"");
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(String::from_utf8(o.stdout).unwrap().contains("-old\n+new\n"));
    assert_eq!(fs::read(&a).unwrap(), b"new\n");
}

#[test]
fn unchanged_file_is_not_rewritten() {
    let d = Dir::new("unchanged");
    let a = d.file("a", b"keep\n");
    let before = fs::metadata(&a).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let o = sed(&["-i", "-e", "s/q/z/", p(&a)], b"");
    assert!(o.status.success());
    assert_eq!(fs::metadata(&a).unwrap().modified().unwrap(), before);
}

#[test]
fn refuses_unsafe_in_place_requests() {
    let d = Dir::new("refuse");
    let a = d.file("a", b"x\n");
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (vec!["-ie", "s/x/y/", p(&a)], "write `-i -e`"),
        (vec!["-i", "", "-e", "s/x/y/", p(&a)], "BSD `-i ''` is not supported"),
        (vec!["-i", "-e", "s/x/y/"], "-i needs at least one FILE"),
        (vec!["-i", "-e", "s/x/y/", "-"], "can't edit stdin in place"),
        (vec!["-i", "-e", "s/x/y/", p(&a), p(&a)], "listed more than once"),
        (vec!["-i", "-e", "s/x/y/", p(&d.0)], "is not a regular file"),
        (vec!["-ibak/x", "-e", "s/x/y/", p(&a)], "is not supported"),
    ];
    for (args, want) in cases {
        let o = sed(&args, b"");
        assert_eq!(o.status.code(), Some(1), "{args:?}");
        assert!(stderr(&o).contains(want), "{args:?}: {}", stderr(&o));
    }
    #[cfg(unix)]
    {
        let link = d.0.join("link");
        std::os::unix::fs::symlink(&a, &link).unwrap();
        let o = sed(&["-i", "-e", "s/x/y/", p(&link)], b"");
        assert!(stderr(&o).contains("is a symbolic link"), "{}", stderr(&o));
    }
    assert_eq!(fs::read(&a).unwrap(), b"x\n");
}

#[test]
fn files_without_in_place_are_one_stream() {
    let d = Dir::new("stream");
    let a = d.file("a", b"1\n2");
    let b = d.file("b", b"3\n4\n");
    let o = sed(&["-e", "$s/$/ last/; 2s/$/ two/", p(&a), p(&b)], b"");
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(o.stdout, b"1\n2 two\n3\n4 last\n");
    let o = sed(&["-e", "s/^/>/", p(&a), "-"], b"s\n");
    assert_eq!(o.stdout, b">1\n>2\n>s\n");
}

#[test]
fn in_place_ranges_restart_per_file() {
    let d = Dir::new("ranges");
    let a = d.file("a", b"## d\n## d\n");
    let b = d.file("b", b"## d\n");
    let o = sed(&["-i", "-e", r"0,/^## d$/s/$/ (first)/", p(&a), p(&b)], b"");
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(fs::read(&a).unwrap(), b"## d (first)\n## d\n");
    assert_eq!(fs::read(&b).unwrap(), b"## d (first)\n");
}
