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

impl Error for WithHint {}

/// Renders an error chain to stderr.
pub fn render(error: &anyhow::Error, ui: &mut Ui) -> io::Result<()> {
    let style = ui.theme().error;
    let label = ui.theme().label;
    let accent = ui.theme().accent;

    let mut hint = None;
    let mut messages = Vec::new();
    for cause in error.chain() {
        match cause.downcast_ref::<WithHint>() {
            Some(with_hint) => {
                hint = Some(with_hint.hint.as_str());
                messages.extend(with_hint.error.chain().map(|cause| cause.to_string()));
            }
            None => messages.push(cause.to_string()),
        }
    }

    let writer = ui.err();
    let mut messages = messages.into_iter();
    let head = messages
        .next()
        .unwrap_or_else(|| "unknown error".to_string());
    writeln!(writer, "{style}error{style:#}: {head}")?;
    for message in messages {
        writeln!(writer, "  {label}caused by{label:#}: {message}")?;
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
