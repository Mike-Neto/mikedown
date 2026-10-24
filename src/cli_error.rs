//! Runtime failures retain their causes and input context until CLI rendering.

use std::io;
use std::path::{Path, PathBuf};

use mikedown::Diagnostic;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum CliError {
    #[error("{}: {source}", path.display())]
    FileIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("stdin: {0}")]
    Stdin(#[source] io::Error),
    #[error("stdout: {0}")]
    Stdout(#[source] io::Error),
    #[error("{}: duplicate input file", .0.display())]
    DuplicateInput(PathBuf),
    #[error("{}: --write requires a regular file, not a symlink", .0.display())]
    InvalidWriteTarget(PathBuf),
    #[error("{}: could not allocate a temporary file", .0.display())]
    TemporaryFileExhausted(PathBuf),
    #[error("{}", render_diagnostics(.path, .diagnostics))]
    Format {
        path: PathBuf,
        diagnostics: Vec<Diagnostic>,
    },
    // Aggregates retain every error rather than inventing a single source chain.
    #[error("{}", render_batch(.0))]
    Batch(Vec<Self>),
}

impl CliError {
    pub(crate) fn file(path: &Path, source: io::Error) -> Self {
        Self::FileIo {
            path: path.to_owned(),
            source,
        }
    }
}

fn render_diagnostics(path: &Path, diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| format!("{}:{diagnostic}", path.display()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_batch(errors: &[CliError]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::unreachable,
    reason = "Tests inspect known malformed input and explicitly constructed error variants"
)]
mod tests {
    use std::error::Error;
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
        assert_eq!(errors.len(), 2);
        assert!(
            matches!(&errors[0], CliError::Format { path, diagnostics: retained }
            if path == Path::new("bad.mkd") && retained == &diagnostics)
        );
        assert!(errors[1].source().is_some_and(<dyn Error>::is::<io::Error>));
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
