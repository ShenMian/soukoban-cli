//! Rendering of solve results.

use std::{io, time::Duration};

use anstyle::Style;
use soukoban::{prelude::*, run_length::rle_encode};

use crate::ui::{
    Ui,
    progress::format_count,
    table::{self, Align, Cell, Column, Table},
    theme::Theme,
};

/// The outcome of solving a single level.
#[derive(Clone, Debug)]
pub enum SolveOutcome {
    /// The level was already solved before the search started.
    AlreadySolved,
    /// A solution was found.
    Solved {
        /// The solution actions.
        actions: Actions,
        /// The search statistics.
        stats: SearchStats,
    },
    /// The search proved that there is no solution.
    NoSolution {
        /// The search statistics.
        stats: SearchStats,
    },
    /// The search was stopped because the time limit was reached.
    Timeout {
        /// The search statistics.
        stats: SearchStats,
    },
}

impl SolveOutcome {
    /// Returns `true` if the level is solved, whether before or during search.
    pub const fn is_solved(&self) -> bool {
        matches!(self, Self::AlreadySolved | Self::Solved { .. })
    }

    /// Returns the search statistics if a search was performed.
    pub const fn stats(&self) -> Option<SearchStats> {
        match self {
            Self::AlreadySolved => None,
            Self::Solved { stats, .. } | Self::NoSolution { stats } | Self::Timeout { stats } => {
                Some(*stats)
            }
        }
    }
}

/// Statistics of a level search.
#[derive(Clone, Copy, Debug)]
pub struct SearchStats {
    /// The number of nodes expanded by the search.
    pub expansions: u64,
    /// The wall clock time the search took.
    pub elapsed: Duration,
}

/// Presentation data of a single level.
pub struct LevelReport<'a> {
    /// The 1-based number of the level in the input.
    pub number: usize,
    /// The display title of the level.
    pub title: &'a str,
    /// The initial map of the level.
    pub map: &'a Map,
    /// The outcome of solving the level.
    pub outcome: &'a SolveOutcome,
}

/// Summary of a batch run.
pub struct SolveSummary {
    /// The number of levels that were processed.
    pub processed: usize,
    /// The number of levels that were selected.
    pub total: usize,
    /// The number of levels that are solved.
    pub solved: usize,
    /// The nodes expanded by the searches of the run.
    pub expansions: u64,
    /// The wall clock time of the whole run.
    pub elapsed: Duration,
}

/// Writes the detailed view of a single level to stdout.
pub fn solve_detail(
    ui: &mut Ui,
    report: &LevelReport<'_>,
    algorithm: &str,
    strategy: &str,
    budget: Option<Duration>,
) -> io::Result<()> {
    let theme = *ui.theme();
    let sep = theme.glyphs.separator;
    let writer = ui.out();

    write!(writer, "{s}{}{s:#}", report.title, s = theme.title)?;
    writeln!(
        writer,
        "{s}{sep}{algorithm}{sep}{strategy}{s:#}",
        s = theme.muted,
    )?;

    write_map_block(writer, report.map, &theme)?;

    match report.outcome {
        SolveOutcome::AlreadySolved => {
            writeln!(
                writer,
                "{s}{}{s:#} already solved",
                theme.glyphs.check,
                s = theme.success_muted
            )?;
        }
        SolveOutcome::Solved { actions, stats } => {
            writeln!(
                writer,
                "{s}{}{s:#} solved in {}{sep}{}{sep}{}",
                theme.glyphs.check,
                format_elapsed(stats.elapsed),
                describe_count(actions.shifts(), "push", "pushes"),
                describe_count(actions.moves(), "move", "moves"),
                s = theme.success
            )?;
            let secondary = actions.secondary_values();
            writeln!(
                writer,
                "  {s}box lines {}{sep}box changes {}{sep}pushing sessions {}{sep}player lines {}{s:#}",
                secondary.box_lines,
                secondary.box_changes,
                secondary.pushing_sessions,
                secondary.player_lines,
                s = theme.muted
            )?;
            writeln!(
                writer,
                "{s}{}{s:#}",
                encode_actions(actions),
                s = theme.value
            )?;
        }
        SolveOutcome::NoSolution { .. } => {
            writeln!(
                writer,
                "{s}{}{s:#} no solution",
                theme.glyphs.cross,
                s = theme.error
            )?;
        }
        SolveOutcome::Timeout { stats } => {
            let budget = budget.map_or_else(|| format_elapsed(stats.elapsed), format_budget);
            writeln!(
                writer,
                "{s}{}{s:#} timeout after {budget}",
                theme.glyphs.timeout,
                s = theme.warning
            )?;
        }
    }
    Ok(())
}

/// Writes the batch result table to stdout.
pub fn solve_table(ui: &mut Ui, reports: &[LevelReport<'_>]) -> io::Result<()> {
    if reports.is_empty() {
        return Ok(());
    }
    let theme = *ui.theme();
    let mut table = Table::new([
        Column::new("#", Align::Right),
        Column::new("Title", Align::Left).shrinkable(),
        Column::new("Status", Align::Left),
        Column::new("Pushes", Align::Right),
        Column::new("Moves", Align::Right),
        Column::new("Nodes", Align::Right),
        Column::new("Time", Align::Right),
    ]);
    for report in reports {
        let (status_text, status_style, pushes, moves) = match report.outcome {
            SolveOutcome::AlreadySolved => (
                "already solved",
                theme.success_muted,
                "-".to_string(),
                "-".to_string(),
            ),
            SolveOutcome::Solved { actions, .. } => (
                "solved",
                theme.success,
                actions.shifts().to_string(),
                actions.moves().to_string(),
            ),
            SolveOutcome::NoSolution { .. } => {
                ("unsolved", theme.error, "-".to_string(), "-".to_string())
            }
            SolveOutcome::Timeout { .. } => {
                ("timeout", theme.warning, "-".to_string(), "-".to_string())
            }
        };
        let stats = report.outcome.stats();
        let nodes = stats.map_or_else(|| "-".to_string(), |stats| format_count(stats.expansions));
        let time = stats.map_or_else(|| "-".to_string(), |stats| format_elapsed(stats.elapsed));
        table.push_row([
            Cell::new(report.number.to_string()),
            Cell::new(report.title),
            Cell::styled(status_text, status_style),
            Cell::new(pushes),
            Cell::new(moves),
            Cell::new(nodes),
            Cell::new(time),
        ]);
    }
    table.render(ui, table::terminal_width())
}

/// Writes the summary line of a batch run to stderr.
pub fn solve_summary(ui: &mut Ui, summary: &SolveSummary) -> io::Result<()> {
    if summary.processed == 0 {
        return Ok(());
    }
    let style = ui.theme().muted;
    let sep = ui.theme().glyphs.separator;
    let levels = if summary.processed < summary.total {
        format!("{} of {} levels", summary.processed, summary.total)
    } else {
        describe_count(summary.total, "level", "levels")
    };
    // An interrupted run processes only part of the levels; average over those.
    let average =
        u32::try_from(summary.processed).map_or(Duration::ZERO, |count| summary.elapsed / count);
    let nodes = summary.expansions / summary.processed as u64;
    writeln!(
        ui.err(),
        "{style}{levels}{sep}{} solved{sep}{}{sep}avg {}/level{sep}avg {} nodes/level{style:#}",
        summary.solved,
        format_elapsed(summary.elapsed),
        format_elapsed(average),
        format_count(nodes)
    )
}

/// Writes a bordered map block to stdout.
fn write_map_block(writer: &mut dyn io::Write, map: &Map, theme: &Theme) -> io::Result<()> {
    let width = usize::try_from(map.dimensions().x).unwrap_or(0);
    let horizontal = theme.glyphs.horizontal.to_string().repeat(width + 2);

    writeln!(
        writer,
        "{s}{}{horizontal}{}{s:#}",
        theme.glyphs.top_left,
        theme.glyphs.top_right,
        s = theme.border
    )?;
    for y in 0..map.dimensions().y {
        write!(
            writer,
            "{s}{}{s:#} ",
            theme.glyphs.vertical,
            s = theme.border
        )?;
        for x in 0..map.dimensions().x {
            match tile_display(map[Point::new(x, y)], theme) {
                Some((glyph, style)) => write!(writer, "{style}{glyph}{style:#}")?,
                None => write!(writer, " ")?,
            }
        }
        writeln!(
            writer,
            " {s}{}{s:#}",
            theme.glyphs.vertical,
            s = theme.border
        )?;
    }
    writeln!(
        writer,
        "{s}{}{horizontal}{}{s:#}",
        theme.glyphs.bottom_left,
        theme.glyphs.bottom_right,
        s = theme.border
    )
}

/// Returns the glyph and style of a map tile, or `None` for empty cells.
///
/// Floor (`_`) and empty (`-`) cells are rendered as spaces: the XSB glyphs
/// only protect trailing whitespace from being stripped by tools like email
/// clients, which is irrelevant for terminal output.
fn tile_display(tiles: Tiles, theme: &Theme) -> Option<(char, Style)> {
    if tiles.contains(Tiles::Box | Tiles::Goal) {
        Some(('*', theme.box_on_goal))
    } else if tiles.contains(Tiles::Player | Tiles::Goal) {
        Some(('+', theme.player))
    } else if tiles.contains(Tiles::Box) {
        Some(('$', theme.box_tile))
    } else if tiles.contains(Tiles::Goal) {
        Some(('.', theme.goal))
    } else if tiles.contains(Tiles::Player) {
        Some(('@', theme.player))
    } else if tiles.contains(Tiles::Wall) {
        Some(('#', theme.wall))
    } else {
        None
    }
}

/// Formats a count with a singular or plural noun.
pub fn describe_count(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {plural}")
    }
}

/// Encodes the actions of a level with run-length encoding.
///
/// Actions only consist of `LURD`, so the encoder cannot fail.
fn encode_actions(actions: &Actions) -> String {
    rle_encode(&actions.to_string()).expect("actions contain no digits")
}

/// Formats an elapsed time for human readable output.
pub fn format_elapsed(duration: Duration) -> String {
    let tenths_ms = (duration.as_nanos() + 50_000) / 100_000;
    if tenths_ms < 10_000 {
        return format!("{}.{} ms", tenths_ms / 10, tenths_ms % 10);
    }
    let centis = (duration.as_nanos() + 5_000_000) / 10_000_000;
    if centis < 6_000 {
        return format!("{}.{:02}s", centis / 100, centis % 100);
    }
    let seconds = (duration.as_millis() + 500) / 1_000;
    format!("{}m {}s", seconds / 60, seconds % 60)
}

/// Formats a time limit for human readable output.
pub fn format_budget(duration: Duration) -> String {
    if duration.subsec_nanos() == 0 {
        let seconds = duration.as_secs();
        if seconds >= 3600 && seconds.is_multiple_of(3600) {
            format!("{}h", seconds / 3600)
        } else if seconds >= 60 && seconds.is_multiple_of(60) {
            format!("{}m", seconds / 60)
        } else {
            format!("{seconds}s")
        }
    } else if duration >= Duration::from_secs(1) {
        format!("{}s", duration.as_secs_f64())
    } else if duration.subsec_nanos().is_multiple_of(1_000_000) {
        format!("{}ms", duration.subsec_millis())
    } else {
        format!("{}ms", duration.as_secs_f64() * 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cli::Format, ui::theme::Glyphs};

    #[test]
    fn renders_empty_tiles_as_spaces() {
        let map: Map = "######\n#@$  #\n#   .#\n######"
            .parse()
            .expect("the map is valid");
        let theme = Theme::new(Glyphs::Unicode);
        let mut buffer = Vec::new();
        write_map_block(&mut buffer, &map, &theme).expect("writing to a buffer succeeds");
        let raw = String::from_utf8(buffer).expect("valid UTF-8");
        let text = console::strip_ansi_codes(&raw);
        assert!(text.contains("│ #@$  # │"));
        assert!(text.contains("│ #   .# │"));
        assert!(!text.contains('_'), "floor glyphs must be hidden");
        assert!(!text.contains('-'), "empty glyphs must be hidden");
    }

    #[test]
    fn formats_elapsed_times() {
        assert_eq!(format_elapsed(Duration::from_micros(12_400)), "12.4 ms");
        assert_eq!(format_elapsed(Duration::from_millis(310)), "310.0 ms");
        assert_eq!(format_elapsed(Duration::from_micros(999_960)), "1.00s");
        assert_eq!(format_elapsed(Duration::from_millis(1_234)), "1.23s");
        assert_eq!(format_elapsed(Duration::from_millis(59_996)), "1m 0s");
        assert_eq!(format_elapsed(Duration::from_secs(75)), "1m 15s");
        assert_eq!(format_elapsed(Duration::from_millis(119_600)), "2m 0s");
    }

    #[test]
    fn formats_time_limits() {
        assert_eq!(format_budget(Duration::from_micros(500)), "0.5ms");
        assert_eq!(format_budget(Duration::from_millis(500)), "500ms");
        assert_eq!(format_budget(Duration::from_secs(30)), "30s");
        assert_eq!(format_budget(Duration::from_secs(300)), "5m");
        assert_eq!(format_budget(Duration::from_secs(3600)), "1h");
        assert_eq!(format_budget(Duration::from_millis(1500)), "1.5s");
    }

    #[test]
    fn describes_counts() {
        assert_eq!(describe_count(1, "push", "pushes"), "1 push");
        assert_eq!(describe_count(0, "push", "pushes"), "0 pushes");
        assert_eq!(describe_count(12, "move", "moves"), "12 moves");
    }

    #[test]
    fn summary_handles_empty_runs_and_ascii_separators() {
        let mut ui = Ui::buffered(Format::Human, Glyphs::Ascii);
        solve_summary(
            &mut ui,
            &SolveSummary {
                processed: 0,
                total: 3,
                solved: 0,
                expansions: 0,
                elapsed: Duration::ZERO,
            },
        )
        .expect("empty summary succeeds");
        solve_summary(
            &mut ui,
            &SolveSummary {
                processed: 2,
                total: 5,
                solved: 1,
                expansions: 40,
                elapsed: Duration::from_millis(100),
            },
        )
        .expect("summary succeeds");
        let (_, err) = ui.into_parts();
        assert!(
            err.is_ascii(),
            "ASCII summary must contain only ASCII: {err}"
        );
        assert!(err.contains("2 of 5 levels - 1 solved"));
    }
}
