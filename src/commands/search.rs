//! The `search` command.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{Duration, Instant},
};

use anyhow::Context as _;
use serde::Serialize;
use soukoban::prelude::*;

use crate::{
    app::{Cancellation, Ctx},
    error::{Result, with_hint},
    input::{self, Input, LevelSpec, SelectedLevel},
    ui::{
        json,
        render::{describe_count, format_elapsed},
        table::{self, Align, Cell, Column, Table},
    },
};

/// Find levels in the `.xsb` files of a directory.
#[derive(clap::Args)]
pub struct Args {
    /// Level file to search for, or `-` to read from stdin.
    pub input: Input,

    /// Directory to search recursively for `.xsb` files.
    #[arg(short, long, value_name = "DIR", default_value = ".")]
    pub dir: PathBuf,

    /// Levels of the input to search for, 1-based: `3`, `1..5`, `2,7,9`, or
    /// `all`.
    #[arg(
        short,
        long,
        value_name = "SPEC",
        default_value = "all",
        value_parser = input::parse_level_spec
    )]
    pub level: LevelSpec,
}

/// Runs the `search` command.
pub fn run(args: &Args, ctx: &mut Ctx) -> Result<ExitCode> {
    let queries = input::load_levels(&args.input, &args.level)?;
    let index = build_index(&queries);
    let started = Instant::now();
    let mut outcome = search_directory(args, queries.len(), &index, &ctx.cancel)?;
    let elapsed = started.elapsed();
    outcome.summary.elapsed_ms = json::elapsed_ms(elapsed);
    render_results(
        ctx,
        &queries,
        &outcome.matches,
        &outcome.summary,
        &outcome.warnings,
        elapsed,
    )?;

    ctx.finish(outcome.interrupted)
}

/// A matching level found in the directory.
#[derive(Serialize)]
struct Match {
    file: String,
    index: usize,
    title: Option<String>,
    metadata: BTreeMap<String, String>,
}

/// The outcome of searching a directory.
struct SearchOutcome {
    matches: Vec<Vec<Match>>,
    summary: Summary,
    warnings: Vec<String>,
    interrupted: bool,
}

/// Indexes the canonicalized query maps by position in `queries`.
fn build_index(queries: &[SelectedLevel]) -> FxHashMap<Map, Vec<usize>> {
    let mut index: FxHashMap<Map, Vec<usize>> = FxHashMap::default();
    for (query_index, query) in queries.iter().enumerate() {
        let map = canonical_map(query.level.map());
        index.entry(map).or_default().push(query_index);
    }
    index
}

/// Searches the directory for the indexed maps.
fn search_directory(
    args: &Args,
    query_count: usize,
    index: &FxHashMap<Map, Vec<usize>>,
    cancel: &Cancellation,
) -> Result<SearchOutcome> {
    let (paths, mut warnings) = xsb_files(&args.dir, cancel)?;
    let mut matches = (0..query_count).map(|_| Vec::new()).collect::<Vec<_>>();
    let mut summary = Summary {
        patterns: query_count,
        ..Summary::default()
    };
    let mut interrupted = cancel.is_requested();

    'files: for path in paths {
        if interrupted || cancel.is_requested() {
            interrupted = true;
            break;
        }

        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                summary.skipped_files += 1;
                warnings.push(format!("skipped `{}`: {error}", path.display()));
                continue;
            }
        };
        summary.files += 1;
        let file = display_path(&args.dir, &path);

        for (level_index, level) in Level::load_from_str(&text).enumerate() {
            if cancel.is_requested() {
                interrupted = true;
                break 'files;
            }
            let level = match level {
                Ok(level) => level,
                Err(error) => {
                    summary.skipped_levels += 1;
                    warnings.push(format!(
                        "skipped level {} of `{file}`: {error}",
                        level_index + 1
                    ));
                    continue;
                }
            };
            summary.levels += 1;

            let map = canonical_map(level.map());
            let Some(query_indexes) = index.get(&map) else {
                continue;
            };

            for &query in query_indexes {
                matches[query].push(Match {
                    file: file.clone(),
                    index: level_index + 1,
                    title: level.metadata().get("title").cloned(),
                    metadata: level.metadata().clone(),
                });
                summary.matches += 1;
            }
        }
    }

    Ok(SearchOutcome {
        matches,
        summary,
        warnings,
        interrupted,
    })
}

/// Returns every `.xsb` file below `root`, sorted by path.
fn xsb_files(root: &Path, cancel: &Cancellation) -> Result<(Vec<PathBuf>, Vec<String>)> {
    let metadata = fs::metadata(root)
        .with_context(|| format!("failed to read directory `{}`", root.display()))
        .map_err(|error| with_hint(error, "check the directory path"))?;
    if !metadata.is_dir() {
        return Err(with_hint(
            anyhow::anyhow!("`{}` is not a directory", root.display()),
            "pass a directory containing `.xsb` files",
        ));
    }

    let mut files = Vec::new();
    let mut warnings = Vec::new();
    let mut pending = vec![root.to_path_buf()];

    while let Some(dir) = pending.pop() {
        if cancel.is_requested() {
            return Ok((files, warnings));
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if dir == root => {
                return Err(anyhow::Error::new(error)
                    .context(format!("failed to read directory `{}`", dir.display())));
            }
            Err(error) => {
                warnings.push(format!("skipped directory `{}`: {error}", dir.display()));
                continue;
            }
        };
        for entry in entries {
            if cancel.is_requested() {
                return Ok((files, warnings));
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warnings.push(format!("skipped entry in `{}`: {error}", dir.display()));
                    continue;
                }
            };
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    warnings.push(format!("skipped `{}`: {error}", path.display()));
                    continue;
                }
            };
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() && is_xsb(&path) {
                files.push(path);
            }
        }
    }

    files.sort();
    Ok((files, warnings))
}

/// Returns `true` if `path` has the `.xsb` extension, ignoring ASCII case.
fn is_xsb(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("xsb"))
}

/// Returns the canonical form of a map.
fn canonical_map(map: &Map) -> Map {
    let mut map = map.clone();
    map.canonicalize();
    map
}

/// Returns `path` relative to the search root when possible.
fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Writes the results of the run.
fn render_results(
    ctx: &mut Ctx,
    queries: &[SelectedLevel],
    matches: &[Vec<Match>],
    summary: &Summary,
    warnings: &[String],
    elapsed: Duration,
) -> Result<()> {
    if ctx.ui.is_json() {
        let body = Body {
            results: queries
                .iter()
                .zip(matches)
                .map(|(query, entries)| ResultEntry {
                    index: query.number(),
                    title: &query.title,
                    matches: entries,
                })
                .collect(),
            summary,
        };
        json::write(&mut ctx.ui, "search", &body)?;
        return Ok(());
    }

    for warning in warnings {
        ctx.ui.warning(warning)?;
    }

    let mut table = Table::new([
        Column::new("#", Align::Right),
        Column::new("Query", Align::Left).shrinkable(),
        Column::new("File", Align::Left).shrinkable(),
        Column::new("Level", Align::Right),
        Column::new("Title", Align::Left).shrinkable(),
    ]);
    for (query, entries) in queries.iter().zip(matches) {
        if entries.is_empty() {
            table.push_row([
                Cell::new(query.number().to_string()),
                Cell::new(query.title.as_str()),
                Cell::new("no match"),
                Cell::new("-"),
                Cell::new("-"),
            ]);
            continue;
        }
        for (position, entry) in entries.iter().enumerate() {
            let first = position == 0;
            table.push_row([
                Cell::new(if first {
                    query.number().to_string()
                } else {
                    String::new()
                }),
                Cell::new(if first { query.title.as_str() } else { "" }),
                Cell::new(entry.file.as_str()),
                Cell::new(entry.index.to_string()),
                Cell::new(entry.title.as_deref().unwrap_or("-")),
            ]);
        }
    }
    table.render(&mut ctx.ui, table::terminal_width())?;

    let mut parts = vec![
        describe_count(summary.patterns, "pattern", "patterns"),
        describe_count(summary.files, "file", "files"),
        describe_count(summary.levels, "level", "levels"),
        describe_count(summary.matches, "match", "matches"),
    ];
    if summary.skipped_files > 0 {
        parts.push(describe_count(
            summary.skipped_files,
            "skipped file",
            "skipped files",
        ));
    }
    if summary.skipped_levels > 0 {
        parts.push(describe_count(
            summary.skipped_levels,
            "skipped level",
            "skipped levels",
        ));
    }
    parts.push(format_elapsed(elapsed));

    let style = ctx.ui.theme().muted;
    let sep = ctx.ui.theme().glyphs.separator;
    writeln!(ctx.ui.err(), "{style}{}{style:#}", parts.join(sep))?;
    Ok(())
}

/// The JSON body of the `search` command.
#[derive(Serialize)]
struct Body<'a> {
    results: Vec<ResultEntry<'a>>,
    summary: &'a Summary,
}

/// A query result in the JSON output.
#[derive(Serialize)]
struct ResultEntry<'a> {
    index: usize,
    title: &'a str,
    matches: &'a [Match],
}

/// The summary of a run in the JSON output.
#[derive(Clone, Debug, Default, Serialize)]
struct Summary {
    patterns: usize,
    files: usize,
    levels: usize,
    matches: usize,
    skipped_files: usize,
    skipped_levels: usize,
    elapsed_ms: f64,
}
