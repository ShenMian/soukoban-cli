//! Rendering of solve results.

use std::{io, time::Duration};

use anstyle::Style;
use soukoban::{prelude::*, run_length::rle_encode};

use crate::ui::{
    Ui,
    table::{self, Align, Cell, Column, Table},
    theme::Theme,
};

/// The outcome of solving a single level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolveStatus {
    /// The level was already solved before the search started.
    AlreadySolved,
    /// A solution was found.
    Solved,
    /// The search proved that there is no solution.
    NoSolution,
    /// The search was stopped because the time limit was reached.
    Timeout,
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
    /// The outcome of the level.
    pub status: SolveStatus,
    /// The solution actions, if the search found one.
    pub actions: Option<&'a Actions>,
    /// The search statistics, if a search was performed.
    pub stats: Option<SearchStats>,
}

/// Summary of a batch run.
pub struct SolveSummary {
    /// The number of levels that were processed.
    pub processed: usize,
    /// The number of levels that were selected.
    pub total: usize,
    /// The number of levels that are solved.
    pub solved: usize,
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
    let writer = ui.out();

    write!(writer, "{s}{}{s:#}", report.title, s = theme.title)?;
    writeln!(
        writer,
        "{s}{sep}{algorithm}{sep}{strategy}{s:#}",
        s = theme.muted,
        sep = theme.separator()
    )?;

    write_map_block(writer, report.map, &theme)?;

    match report.status {
        SolveStatus::AlreadySolved => {
            writeln!(
                writer,
                "{s}{}{s:#} already solved",
                theme.check(),
                s = theme.success_muted
            )?;
        }
        SolveStatus::Solved => {
            let actions = report.actions.expect("solved levels carry actions");
            let stats = report.stats.expect("solved levels carry statistics");
            writeln!(
                writer,
                "{s}{}{s:#} solved in {} · {} · {}",
                theme.check(),
                format_elapsed(stats.elapsed),
                describe_count(actions.shifts(), "push", "pushes"),
                describe_count(actions.moves(), "move", "moves"),
                s = theme.success
            )?;
            let secondary = actions.secondary_values();
            writeln!(
                writer,
                "  {s}box lines {} · box changes {} · pushing sessions {} · player lines {}{s:#}",
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
        SolveStatus::NoSolution => {
            writeln!(
                writer,
                "{s}{}{s:#} no solution",
                theme.cross(),
                s = theme.error
            )?;
        }
        SolveStatus::Timeout => {
            let budget = budget.map_or_else(
                || {
                    format_elapsed(
                        report
                            .stats
                            .expect("timed out levels carry statistics")
                            .elapsed,
                    )
                },
                format_budget,
            );
            writeln!(
                writer,
                "{s}{}{s:#} timeout after {budget}",
                theme.timeout(),
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
        Column::new("Time", Align::Right),
        Column::new("Solution", Align::Left).shrinkable(),
    ]);
    for report in reports {
        let pushes = report
            .actions
            .map_or_else(|| "-".to_string(), |actions| actions.shifts().to_string());
        let moves = report
            .actions
            .map_or_else(|| "-".to_string(), |actions| actions.moves().to_string());
        let time = report
            .stats
            .map_or_else(|| "-".to_string(), |stats| format_elapsed(stats.elapsed));
        let solution = report
            .actions
            .filter(|actions| !actions.is_empty())
            .map_or_else(|| "-".to_string(), encode_actions);
        table.push_row([
            Cell::new(report.number.to_string()),
            Cell::new(report.title),
            Cell::styled(
                status_text(report.status),
                status_style(report.status, &theme),
            ),
            Cell::new(pushes),
            Cell::new(moves),
            Cell::new(time),
            Cell::new(solution),
        ]);
    }
    table.render(ui, table::terminal_width())
}

/// Writes the summary line of a batch run to stderr.
pub fn solve_summary(ui: &mut Ui, summary: &SolveSummary) -> io::Result<()> {
    let style = ui.theme().muted;
    let levels = if summary.processed < summary.total {
        format!("{} of {} levels", summary.processed, summary.total)
    } else {
        describe_count(summary.total, "level", "levels")
    };
    // An interrupted run processes only part of the levels; average over those.
    let average = summary.elapsed / summary.processed as u32;
    writeln!(
        ui.err(),
        "{style}{levels} · {} solved · {} · avg {}/level{style:#}",
        summary.solved,
        format_elapsed(summary.elapsed),
        format_elapsed(average)
    )
}

/// Writes a bordered map block to stdout.
fn write_map_block(writer: &mut dyn io::Write, map: &Map, theme: &Theme) -> io::Result<()> {
    let width = map.dimensions().x as usize;
    let horizontal = theme.horizontal().to_string().repeat(width + 2);

    writeln!(
        writer,
        "{s}{}{horizontal}{}{s:#}",
        theme.top_left(),
        theme.top_right(),
        s = theme.border
    )?;
    for y in 0..map.dimensions().y {
        write!(writer, "{s}{}{s:#} ", theme.vertical(), s = theme.border)?;
        for x in 0..map.dimensions().x {
            match tile_display(map[Point::new(x, y)], theme) {
                Some((glyph, style)) => write!(writer, "{style}{glyph}{style:#}")?,
                None => write!(writer, " ")?,
            }
        }
        writeln!(writer, " {s}{}{s:#}", theme.vertical(), s = theme.border)?;
    }
    writeln!(
        writer,
        "{s}{}{horizontal}{}{s:#}",
        theme.bottom_left(),
        theme.bottom_right(),
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

/// Returns the status text of a level.
fn status_text(status: SolveStatus) -> &'static str {
    match status {
        SolveStatus::AlreadySolved => "already solved",
        SolveStatus::Solved => "solved",
        SolveStatus::NoSolution => "unsolved",
        SolveStatus::Timeout => "timeout",
    }
}

/// Returns the status style of a level.
fn status_style(status: SolveStatus, theme: &Theme) -> Style {
    match status {
        SolveStatus::AlreadySolved => theme.success_muted,
        SolveStatus::Solved => theme.success,
        SolveStatus::NoSolution => theme.error,
        SolveStatus::Timeout => theme.warning,
    }
}

/// Formats a count with a singular or plural noun.
pub(crate) fn describe_count(count: usize, singular: &str, plural: &str) -> String {
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
    let seconds = duration.as_secs_f64();
    if seconds < 1.0 {
        format!("{:.1} ms", seconds * 1000.0)
    } else if seconds < 60.0 {
        format!("{seconds:.2}s")
    } else {
        let minutes = (seconds / 60.0) as u64;
        format!("{minutes}m {:.0}s", seconds - minutes as f64 * 60.0)
    }
}

/// Formats a time limit for human readable output.
pub fn format_budget(duration: Duration) -> String {
    let seconds = duration.as_secs_f64();
    if seconds >= 3600.0 && seconds % 3600.0 == 0.0 {
        format!("{}h", seconds / 3600.0)
    } else if seconds >= 60.0 && seconds % 60.0 == 0.0 {
        format!("{}m", seconds / 60.0)
    } else if seconds >= 1.0 {
        format!("{seconds}s")
    } else {
        format!("{}ms", (seconds * 1000.0).round() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Glyphs;

    /// Strips SGR escape sequences from rendered output.
    fn strip_styles(text: &str) -> String {
        let mut result = String::new();
        let mut in_escape = false;
        for char in text.chars() {
            if in_escape {
                if char == 'm' {
                    in_escape = false;
                }
            } else if char == '\u{1b}' {
                in_escape = true;
            } else {
                result.push(char);
            }
        }
        result
    }

    #[test]
    fn renders_empty_tiles_as_spaces() {
        let map: Map = "######\n#@$  #\n#   .#\n######"
            .parse()
            .expect("the map is valid");
        let theme = Theme::new(Glyphs::Unicode);
        let mut buffer = Vec::new();
        write_map_block(&mut buffer, &map, &theme).expect("writing to a buffer succeeds");
        let text = strip_styles(&String::from_utf8(buffer).expect("valid UTF-8"));
        assert!(text.contains("│ #@$  # │"));
        assert!(text.contains("│ #   .# │"));
        assert!(!text.contains('_'), "floor glyphs must be hidden");
        assert!(!text.contains('-'), "empty glyphs must be hidden");
    }

    #[test]
    fn formats_elapsed_times() {
        assert_eq!(format_elapsed(Duration::from_micros(12_400)), "12.4 ms");
        assert_eq!(format_elapsed(Duration::from_millis(310)), "310.0 ms");
        assert_eq!(format_elapsed(Duration::from_millis(1_234)), "1.23s");
        assert_eq!(format_elapsed(Duration::from_secs(75)), "1m 15s");
    }

    #[test]
    fn formats_time_limits() {
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
}
