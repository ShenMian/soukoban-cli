//! Input reading and level selection.

use std::{
    ffi::OsString,
    fmt::{self, Display},
    fs,
    io::{self, Read as _},
    path::PathBuf,
};

use anyhow::Context as _;
use soukoban::prelude::*;

use crate::{
    error::{Result, with_hint},
    ui::render::describe_count,
};

/// A level input source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// The standard input.
    Stdin,
    /// A file path.
    File(PathBuf),
}

impl Input {
    /// Returns a short human readable name of the input.
    pub fn display_name(&self) -> String {
        match self {
            Self::Stdin => "stdin".to_string(),
            Self::File(path) => path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            ),
        }
    }
}

impl From<OsString> for Input {
    fn from(value: OsString) -> Self {
        if value == "-" {
            Self::Stdin
        } else {
            Self::File(PathBuf::from(value))
        }
    }
}

impl From<&str> for Input {
    fn from(value: &str) -> Self {
        OsString::from(value).into()
    }
}

/// Reads an input source into a string.
pub fn read(input: &Input) -> Result<String> {
    match input {
        Input::Stdin => {
            let mut text = String::new();
            io::stdin()
                .read_to_string(&mut text)
                .map_err(anyhow::Error::from)
                .context("failed to read from stdin")
                .map_err(|error| with_hint(error, "check that the input is valid UTF-8"))?;
            Ok(text)
        }
        Input::File(path) => fs::read_to_string(path)
            .with_context(|| format!("failed to read `{}`", path.display()))
            .map_err(|error| with_hint(error, "check the path, or use `-` to read from stdin")),
    }
}

/// A parsed level selected from an input source.
#[derive(Clone, Debug)]
pub struct SelectedLevel {
    /// The 0-based index of the level in the input.
    pub index: usize,
    /// The display title of the level.
    pub title: String,
    /// The parsed level.
    pub level: Level,
}

impl SelectedLevel {
    /// Returns the 1-based number of the level in the input.
    pub const fn number(&self) -> usize {
        self.index + 1
    }
}

/// Reads `input`, resolves `spec`, and parses every selected level.
pub fn load_levels(input: &Input, spec: &LevelSpec) -> Result<Vec<SelectedLevel>> {
    let text = read(input)?;
    let name = input.display_name();
    let count = Level::load_from_str(&text).count();

    if count == 0 {
        return Err(with_hint(
            anyhow::anyhow!("no levels found in `{name}`"),
            "check that the input is a valid XSB file",
        ));
    }

    let indices = spec.select(count).map_err(|error| {
        with_hint(
            anyhow::Error::new(error),
            format!("the input has {}", describe_count(count, "level", "levels")),
        )
    })?;

    let mut slots: Vec<Option<std::result::Result<Level, ParseLevelError>>> =
        (0..count).map(|_| None).collect();
    let mut sorted = indices.clone();
    sorted.sort_unstable();

    let mut iter = Level::load_from_str(&text);
    let mut cursor = 0;
    for index in sorted {
        let result = iter
            .nth(index - cursor)
            .expect("selected index is within bounds");
        slots[index] = Some(result);
        cursor = index + 1;
    }

    let mut selected = Vec::with_capacity(indices.len());
    for index in indices {
        let result = slots[index]
            .take()
            .expect("selected indices are unique and parsed");
        let level = result.map_err(|error| {
            anyhow::Error::new(error)
                .context(format!("failed to parse level {} of `{name}`", index + 1))
        })?;
        let title = resolve_title(&level, &name, index);
        selected.push(SelectedLevel {
            index,
            title,
            level,
        });
    }
    Ok(selected)
}

/// Returns the display title of a level.
pub fn resolve_title(level: &Level, input_name: &str, index: usize) -> String {
    level
        .metadata()
        .get("title")
        .cloned()
        .unwrap_or_else(|| format!("{input_name} #{}", index + 1))
}

/// One segment of a level selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Segment {
    /// A single 1-based level number.
    Single(usize),
    /// An inclusive 1-based level range.
    Range(usize, usize),
}

impl Segment {
    fn describe(self) -> String {
        match self {
            Self::Single(level) => format!("level {level}"),
            Self::Range(start, end) => format!("level range {start}..{end}"),
        }
    }
}

/// A parsed `--level` selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelSpec {
    segments: Vec<Segment>,
    all: bool,
}

impl LevelSpec {
    /// Resolves the selection into zero-based level indices.
    ///
    /// Indices are deduplicated in selection order.
    pub fn select(&self, count: usize) -> std::result::Result<Vec<usize>, SelectError> {
        if self.all {
            return Ok((0..count).collect());
        }
        let mut selected = Vec::new();
        let mut seen = vec![false; count];
        for &segment in &self.segments {
            let (start, end) = match segment {
                Segment::Single(level) => (level, level),
                Segment::Range(start, end) => (start, end),
            };
            if end > count {
                return Err(SelectError::out_of_bounds(segment.describe()));
            }
            for level in start..=end {
                let index = level - 1;
                if !seen[index] {
                    seen[index] = true;
                    selected.push(index);
                }
            }
        }
        Ok(selected)
    }
}

/// Parses a level selection such as `all`, `3`, `1..5` or `2,7,9`.
pub fn parse_level_spec(text: &str) -> std::result::Result<LevelSpec, String> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("all") {
        return Ok(LevelSpec {
            segments: Vec::new(),
            all: true,
        });
    }

    let mut segments = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(format!("invalid level selection `{text}`"));
        }
        let segment = if let Some((start, end)) = part.split_once("..") {
            let start =
                parse_level_number(start).map_err(|_| format!("invalid level range `{part}`"))?;
            let end =
                parse_level_number(end).map_err(|_| format!("invalid level range `{part}`"))?;
            if start > end {
                return Err(format!("invalid level range `{part}`"));
            }
            Segment::Range(start, end)
        } else {
            Segment::Single(parse_level_number(part)?)
        };
        segments.push(segment);
    }
    Ok(LevelSpec {
        segments,
        all: false,
    })
}

fn parse_level_number(text: &str) -> std::result::Result<usize, String> {
    let level = text
        .parse::<usize>()
        .map_err(|_| format!("invalid level `{text}`"))?;
    if level == 0 {
        return Err("level numbers start at 1".to_string());
    }
    Ok(level)
}

/// An error resolving a [`LevelSpec`] against the levels of an input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectError {
    description: String,
}

impl SelectError {
    fn out_of_bounds(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }
}

impl Display for SelectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} out of bounds", self.description)
    }
}

impl std::error::Error for SelectError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(text: &str) -> LevelSpec {
        parse_level_spec(text).expect("valid spec")
    }

    #[test]
    fn parses_and_selects_all() {
        assert_eq!(spec("all").select(3).unwrap(), vec![0, 1, 2]);
        assert_eq!(spec("ALL").select(2).unwrap(), vec![0, 1]);
    }

    #[test]
    fn parses_single_levels() {
        assert_eq!(spec("3").select(5).unwrap(), vec![2]);
    }

    #[test]
    fn parses_ranges() {
        assert_eq!(spec("1..5").select(5).unwrap(), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn parses_lists_and_combinations() {
        assert_eq!(spec("2,7,9").select(10).unwrap(), vec![1, 6, 8]);
        assert_eq!(spec("1..3,10").select(10).unwrap(), vec![0, 1, 2, 9]);
    }

    #[test]
    fn deduplicates_in_selection_order() {
        assert_eq!(spec("3,2..4,2").select(5).unwrap(), vec![2, 1, 3]);
    }

    #[test]
    fn rejects_invalid_specs() {
        for text in [
            "", "  ", "0", "1..", "..5", "5..3", "1,,2", ",1", "1,", "x", "1.5",
        ] {
            assert!(
                parse_level_spec(text).is_err(),
                "spec `{text}` should be rejected"
            );
        }
    }

    #[test]
    fn rejects_out_of_bounds_selections() {
        assert_eq!(
            spec("1..5").select(3).unwrap_err().to_string(),
            "level range 1..5 out of bounds"
        );
        assert_eq!(
            spec("9").select(3).unwrap_err().to_string(),
            "level 9 out of bounds"
        );
    }

    #[test]
    fn displays_input_names() {
        assert_eq!(Input::from("-").display_name(), "stdin");
        assert_eq!(Input::from("levels.xsb").display_name(), "levels.xsb");
        assert_eq!(Input::from("dir/levels.xsb").display_name(), "levels.xsb");
    }

    #[test]
    fn loads_only_selected_levels_in_requested_order() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("basic.xsb");
        let loaded = load_levels(&Input::File(path), &spec("3,1")).expect("levels load");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].number(), 3);
        assert_eq!(loaded[1].number(), 1);
        assert_eq!(loaded[1].title, "First");
    }
}
