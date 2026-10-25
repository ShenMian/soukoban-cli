//! Integration tests for the `search` command.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

/// Returns the path of a test fixture.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Returns the fixture directory searched by the tests.
fn collections() -> PathBuf {
    fixture("collections")
}

/// Builds a command with deterministic output settings.
fn command() -> Command {
    let mut command = Command::cargo_bin("soukoban").expect("the binary is built");
    command.args(["--color", "never", "--progress", "never"]);
    command
}

/// Normalizes path separators for assertions that run on every platform.
fn normalize_paths(text: &str) -> String {
    text.replace('\\', "/")
}

#[test]
fn search_finds_canonical_matches_by_default() {
    let output = command()
        .args(["search"])
        .arg(fixture("single.xsb"))
        .arg("--dir")
        .arg(collections())
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let stdout = normalize_paths(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("single.xsb #1"));
    assert!(stdout.contains("found.xsb"));
    assert!(stdout.contains("Found"));
    assert!(stdout.contains("nested/rotated.xsb"));
    assert!(stdout.contains("Rotated"));
    assert!(!stdout.contains("unrelated.xsb"));
    assert!(!stdout.contains("ignored.txt"));
    assert!(!stdout.contains("no match"));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("skipped level 1 of `broken.xsb`"));
    assert!(stderr.contains("2 matches"));
    assert!(stderr.contains("1 skipped level"));
    let summary = stderr.lines().last().expect("the summary line is written");
    let elapsed = summary.rsplit(" · ").next().expect("the time is written");
    assert!(
        elapsed.ends_with("ms") || elapsed.ends_with('s'),
        "the summary must show the elapsed time: {summary}"
    );
}

#[test]
fn summary_uses_the_same_style_as_solve() {
    let output = Command::cargo_bin("soukoban")
        .expect("the binary is built")
        .args(["--color", "always", "--progress", "never", "search"])
        .arg(fixture("single.xsb"))
        .arg("--dir")
        .arg(collections())
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let summary = stderr.lines().last().expect("the summary line is written");
    assert!(
        summary.contains("\u{1b}[2m"),
        "the summary must use the muted style: {summary}"
    );
}

#[test]
fn json_output_matches_the_schema() {
    let output = command()
        .args(["search", "--format", "json"])
        .arg(fixture("single.xsb"))
        .arg("--dir")
        .arg(collections())
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stderr.is_empty(),
        "stderr should stay empty in JSON format"
    );

    let document: Value = serde_json::from_slice(&output.stdout).expect("stdout is valid JSON");
    assert_eq!(document["schema"], 1);
    assert_eq!(document["command"], "search");

    let results = document["results"].as_array().expect("results is an array");
    assert_eq!(results.len(), 1);
    let result = &results[0];
    assert_eq!(result["index"], 1);
    assert_eq!(result["title"], "single.xsb #1");

    let matches = result["matches"].as_array().expect("matches is an array");
    assert_eq!(matches.len(), 2);
    let files = matches
        .iter()
        .map(|entry| normalize_paths(entry["file"].as_str().expect("the file is a string")))
        .collect::<Vec<_>>();
    assert_eq!(files, vec!["found.xsb", "nested/rotated.xsb"]);
    assert_eq!(matches[0]["index"], 1);
    assert_eq!(matches[0]["title"], "Found");
    assert_eq!(matches[0]["metadata"]["title"], "Found");
    assert_eq!(matches[1]["title"], "Rotated");

    let summary = &document["summary"];
    assert_eq!(summary["patterns"], 1);
    assert_eq!(summary["files"], 4);
    assert_eq!(summary["levels"], 3);
    assert_eq!(summary["matches"], 2);
    assert_eq!(summary["skipped_files"], 0);
    assert_eq!(summary["skipped_levels"], 1);
    assert!(summary["elapsed_ms"].is_number());
    assert!(summary.get("canonical").is_none());
}

#[test]
fn no_matches_exit_successfully() {
    let output = command()
        .args(["search", "--level", "2"])
        .arg(fixture("basic.xsb"))
        .arg("--dir")
        .arg(collections())
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No Solution"));
    assert!(stdout.contains("no match"));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("0 matches"));
}

#[test]
fn reads_the_query_from_stdin() {
    let level = std::fs::read_to_string(fixture("single.xsb")).expect("the fixture is readable");
    let output = command()
        .args(["search", "--format", "json", "-", "--dir"])
        .arg(collections())
        .write_stdin(level)
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let document: Value = serde_json::from_slice(&output.stdout).expect("stdout is valid JSON");
    assert_eq!(document["results"][0]["title"], "stdin #1");
    assert_eq!(
        document["results"][0]["matches"].as_array().map(Vec::len),
        Some(2)
    );
}

#[test]
fn search_falls_back_for_unsolvable_levels() {
    let output = command()
        .args(["search", "--level", "2"])
        .arg(fixture("basic.xsb"))
        .arg("--dir")
        .arg(fixture("unsolvable"))
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(0));
    let stdout = normalize_paths(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("unsolvable.xsb"));
    assert!(stdout.contains("No Solution"));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("1 match"));
}

#[test]
fn missing_directories_show_a_hint() {
    let output = command()
        .args(["search"])
        .arg(fixture("single.xsb"))
        .args(["--dir", "definitely-missing"])
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error: failed to read directory `definitely-missing`"));
    assert!(stderr.contains("help: check the directory path"));
}

#[test]
fn invalid_queries_are_reported() {
    let output = command()
        .args(["search"])
        .arg(fixture("invalid.xsb"))
        .arg("--dir")
        .arg(collections())
        .output()
        .expect("the command runs");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error: failed to parse level 1 of `invalid.xsb`"));
    assert!(stderr.contains("caused by: mismatch between number of boxes and goals"));
}
