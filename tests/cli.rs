#![allow(
    clippy::tests_outside_test_module,
    clippy::unwrap_used,
    reason = "Integration tests use unwraps to report failed assertions and filesystem operations"
)]

use core::sync::atomic::{AtomicUsize, Ordering};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Output, Stdio};

struct Workspace(PathBuf);

impl Workspace {
    #[expect(
        clippy::create_dir,
        reason = "Exclusive creation prevents reusing another test directory"
    )]
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = env::temp_dir().join(format!(
            "mikedown-test-{}-{}",
            process::id(),
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
        #[expect(
            clippy::let_underscore_must_use,
            reason = "Test-directory cleanup is best effort and must not panic during destruction"
        )]
        let _: io::Result<()> = fs::remove_dir_all(&self.0);
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
        assert_eq!(output.stderr, b"");
    }
    let workspace = Workspace::new();
    let path = workspace.file("note.mkd", "#    Heading\n");
    let output = command().arg("fmt").arg(&path).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"# Heading\n");
    assert_eq!(fs::read_to_string(path).unwrap(), "#    Heading\n");
}

#[test]
fn item_reference_errors_prevent_all_batch_writes() {
    let workspace = Workspace::new();
    let good_source = "#    Heading\n";
    let bad_source = "See [](item:missing).\n";
    let good = workspace.file("good.mkd", good_source);
    let bad = workspace.file("bad.mkd", bad_source);
    let output = command()
        .args(["fmt", "--write"])
        .arg(&good)
        .arg(&bad)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(fs::read_to_string(good).unwrap(), good_source);
    assert_eq!(fs::read_to_string(bad).unwrap(), bad_source);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("bad.mkd"));
    assert!(stderr.contains("line: 1, column: 13"));
    assert!(stderr.contains("unknown item reference 'missing'"));
}

#[test]
fn check_and_write_have_correct_exit_codes() {
    let workspace = Workspace::new();
    let path = workspace.file("note.mkd", "#    Heading\n");
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    let output = command()
        .args(["fmt", "--check"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"");
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
    let good = workspace.file("good.mkd", "#   Good\n");
    let bad = workspace.file("bad.mkd", "@paragraph width=invalid\ntext\n");
    let output = command()
        .args(["fmt", "--write"])
        .args([&good, &bad])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(fs::read_to_string(&good).unwrap(), "#   Good\n");
    assert_eq!(
        fs::read_to_string(&bad).unwrap(),
        "@paragraph width=invalid\ntext\n"
    );
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("line: 1, column: 12, message: \"invalid value")
    );
    let missing = workspace.0.join("missing.mkd");
    assert_eq!(
        command()
            .args(["fmt", "--write"])
            .args([&good, &missing])
            .output()
            .unwrap()
            .status
            .code(),
        Some(1)
    );
    assert_eq!(fs::read_to_string(good).unwrap(), "#   Good\n");
    assert_eq!(fs::read_dir(&workspace.0).unwrap().count(), 2);
}

#[test]
fn successful_batch_writes_all_inputs_and_preserves_permissions() {
    let workspace = Workspace::new();
    let first = workspace.file("first.mkd", "#   First\n");
    let second = workspace.file("second.mkd", "#   Second\n");
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
    let path = workspace.file("bad.mkd", "");
    fs::write(&path, [0xff, 0xfe]).unwrap();
    assert_eq!(
        command()
            .args(["fmt", "--write"])
            .arg(&path)
            .output()
            .unwrap()
            .status
            .code(),
        Some(1)
    );
    assert_eq!(fs::read(path).unwrap(), [0xff, 0xfe]);
    let path = workspace.file("-note.mkd", "#    Hello\n");
    let output = command()
        .current_dir(&workspace.0)
        .args(["fmt", "--", "-note.mkd"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"# Hello\n");
    assert!(Path::new(&path).exists());
}

#[test]
fn duplicate_write_inputs_are_rejected_before_changes() {
    let workspace = Workspace::new();
    let path = workspace.file("note.mkd", "#   Hello\n");
    let output = command()
        .args(["fmt", "--write"])
        .args([&path, &path])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(fs::read_to_string(path).unwrap(), "#   Hello\n");
}

#[cfg(unix)]
#[test]
fn symlink_write_is_rejected_before_other_files_change() {
    use std::os::unix::fs::symlink;

    let workspace = Workspace::new();
    let original = workspace.file("original.mkd", "#   Hello\n");
    let other = workspace.file("other.mkd", "#   Other\n");
    let link = workspace.0.join("linked.mkd");
    symlink(&original, &link).unwrap();
    let output = command()
        .args(["fmt", "--write"])
        .args([&other, &link])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
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
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.starts_with("Error: Batch([Format {"));
    assert!(stderr.contains("path: \"-\""));
    assert!(stderr.contains("line: 1, column: 1"));
    assert!(stderr.contains("ol annotation has an incompatible target block"));
}

#[test]
fn clap_help_and_version_use_stdout() {
    for arguments in [
        vec![],
        vec!["--help"],
        vec!["-h"],
        vec!["fmt", "--help"],
        vec!["fmt", "-h"],
    ] {
        let output = run(&arguments, None);
        assert_eq!(output.status.code(), Some(0), "{arguments:?}");
        assert_eq!(output.stderr, b"");
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("Usage:"));
        assert!(help.contains("fmt"));
        if arguments.first() == Some(&"fmt") {
            assert!(help.contains("--write"));
            assert!(help.contains("--check"));
            assert!(help.contains("stdin"));
        }
    }
    for version in ["--version", "-V"] {
        let output = run(&[version], None);
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stderr, b"");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("mikedown {}\n", env!("CARGO_PKG_VERSION"))
        );
    }
}

#[test]
fn clap_usage_errors_precede_io_and_use_stderr() {
    for arguments in [
        vec!["fmt", "one", "two"],
        vec!["fmt", "-", "-"],
        vec!["fmt", "--write"],
        vec!["fmt", "--check"],
        vec!["fmt", "--write", "-"],
        vec!["fmt", "--check", "named", "-"],
        vec!["fmt", "--write", "--check", "named"],
        vec!["fmt", "--check", "--write", "--check", "named"],
        vec!["fmt", "--unknown"],
        vec!["other"],
    ] {
        let output = run(&arguments, Some("#  Input\n"));
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert_eq!(output.stdout, b"");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("error:"), "{arguments:?}: {error}");
        assert!(error.contains("Usage:"), "{arguments:?}: {error}");
        assert!(!error.contains("No such file"), "{arguments:?}: {error}");
    }
}

#[test]
fn repeated_mode_flags_and_options_after_paths_are_accepted() {
    let workspace = Workspace::new();
    let path = workspace.file("note.mkd", "#   Hello\n");
    let output = command()
        .arg("fmt")
        .arg(&path)
        .args(["--check", "--check"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(fs::read_to_string(&path).unwrap(), "#   Hello\n");
    let output = command()
        .args(["fmt", "--write", "--write"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"");
    assert_eq!(output.stderr, b"");
    assert_eq!(fs::read_to_string(&path).unwrap(), "# Hello\n");
}

#[test]
fn double_dash_preserves_option_shaped_filenames() {
    let workspace = Workspace::new();
    let path = workspace.file("--check", "#   Hello\n");
    let output = command()
        .current_dir(&workspace.0)
        .args(["fmt", "--write", "--", "--check"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"");
    assert_eq!(output.stderr, b"");
    assert_eq!(fs::read_to_string(path).unwrap(), "# Hello\n");
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn non_utf8_paths_work_in_stdout_and_write_modes() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let workspace = Workspace::new();
    let path = workspace
        .0
        .join(OsString::from_vec(b"note-\xff.mkd".to_vec()));
    fs::write(&path, "#   Hello\n").unwrap();
    let output = command().arg("fmt").arg(&path).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"# Hello\n");
    assert_eq!(output.stderr, b"");
    let output = command()
        .args(["fmt", "--write"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(path).unwrap(), "# Hello\n");
}

#[cfg(unix)]
#[test]
fn non_utf8_path_arguments_reach_file_io() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    // macOS filesystems reject invalid UTF-8 filenames, so a missing path checks
    // argument acceptance without requiring the filesystem to store one.
    let workspace = Workspace::new();
    let path = workspace
        .0
        .join(OsString::from_vec(b"missing-\xff.mkd".to_vec()));
    for mode in [None, Some("--write"), Some("--check")] {
        let mut command = command();
        command.arg("fmt");
        if let Some(mode) = mode {
            command.arg(mode);
        }
        let output = command.arg(&path).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stdout, b"");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.starts_with("Error: Batch([FileIo {"), "{error}");
        assert!(error.contains("missing-"), "{error}");
        assert!(!error.contains("Usage:"), "{error}");
    }
}

#[test]
fn batch_errors_report_all_failures_without_writing_valid_inputs() {
    let workspace = Workspace::new();
    let good = workspace.file("good.mkd", "#   Good\n");
    let bad = workspace.file("bad.mkd", "@paragraph width=invalid\ntext\n");
    let missing = workspace.0.join("missing.mkd");
    let other_bad = workspace.file("other-bad.mkd", "```\nunclosed\n");
    let output = command()
        .args(["fmt", "--write"])
        .args([&good, &bad, &missing, &other_bad])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"");
    let stderr = String::from_utf8(output.stderr).unwrap();
    let bad_position = stderr.find("bad.mkd").unwrap();
    let missing_position = stderr.find("missing.mkd").unwrap();
    let other_position = stderr.find("other-bad.mkd").unwrap();
    assert!(stderr.contains("line: 1, column: 12"));
    assert!(stderr.contains("invalid value 'invalid' for width"));
    assert!(stderr.contains("FileIo {"));
    assert!(stderr.contains("line: 1, column: 1"));
    assert!(stderr.contains("unclosed code fence"));
    assert!(bad_position < missing_position && missing_position < other_position);
    assert!(stderr.starts_with("Error: Batch(["));
    assert_eq!(fs::read_to_string(good).unwrap(), "#   Good\n");
    assert_eq!(fs::read_dir(&workspace.0).unwrap().count(), 3);
}

#[test]
fn markdown_extension_supports_stdout_check_and_write() {
    let workspace = Workspace::new();
    let original = "#    Notes\n\n@table style=pipes\n| Item | Count |\n| Coffee | 2 |\n";
    let markdown = workspace.file("notes.md", original);
    let native = workspace.file("notes.mkd", original);
    let expected = mikedown::format(original).unwrap();
    for path in [&markdown, &native] {
        let output = command().arg("fmt").arg(path).output().unwrap();
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, expected.as_bytes());
        assert_eq!(output.stderr, b"");
        assert_eq!(fs::read_to_string(path).unwrap(), original);

        let output = command()
            .args(["fmt", "--check"])
            .arg(path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stdout, b"");
        assert_eq!(fs::read_to_string(path).unwrap(), original);

        let output = command()
            .args(["fmt", "--write"])
            .arg(path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, b"");
        assert_eq!(output.stderr, b"");
        assert_eq!(fs::read_to_string(path).unwrap(), expected);

        let output = command()
            .args(["fmt", "--check"])
            .arg(path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, b"");
        assert_eq!(output.stderr, b"");
    }
}

#[test]
fn unsupported_markdown_document_version_prevents_batch_writes() {
    let workspace = Workspace::new();
    let original = "@document mikedown=999.0.0\n\n#   Future\n";
    let future = workspace.file("future.md", original);
    let good = workspace.file("good.md", "#   Notes\n");
    let output = command()
        .args(["fmt", "--write"])
        .args([&good, &future])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.starts_with("Error: Batch([Format {"));
    assert!(stderr.contains("future.md"));
    assert!(stderr.contains("line: 1, column: 11"));
    assert!(stderr.contains("unsupported Mikedown version '999.0.0'; supported version is 0.1.0"));
    assert_eq!(fs::read_to_string(good).unwrap(), "#   Notes\n");
    assert_eq!(fs::read_to_string(future).unwrap(), original);
}

#[test]
fn markdown_grammar_is_shared_by_extensions_and_stdin() {
    let workspace = Workspace::new();
    let source = include_str!("fixtures/hard-breaks.input.md");
    let expected = include_str!("fixtures/hard-breaks.expected.md");
    for name in ["readme.md", "readme.mkd"] {
        let path = workspace.file(name, source);
        let output = run(&["fmt", path.to_str().unwrap()], None);
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    }
    let output = run(&["fmt"], Some(source));
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}
