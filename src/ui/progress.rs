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
const SINGLE_TEMPLATE: &str = "{spinner} {msg}";
/// The template of a worker line, which has no spinner of its own.
const WORKER_TEMPLATE: &str = "{msg}";

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
/// A run has one batch line, which reports the level that is currently solved
/// (or the totals of the run when several levels are solved at once), and one
/// line per worker below it during a parallel run.
pub struct Progress {
    /// The display of a parallel run. It owns the batch line and the lines of
    /// its workers.
    multi: Option<MultiProgress>,
    /// The batch line.
    bar: Option<ProgressBar>,
    /// The style of the worker lines.
    line_style: ProgressStyle,
    /// The number of worker lines that fit below the batch line.
    lanes: usize,
    /// Expanded nodes of all workers.
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
            bar: None,
            line_style: style(ui, WORKER_TEMPLATE),
            lanes: 0,
            expansions: Arc::new(AtomicU64::new(0)),
            started: Instant::now(),
        };
        if !enabled {
            return progress;
        }
        let template = if total > 1 {
            BATCH_TEMPLATE
        } else {
            SINGLE_TEMPLATE
        };
        let target = draw_target(ui);
        if jobs > 1 {
            let multi = MultiProgress::with_draw_target(target);
            let bar = multi.add(bar(ui, template, Some(total)));
            // Spinners only advance with a steady tick; without one the
            // batch spinner would move once per finished level.
            bar.enable_steady_tick(TICK_INTERVAL);
            progress.lanes = jobs.min(worker_lanes());
            progress.multi = Some(multi);
            progress.bar = Some(bar);
            // Workers report their statistics only once a search starts.
            progress.set_totals();
        } else {
            let bar = bar(ui, template, Some(total));
            bar.set_draw_target(target);
            bar.enable_steady_tick(TICK_INTERVAL);
            progress.bar = Some(bar);
        }
        progress
    }

    /// Creates a line for the worker at `index`.
    ///
    /// The line is removed from the display when the returned guard is dropped.
    /// Workers beyond the lines that fit into the terminal get `None`; their
    /// levels are still counted in the totals of the run.
    pub fn worker_line(&self, index: usize) -> Option<WorkerLine> {
        let multi = self.multi.clone()?;
        if index >= self.lanes {
            return None;
        }
        let bar = ProgressBar::new_spinner();
        bar.set_style(self.line_style.clone());
        Some(WorkerLine {
            bar: multi.add(bar),
            multi,
        })
    }

    /// Runs `f` while the display is suspended, so that output written by `f`
    /// is not overwritten by a redraw.
    pub fn suspend<F: FnOnce() -> R, R>(&self, f: F) -> R {
        match (&self.multi, &self.bar) {
            (Some(multi), _) => multi.suspend(f),
            (None, Some(bar)) => bar.suspend(f),
            (None, None) => f(),
        }
    }

    /// Moves the bar to `completed`, the number of finished levels.
    pub fn set_position(&self, completed: u64) {
        if let Some(bar) = &self.bar {
            bar.set_position(completed);
        }
    }

    /// Updates the message of the batch line.
    pub fn set_message(&self, message: String) {
        if let Some(bar) = &self.bar {
            bar.set_message(message);
        }
    }

    /// Stops the spinners and clears the progress display.
    pub fn finish(&self) {
        // The ticker of the batch line keeps drawing until it is stopped.
        if let Some(bar) = &self.bar {
            bar.disable_steady_tick();
        }
        if let Some(multi) = &self.multi {
            let _ = multi.clear();
        } else if let Some(bar) = &self.bar {
            bar.finish_and_clear();
        }
    }

    fn is_enabled(&self) -> bool {
        self.bar.is_some()
    }

    /// Adds expanded nodes to the run totals.
    fn add_expansions(&self, expansions: u64) {
        self.expansions.fetch_add(expansions, Ordering::Relaxed);
    }

    /// Updates the batch message with the run totals.
    fn set_totals(&self) {
        let elapsed = self.started.elapsed();
        let expansions = self.expansions.load(Ordering::Relaxed);
        self.set_message(format!(
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

/// A line of a worker in a parallel run.
///
/// The line is removed from the display when the guard is dropped.
pub struct WorkerLine {
    bar: ProgressBar,
    multi: MultiProgress,
}

impl WorkerLine {
    /// Updates the message of the line.
    pub fn set_message(&self, message: String) {
        self.bar.set_message(message);
    }
}

impl Drop for WorkerLine {
    fn drop(&mut self) {
        self.multi.remove(&self.bar);
    }
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

/// Returns how many worker lines fit below the batch line.
fn worker_lanes() -> usize {
    let rows = Term::stderr()
        .size_checked()
        .map_or(DEFAULT_ROWS, |(rows, _)| usize::from(rows));
    rows.saturating_sub(2).max(1)
}

/// Where a [`SearchReporter`] writes its message.
#[derive(Clone, Copy)]
enum Target<'a> {
    /// The batch line, used when levels are solved one at a time.
    Batch,
    /// The line of the worker that solves the level, used when levels are
    /// solved in parallel.
    Worker(Option<&'a WorkerLine>),
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
}

impl<'a> SearchReporter<'a> {
    /// Creates a reporter that writes the statistics of the level numbered
    /// `number` to the batch line.
    pub fn level(progress: &'a Progress, number: usize) -> Self {
        Self::new(progress, Target::Batch, number)
    }

    /// Creates a reporter that writes the statistics of the level numbered
    /// `number` to the line of a worker and feeds the run totals.
    pub fn worker(progress: &'a Progress, line: Option<&'a WorkerLine>, number: usize) -> Self {
        Self::new(progress, Target::Worker(line), number)
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
            Target::Batch => self.progress.set_message(message),
            Target::Worker(line) => {
                if let Some(line) = line {
                    line.set_message(message);
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
    fn on_search_start(&mut self, _root: &Node) {
        self.started = Instant::now();
        self.refresh();
    }

    fn on_node_expanded(&mut self, _node: &Node) {
        self.expansions += 1;
        if self.last_refresh.elapsed() >= REFRESH_INTERVAL {
            self.refresh();
        }
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
}
