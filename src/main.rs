#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "The CLI owns stdout output and stderr diagnostics"
)]

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HELP: &str = "Mikedown — format plain-text documents\n\nUsage:\n  mikedown fmt [FILE|-]\n  mikedown fmt --write FILE...\n  mikedown fmt --check FILE...\n\nWithout a file, fmt reads stdin and writes stdout.\n--write validates every input before atomically replacing changed files.\n--check reports changes without writing (exit 1); errors exit 2.\nUse -- before filenames beginning with '-'.\n";

#[derive(Clone, Copy, Eq, PartialEq)]
enum Mode {
    Stdout,
    Write,
    Check,
}

struct Input {
    path: PathBuf,
    original: String,
    formatted: String,
    permissions: Option<Permissions>,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("mikedown: {message}");
            ExitCode::from(2)
        }
    }
}

fn arguments() -> Result<Option<(Mode, Vec<PathBuf>)>, String> {
    let mut arguments = std::env::args_os().skip(1);
    let Some(command) = arguments.next() else {
        print!("{HELP}");
        return Ok(None);
    };
    if command == "--help" || command == "-h" {
        print!("{HELP}");
        return Ok(None);
    }
    if command == "--version" || command == "-V" {
        println!("mikedown {}", env!("CARGO_PKG_VERSION"));
        return Ok(None);
    }
    if command != "fmt" {
        return Err("expected 'fmt'; use --help for usage".into());
    }
    let mut mode = Mode::Stdout;
    let mut paths = Vec::new();
    let mut positional = false;
    for argument in arguments {
        if !positional && argument == "--" {
            positional = true;
            continue;
        }
        if !positional && (argument == "--help" || argument == "-h") {
            print!("{HELP}");
            return Ok(None);
        }
        if !positional && (argument == "--write" || argument == "--check") {
            let requested = if argument == "--write" {
                Mode::Write
            } else {
                Mode::Check
            };
            if mode != Mode::Stdout && mode != requested {
                return Err("--write and --check are mutually exclusive".into());
            }
            mode = requested;
        } else if !positional && argument != "-" && argument.to_string_lossy().starts_with('-') {
            return Err(format!(
                "unknown option '{}'; use -- before a filename beginning with '-'",
                argument.to_string_lossy()
            ));
        } else {
            paths.push(PathBuf::from(argument));
        }
    }
    if mode == Mode::Stdout && paths.len() > 1 {
        return Err("stdout mode accepts only one file".into());
    }
    if mode != Mode::Stdout && (paths.is_empty() || paths.iter().any(|p| p == Path::new("-"))) {
        return Err("--write and --check require file paths, not stdin".into());
    }
    if paths.is_empty() {
        paths.push(PathBuf::from("-"));
    }

    Ok(Some((mode, paths)))
}

fn run() -> Result<u8, String> {
    let Some((mode, paths)) = arguments()? else {
        return Ok(0);
    };
    // Preflight the whole batch, including permissions, before any replacement.
    let mut inputs = Vec::new();
    let mut failures = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for path in paths {
        match read_input(&path, mode) {
            Ok(input) => {
                if mode == Mode::Write {
                    let canonical =
                        fs::canonicalize(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                    if !seen.insert(canonical) {
                        return Err(format!("{}: duplicate input file", path.display()));
                    }
                }
                inputs.push(input);
            }
            Err(message) => failures.push(message),
        }
    }
    if !failures.is_empty() {
        return Err(failures.join("\n"));
    }
    match mode {
        Mode::Stdout => {
            if let Some(input) = inputs.first() {
                io::stdout()
                    .lock()
                    .write_all(input.formatted.as_bytes())
                    .map_err(|e| e.to_string())?;
            }
            Ok(0)
        }
        Mode::Check => {
            let mut changed = false;
            for input in inputs {
                if input.original != input.formatted {
                    eprintln!("{}: needs formatting", input.path.display());
                    changed = true;
                }
            }
            Ok(u8::from(changed))
        }
        Mode::Write => {
            let mut staged = Vec::new();
            for input in &inputs {
                if input.original != input.formatted {
                    staged.push(stage(input)?);
                }
            }
            for replacement in &staged {
                replacement.commit()?;
            }
            Ok(0)
        }
    }
}

fn read_input(path: &Path, mode: Mode) -> Result<Input, String> {
    let (original, permissions) = if path == Path::new("-") {
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .map_err(|e| format!("stdin: {e}"))?;
        (text, None)
    } else {
        let metadata =
            fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if mode == Mode::Write && (metadata.file_type().is_symlink() || !metadata.is_file()) {
            return Err(format!(
                "{}: --write requires a regular file, not a symlink",
                path.display()
            ));
        }
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        (text, Some(metadata.permissions()))
    };
    let formatted = mikedown::format(&original).map_err(|diagnostics| {
        diagnostics
            .iter()
            .map(|diagnostic| format!("{}:{diagnostic}", path.display()))
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    Ok(Input {
        path: path.to_owned(),
        original,
        formatted,
        permissions,
    })
}

struct Replacement {
    temporary: PathBuf,
    destination: PathBuf,
}

impl Replacement {
    fn commit(&self) -> Result<(), String> {
        fs::rename(&self.temporary, &self.destination)
            .map_err(|e| format!("{}: {e}", self.destination.display()))?;
        Ok(())
    }
}

impl Drop for Replacement {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.temporary);
    }
}

fn stage(input: &Input) -> Result<Replacement, String> {
    let parent = input
        .path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut opened: Option<(PathBuf, File)> = None;
    for sequence in 0..1000 {
        let mut name = OsString::from(".mikedown-");
        name.push(std::process::id().to_string());
        name.push(format!("-{sequence}.tmp"));
        let path = parent.join(name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        };
        match options.open(&path) {
            Ok(file) => {
                opened = Some((path, file));
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("{}: {e}", input.path.display())),
        }
    }
    let (temporary, mut file) = opened.ok_or_else(|| {
        format!(
            "{}: could not allocate a temporary file",
            input.path.display()
        )
    })?;
    let replacement = Replacement {
        temporary,
        destination: input.path.clone(),
    };
    file.write_all(input.formatted.as_bytes())
        .map_err(|e| format!("{}: {e}", input.path.display()))?;
    if let Some(permissions) = &input.permissions {
        file.set_permissions(permissions.clone())
            .map_err(|e| format!("{}: {e}", input.path.display()))?;
    }
    file.sync_all()
        .map_err(|e| format!("{}: {e}", input.path.display()))?;
    drop(file);
    Ok(replacement)
}
