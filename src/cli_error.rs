//! Runtime failures retain their causes and input context until CLI rendering.

use std::io;
use std::path::{Path, PathBuf};

use mikedown::Diagnostic;
use thiserror::Error;

/// Runtime failures retaining their source errors and affected paths.
#[derive(Debug, Error)]
pub enum CliError {
    // Aggregates retain every error rather than inventing a single source chain.
    /// All input failures collected before any output files are changed.
    #[error("{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))]
    Batch(Vec<Self>),
    /// A repeated canonical path in a write batch.
    #[error("{}: duplicate input file", .0.display())]
    DuplicateInput(PathBuf),
    /// Filesystem failure for a particular input or replacement.
    #[error("{}: {source}", path.display())]
    FileIo {
        /// Path associated with the failure.
        path: PathBuf,
        /// Original operating-system I/O error.
        #[source]
        source: io::Error,
    },
    /// Source-located document validation failures.
    #[error("{}", diagnostics.iter().map(|diagnostic| format!("{}:{diagnostic}", path.display())).collect::<Vec<_>>().join("\n"))]
    Format {
        /// Path associated with the failure.
        path: PathBuf,
        /// All formatting diagnostics retained for this input.
        diagnostics: Vec<Diagnostic>,
    },
    /// Write target that is not an ordinary regular file.
    #[error("{}: --write requires a regular file, not a symlink", .0.display())]
    InvalidWriteTarget(PathBuf),
    /// Failure reading UTF-8 text from standard input.
    #[error("stdin: {0}")]
    Stdin(#[source] io::Error),
    /// Failure writing formatted text or help to standard output.
    #[error("stdout: {0}")]
    Stdout(#[source] io::Error),
    /// No staging filename was available after bounded retries.
    #[error("{}: could not allocate a temporary file", .0.display())]
    TemporaryFileExhausted(PathBuf),
}

impl CliError {
    /// Attach an input path to an underlying filesystem error.
    pub(crate) fn file(path: &Path, source: io::Error) -> Self {
        Self::FileIo {
            path: path.to_owned(),
            source,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::unreachable,
    reason = "Tests inspect known malformed input and explicitly constructed error variants"
)]
mod tests {
    use core::error::Error;
    use std::io;
    use std::path::{Path, PathBuf};

    use super::CliError;

    #[test]
    fn io_errors_retain_their_original_causes_and_context() {
        let errors = [
            (
                CliError::file(
                    Path::new("notes.mkd"),
                    io::Error::from(io::ErrorKind::NotFound),
                ),
                "notes.mkd:",
            ),
            (
                CliError::Stdin(io::Error::from(io::ErrorKind::InvalidData)),
                "stdin:",
            ),
            (
                CliError::Stdout(io::Error::from(io::ErrorKind::BrokenPipe)),
                "stdout:",
            ),
        ];
        for (error, prefix) in errors {
            assert!(error.to_string().starts_with(prefix));
            assert!(error.source().is_some_and(<dyn Error>::is::<io::Error>));
        }
        let error = CliError::file(
            Path::new("notes.mkd"),
            io::Error::from(io::ErrorKind::NotFound),
        );
        assert!(matches!(&error, CliError::FileIo { path, source }
            if path == Path::new("notes.mkd") && source.kind() == io::ErrorKind::NotFound));
    }

    #[test]
    fn batch_errors_retain_structured_diagnostics_and_individual_causes() {
        let diagnostics = mikedown::format("@paragraph width=invalid\ntext").unwrap_err();
        let error = CliError::Batch(vec![
            CliError::Format {
                path: PathBuf::from("bad.mkd"),
                diagnostics: diagnostics.clone(),
            },
            CliError::file(
                Path::new("missing.mkd"),
                io::Error::from(io::ErrorKind::NotFound),
            ),
        ]);
        assert!(
            error
                .to_string()
                .starts_with("bad.mkd:1:12: invalid value 'invalid' for width\nmissing.mkd:")
        );
        assert!(error.source().is_none());
        let CliError::Batch(errors) = error else {
            unreachable!("constructed a batch error");
        };
        let [format_error, io_error] = errors.as_slice() else {
            unreachable!("constructed exactly two batch errors");
        };
        assert!(
            matches!(format_error, CliError::Format { path, diagnostics: retained }
            if path == Path::new("bad.mkd") && retained == &diagnostics)
        );
        assert!(io_error.source().is_some_and(<dyn Error>::is::<io::Error>));
    }

    #[test]
    fn write_failures_have_distinct_variants_and_messages() {
        let path = PathBuf::from("notes.mkd");
        for (error, message) in [
            (
                CliError::DuplicateInput(path.clone()),
                "notes.mkd: duplicate input file",
            ),
            (
                CliError::InvalidWriteTarget(path.clone()),
                "notes.mkd: --write requires a regular file, not a symlink",
            ),
            (
                CliError::TemporaryFileExhausted(path),
                "notes.mkd: could not allocate a temporary file",
            ),
        ] {
            assert_eq!(error.to_string(), message);
            assert!(error.source().is_none());
        }
    }
}
