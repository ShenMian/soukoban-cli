//! Semantic styles and glyphs shared by the presentation layer.

use anstyle::{AnsiColor, Style};

/// The glyph set used for borders and decorations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyphs {
    /// Unicode box drawing and status glyphs.
    Unicode,
    /// ASCII glyphs for terminals without Unicode support.
    Ascii,
}

/// The semantic style sheet and glyph set used by the UI.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    /// Titles and headings.
    pub title: Style,
    /// Dim labels such as table headers.
    pub label: Style,
    /// Emphasized values such as solutions.
    pub value: Style,
    /// Successful outcomes.
    pub success: Style,
    /// Successful outcomes that required no search, such as already solved
    /// levels.
    pub success_muted: Style,
    /// Warnings and timeouts.
    pub warning: Style,
    /// Errors and unsolvable levels.
    pub error: Style,
    /// Emphasized secondary information such as `help:`.
    pub accent: Style,
    /// Deemphasized information.
    pub muted: Style,
    /// Borders and rules.
    pub border: Style,
    /// Walls in rendered maps.
    pub wall: Style,
    /// Boxes in rendered maps.
    pub box_tile: Style,
    /// Goals in rendered maps.
    pub goal: Style,
    /// Boxes on goals in rendered maps.
    pub box_on_goal: Style,
    /// The player in rendered maps.
    pub player: Style,
    glyphs: Glyphs,
}

impl Theme {
    /// Creates the theme with the given glyph set.
    pub fn new(glyphs: Glyphs) -> Self {
        Self {
            title: Style::new().bold(),
            label: Style::new().dimmed(),
            value: Style::new().bold(),
            success: AnsiColor::Green.on_default(),
            success_muted: AnsiColor::Green
                .on_default()
                .effects(anstyle::Effects::DIMMED),
            warning: AnsiColor::Yellow.on_default(),
            error: AnsiColor::Red.on_default(),
            accent: AnsiColor::Cyan.on_default(),
            muted: Style::new().dimmed(),
            border: Style::new().dimmed(),
            wall: AnsiColor::BrightBlue.on_default(),
            box_tile: AnsiColor::Yellow.on_default(),
            goal: AnsiColor::Green.on_default(),
            box_on_goal: AnsiColor::BrightGreen.on_default(),
            player: AnsiColor::BrightCyan.on_default(),
            glyphs,
        }
    }

    /// Returns the horizontal border glyph.
    pub fn horizontal(&self) -> char {
        match self.glyphs {
            Glyphs::Unicode => '─',
            Glyphs::Ascii => '-',
        }
    }

    /// Returns the vertical border glyph.
    pub fn vertical(&self) -> char {
        match self.glyphs {
            Glyphs::Unicode => '│',
            Glyphs::Ascii => '|',
        }
    }

    /// Returns the top left border glyph.
    pub fn top_left(&self) -> char {
        match self.glyphs {
            Glyphs::Unicode => '╭',
            Glyphs::Ascii => '+',
        }
    }

    /// Returns the top right border glyph.
    pub fn top_right(&self) -> char {
        match self.glyphs {
            Glyphs::Unicode => '╮',
            Glyphs::Ascii => '+',
        }
    }

    /// Returns the bottom left border glyph.
    pub fn bottom_left(&self) -> char {
        match self.glyphs {
            Glyphs::Unicode => '╰',
            Glyphs::Ascii => '+',
        }
    }

    /// Returns the bottom right border glyph.
    pub fn bottom_right(&self) -> char {
        match self.glyphs {
            Glyphs::Unicode => '╯',
            Glyphs::Ascii => '+',
        }
    }

    /// Returns the success glyph.
    pub fn check(&self) -> &'static str {
        match self.glyphs {
            Glyphs::Unicode => "✓",
            Glyphs::Ascii => "v",
        }
    }

    /// Returns the failure glyph.
    pub fn cross(&self) -> &'static str {
        match self.glyphs {
            Glyphs::Unicode => "✗",
            Glyphs::Ascii => "x",
        }
    }

    /// Returns the timeout glyph.
    pub fn timeout(&self) -> &'static str {
        match self.glyphs {
            Glyphs::Unicode => "◷",
            Glyphs::Ascii => "!",
        }
    }

    /// Returns the separator between items of a line.
    pub fn separator(&self) -> &'static str {
        match self.glyphs {
            Glyphs::Unicode => " · ",
            Glyphs::Ascii => " - ",
        }
    }

    /// Returns the ellipsis appended to truncated text.
    pub fn ellipsis(&self) -> &'static str {
        match self.glyphs {
            Glyphs::Unicode => "…",
            Glyphs::Ascii => "...",
        }
    }

    /// Returns the progress bar characters (finished, partial, remaining).
    pub fn bar_chars(&self) -> &'static str {
        match self.glyphs {
            Glyphs::Unicode => "█▉░",
            Glyphs::Ascii => "#=-",
        }
    }

    /// Returns the spinner tick characters.
    pub fn ticks(&self) -> &'static str {
        match self.glyphs {
            Glyphs::Unicode => "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏",
            Glyphs::Ascii => "-\\|/",
        }
    }
}

/// Returns the clap help styles, matching the application theme.
pub fn clap_styles() -> clap::builder::styling::Styles {
    use clap::builder::styling::{AnsiColor, Effects, Styles};

    Styles::styled()
        .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
        .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
        .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
        .placeholder(AnsiColor::Cyan.on_default())
        .error(AnsiColor::Red.on_default().effects(Effects::BOLD))
        .valid(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
        .invalid(AnsiColor::Yellow.on_default().effects(Effects::BOLD))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_glyphs_replace_unicode_glyphs() {
        let unicode = Theme::new(Glyphs::Unicode);
        let ascii = Theme::new(Glyphs::Ascii);

        assert_eq!(unicode.horizontal(), '─');
        assert_eq!(ascii.horizontal(), '-');
        assert_eq!(unicode.top_left(), '╭');
        assert_eq!(ascii.top_left(), '+');
        assert_eq!(unicode.vertical(), '│');
        assert_eq!(ascii.vertical(), '|');
        assert_eq!(unicode.check(), "✓");
        assert_eq!(ascii.check(), "v");
        assert_eq!(unicode.ellipsis(), "…");
        assert_eq!(ascii.ellipsis(), "...");
        assert_eq!(unicode.bar_chars(), "█▉░");
        assert_eq!(ascii.bar_chars(), "#=-");
    }

    #[test]
    fn success_styles_are_distinct_and_colored() {
        let theme = Theme::new(Glyphs::Unicode);
        assert!(!theme.success.is_plain());
        assert!(!theme.success_muted.is_plain());
        assert_ne!(theme.success, theme.success_muted);
    }

    #[test]
    fn tile_styles_are_distinct_and_colored() {
        let theme = Theme::new(Glyphs::Unicode);
        let styles = [
            theme.wall,
            theme.box_tile,
            theme.goal,
            theme.box_on_goal,
            theme.player,
        ];
        for style in styles {
            assert!(!style.is_plain(), "a tile style must render escape codes");
        }
        for (index, first) in styles.iter().enumerate() {
            for second in &styles[index + 1..] {
                assert_ne!(first, second, "tile styles must be distinct");
            }
        }
    }
}
