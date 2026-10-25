//! A lightweight table renderer aware of Unicode display widths.

use std::io::{self};

use anstyle::Style;
use console::Term;
use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

use super::{Ui, theme::Theme};

/// The number of spaces between two columns.
const COLUMN_GAP: usize = 2;
/// The width used when the terminal size cannot be determined.
const DEFAULT_WIDTH: usize = 80;
/// The smallest width a shrinkable column is reduced to.
const MIN_COLUMN_WIDTH: usize = 1;

/// Horizontal alignment of a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// Align the content to the left.
    Left,
    /// Align the content to the right.
    Right,
}

/// A table column.
#[derive(Clone, Debug)]
pub struct Column {
    header: String,
    align: Align,
    shrink: bool,
}

impl Column {
    /// Creates a column with the given header and alignment.
    pub fn new(header: impl Into<String>, align: Align) -> Self {
        Self {
            header: header.into(),
            align,
            shrink: false,
        }
    }

    /// Marks the column as shrinkable when the table overflows.
    #[must_use]
    pub fn shrinkable(mut self) -> Self {
        self.shrink = true;
        self
    }
}

/// A table cell.
#[derive(Clone, Debug)]
pub struct Cell {
    text: String,
    style: Option<Style>,
}

impl Cell {
    /// Creates an unstyled cell.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: None,
        }
    }

    /// Creates a styled cell.
    pub fn styled(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style: Some(style),
        }
    }
}

/// A text table that adapts to the terminal width.
pub struct Table {
    columns: Vec<Column>,
    rows: Vec<Vec<Cell>>,
}

impl Table {
    /// Creates an empty table with the given columns.
    pub fn new(columns: impl IntoIterator<Item = Column>) -> Self {
        Self {
            columns: columns.into_iter().collect(),
            rows: Vec::new(),
        }
    }

    /// Appends a row with one cell per column.
    pub fn push_row(&mut self, cells: impl IntoIterator<Item = Cell>) {
        let row: Vec<Cell> = cells.into_iter().collect();
        debug_assert_eq!(row.len(), self.columns.len());
        self.rows.push(row);
    }

    /// Renders the table to stdout, truncated to `max_width` columns.
    pub fn render(&self, ui: &mut Ui, max_width: usize) -> io::Result<()> {
        let theme = *ui.theme();
        let rows = self.layout(max_width, &theme);
        let gap = " ".repeat(COLUMN_GAP);
        let writer = ui.out();
        for row in &rows {
            let mut first = true;
            for cell in row {
                if !first {
                    write!(writer, "{gap}")?;
                }
                first = false;
                match cell.style {
                    Some(style) => write!(writer, "{style}{}{style:#}", cell.text)?,
                    None => write!(writer, "{}", cell.text)?,
                }
            }
            writeln!(writer)?;
        }
        Ok(())
    }

    /// Computes the padded and truncated text of every cell.
    fn layout(&self, max_width: usize, theme: &Theme) -> Vec<Vec<Cell>> {
        let mut widths: Vec<usize> = self
            .columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                let header = display_width(&column.header);
                self.rows
                    .iter()
                    .map(|row| display_width(&row[index].text))
                    .fold(header, usize::max)
            })
            .collect();
        shrink_to_fit(&self.columns, &mut widths, max_width);

        let mut rows = Vec::with_capacity(self.rows.len() + 1);
        rows.push(trim_last(
            self.columns
                .iter()
                .zip(&widths)
                .map(|(column, &width)| Cell {
                    text: pad(&column.header, width, column.align, theme.ellipsis()),
                    style: Some(theme.label),
                })
                .collect(),
        ));
        for row in &self.rows {
            rows.push(trim_last(
                row.iter()
                    .zip(&self.columns)
                    .zip(&widths)
                    .map(|((cell, column), &width)| Cell {
                        text: pad(&cell.text, width, column.align, theme.ellipsis()),
                        style: cell.style,
                    })
                    .collect(),
            ));
        }
        rows
    }
}

/// Removes unused padding from the last cell of a row.
fn trim_last(mut row: Vec<Cell>) -> Vec<Cell> {
    if let Some(cell) = row.last_mut() {
        let trimmed = cell.text.trim_end().len();
        cell.text.truncate(trimmed);
    }
    row
}

/// Returns the width of the terminal attached to stdout, in columns.
pub fn terminal_width() -> usize {
    Term::stdout()
        .size_checked()
        .map_or(DEFAULT_WIDTH, |(_, width)| usize::from(width))
}

/// Reduces shrinkable columns from right to left until the table fits.
fn shrink_to_fit(columns: &[Column], widths: &mut [usize], max_width: usize) {
    let total: usize = widths.iter().sum::<usize>() + COLUMN_GAP * columns.len().saturating_sub(1);
    let mut excess = total.saturating_sub(max_width);
    for index in (0..columns.len()).rev() {
        if excess == 0 {
            break;
        }
        if !columns[index].shrink {
            continue;
        }
        let width = &mut widths[index];
        let reduction = excess.min(width.saturating_sub(MIN_COLUMN_WIDTH));
        *width -= reduction;
        excess -= reduction;
    }
}

/// Returns the display width of a string in terminal columns.
fn display_width(text: &str) -> usize {
    text.width()
}

/// Pads or truncates `text` to exactly `width` display columns.
fn pad(text: &str, width: usize, align: Align, ellipsis: &str) -> String {
    let text = truncate(text, width, ellipsis);
    let padding = width.saturating_sub(display_width(&text));
    match align {
        Align::Left => format!("{text}{}", " ".repeat(padding)),
        Align::Right => format!("{}{text}", " ".repeat(padding)),
    }
}

/// Truncates `text` to `width` display columns, appending `ellipsis`.
fn truncate(text: &str, width: usize, ellipsis: &str) -> String {
    if display_width(text) <= width {
        return text.to_string();
    }
    let ellipsis_width = display_width(ellipsis);
    if width <= ellipsis_width {
        return take_width(ellipsis, width);
    }
    let mut result = take_width(text, width - ellipsis_width);
    result.push_str(ellipsis);
    result
}

/// Takes characters from the start of `text` while `width` columns fit.
fn take_width(text: &str, width: usize) -> String {
    let mut result = String::new();
    let mut used = 0;
    for char in text.chars() {
        let char_width = char.width().unwrap_or(0);
        if used + char_width > width {
            break;
        }
        result.push(char);
        used += char_width;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Glyphs;

    fn test_theme() -> Theme {
        Theme::new(Glyphs::Unicode)
    }

    /// Renders the table as plain text lines.
    fn rows(table: &Table, width: usize) -> Vec<String> {
        table
            .layout(width, &test_theme())
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| cell.text.as_str())
                    .collect::<Vec<_>>()
                    .join("  ")
            })
            .collect()
    }

    fn sample_table() -> Table {
        let mut table = Table::new([
            Column::new("#", Align::Right),
            Column::new("Title", Align::Left).shrinkable(),
            Column::new("Solution", Align::Left).shrinkable(),
        ]);
        table.push_row([Cell::new("1"), Cell::new("First"), Cell::new("R")]);
        table.push_row([
            Cell::new("12"),
            Cell::new("A rather long title"),
            Cell::new("uLLdR"),
        ]);
        table
    }

    #[test]
    fn pads_and_aligns_columns() {
        let lines = rows(&sample_table(), 80);
        assert_eq!(lines[0], " #  Title                Solution");
        assert_eq!(lines[1], " 1  First                R");
        assert_eq!(lines[2], "12  A rather long title  uLLdR");
    }

    #[test]
    fn truncates_shrinkable_columns() {
        let lines = rows(&sample_table(), 24);
        for line in &lines {
            assert!(
                display_width(line) <= 24,
                "line `{line}` exceeds the width limit"
            );
        }
        assert!(lines[0].contains('…'));
        assert!(lines[2].contains('…'));
    }

    #[test]
    fn counts_cjk_characters_as_two_columns() {
        let mut table = Table::new([
            Column::new("#", Align::Right),
            Column::new("Title", Align::Left),
        ]);
        table.push_row([Cell::new("1"), Cell::new("仓库")]);
        let lines = rows(&table, 80);
        assert_eq!(lines[0], "#  Title");
        assert_eq!(lines[1], "1  仓库");
    }

    #[test]
    fn truncates_on_character_boundaries() {
        assert_eq!(truncate("仓库番", 4, "…"), "仓…");
        assert_eq!(truncate("abc", 5, "…"), "abc");
    }
}
