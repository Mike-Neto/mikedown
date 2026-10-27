extern crate alloc;

mod cli_error;

use clap::{Args, CommandFactory as _, Parser, Subcommand, error::ErrorKind};

use cli_error::CliError;

use alloc::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{self, ExitCode};

/// Top-level command-line arguments.
#[derive(Parser)]
#[command(name = "mikedown", version, about = "Format plain-text documents")]
struct Cli {
    /// Requested subcommand, or none when displaying general help.
    #[command(subcommand)]
    command: Option<Command>,
}

/// Supported command-line operations.
#[derive(Subcommand)]
enum Command {
    /// Format a file or stdin.
    Fmt(FormatArgs),
}

/// Formatting inputs and mutually exclusive output modes.
#[derive(Args)]
#[command(args_override_self = true)]
struct FormatArgs {
    /// Report changes without writing (exit 1); runtime errors exit 1, usage errors exit 2.
    #[arg(long)]
    check: bool,
    /// Input paths; omit or use - for stdin in stdout mode.
    #[arg(value_name = "FILE")]
    paths: Vec<PathBuf>,
    /// Validate all inputs, then atomically replace changed files.
    #[arg(long, conflicts_with = "check")]
    write: bool,
}

/// Validated output behavior for the whole input batch.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Mode {
    /// Report inputs that would change without writing them.
    Check,
    /// Write one formatted input to standard output.
    Stdout,
    /// Stage and replace changed regular files.
    Write,
}

/// An input validated and formatted before any batch writes occur.
struct Input {
    /// Canonical output ready to emit or stage.
    formatted: String,
    /// Original UTF-8 input used to detect changes.
    original: String,
    /// Input path, or a dash for standard input.
    path: PathBuf,
    /// Original file permissions to preserve during replacement.
    permissions: Option<Permissions>,
}

/// A staged file removed on drop unless committed by rename.
struct Replacement {
    /// Original file path to replace atomically.
    destination: PathBuf,
    /// Exclusively created staging file beside the destination.
    temporary: PathBuf,
}

impl Replacement {
    /// Atomically rename the staged output over its destination.
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

#[expect(
    clippy::too_many_lines,
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "The CLI keeps single-use argument, validation, and atomic-write stages inline"
)]
fn main() -> Result<ExitCode, CliError> {
    // Parse and validate command-line arguments.
    let cli = Cli::parse();
    let Some(Command::Fmt(arguments)) = cli.command else {
        Cli::command().print_help().map_err(CliError::Stdout)?;
        println!();
        return Ok(ExitCode::SUCCESS);
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
        && (paths.is_empty() || paths.iter().any(|path| path == Path::new("-")))
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
    // Preflight the whole batch, including permissions, before any replacement.
    let mut inputs = Vec::new();
    let mut failures = Vec::new();
    let mut seen = BTreeSet::new();
    for path in paths {
        // Read and format this input before writing any files.
        let read_result: Result<Input, CliError> = 'read_input: {
            let input_path = path.as_path();

            let (original, permissions) = if input_path == Path::new("-") {
                let mut text = String::new();
                match io::stdin()
                    .read_to_string(&mut text)
                    .map_err(CliError::Stdin)
                {
                    Ok(value) => value,
                    Err(failure) => break 'read_input Err(failure),
                };
                (text, None)
            } else {
                let metadata = match fs::symlink_metadata(input_path)
                    .map_err(|source| CliError::file(input_path, source))
                {
                    Ok(value) => value,
                    Err(failure) => break 'read_input Err(failure),
                };
                if mode == Mode::Write && (metadata.file_type().is_symlink() || !metadata.is_file())
                {
                    break 'read_input Err(CliError::InvalidWriteTarget(input_path.to_owned()));
                }
                let text = match fs::read_to_string(input_path)
                    .map_err(|source| CliError::file(input_path, source))
                {
                    Ok(value) => value,
                    Err(failure) => break 'read_input Err(failure),
                };
                (text, Some(metadata.permissions()))
            };
            let formatted =
                match mikedown::format(&original).map_err(|diagnostics| CliError::Format {
                    path: input_path.to_owned(),
                    diagnostics,
                }) {
                    Ok(value) => value,
                    Err(failure) => break 'read_input Err(failure),
                };
            Ok(Input {
                path: input_path.to_owned(),
                original,
                formatted,
                permissions,
            })
        };
        match read_result {
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
            Ok(ExitCode::SUCCESS)
        }
        Mode::Check => {
            let mut changed = false;
            for input in inputs {
                if input.original != input.formatted {
                    eprintln!("{}: needs formatting", input.path.display());
                    changed = true;
                }
            }
            Ok(ExitCode::from(u8::from(changed)))
        }
        Mode::Write => {
            let mut staged = Vec::new();
            for input in &inputs {
                if input.original != input.formatted {
                    staged.push({
                        // Stage an atomic replacement beside its destination.
                        let parent = input
                            .path
                            .parent()
                            .filter(|path| !path.as_os_str().is_empty())
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
                                use std::os::unix::fs::OpenOptionsExt as _;
                                options.mode(0o600);
                            };
                            match options.open(&path) {
                                Ok(file) => {
                                    opened = Some((path, file));
                                    break;
                                }
                                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                                Err(source) => return Err(CliError::file(&input.path, source)),
                            }
                        }
                        let (temporary, mut file) = opened
                            .ok_or_else(|| CliError::TemporaryFileExhausted(input.path.clone()))?;
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
                        replacement
                    });
                }
            }
            for replacement in &staged {
                replacement.commit()?;
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}
