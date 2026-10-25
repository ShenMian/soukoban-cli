//! Input reading and level selection.

use std::{
    fmt::{self, Display},
    fs,
    io::{self, Read as _},
    path::PathBuf,
};

use anyhow::Context as _;

use crate::error::with_hint;

/// A level input source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// The standard input.
    Stdin,
    /// A file path.
    File(PathBuf),
}

impl Input {
    /// Parses a command line argument; `-` selects the standard input.
    pub fn parse(text: &str) -> std::result::Result<Self, String> {
        Ok(if text == "-" {
            Self::Stdin
        } else {
            Self::File(PathBuf::from(text))
        })
    }

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

/// Reads an input source into a string.
pub fn read(input: &Input) -> anyhow::Result<String> {
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
                if !selected.contains(&index) {
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
        assert_eq!(Input::parse("-").unwrap().display_name(), "stdin");
        assert_eq!(
            Input::parse("levels.xsb").unwrap().display_name(),
            "levels.xsb"
        );
        assert_eq!(
            Input::parse("dir/levels.xsb").unwrap().display_name(),
            "levels.xsb"
        );
    }
}
