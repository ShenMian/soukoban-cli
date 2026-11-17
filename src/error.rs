//! Error wrapping, rendering, and process exit codes.

use std::{error::Error, fmt, io, process::ExitCode};

use crate::ui::Ui;

/// The exit code of a normal run, including unsolved and timed out levels.
pub const EXIT_SUCCESS: u8 = 0;
/// The exit code of a run that stopped with an error.
pub const EXIT_ERROR: u8 = 1;
/// The exit code of a run interrupted by the user, following POSIX convention.
pub const EXIT_INTERRUPT: u8 = 130;

/// A specialized `Result` type for command results.
pub type Result<T> = anyhow::Result<T>;

/// Attaches a `help:` hint to an error.
pub fn with_hint(error: anyhow::Error, hint: impl Into<String>) -> anyhow::Error {
    WithHint::wrap(error, hint)
}

/// An error wrapper carrying a hint for [`render`].
#[derive(Debug)]
struct WithHint {
    error: anyhow::Error,
    hint: String,
}

impl WithHint {
    fn wrap(error: anyhow::Error, hint: impl Into<String>) -> anyhow::Error {
        anyhow::Error::new(Self {
            error,
            hint: hint.into(),
        })
    }
}

impl fmt::Display for WithHint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl Error for WithHint {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.error.source()
    }
}

/// Renders an error chain to stderr.
pub fn render(error: &anyhow::Error, ui: &mut Ui) -> io::Result<()> {
    let style = ui.theme().error;
    let label = ui.theme().label;
    let accent = ui.theme().accent;

    let hint = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<WithHint>())
        .map(|with_hint| with_hint.hint.as_str());

    let writer = ui.err();
    let mut chain = error.chain();
    let head = chain
        .next()
        .map_or_else(|| "unknown error".to_string(), ToString::to_string);
    writeln!(writer, "{style}error{style:#}: {head}")?;
    for cause in chain {
        writeln!(writer, "  {label}caused by{label:#}: {cause}")?;
    }
    if let Some(hint) = hint {
        writeln!(writer, "  {accent}help{accent:#}: {hint}")?;
    }
    Ok(())
}

/// Returns `true` if the error was caused by a broken pipe.
pub fn is_broken_pipe(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .is_some_and(|io_error| io_error.kind() == io::ErrorKind::BrokenPipe)
    })
}

/// Maps a runtime error to a process exit code.
pub fn map_error(_error: &anyhow::Error) -> ExitCode {
    ExitCode::from(EXIT_ERROR)
}

#[cfg(test)]
mod tests {
    use crate::{cli::Format, ui::theme::Glyphs};

    use super::*;

    #[test]
    fn renders_causes_and_hints_across_context_layers() {
        let inner = io::Error::new(io::ErrorKind::NotFound, "file missing");
        let error = with_hint(
            anyhow::Error::new(inner).context("failed to read `levels.xsb`"),
            "check the path",
        )
        .context("could not load input");

        let mut ui = Ui::buffered(Format::Human, Glyphs::Ascii);
        render(&error, &mut ui).expect("rendering succeeds");
        let (out, err) = ui.into_parts();
        assert_eq!(out, "");
        assert_eq!(
            err,
            "error: could not load input\n  \
             caused by: failed to read `levels.xsb`\n  \
             caused by: file missing\n  \
             help: check the path\n"
        );
    }

    #[test]
    fn detects_broken_pipes_through_hints_and_contexts() {
        let pipe = io::Error::from(io::ErrorKind::BrokenPipe);
        let error = with_hint(
            anyhow::Error::new(pipe).context("write failed"),
            "try again",
        );
        assert!(is_broken_pipe(&error));

        let other = with_hint(anyhow::anyhow!("invalid map"), "check the file");
        assert!(!is_broken_pipe(&other));
    }
}
