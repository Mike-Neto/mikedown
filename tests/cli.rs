#![allow(
    clippy::tests_outside_test_module,
    clippy::unwrap_used,
    clippy::panic,
    reason = "Integration assertions report invalid output and failed filesystem operations"
)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Workspace(PathBuf);

impl Workspace {
    #[expect(
        clippy::create_dir,
        reason = "Exclusive creation prevents reusing another test directory"
    )]
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "mikedown-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mikedown"))
}

fn run(arguments: &[&str], stdin: Option<&str>) -> Output {
    let mut child = command()
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(text) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    drop(child.stdin.take());
    child.wait_with_output().unwrap()
}

#[test]
fn stdin_and_stdout_are_a_formatter_pipeline() {
    for args in [&["fmt"][..], &["fmt", "-"][..]] {
        let output = run(args, Some("#    Heading\n\n@ol\n- a\n- b\n"));
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "# Heading\n\n@ol\n1. a\n2. b\n"
        );
        assert!(output.stderr.is_empty());
    }
    let workspace = Workspace::new();
    let path = workspace.file("note.mike", "#    Heading\n");
    let output = command().arg("fmt").arg(&path).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"# Heading\n");
    assert_eq!(fs::read_to_string(path).unwrap(), "#    Heading\n");
}

#[test]
fn check_and_write_have_correct_exit_codes() {
    let workspace = Workspace::new();
    let path = workspace.file("note.mike", "#    Heading\n");
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    let output = command()
        .args(["fmt", "--check"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("needs formatting")
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "#    Heading\n");
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
    let output = command()
        .args(["fmt", "--write"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), "# Heading\n");
    assert!(
        command()
            .args(["fmt", "--check"])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    assert!(
        command()
            .args(["fmt", "--write"])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    assert_eq!(fs::read_dir(&workspace.0).unwrap().count(), 1);
}

#[test]
fn whole_batch_validates_before_writes() {
    let workspace = Workspace::new();
    let good = workspace.file("good.mike", "#   Good\n");
    let bad = workspace.file("bad.mike", "@paragraph width=invalid\ntext\n");
    let output = command()
        .args(["fmt", "--write"])
        .args([&good, &bad])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read_to_string(&good).unwrap(), "#   Good\n");
    assert_eq!(
        fs::read_to_string(&bad).unwrap(),
        "@paragraph width=invalid\ntext\n"
    );
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("bad.mike:1:12: invalid value")
    );
    let missing = workspace.0.join("missing.mike");
    assert_eq!(
        command()
            .args(["fmt", "--write"])
            .args([&good, &missing])
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );
    assert_eq!(fs::read_to_string(good).unwrap(), "#   Good\n");
    assert_eq!(fs::read_dir(&workspace.0).unwrap().count(), 2);
}

#[test]
fn successful_batch_writes_all_inputs_and_preserves_permissions() {
    let workspace = Workspace::new();
    let first = workspace.file("first.mike", "#   First\n");
    let second = workspace.file("second.mike", "#   Second\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&first, fs::Permissions::from_mode(0o640)).unwrap();
    };
    let output = command()
        .args(["fmt", "--write"])
        .args([&first, &second])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&first).unwrap(), "# First\n");
    assert_eq!(fs::read_to_string(&second).unwrap(), "# Second\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(first).unwrap().permissions().mode() & 0o777,
            0o640
        );
    };
    assert_eq!(fs::read_dir(&workspace.0).unwrap().count(), 2);
}

#[test]
fn invalid_usage_and_invalid_utf8_do_not_write() {
    for args in [
        vec!["fmt", "--write"],
        vec!["fmt", "--check", "-"],
        vec!["fmt", "--write", "--check", "x"],
        vec!["fmt", "--unknown"],
        vec!["other"],
    ] {
        assert_eq!(run(&args, None).status.code(), Some(2), "{args:?}");
    }
    let workspace = Workspace::new();
    let path = workspace.file("bad.mike", "");
    fs::write(&path, [0xff, 0xfe]).unwrap();
    assert_eq!(
        command()
            .args(["fmt", "--write"])
            .arg(&path)
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );
    assert_eq!(fs::read(path).unwrap(), [0xff, 0xfe]);
    let path = workspace.file("-note.mike", "#    Hello\n");
    let output = command()
        .current_dir(&workspace.0)
        .args(["fmt", "--", "-note.mike"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"# Hello\n");
    assert!(Path::new(&path).exists());
}

#[test]
fn duplicate_write_inputs_are_rejected_before_changes() {
    let workspace = Workspace::new();
    let path = workspace.file("note.mike", "#   Hello\n");
    let output = command()
        .args(["fmt", "--write"])
        .args([&path, &path])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read_to_string(path).unwrap(), "#   Hello\n");
}

#[cfg(unix)]
#[test]
fn symlink_write_is_rejected_before_other_files_change() {
    let workspace = Workspace::new();
    let original = workspace.file("original.mike", "#   Hello\n");
    let other = workspace.file("other.mike", "#   Other\n");
    let link = workspace.0.join("linked.mike");
    std::os::unix::fs::symlink(&original, &link).unwrap();
    let output = command()
        .args(["fmt", "--write"])
        .args([&other, &link])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read_to_string(&original).unwrap(), "#   Hello\n");
    assert_eq!(fs::read_to_string(&other).unwrap(), "#   Other\n");
    assert!(
        command()
            .arg("fmt")
            .arg(link)
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn help_version_and_error_output() {
    assert!(run(&["--help"], None).status.success());
    assert!(
        run(&["--version"], None)
            .stdout
            .starts_with(b"mikedown 0.1.0")
    );
    let output = run(&["fmt"], Some("@ol\ntext"));
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr).unwrap().contains("-:1:1:"));
}
