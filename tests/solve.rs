//! Integration tests for the `solve` command.

use std::{
    fs,
    io::{BufRead as _, BufReader, Write as _},
    path::{Path, PathBuf},
    process::{Command as StdCommand, Stdio},
};

use assert_cmd::Command;
use serde_json::Value;

/// Returns the path of a test fixture.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Builds a command with deterministic output settings.
fn command() -> Command {
    let mut command = Command::cargo_bin("soukoban").expect("the binary is built");
    command.args(["--color", "never", "--progress", "never"]);
    command
}

/// Snapshots human readable output with volatile fields filtered.
fn assert_human_snapshot(name: &str, contents: &str) {
    let mut settings = insta::Settings::clone_current();
    settings.add_filter(r"\d+(?:\.\d+)?(?: ms|s)", "[elapsed]");
    settings.bind(|| insta::assert_snapshot!(name, contents));
}

/// Builds a large open level that the search cannot finish within a short
/// time limit.
fn large_open_level(size: usize) -> String {
    let mut level = String::new();
    level.push_str(&"#".repeat(size));
    level.push('\n');
    for y in 1..size - 1 {
        level.push('#');
        for x in 1..size - 1 {
            let symbol = match (x, y) {
                (1, 1) => '@',
                (x, y) if (x, y) == (size / 2, size / 2) => '$',
                (x, y) if (x, y) == (size - 2, size - 2) => '.',
                _ => ' ',
            };
            level.push(symbol);
        }
        level.push('#');
        level.push('\n');
    }
    level.push_str(&"#".repeat(size));
    level.push('\n');
    level
}

#[test]
fn json_output_matches_the_schema() {
    let output = command()
        .args(["solve", "--format", "json", "--level", "1"])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stderr.is_empty(),
        "stderr should stay empty in JSON format"
    );

    let document: Value = serde_json::from_slice(&output.stdout).expect("stdout is valid JSON");
    assert_eq!(document["schema"], 1);
    assert_eq!(document["command"], "solve");

    let results = document["results"].as_array().expect("results is an array");
    assert_eq!(results.len(), 1);
    let result = &results[0];
    assert_eq!(result["index"], 1);
    assert_eq!(result["title"], "First");
    assert_eq!(result["status"], "solved");
    assert_eq!(result["metadata"]["title"], "First");
    assert_eq!(result["metadata"]["author"], "Test Author");
    assert_eq!(result["solution"]["lurd"], "R");
    assert_eq!(result["solution"]["moves"], 1);
    assert_eq!(result["solution"]["pushes"], 1);
    assert_eq!(result["solution"]["box_lines"], 1);
    assert_eq!(result["solution"]["pushing_sessions"], 1);
    assert_eq!(result["solution"]["player_lines"], 1);
    assert_eq!(result["search"]["algorithm"], "astar");
    assert_eq!(result["search"]["strategy"], "quick");
    assert!(result["search"]["expansions"].is_u64());
    assert!(result["search"]["elapsed_ms"].is_number());
    assert!(result["error"].is_null());

    assert_eq!(document["summary"]["levels"], 1);
    assert_eq!(document["summary"]["solved"], 1);
    assert!(document["summary"]["elapsed_ms"].is_number());
}

#[test]
fn json_output_reports_all_level_statuses() {
    let output = command()
        .args(["solve", "--format", "json"])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let document: Value = serde_json::from_slice(&output.stdout).expect("stdout is valid JSON");
    let results = document["results"].as_array().expect("results is an array");
    assert_eq!(results.len(), 5);

    assert_eq!(results[0]["status"], "solved");
    assert_eq!(results[1]["status"], "unsolved");
    assert_eq!(results[2]["status"], "solved");
    assert_eq!(results[2]["solution"]["lurd"], "");
    assert!(results[2]["search"].is_null());
    assert_eq!(results[3]["status"], "unsolved");
    assert!(
        results[3]["search"].is_object(),
        "the search reports the rejected initial state"
    );
    assert_eq!(results[4]["status"], "solved");
    assert_eq!(results[4]["title"], "仓库");

    assert_eq!(document["summary"]["levels"], 5);
    assert_eq!(document["summary"]["solved"], 3);
}

#[test]
fn json_output_has_no_decorations_even_with_forced_progress() {
    let mut command = Command::cargo_bin("soukoban").expect("the binary is built");
    let output = command
        .args([
            "--color",
            "never",
            "--progress",
            "always",
            "solve",
            "--format",
            "json",
            "--level",
            "1",
        ])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty(), "the JSON format must stay quiet");
    assert!(output.stdout.starts_with(b"{\n"));
}

#[test]
fn human_single_level_detail() {
    let output = command()
        .args(["solve", "--level", "1"])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    assert_human_snapshot(
        "human_single_level_detail",
        &String::from_utf8_lossy(&output.stdout),
    );
}

#[test]
fn human_unsolved_detail() {
    let output = command()
        .args(["solve", "--level", "2"])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    assert_human_snapshot(
        "human_unsolved_detail",
        &String::from_utf8_lossy(&output.stdout),
    );
}

#[test]
fn human_stalled_detail() {
    let output = command()
        .args(["solve", "--level", "4"])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    assert_human_snapshot(
        "human_stalled_detail",
        &String::from_utf8_lossy(&output.stdout),
    );
}

#[test]
fn human_batch_table() {
    let output = command()
        .args(["solve"])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    assert_human_snapshot(
        "human_batch_table",
        &String::from_utf8_lossy(&output.stdout),
    );
}

#[test]
fn human_table_compresses_solutions() {
    let output = command()
        .args(["solve"])
        .arg(fixture("runs.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("5R"), "push runs must be compressed");
    assert!(stdout.contains("r2R"), "mixed solutions must be compressed");
    assert!(
        !stdout.contains("RRRRR"),
        "the uncompressed solution must not be shown"
    );
}

#[test]
fn human_detail_compresses_solutions() {
    let output = command()
        .args(["solve", "--level", "1"])
        .arg(fixture("runs.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("5R"), "push runs must be compressed");
    assert!(
        !stdout.contains("RRRRR"),
        "the uncompressed solution must not be shown"
    );

    let output = command()
        .args(["solve", "--level", "2"])
        .arg(fixture("runs.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("r2R"), "mixed solutions must be compressed");
}

#[test]
fn json_keeps_the_uncompressed_solution() {
    let output = command()
        .args(["solve", "--format", "json"])
        .arg(fixture("runs.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let document: Value = serde_json::from_slice(&output.stdout).expect("stdout is valid JSON");
    assert_eq!(document["results"][0]["solution"]["lurd"], "RRRRR");
    assert_eq!(document["results"][1]["solution"]["lurd"], "rRR");
}

#[test]
fn human_batch_summary() {
    let output = command()
        .args(["solve"])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    assert_human_snapshot(
        "human_batch_summary",
        &String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn already_solved_levels_are_reported_without_search() {
    let output = command()
        .args(["solve", "--level", "3"])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("already solved"));
    assert!(!stdout.contains("solved in"));
}

#[test]
fn selects_levels_in_selection_order() {
    let output = command()
        .args(["solve", "--format", "json", "--level", "2,4,1"])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let document: Value = serde_json::from_slice(&output.stdout).expect("stdout is valid JSON");
    let indexes = document["results"]
        .as_array()
        .expect("results is an array")
        .iter()
        .map(|result| result["index"].as_u64().expect("index is a number"))
        .collect::<Vec<_>>();
    assert_eq!(indexes, vec![2, 4, 1]);
}

#[test]
fn accepts_algorithm_and_strategy_options() {
    let output = command()
        .args([
            "solve",
            "--format",
            "json",
            "--algorithm",
            "ida-star",
            "--strategy",
            "push-optimal",
            "--level",
            "1",
        ])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let document: Value = serde_json::from_slice(&output.stdout).expect("stdout is valid JSON");
    let result = &document["results"][0];
    assert_eq!(result["status"], "solved");
    assert_eq!(result["search"]["algorithm"], "ida-star");
    assert_eq!(result["search"]["strategy"], "push-optimal");
}

#[test]
fn reads_levels_from_stdin() {
    let level = std::fs::read_to_string(fixture("single.xsb")).expect("the fixture is readable");
    let output = command()
        .args(["solve", "--format", "json", "-"])
        .write_stdin(level)
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let document: Value = serde_json::from_slice(&output.stdout).expect("stdout is valid JSON");
    assert_eq!(document["results"][0]["title"], "stdin #1");
    assert_eq!(document["results"][0]["solution"]["lurd"], "R");
}

#[test]
fn timeout_reports_timeout_status() {
    let output = command()
        .args(["solve", "--format", "json", "--timeout", "1ms", "-"])
        .write_stdin(large_open_level(200))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let document: Value = serde_json::from_slice(&output.stdout).expect("stdout is valid JSON");
    let result = &document["results"][0];
    assert_eq!(result["status"], "timeout");
    assert!(result["solution"].is_null());
    assert!(result["search"].is_object());
}

#[test]
fn ascii_mode_replaces_unicode_glyphs() {
    let output = command()
        .args(["--ascii", "solve", "--level", "1"])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("+-------+"));
    assert!(stdout.contains("| ##### |"));
    assert!(stdout.contains("v solved in"));
    assert!(!stdout.contains('╭'));
}

#[test]
fn level_maps_are_colorized() {
    let run = |level: &str| {
        let output = Command::cargo_bin("soukoban")
            .expect("the binary is built")
            .args([
                "--color",
                "always",
                "--progress",
                "never",
                "solve",
                "--level",
                level,
            ])
            .arg(fixture("basic.xsb"))
            .output()
            .expect("the command runs");
        assert_eq!(output.status.code(), Some(0));
        String::from_utf8(output.stdout).expect("stdout is UTF-8")
    };

    let stdout = run("1");
    assert!(stdout.contains("\u{1b}[94m#"), "walls are bright blue");
    assert!(stdout.contains("\u{1b}[96m@"), "the player is bright cyan");
    assert!(stdout.contains("\u{1b}[33m$"), "boxes are yellow");
    assert!(stdout.contains("\u{1b}[32m."), "goals are green");

    let stdout = run("3");
    assert!(
        stdout.contains("\u{1b}[92m*"),
        "boxes on goals are bright green"
    );
}

#[test]
fn auto_color_respects_the_color_environment() {
    let run = |variables: &[(&str, &str)]| {
        let mut command = Command::cargo_bin("soukoban").expect("the binary is built");
        command
            .env_remove("NO_COLOR")
            .env_remove("CLICOLOR")
            .env_remove("CLICOLOR_FORCE")
            .args(["--progress", "never", "solve", "--level", "1"])
            .arg(fixture("basic.xsb"));
        for &(name, value) in variables {
            command.env(name, value);
        }
        let output = command.output().expect("the command runs");
        assert_eq!(output.status.code(), Some(0));
        String::from_utf8(output.stdout).expect("stdout is UTF-8")
    };

    // The color choice defaults to `auto`, which `CLICOLOR_FORCE` turns into
    // color even when the stream is not a terminal.
    let forced = run(&[("CLICOLOR_FORCE", "1")]);
    assert!(forced.contains("\u{1b}["), "CLICOLOR_FORCE must colorize");

    // `NO_COLOR` takes precedence over `CLICOLOR_FORCE`.
    let disabled = run(&[("CLICOLOR_FORCE", "1"), ("NO_COLOR", "1")]);
    assert!(
        !disabled.contains("\u{1b}["),
        "NO_COLOR must disable colors"
    );
}

#[test]
fn batch_progress_shows_the_overall_bar() {
    let output = Command::cargo_bin("soukoban")
        .expect("the binary is built")
        .args(["--color", "never", "--progress", "always", "solve"])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("1/5"), "the batch position must be shown");
    assert!(
        stderr.contains("#1    · expanded"),
        "the level stats must be shown"
    );
    assert!(stderr.contains('█'), "the bar must have a filled part");
    assert!(stderr.contains('░'), "the bar must have a remaining part");
}

#[test]
fn single_level_progress_has_no_bar() {
    let output = Command::cargo_bin("soukoban")
        .expect("the binary is built")
        .args([
            "--color",
            "never",
            "--progress",
            "always",
            "solve",
            "--level",
            "1",
        ])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("#1    · expanded"));
    assert!(
        !stderr.contains("1/1"),
        "single levels show no batch position"
    );
    assert!(!stderr.contains('█'), "single levels use a plain spinner");
}

#[test]
fn parallel_runs_keep_the_selection_order() {
    let output = Command::cargo_bin("soukoban")
        .expect("the binary is built")
        .args([
            "--color",
            "never",
            "--progress",
            "never",
            "solve",
            "-j",
            "4",
            "--format",
            "json",
        ])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let document: Value = serde_json::from_slice(&output.stdout).expect("stdout is valid JSON");
    let results = document["results"].as_array().expect("results is an array");
    let indexes = results
        .iter()
        .map(|result| result["index"].as_u64().expect("index is a number"))
        .collect::<Vec<_>>();
    let statuses = results
        .iter()
        .map(|result| {
            result["status"]
                .as_str()
                .expect("status is a string")
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(indexes, vec![1, 2, 3, 4, 5]);
    assert_eq!(
        statuses,
        vec!["solved", "unsolved", "solved", "unsolved", "solved"]
    );
}

#[test]
fn parallel_progress_shows_the_run_totals_and_a_line_per_worker() {
    // The bar only advances when a level finishes, so the run must keep one
    // level alive while the others complete: otherwise a level-finishing draw
    // may never land before the display is cleared (the run is far shorter
    // than the redraw interval), and no filled cell would be captured.
    let mut input = large_open_level(200);
    input.push('\n');
    input.push_str(&fs::read_to_string(fixture("basic.xsb")).expect("the fixture is readable"));
    let output = Command::cargo_bin("soukoban")
        .expect("the binary is built")
        .args([
            "--color",
            "never",
            "--progress",
            "always",
            "solve",
            "-j",
            "4",
            "--timeout",
            "1s",
            "-",
        ])
        .write_stdin(input)
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The batch line reports the totals of the run.
    assert!(stderr.contains("expanded"), "the run totals must be shown");
    assert!(stderr.contains("speed"), "the run speed must be shown");
    assert!(stderr.contains('█'), "the bar must be shown");
    // Every worker reports the level it is solving on its own line.
    assert!(
        stderr.contains("#1    · expanded"),
        "the level of a worker must be shown"
    );
    assert!(
        stderr.contains("#2    · expanded"),
        "each worker must report its own level"
    );
}

#[test]
fn parallel_progress_advances_the_batch_spinner() {
    let level = large_open_level(200);
    let output = Command::cargo_bin("soukoban")
        .expect("the binary is built")
        .args([
            "--color",
            "never",
            "--progress",
            "always",
            "solve",
            "-j",
            "2",
            "--timeout",
            "300ms",
            "-",
        ])
        .write_stdin(format!("{level}\n{level}\n{level}"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let ticks = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";
    // The batch line looks like `⠹ █████▉...`; its spinner must step through
    // the tick characters while the run is active.
    let advanced = ticks
        .chars()
        .skip(1)
        .any(|tick| stderr.contains(&format!("{tick} ")));
    assert!(
        advanced,
        "the batch spinner must advance while the run is active"
    );
    // Worker lines look like `#1    · expanded ...`; they carry no spinner.
    assert!(
        !ticks
            .chars()
            .any(|tick| stderr.contains(&format!("{tick} #"))),
        "worker lines must not have a spinner"
    );
}

#[test]
fn verbose_output_reports_the_replay_check() {
    let output = command()
        .args(["-v", "solve", "--level", "1"])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("debug: solution replay verified"));
}

#[test]
fn missing_files_show_a_hint() {
    let output = command()
        .args(["solve", "definitely-missing.xsb"])
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error: failed to read `definitely-missing.xsb`"));
    assert!(stderr.contains("help: check the path, or use `-` to read from stdin"));
}

#[test]
fn parse_errors_are_reported() {
    let output = command()
        .args(["solve"])
        .arg(fixture("invalid.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error: failed to parse level 1 of `invalid.xsb`"));
    assert!(stderr.contains("caused by: mismatch between number of boxes and goals"));
}

#[test]
fn out_of_bounds_levels_show_the_level_count() {
    let output = command()
        .args(["solve", "--level", "9"])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error: level 9 out of bounds"));
    assert!(stderr.contains("help: the input has 5 levels"));
}

#[test]
fn empty_input_reports_no_levels() {
    let output = command()
        .args(["solve", "-"])
        .write_stdin("")
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error: no levels found in `stdin`"));
}

#[test]
fn usage_errors_exit_with_code_two() {
    let output = command()
        .args(["solve", "--timeout", "nope"])
        .arg(fixture("basic.xsb"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid duration `nope`"));
}

#[test]
fn broken_pipes_exit_silently() {
    let mut child = StdCommand::new(env!("CARGO_BIN_EXE_soukoban"))
        .args([
            "--color",
            "never",
            "--progress",
            "never",
            "solve",
            "--timeout",
            "1ms",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary starts");
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(large_open_level(200).as_bytes())
        .expect("stdin accepts the level");

    let mut reader = BufReader::new(child.stdout.take().expect("stdout is piped"));
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("the first line is readable");
    drop(reader);

    let status = child.wait().expect("the child exits");
    assert_eq!(status.code(), Some(0));
}
