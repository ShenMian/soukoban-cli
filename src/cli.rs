//! The command line interface definition.

use std::{fmt, time::Duration};

use clap::{ArgAction, Args, CommandFactory, FromArgMatches, Subcommand, ValueEnum};

use crate::commands::{search, solve};

/// A command line tool for the soukoban library.
#[derive(clap::Parser)]
#[command(name = "soukoban", version, styles = crate::ui::theme::clap_styles())]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Parses the command line, exiting on usage errors and `--help`.
    pub fn parse() -> Self {
        let matches = <Self as CommandFactory>::command()
            .bin_name("soukoban")
            .get_matches();
        <Self as FromArgMatches>::from_arg_matches(&matches).unwrap_or_else(|error| error.exit())
    }
}

/// A subcommand.
#[derive(Subcommand)]
pub enum Command {
    /// Search for a solution to a level.
    Solve(solve::Args),
    /// Find levels in the `.xsb` files of a directory.
    Search(search::Args),
}

/// Options accepted by every subcommand.
#[derive(Args)]
pub struct GlobalArgs {
    /// Output format.
    #[arg(long, value_enum, global = true, default_value_t = Format::Human)]
    pub format: Format,

    /// Increase diagnostic verbosity; can be repeated.
    #[arg(short, long, global = true, action = ArgAction::Count)]
    pub verbose: u8,

    /// When to use colored output.
    #[arg(long, value_enum, global = true, default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,

    /// Use ASCII characters for borders and decorations.
    #[arg(long, global = true)]
    pub ascii: bool,

    /// When to show search progress.
    #[arg(long, value_enum, global = true, default_value_t = ProgressChoice::Auto)]
    pub progress: ProgressChoice,
}

/// The output format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// Human readable output.
    Human,
    /// Machine readable JSON output.
    Json,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Human => "human",
            Self::Json => "json",
        })
    }
}

/// When to use colored output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ColorChoice {
    /// Colorize when the stream is a terminal; respects `NO_COLOR`.
    Auto,
    /// Always colorize.
    Always,
    /// Never colorize.
    Never,
}

impl fmt::Display for ColorChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        })
    }
}

impl From<ColorChoice> for anstream::ColorChoice {
    fn from(choice: ColorChoice) -> Self {
        match choice {
            ColorChoice::Auto => Self::Auto,
            ColorChoice::Always => Self::Always,
            ColorChoice::Never => Self::Never,
        }
    }
}

/// When to show search progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ProgressChoice {
    /// Show progress when the output is human readable and stderr is a
    /// terminal.
    Auto,
    /// Always show progress.
    Always,
    /// Never show progress.
    Never,
}

impl fmt::Display for ProgressChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        })
    }
}

/// The search algorithm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Algorithm {
    /// A* search.
    Astar,
    /// IDA* search.
    IdaStar,
    /// Breadth-first search.
    Bfs,
}

impl Algorithm {
    /// Returns the identifier used by the library and the JSON output.
    pub fn id(self) -> &'static str {
        match self {
            Self::Astar => "astar",
            Self::IdaStar => "ida-star",
            Self::Bfs => "bfs",
        }
    }

    /// Returns the name used in human readable output.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Astar => "A*",
            Self::IdaStar => "IDA*",
            Self::Bfs => "BFS",
        }
    }
}

impl fmt::Display for Algorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

impl From<Algorithm> for soukoban::solver::Algorithm {
    fn from(algorithm: Algorithm) -> Self {
        match algorithm {
            Algorithm::Astar => Self::AStar,
            Algorithm::IdaStar => Self::IDAStar,
            Algorithm::Bfs => Self::Bfs,
        }
    }
}

/// The search strategy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Strategy {
    /// Find any solution as quickly as possible.
    Quick,
    /// Find the push optimal solution.
    PushOptimal,
    /// Find the move optimal solution.
    MoveOptimal,
}

impl Strategy {
    /// Returns the identifier used by the JSON output.
    pub fn id(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::PushOptimal => "push-optimal",
            Self::MoveOptimal => "move-optimal",
        }
    }
}

impl fmt::Display for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

impl From<Strategy> for soukoban::solver::Strategy {
    fn from(strategy: Strategy) -> Self {
        match strategy {
            Strategy::Quick => Self::Quick,
            Strategy::PushOptimal => Self::PushOptimal,
            Strategy::MoveOptimal => Self::MoveOptimal,
        }
    }
}

/// Parses a duration such as `500ms`, `30s`, `5m` or `1h`.
///
/// A missing unit is interpreted as seconds.
pub fn parse_duration(text: &str) -> Result<Duration, String> {
    let text = text.trim();
    let split = text
        .find(|char: char| !char.is_ascii_digit() && char != '.')
        .unwrap_or(text.len());
    let (value, unit) = text.split_at(split);
    let value: f64 = value
        .parse()
        .map_err(|_| format!("invalid duration `{text}`"))?;
    let seconds = match unit.to_ascii_lowercase().as_str() {
        "" | "s" => value,
        "ms" => value / 1000.0,
        "m" => value * 60.0,
        "h" => value * 3600.0,
        _ => return Err(format!("invalid duration unit in `{text}`")),
    };
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err(format!("duration must be positive: `{text}`"));
    }
    Ok(Duration::from_secs_f64(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
        assert_eq!(parse_duration("2").unwrap(), Duration::from_secs(2));
        assert_eq!(parse_duration("0.5s").unwrap(), Duration::from_millis(500));
    }

    #[test]
    fn rejects_invalid_durations() {
        for text in ["", "abc", "0", "-5", "1x", "1..5", "s"] {
            assert!(
                parse_duration(text).is_err(),
                "duration `{text}` should be rejected"
            );
        }
    }
}
