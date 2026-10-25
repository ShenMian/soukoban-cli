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
use soukoban::{deadlock::compute_deadlocked_boxes, prelude::*};

use crate::{
    app::{Cancellation, Ctx},
    error::{self, Result, with_hint},
    input::{self, Input, LevelSpec},
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
    #[arg(value_parser = Input::parse)]
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
pub fn run(args: Args, ctx: &mut Ctx) -> Result<ExitCode> {
    let text = input::read(&args.input)?;
    let name = args.input.display_name();
    let levels = Level::load_from_str(&text).collect::<Vec<_>>();

    if levels.is_empty() {
        return Err(with_hint(
            anyhow::anyhow!("no levels found in `{name}`"),
            "check that the input is a valid XSB file",
        ));
    }

    let selected = args.level.select(levels.len()).map_err(|error| {
        with_hint(
            anyhow::Error::new(error),
            format!(
                "the input has {}",
                describe_count(levels.len(), "level", "levels")
            ),
        )
    })?;

    for &index in &selected {
        if let Err(error) = &levels[index] {
            return Err(anyhow::Error::new(error.clone())
                .context(format!("failed to parse level {} of `{name}`", index + 1)));
        }
    }

    let (queries, index) = build_index(&levels, &selected, &name);
    let started = Instant::now();
    let mut outcome = search_directory(&args, queries.len(), &index, &ctx.cancel)?;
    let elapsed = started.elapsed();
    outcome.summary.elapsed_ms = elapsed_ms(elapsed);
    render_results(
        ctx,
        &queries,
        &outcome.matches,
        &outcome.summary,
        &outcome.warnings,
        elapsed,
    )?;

    if outcome.interrupted {
        if !ctx.ui.is_json() {
            ctx.ui.warning("interrupted")?;
        }
        return Ok(ExitCode::from(error::EXIT_INTERRUPT));
    }
    Ok(ExitCode::from(error::EXIT_SUCCESS))
}

/// A parsed level of an input.
type LevelResult = std::result::Result<Level, ParseLevelError>;

/// A level of the input to search for.
struct Query {
    /// The 1-based level number in the input.
    index: usize,
    title: String,
}

/// A matching level found in the directory.
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

/// Prepares the query maps and indexes them for lookup.
fn build_index(
    levels: &[LevelResult],
    selected: &[usize],
    name: &str,
) -> (Vec<Query>, FxHashMap<Map, Vec<usize>>) {
    let mut queries = Vec::with_capacity(selected.len());
    let mut index = FxHashMap::default();

    for &level_index in selected {
        let level = levels[level_index]
            .as_ref()
            .expect("selected levels are parsed");
        let map = canonical_map(level.map());
        let query = queries.len();
        queries.push(Query {
            index: level_index + 1,
            title: resolve_title(level, name, level_index),
        });
        index.entry(map).or_insert_with(Vec::new).push(query);
    }

    (queries, index)
}

/// Searches the directory for the indexed maps.
fn search_directory(
    args: &Args,
    query_count: usize,
    index: &FxHashMap<Map, Vec<usize>>,
    cancel: &Cancellation,
) -> Result<SearchOutcome> {
    let (paths, mut warnings) = xsb_files(&args.dir)?;
    let mut matches = (0..query_count).map(|_| Vec::new()).collect::<Vec<_>>();
    let mut summary = Summary {
        patterns: query_count,
        ..Summary::default()
    };
    let mut interrupted = false;

    for path in paths {
        if cancel.is_requested() {
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
fn xsb_files(root: &Path) -> Result<(Vec<PathBuf>, Vec<String>)> {
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

/// Returns the canonical form of a map, or an exact copy when canonicalization
/// is unsafe.
///
/// `Map::canonicalize` assumes that every deadlocked box is on a goal.
fn canonical_map(map: &Map) -> Map {
    let mut map = map.clone();
    if map
        .goal_positions()
        .is_superset(&compute_deadlocked_boxes(&map))
    {
        map.canonicalize();
    }
    map
}

/// Returns `path` relative to the search root when possible.
fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Returns the display title of a level.
fn resolve_title(level: &Level, input_name: &str, index: usize) -> String {
    level
        .metadata()
        .get("title")
        .cloned()
        .unwrap_or_else(|| format!("{input_name} #{}", index + 1))
}

/// Writes the results of the run.
fn render_results(
    ctx: &mut Ctx,
    queries: &[Query],
    matches: &[Vec<Match>],
    summary: &Summary,
    warnings: &[String],
    elapsed: Duration,
) -> Result<()> {
    if ctx.ui.is_json() {
        let body = Body {
            results: queries
                .iter()
                .enumerate()
                .map(|(query_index, query)| ResultEntry {
                    index: query.index,
                    title: &query.title,
                    matches: matches[query_index]
                        .iter()
                        .map(|entry| MatchEntry {
                            file: &entry.file,
                            index: entry.index,
                            title: entry.title.as_deref(),
                            metadata: &entry.metadata,
                        })
                        .collect(),
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
    for (query_index, query) in queries.iter().enumerate() {
        let entries = &matches[query_index];
        if entries.is_empty() {
            table.push_row([
                Cell::new(query.index.to_string()),
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
                    query.index.to_string()
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
    writeln!(ctx.ui.err(), "{style}{}{style:#}", parts.join(" · "))?;
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
    matches: Vec<MatchEntry<'a>>,
}

/// A matching level in the JSON output.
#[derive(Serialize)]
struct MatchEntry<'a> {
    file: &'a str,
    index: usize,
    title: Option<&'a str>,
    metadata: &'a BTreeMap<String, String>,
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

/// Converts a duration to milliseconds, rounded to microsecond precision.
fn elapsed_ms(duration: Duration) -> f64 {
    (duration.as_secs_f64() * 1_000_000.0).round() / 1000.0
}
