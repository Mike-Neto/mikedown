extern crate alloc;

mod cli_error;

use clap::{Args, CommandFactory, Parser, Subcommand, error::ErrorKind};

use cli_error::CliError;

use alloc::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{self, ExitCode};

#[derive(Parser)]
#[command(name = "mikedown", version, about = "Format plain-text documents")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Format a file or stdin.
    Fmt(FormatArgs),
}

#[derive(Args)]
#[command(args_override_self = true)]
struct FormatArgs {
    /// Validate all inputs, then atomically replace changed files.
    #[arg(long, conflicts_with = "check")]
    write: bool,
    /// Report changes without writing (exit 1); runtime errors exit 1, usage errors exit 2.
    #[arg(long)]
    check: bool,
    /// Input paths; omit or use - for stdin in stdout mode.
    #[arg(value_name = "FILE")]
    paths: Vec<PathBuf>,
}

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

#[expect(
    clippy::print_stdout,
    reason = "The CLI prints a trailing newline after its help output"
)]
fn arguments() -> Result<Option<(Mode, Vec<PathBuf>)>, CliError> {
    let cli = Cli::parse();
    let Some(Command::Fmt(arguments)) = cli.command else {
        Cli::command().print_help().map_err(CliError::Stdout)?;
        println!();
        return Ok(None);
    };
    let mode = if arguments.write {
        Mode::Write
    } else if arguments.check {
        Mode::Check
    } else {
        Mode::Stdout
    };
    let mut paths = arguments.paths;
    let invalid = if mode == Mode::Stdout && paths.len() > 1 {
        Some("stdout mode accepts only one file")
    } else if mode != Mode::Stdout
        && (paths.is_empty() || paths.iter().any(|p| p == Path::new("-")))
    {
        Some("--write and --check require file paths, not stdin")
    } else {
        None
    };
    if let Some(message) = invalid {
        FormatArgs::augment_args(clap::Command::new("fmt").bin_name("mikedown fmt"))
            .error(ErrorKind::ValueValidation, message)
            .exit();
    }
    if paths.is_empty() {
        paths.push(PathBuf::from("-"));
    }
    Ok(Some((mode, paths)))
}

#[expect(
    clippy::print_stderr,
    reason = "Check mode reports files needing formatting on stderr"
)]
fn run() -> Result<u8, CliError> {
    let Some((mode, paths)) = arguments()? else {
        return Ok(0);
    };
    // Preflight the whole batch, including permissions, before any replacement.
    let mut inputs = Vec::new();
    let mut failures = Vec::new();
    let mut seen = BTreeSet::new();
    for path in paths {
        match read_input(&path, mode) {
            Ok(input) => {
                if mode == Mode::Write {
                    let canonical =
                        fs::canonicalize(&path).map_err(|source| CliError::file(&path, source))?;
                    if !seen.insert(canonical) {
                        return Err(CliError::DuplicateInput(path));
                    }
                }
                inputs.push(input);
            }
            Err(error) => failures.push(error),
        }
    }
    if !failures.is_empty() {
        return Err(CliError::Batch(failures));
    }
    match mode {
        Mode::Stdout => {
            if let Some(input) = inputs.first() {
                io::stdout()
                    .lock()
                    .write_all(input.formatted.as_bytes())
                    .map_err(CliError::Stdout)?;
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

fn read_input(path: &Path, mode: Mode) -> Result<Input, CliError> {
    let (original, permissions) = if path == Path::new("-") {
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .map_err(CliError::Stdin)?;
        (text, None)
    } else {
        let metadata = fs::symlink_metadata(path).map_err(|source| CliError::file(path, source))?;
        if mode == Mode::Write && (metadata.file_type().is_symlink() || !metadata.is_file()) {
            return Err(CliError::InvalidWriteTarget(path.to_owned()));
        }
        let text = fs::read_to_string(path).map_err(|source| CliError::file(path, source))?;
        (text, Some(metadata.permissions()))
    };
    let formatted = mikedown::format(&original).map_err(|diagnostics| CliError::Format {
        path: path.to_owned(),
        diagnostics,
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
    fn commit(&self) -> Result<(), CliError> {
        fs::rename(&self.temporary, &self.destination)
            .map_err(|source| CliError::file(&self.destination, source))?;
        Ok(())
    }
}

impl Drop for Replacement {
    fn drop(&mut self) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "Temporary-file cleanup is best effort and must not panic during destruction"
        )]
        let _: io::Result<()> = fs::remove_file(&self.temporary);
    }
}

fn stage(input: &Input) -> Result<Replacement, CliError> {
    let parent = input
        .path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut opened: Option<(PathBuf, File)> = None;
    for sequence in 0..1000 {
        let mut name = OsString::from(".mikedown-");
        name.push(process::id().to_string());
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
            Err(source) => return Err(CliError::file(&input.path, source)),
        }
    }
    let (temporary, mut file) =
        opened.ok_or_else(|| CliError::TemporaryFileExhausted(input.path.clone()))?;
    let replacement = Replacement {
        temporary,
        destination: input.path.clone(),
    };
    file.write_all(input.formatted.as_bytes())
        .map_err(|source| CliError::file(&input.path, source))?;
    if let Some(permissions) = &input.permissions {
        file.set_permissions(permissions.clone())
            .map_err(|source| CliError::file(&input.path, source))?;
    }
    file.sync_all()
        .map_err(|source| CliError::file(&input.path, source))?;
    drop(file);
    Ok(replacement)
}

fn main() -> Result<ExitCode, CliError> {
    run().map(ExitCode::from)
}
