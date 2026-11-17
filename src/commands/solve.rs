//! The `solve` command.

use std::{
    collections::BTreeMap,
    num::NonZero,
    process::ExitCode,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;
use soukoban::{
    prelude::*,
    solver::{Node, Observer, Solver},
};

use crate::{
    app::{Cancellation, Ctx},
    cli::{Algorithm, Strategy},
    error::{self, Result, with_hint},
    input::{self, Input, LevelSpec, SelectedLevel},
    ui::{
        json,
        progress::{self, Progress, SearchReporter},
        render::{self, LevelReport, SearchStats, SolveStatus, SolveSummary},
    },
};

/// Search for a solution to a level.
#[derive(clap::Args)]
pub struct Args {
    /// Level file to solve, or `-` to read from stdin.
    pub input: Input,

    /// Levels to solve, 1-based: `3`, `1..5`, `2,7,9`, or `all`.
    #[arg(
        short,
        long,
        value_name = "SPEC",
        default_value = "all",
        value_parser = input::parse_level_spec
    )]
    pub level: LevelSpec,

    /// Search algorithm.
    #[arg(short, long, value_name = "ALG", value_enum, default_value_t = Algorithm::Astar)]
    pub algorithm: Algorithm,

    /// Search strategy.
    #[arg(short, long, value_name = "STRAT", value_enum, default_value_t = Strategy::Quick)]
    pub strategy: Strategy,

    /// Time limit per level; a missing unit is interpreted as seconds.
    #[arg(long, value_name = "DURATION", value_parser = crate::cli::parse_duration)]
    pub timeout: Option<Duration>,

    /// Solve up to N levels in parallel; `0` uses all available cores.
    #[arg(short = 'j', long, value_name = "N", default_value_t = 1)]
    pub jobs: usize,
}

/// Runs the `solve` command.
pub fn run(args: &Args, ctx: &mut Ctx) -> Result<ExitCode> {
    if args.algorithm == Algorithm::Bfs && args.strategy == Strategy::MoveOptimal {
        anyhow::bail!("`--algorithm bfs` cannot be used with `--strategy move-optimal`");
    }

    let selected = input::load_levels(&args.input, &args.level)?;
    let jobs = resolve_jobs(args.jobs).min(selected.len());
    let started = Instant::now();
    let (processed, interrupted) = if jobs > 1 {
        solve_in_parallel(args, ctx, &selected, jobs)?
    } else {
        solve_in_sequence(args, ctx, &selected)?
    };
    let elapsed = started.elapsed();

    render_results(args, ctx, &processed, selected.len(), elapsed)?;

    if interrupted {
        if !ctx.ui.is_json() {
            ctx.ui.warning("interrupted")?;
        }
        return Ok(ExitCode::from(error::EXIT_INTERRUPT));
    }
    Ok(ExitCode::from(error::EXIT_SUCCESS))
}

/// Resolves the `--jobs` option into a number of worker threads.
fn resolve_jobs(jobs: usize) -> usize {
    if jobs == 0 {
        thread::available_parallelism().map_or(1, NonZero::get)
    } else {
        jobs
    }
}

/// Solves the selected levels one after another.
fn solve_in_sequence<'a>(
    args: &Args,
    ctx: &mut Ctx,
    selected: &'a [SelectedLevel],
) -> Result<(Vec<Processed<'a>>, bool)> {
    let total = selected.len();
    let bar = Progress::new(&ctx.ui, progress::enabled(&ctx.ui), total as u64, 1);
    let line = bar.level_line(0);
    let mut processed = Vec::with_capacity(total);
    let mut interrupted = false;

    for entry in selected {
        if ctx.cancel.is_requested() {
            interrupted = true;
            break;
        }
        // Levels that are already solved do not start a search, so their
        // statistics are never reported.
        let mut reporter = bar.begin_level(line.as_ref(), entry.number());
        let Some(outcome) = solve_level(&entry.level, args, &ctx.cancel, &mut reporter)? else {
            interrupted = true;
            break;
        };
        if matches!(outcome, Outcome::Solved { .. }) {
            bar.suspend(|| ctx.ui.debug("solution replay verified"))?;
        }
        bar.set_completed(processed.len() as u64 + 1);
        processed.push(Processed { entry, outcome });
    }
    bar.finish();
    Ok((processed, interrupted))
}

/// Solves the selected levels with `jobs` worker threads.
///
/// The workers take levels from a shared queue and send their outcomes to this
/// thread, which keeps the results in selection order, so that neither the
/// table nor the JSON document depend on which level finishes first.
fn solve_in_parallel<'a>(
    args: &Args,
    ctx: &mut Ctx,
    selected: &'a [SelectedLevel],
    jobs: usize,
) -> Result<(Vec<Processed<'a>>, bool)> {
    let total = selected.len();
    let bar = Progress::new(&ctx.ui, progress::enabled(&ctx.ui), total as u64, jobs);
    let cancel = ctx.cancel.clone();
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    let mut slots = (0..total)
        .map(|_| None)
        .collect::<Vec<Option<Processed<'a>>>>();
    let mut completed = 0u64;
    let mut failure: Option<anyhow::Error> = None;

    thread::scope(|scope| {
        for worker in 0..jobs {
            let sender = sender.clone();
            let cancel = cancel.clone();
            let bar = &bar;
            let next = &next;
            scope.spawn(move || {
                // The line of this worker; it is removed when the worker runs
                // out of levels or is cancelled.
                let line = bar.level_line(worker);
                while !cancel.is_requested() {
                    let position = next.fetch_add(1, Ordering::Relaxed);
                    let Some(entry) = selected.get(position) else {
                        break;
                    };
                    let mut reporter = bar.begin_level(line.as_ref(), entry.number());
                    let outcome = match solve_level(&entry.level, args, &cancel, &mut reporter) {
                        Ok(Some(outcome)) => outcome,
                        // Stopped by the user; the level is not reported.
                        Ok(None) => break,
                        Err(error) => {
                            // The collector reports the error and stops the
                            // remaining workers.
                            let _ = sender.send(Err(error));
                            break;
                        }
                    };
                    if sender.send(Ok((position, outcome))).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);

        for message in receiver {
            match message {
                Ok((position, outcome)) => {
                    if failure.is_some() {
                        continue;
                    }
                    if matches!(outcome, Outcome::Solved { .. })
                        && let Err(error) = bar.suspend(|| ctx.ui.debug("solution replay verified"))
                    {
                        failure = Some(error.into());
                        cancel.request();
                        continue;
                    }
                    slots[position] = Some(Processed {
                        entry: &selected[position],
                        outcome,
                    });
                    completed += 1;
                    bar.set_completed(completed);
                }
                Err(error) => {
                    // Stop the remaining workers; their levels are not
                    // reported.
                    failure.get_or_insert(error);
                    cancel.request();
                }
            }
        }
    });
    bar.finish();

    if let Some(error) = failure {
        return Err(error);
    }
    let interrupted = completed < total as u64;
    Ok((slots.into_iter().flatten().collect(), interrupted))
}

/// A processed level of the current run.
struct Processed<'a> {
    entry: &'a SelectedLevel,
    outcome: Outcome,
}

/// The outcome of solving one level.
enum Outcome {
    AlreadySolved,
    Solved {
        actions: Actions,
        stats: SearchStats,
    },
    NoSolution {
        stats: SearchStats,
    },
    Timeout {
        stats: SearchStats,
    },
}

impl Outcome {
    const fn status(&self) -> SolveStatus {
        match self {
            Self::AlreadySolved => SolveStatus::AlreadySolved,
            Self::Solved { .. } => SolveStatus::Solved,
            Self::NoSolution { .. } => SolveStatus::NoSolution,
            Self::Timeout { .. } => SolveStatus::Timeout,
        }
    }

    const fn actions(&self) -> Option<&Actions> {
        match self {
            Self::Solved { actions, .. } => Some(actions),
            Self::AlreadySolved | Self::NoSolution { .. } | Self::Timeout { .. } => None,
        }
    }

    const fn stats(&self) -> Option<SearchStats> {
        match self {
            Self::AlreadySolved => None,
            Self::Solved { stats, .. } | Self::NoSolution { stats } | Self::Timeout { stats } => {
                Some(*stats)
            }
        }
    }

    const fn is_solved(&self) -> bool {
        matches!(self, Self::Solved { .. } | Self::AlreadySolved)
    }
}

/// Solves one level, reporting its progress to `reporter`.
///
/// Returns `None` if the search was interrupted.
fn solve_level(
    level: &Level,
    args: &Args,
    cancel: &Cancellation,
    reporter: &mut SearchReporter<'_>,
) -> Result<Option<Outcome>> {
    if cancel.is_requested() {
        return Ok(None);
    }
    if level.is_solved() {
        return Ok(Some(Outcome::AlreadySolved));
    }

    let solver = Solver::new(level.map().clone(), args.strategy.into());
    if cancel.is_requested() {
        return Ok(None);
    }

    let deadline = args
        .timeout
        .and_then(|timeout| Instant::now().checked_add(timeout));
    let started = Instant::now();
    let mut observer = (&mut *reporter, StopGuard::new(&solver, cancel, deadline));
    let result = solver.search_with_observer(args.algorithm.into(), &mut observer);
    let stats = SearchStats {
        expansions: reporter.expansions(),
        elapsed: started.elapsed(),
    };

    match result {
        Ok(actions) => {
            verify_solution(level, &actions)?;
            Ok(Some(Outcome::Solved { actions, stats }))
        }
        Err(SearchError::NoSolution) => Ok(Some(Outcome::NoSolution { stats })),
        Err(SearchError::Interrupted) => {
            if cancel.is_requested() {
                Ok(None)
            } else {
                Ok(Some(Outcome::Timeout { stats }))
            }
        }
    }
}

/// Replays the solution on the level as a defensive check.
fn verify_solution(level: &Level, actions: &Actions) -> Result<()> {
    let mut replay = level.clone();
    let valid = replay
        .execute_batch(actions.iter().map(Action::direction))
        .is_ok()
        && replay.is_solved();
    if !valid {
        return Err(with_hint(
            anyhow::anyhow!("the solution returned by the solver is invalid"),
            "this is a bug, please report it",
        ));
    }
    Ok(())
}

/// Stops the solver when the cancellation flag is set or the deadline passes.
struct StopGuard<'a> {
    solver: &'a Solver,
    cancel: &'a Cancellation,
    deadline: Option<Instant>,
    stopped: bool,
}

impl<'a> StopGuard<'a> {
    const fn new(solver: &'a Solver, cancel: &'a Cancellation, deadline: Option<Instant>) -> Self {
        Self {
            solver,
            cancel,
            deadline,
            stopped: false,
        }
    }

    fn check(&mut self) {
        if !self.stopped
            && (self.cancel.is_requested()
                || self
                    .deadline
                    .is_some_and(|deadline| Instant::now() >= deadline))
        {
            self.stopped = true;
            self.solver.request_stop();
        }
    }
}

impl Observer for StopGuard<'_> {
    fn on_node_expanded(&mut self, _node: &Node) {
        self.check();
    }

    fn on_successor(&mut self, _parent: &Node, _child: &Node) {
        self.check();
    }
}

/// Writes the results of the run.
fn render_results(
    args: &Args,
    ctx: &mut Ctx,
    processed: &[Processed<'_>],
    total: usize,
    elapsed: Duration,
) -> Result<()> {
    let reports = processed
        .iter()
        .map(|item| LevelReport {
            number: item.entry.number(),
            title: &item.entry.title,
            map: item.entry.level.map(),
            status: item.outcome.status(),
            actions: item.outcome.actions(),
            stats: item.outcome.stats(),
        })
        .collect::<Vec<_>>();

    if ctx.ui.is_json() {
        let body = build_body(args, processed, elapsed);
        json::write(&mut ctx.ui, "solve", &body)?;
        return Ok(());
    }

    if total == 1 && !processed.is_empty() {
        render::solve_detail(
            &mut ctx.ui,
            &reports[0],
            args.algorithm.display_name(),
            args.strategy.id(),
            args.timeout,
        )?;
        return Ok(());
    }

    if !reports.is_empty() {
        render::solve_table(&mut ctx.ui, &reports)?;
        render::solve_summary(
            &mut ctx.ui,
            &SolveSummary {
                processed: reports.len(),
                total,
                solved: reports
                    .iter()
                    .filter(|report| {
                        matches!(
                            report.status,
                            SolveStatus::Solved | SolveStatus::AlreadySolved
                        )
                    })
                    .count(),
                expansions: reports
                    .iter()
                    .filter_map(|report| report.stats.map(|stats| stats.expansions))
                    .sum(),
                elapsed,
            },
        )?;
    }
    Ok(())
}

/// The JSON body of the `solve` command.
#[derive(Serialize)]
struct Body<'a> {
    results: Vec<ResultEntry<'a>>,
    summary: Summary,
}

/// A level result in the JSON output.
#[derive(Serialize)]
struct ResultEntry<'a> {
    index: usize,
    title: &'a str,
    status: &'static str,
    metadata: &'a BTreeMap<String, String>,
    solution: Option<Solution>,
    search: Option<Search>,
    error: Option<&'static str>,
}

/// A solution in the JSON output.
#[derive(Serialize)]
struct Solution {
    lurd: String,
    moves: usize,
    pushes: usize,
    box_lines: i32,
    box_changes: i32,
    pushing_sessions: i32,
    player_lines: i32,
}

impl Solution {
    fn from_actions(actions: &Actions) -> Self {
        let secondary = actions.secondary_values();
        Self {
            lurd: actions.to_string(),
            moves: actions.moves(),
            pushes: actions.shifts(),
            box_lines: secondary.box_lines,
            box_changes: secondary.box_changes,
            pushing_sessions: secondary.pushing_sessions,
            player_lines: secondary.player_lines,
        }
    }
}

/// Search statistics in the JSON output.
#[derive(Serialize)]
struct Search {
    algorithm: &'static str,
    strategy: &'static str,
    expansions: u64,
    elapsed_ms: f64,
}

impl Search {
    fn new(stats: SearchStats, algorithm: &'static str, strategy: &'static str) -> Self {
        Self {
            algorithm,
            strategy,
            expansions: stats.expansions,
            elapsed_ms: json::elapsed_ms(stats.elapsed),
        }
    }
}

/// The summary of a run in the JSON output.
#[derive(Serialize)]
struct Summary {
    levels: usize,
    solved: usize,
    elapsed_ms: f64,
}

fn build_body<'a>(args: &Args, processed: &'a [Processed<'_>], elapsed: Duration) -> Body<'a> {
    let algorithm = args.algorithm.id();
    let strategy = args.strategy.id();
    let results = processed
        .iter()
        .map(|item| {
            let (status, solution, search) = match &item.outcome {
                Outcome::AlreadySolved => (
                    "solved",
                    Some(Solution::from_actions(&Actions::new())),
                    None,
                ),
                Outcome::Solved { actions, stats } => (
                    "solved",
                    Some(Solution::from_actions(actions)),
                    Some(Search::new(*stats, algorithm, strategy)),
                ),
                Outcome::NoSolution { stats } => (
                    "unsolved",
                    None,
                    Some(Search::new(*stats, algorithm, strategy)),
                ),
                Outcome::Timeout { stats } => (
                    "timeout",
                    None,
                    Some(Search::new(*stats, algorithm, strategy)),
                ),
            };
            ResultEntry {
                index: item.entry.number(),
                title: &item.entry.title,
                status,
                metadata: item.entry.level.metadata(),
                solution,
                search,
                error: None,
            }
        })
        .collect();
    Body {
        results,
        summary: Summary {
            levels: processed.len(),
            solved: processed
                .iter()
                .filter(|item| item.outcome.is_solved())
                .count(),
            elapsed_ms: json::elapsed_ms(elapsed),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_the_jobs_option() {
        assert_eq!(resolve_jobs(1), 1);
        assert_eq!(resolve_jobs(4), 4);
        assert!(
            resolve_jobs(0) >= 1,
            "0 must resolve to a usable core count"
        );
    }
}
