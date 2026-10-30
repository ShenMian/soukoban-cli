//! Progress reporting for long running searches.

use std::{
    io::{self, IsTerminal as _},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use console::Term;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use soukoban::{
    prelude::{Actions, SearchError},
    solver::{Node, Observer},
};

use crate::cli::ProgressChoice;

use super::Ui;

/// How often the progress message is refreshed while searching.
const REFRESH_INTERVAL: Duration = Duration::from_millis(80);
/// How often the batch spinner advances while progress is shown.
const TICK_INTERVAL: Duration = Duration::from_millis(100);
/// The minimum elapsed time before a node rate is reported.
const RATE_WINDOW: Duration = Duration::from_millis(100);
/// The terminal height assumed when it cannot be determined.
const DEFAULT_ROWS: usize = 24;
/// The template of the batch line.
const BATCH_TEMPLATE: &str = "{spinner} {bar:14} {pos:>4}/{len} · {msg}";
/// The template of a single level, which has no batch position.
const SINGLE_TEMPLATE: &str = "{spinner} {bar:14} · {msg}";
/// The template of a level line below the batch line, which has no spinner of
/// its own.
const LEVEL_TEMPLATE: &str = "  {bar:14} · {msg}";

/// The width of the level number in a progress message.
const LEVEL_WIDTH: usize = 4;
/// The width of a statistic in a progress message.
///
/// The statistics change while a search runs; padding them keeps the labels
/// and separators of the line in place.
const STAT_WIDTH: usize = 6;

/// Returns the label of a level for a progress message.
pub fn level_label(number: usize) -> String {
    format!("#{number:<LEVEL_WIDTH$}")
}

/// Describes the statistics of a level, padded to keep a stable layout.
fn level_message(number: usize, expansions: u64, elapsed: Duration) -> String {
    format!(
        "{label} · expanded {:>STAT_WIDTH$} · speed {:>STAT_WIDTH$}/s · elapsed {:>STAT_WIDTH$.1}s",
        format_count(expansions),
        format_count(rate(expansions, elapsed)),
        elapsed.as_secs_f64(),
        label = level_label(number)
    )
}

/// Returns whether progress reporting is enabled for the given settings.
///
/// Progress is never shown for the JSON format.
pub fn enabled(ui: &Ui) -> bool {
    if ui.is_json() {
        return false;
    }
    match ui.progress() {
        ProgressChoice::Always => true,
        ProgressChoice::Never => false,
        ProgressChoice::Auto => io::stderr().is_terminal(),
    }
}

/// Returns whether progress must be drawn even without a terminal.
fn forced(ui: &Ui) -> bool {
    !ui.is_json() && ui.progress() == ProgressChoice::Always && !io::stderr().is_terminal()
}

/// A progress display that becomes a no-op when progress reporting is
/// disabled.
///
/// A run over several levels has one batch line with the totals of the run and
/// one line per level below it: a single line in a sequential run, and one line
/// per worker during a parallel run. Each level line carries a bar of the
/// heuristic progress of its level. A run over a single level shows that line
/// alone.
pub struct Progress {
    /// The display of a run over several levels. It owns the batch line and the
    /// lines of its levels.
    multi: Option<MultiProgress>,
    /// The batch line.
    batch: Option<ProgressBar>,
    /// The line of a run over a single level.
    single: Option<ProgressBar>,
    /// The style of the level lines.
    line_style: ProgressStyle,
    /// The number of level lines that fit below the batch line.
    lanes: usize,
    /// Expanded nodes of all levels.
    expansions: Arc<AtomicU64>,
    /// When the run started.
    started: Instant,
}

impl Progress {
    /// Creates the progress display of a run over `total` levels solved by
    /// `jobs` workers.
    pub fn new(ui: &Ui, enabled: bool, total: u64, jobs: usize) -> Self {
        let mut progress = Self {
            multi: None,
            batch: None,
            single: None,
            line_style: style(ui, LEVEL_TEMPLATE),
            lanes: 0,
            expansions: Arc::new(AtomicU64::new(0)),
            started: Instant::now(),
        };
        if !enabled {
            return progress;
        }
        let target = draw_target(ui);
        if total > 1 {
            let multi = MultiProgress::with_draw_target(target);
            let batch = multi.add(bar(ui, BATCH_TEMPLATE, Some(total)));
            // Spinners only advance with a steady tick; without one the
            // batch spinner would move once per finished level.
            batch.enable_steady_tick(TICK_INTERVAL);
            progress.lanes = jobs.min(level_lanes());
            progress.multi = Some(multi);
            progress.batch = Some(batch);
            progress.set_totals();
        } else {
            let single = bar(ui, SINGLE_TEMPLATE, Some(1));
            single.set_draw_target(target);
            single.enable_steady_tick(TICK_INTERVAL);
            progress.single = Some(single);
        }
        progress
    }

    /// Creates a line for the level solved in lane `index`.
    ///
    /// The line is removed from the display when the returned guard is dropped.
    /// Lanes beyond the lines that fit into the terminal get `None`; their
    /// levels are still counted in the totals of the run.
    pub fn level_line(&self, index: usize) -> Option<LevelLine> {
        let multi = self.multi.clone()?;
        if index >= self.lanes {
            return None;
        }
        let bar = ProgressBar::new(1);
        bar.set_style(self.line_style.clone());
        Some(LevelLine {
            bar: multi.add(bar),
            multi,
        })
    }

    /// Runs `f` while the display is suspended, so that output written by `f`
    /// is not overwritten by a redraw.
    pub fn suspend<F: FnOnce() -> R, R>(&self, f: F) -> R {
        if let Some(multi) = &self.multi {
            multi.suspend(f)
        } else if let Some(bar) = &self.single {
            bar.suspend(f)
        } else {
            f()
        }
    }

    /// Moves the batch bar to `completed`, the number of finished levels.
    pub fn set_completed(&self, completed: u64) {
        if let Some(bar) = &self.batch {
            bar.set_position(completed);
        }
    }

    /// Updates the message of the single-level line.
    pub fn set_message(&self, message: String) {
        if let Some(bar) = &self.single {
            bar.set_message(message);
        }
    }

    /// Stops the spinners and clears the progress display.
    pub fn finish(&self) {
        // The tickers keep drawing until they are stopped.
        if let Some(bar) = &self.batch {
            bar.disable_steady_tick();
        }
        if let Some(bar) = &self.single {
            bar.disable_steady_tick();
            bar.finish_and_clear();
        }
        if let Some(multi) = &self.multi {
            let _ = multi.clear();
        }
    }

    fn is_enabled(&self) -> bool {
        self.batch.is_some() || self.single.is_some()
    }

    /// Adds expanded nodes to the run totals.
    fn add_expansions(&self, expansions: u64) {
        self.expansions.fetch_add(expansions, Ordering::Relaxed);
    }

    /// Updates the single-level line with the level statistics and progress.
    fn set_single(&self, message: String, best: i32, max: i32) {
        let Some(bar) = &self.single else {
            return;
        };
        bar.set_message(message);
        if let Some((position, length)) = heuristic_position(best, max) {
            bar.set_length(length);
            bar.set_position(position);
        }
    }

    /// Updates the batch message with the run totals.
    fn set_totals(&self) {
        let Some(bar) = &self.batch else {
            return;
        };
        let elapsed = self.started.elapsed();
        let expansions = self.expansions.load(Ordering::Relaxed);
        bar.set_message(format!(
            "expanded {:>STAT_WIDTH$} · speed {:>STAT_WIDTH$}/s · elapsed {:>STAT_WIDTH$.1}s",
            format_count(expansions),
            format_count(rate(expansions, elapsed)),
            elapsed.as_secs_f64()
        ));
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        // Clear the display when the run ends early, e.g. with an error.
        self.finish();
    }
}

/// A line of a level in a run over several levels.
///
/// The line is removed from the display when the guard is dropped.
pub struct LevelLine {
    bar: ProgressBar,
    multi: MultiProgress,
}

impl LevelLine {
    /// Updates the message of the line.
    pub fn set_message(&self, message: String) {
        self.bar.set_message(message);
    }

    /// Fills the bar to the heuristic progress of the level.
    fn set_heuristic(&self, best: i32, max: i32) {
        if let Some((position, length)) = heuristic_position(best, max) {
            self.bar.set_length(length);
            self.bar.set_position(position);
        }
    }
}

impl Drop for LevelLine {
    fn drop(&mut self) {
        self.multi.remove(&self.bar);
    }
}

/// Maps the smallest heuristic value seen so far onto a bar.
///
/// The bar is filled to `1 - best / max`, where `max` is the heuristic of the
/// initial node. Returns `None` while the search has not reported its initial
/// value yet, which also covers searches that stop before they start, such as
/// deadlocked levels.
fn heuristic_position(best: i32, max: i32) -> Option<(u64, u64)> {
    if max <= 0 {
        return None;
    }
    let best = best.clamp(0, max);
    Some(((max - best) as u64, max as u64))
}

/// Builds a progress style with the application glyphs.
fn style(ui: &Ui, template: &str) -> ProgressStyle {
    ProgressStyle::with_template(template)
        .expect("static progress template is valid")
        .progress_chars(ui.theme().bar_chars())
        .tick_chars(ui.theme().ticks())
}

/// Builds a progress bar with the application style.
fn bar(ui: &Ui, template: &str, total: Option<u64>) -> ProgressBar {
    let bar = ProgressBar::with_draw_target(total, ProgressDrawTarget::stderr());
    bar.set_style(style(ui, template));
    bar
}

/// Returns the draw target of the progress display.
fn draw_target(ui: &Ui) -> ProgressDrawTarget {
    if forced(ui) {
        // The default target is hidden for non-terminals; an explicit
        // `--progress always` overrides that.
        ProgressDrawTarget::term_like(Box::new(Term::buffered_stderr()))
    } else {
        ProgressDrawTarget::stderr()
    }
}

/// Returns how many level lines fit below the batch line.
fn level_lanes() -> usize {
    let rows = Term::stderr()
        .size_checked()
        .map_or(DEFAULT_ROWS, |(rows, _)| usize::from(rows));
    rows.saturating_sub(2).max(1)
}

/// Where a [`SearchReporter`] writes its message.
#[derive(Clone, Copy)]
enum Target<'a> {
    /// The line of a run over a single level.
    Single,
    /// A level line below the batch line.
    Line(Option<&'a LevelLine>),
}

/// The observer that counts expanded nodes and updates the progress message.
pub struct SearchReporter<'a> {
    progress: &'a Progress,
    target: Target<'a>,
    number: usize,
    expansions: u64,
    /// The expansions already added to the run totals.
    reported: u64,
    started: Instant,
    last_refresh: Instant,
    /// The heuristic of the initial node; the length of the level bar.
    max_heuristic: i32,
    /// The smallest heuristic value seen so far.
    best_heuristic: i32,
}

impl<'a> SearchReporter<'a> {
    /// Creates a reporter that writes the statistics of the single level
    /// numbered `number` to the line of the run.
    pub fn single(progress: &'a Progress, number: usize) -> Self {
        Self::new(progress, Target::Single, number)
    }

    /// Creates a reporter that writes the statistics of the level numbered
    /// `number` to a level line and feeds the run totals.
    pub fn line(progress: &'a Progress, line: Option<&'a LevelLine>, number: usize) -> Self {
        Self::new(progress, Target::Line(line), number)
    }

    fn new(progress: &'a Progress, target: Target<'a>, number: usize) -> Self {
        let now = Instant::now();
        Self {
            progress,
            target,
            number,
            expansions: 0,
            reported: 0,
            started: now,
            last_refresh: now,
            max_heuristic: 0,
            best_heuristic: 0,
        }
    }

    /// Returns the number of nodes expanded so far.
    pub fn expansions(&self) -> u64 {
        self.expansions
    }

    fn refresh(&mut self) {
        self.last_refresh = Instant::now();
        if !self.progress.is_enabled() {
            return;
        }
        let elapsed = self.started.elapsed();
        let message = level_message(self.number, self.expansions, elapsed);
        match self.target {
            Target::Single => {
                self.progress
                    .set_single(message, self.best_heuristic, self.max_heuristic);
            }
            Target::Line(line) => {
                if let Some(line) = line {
                    line.set_message(message);
                    line.set_heuristic(self.best_heuristic, self.max_heuristic);
                }
                if self.expansions > self.reported {
                    self.progress
                        .add_expansions(self.expansions - self.reported);
                    self.reported = self.expansions;
                    self.progress.set_totals();
                }
            }
        }
    }
}

impl Observer for SearchReporter<'_> {
    fn on_search_start(&mut self, root: &Node) {
        self.started = Instant::now();
        self.max_heuristic = root.heuristic();
        self.best_heuristic = self.max_heuristic;
        self.refresh();
    }

    fn on_node_expanded(&mut self, node: &Node) {
        self.expansions += 1;
        self.best_heuristic = self.best_heuristic.min(node.heuristic());
        if self.last_refresh.elapsed() >= REFRESH_INTERVAL {
            self.refresh();
        }
    }

    fn on_successor(&mut self, _parent: &Node, child: &Node) {
        self.best_heuristic = self.best_heuristic.min(child.heuristic());
    }

    fn on_search_end(&mut self, _result: &Result<Actions, SearchError>) {
        // Show the final statistics of the level until the next one starts.
        self.refresh();
    }
}

/// Returns the number of nodes expanded per second.
///
/// The rate is suppressed until [`RATE_WINDOW`] has passed, so that the
/// startup refresh does not divide by a nearly zero duration.
fn rate(expansions: u64, elapsed: Duration) -> u64 {
    if elapsed < RATE_WINDOW {
        return 0;
    }
    (expansions as f64 / elapsed.as_secs_f64()) as u64
}

/// Formats a node count in a compact human readable form.
pub fn format_count(count: u64) -> String {
    if count < 1_000 {
        count.to_string()
    } else if count < 999_950 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else if count < 999_950_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else {
        format!("{:.1}G", count as f64 / 1_000_000_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_node_counts() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1_250), "1.2K");
        assert_eq!(format_count(1_200_000), "1.2M");
        assert_eq!(format_count(3_400_000_000), "3.4G");
    }

    #[test]
    fn computes_node_rates() {
        assert_eq!(rate(0, Duration::from_secs(1)), 0);
        assert_eq!(rate(1_000, Duration::from_millis(50)), 0);
        assert_eq!(rate(1_000, Duration::from_secs(1)), 1_000);
        assert_eq!(rate(2_000_000, Duration::from_secs(1)), 2_000_000);
    }

    #[test]
    fn pads_the_progress_message() {
        assert_eq!(level_label(1), "#1   ");
        assert_eq!(level_label(155), "#155 ");
        assert_eq!(
            level_message(1, 0, Duration::ZERO),
            "#1    · expanded      0 · speed      0/s · elapsed    0.0s"
        );
        assert_eq!(
            level_message(155, 217_300, Duration::from_millis(2_500)),
            "#155  · expanded 217.3K · speed  86.9K/s · elapsed    2.5s"
        );
    }

    #[test]
    fn maps_heuristics_onto_bars() {
        // No initial value yet, e.g. a level that is deadlocked at the root.
        assert_eq!(heuristic_position(0, 0), None);
        assert_eq!(heuristic_position(0, 30), Some((30, 30)));
        assert_eq!(heuristic_position(10, 30), Some((20, 30)));
        assert_eq!(heuristic_position(30, 30), Some((0, 30)));
        assert_eq!(heuristic_position(40, 30), Some((0, 30)));
    }
}
