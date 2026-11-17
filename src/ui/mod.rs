//! The terminal output facade.

pub mod json;
pub mod progress;
pub mod render;
pub mod table;
pub mod theme;

use std::{
    fmt::Display,
    io::{self, Write as _},
};

use anstream::{AutoStream, ColorChoice};
use serde::Serialize;

use crate::cli::{Format, ProgressChoice};

use theme::Theme;

/// The output stream of a [`Ui`].
///
/// The concrete standard streams are kept instead of a single boxed writer:
/// `anstream` detects terminals only for its own stream types, so a boxed
/// writer is always reported as a non-terminal, which would silently turn
/// `--color auto` into `--color never`. Keeping the concrete types also
/// enables the Windows console fallback of `anstream`.
enum Output {
    Stdout(AutoStream<io::Stdout>),
    Stderr(AutoStream<io::Stderr>),
    #[cfg(test)]
    Buffer(AutoStream<Vec<u8>>),
}

impl io::Write for Output {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Stdout(stream) => stream.write(buf),
            Self::Stderr(stream) => stream.write(buf),
            #[cfg(test)]
            Self::Buffer(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Stdout(stream) => stream.flush(),
            Self::Stderr(stream) => stream.flush(),
            #[cfg(test)]
            Self::Buffer(stream) => stream.flush(),
        }
    }
}

/// The UI facade: owns the output streams and the presentation settings.
///
/// Commands must present their results through this type; errors, warnings,
/// and progress go to stderr, payloads go to stdout.
pub struct Ui {
    out: Output,
    err: Output,
    format: Format,
    verbose: u8,
    progress: ProgressChoice,
    theme: Theme,
}

impl Ui {
    /// Creates a `Ui` writing to the standard output and error streams.
    pub fn stdout(
        format: Format,
        verbose: u8,
        progress: ProgressChoice,
        theme: Theme,
        color: ColorChoice,
    ) -> Self {
        Self {
            out: Output::Stdout(AutoStream::new(io::stdout(), color)),
            err: Output::Stderr(AutoStream::new(io::stderr(), color)),
            format,
            verbose,
            progress,
            theme,
        }
    }

    /// Returns `true` if the JSON format is selected.
    pub const fn is_json(&self) -> bool {
        matches!(self.format, Format::Json)
    }

    /// Returns the global progress choice.
    pub const fn progress(&self) -> ProgressChoice {
        self.progress
    }

    /// Returns the theme.
    pub const fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Writes a warning line to stderr.
    pub fn warning(&mut self, message: impl Display) -> io::Result<()> {
        let style = self.theme.warning;
        writeln!(self.err, "{style}warning{style:#}: {message}")
    }

    /// Writes a diagnostic line to stderr when verbose output is enabled.
    pub fn debug(&mut self, message: impl Display) -> io::Result<()> {
        if self.verbose == 0 {
            return Ok(());
        }
        let style = self.theme.muted;
        writeln!(self.err, "{style}debug{style:#}: {message}")
    }

    /// Writes `value` to stdout as pretty JSON if the JSON format is selected.
    pub fn json(&mut self, value: &impl Serialize) -> io::Result<()> {
        if !self.is_json() {
            return Ok(());
        }
        serde_json::to_writer_pretty(&mut self.out, value).map_err(io::Error::other)?;
        writeln!(self.out)
    }

    pub(crate) fn out(&mut self) -> &mut dyn io::Write {
        &mut self.out
    }

    pub(crate) fn err(&mut self) -> &mut dyn io::Write {
        &mut self.err
    }
}

#[cfg(test)]
impl Ui {
    /// Creates an in-memory `Ui` for unit tests.
    pub(crate) fn buffered(format: Format, glyphs: theme::Glyphs) -> Self {
        Self {
            out: Output::Buffer(AutoStream::never(Vec::new())),
            err: Output::Buffer(AutoStream::never(Vec::new())),
            format,
            verbose: 0,
            progress: ProgressChoice::Never,
            theme: Theme::new(glyphs),
        }
    }

    /// Consumes the buffered `Ui` and returns `(stdout, stderr)`.
    pub(crate) fn into_parts(self) -> (String, String) {
        fn contents(stream: Output) -> String {
            match stream {
                Output::Buffer(stream) => {
                    String::from_utf8(stream.into_inner()).expect("valid UTF-8")
                }
                Output::Stdout(_) | Output::Stderr(_) => panic!("expected a buffer stream"),
            }
        }
        (contents(self.out), contents(self.err))
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::theme::Glyphs;

    use super::*;

    #[test]
    fn writes_warnings_to_the_error_stream() {
        let mut ui = Ui::buffered(Format::Human, Glyphs::Ascii);
        ui.warning("check the input").expect("writing succeeds");
        let (out, err) = ui.into_parts();
        assert_eq!(err, "warning: check the input\n");
        assert_eq!(out, "");
    }

    #[test]
    fn json_output_respects_the_format() {
        let mut ui = Ui::buffered(Format::Human, Glyphs::Ascii);
        ui.json(&"human").expect("writing succeeds");
        let (out, _) = ui.into_parts();
        assert_eq!(out, "");

        let mut ui = Ui::buffered(Format::Json, Glyphs::Ascii);
        ui.json(&"json").expect("writing succeeds");
        let (out, _) = ui.into_parts();
        assert_eq!(out, "\"json\"\n");
    }

    #[test]
    fn passes_the_color_choice_to_the_standard_streams() {
        // `Ui::stdout` must hand `ColorChoice::Auto` and the concrete standard
        // streams to `anstream`: a boxed writer is never reported as a
        // terminal, which would silently disable the automatic detection.
        let never = Ui::stdout(
            Format::Human,
            0,
            ProgressChoice::Never,
            Theme::new(Glyphs::Ascii),
            ColorChoice::Never,
        );
        let (Output::Stdout(out), Output::Stderr(err)) = (&never.out, &never.err) else {
            panic!("the standard streams are expected");
        };
        assert_eq!(out.current_choice(), ColorChoice::Never);
        assert_eq!(err.current_choice(), ColorChoice::Never);

        let always = Ui::stdout(
            Format::Human,
            0,
            ProgressChoice::Never,
            Theme::new(Glyphs::Ascii),
            ColorChoice::Always,
        );
        let (Output::Stdout(out), Output::Stderr(err)) = (&always.out, &always.err) else {
            panic!("the standard streams are expected");
        };
        assert_ne!(out.current_choice(), ColorChoice::Never);
        assert_ne!(err.current_choice(), ColorChoice::Never);
    }
}
