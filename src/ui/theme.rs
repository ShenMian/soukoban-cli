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

/// Box drawing and status glyphs for a theme.
#[derive(Clone, Copy, Debug)]
pub struct GlyphSet {
    /// Horizontal border character.
    pub horizontal: char,
    /// Vertical border character.
    pub vertical: char,
    /// Top left border character.
    pub top_left: char,
    /// Top right border character.
    pub top_right: char,
    /// Bottom left border character.
    pub bottom_left: char,
    /// Bottom right border character.
    pub bottom_right: char,
    /// Success glyph.
    pub check: &'static str,
    /// Failure glyph.
    pub cross: &'static str,
    /// Timeout glyph.
    pub timeout: &'static str,
    /// Separator between items of a line.
    pub separator: &'static str,
    /// Ellipsis appended to truncated text.
    pub ellipsis: &'static str,
    /// Progress bar characters (finished, partial, remaining).
    pub bar_chars: &'static str,
    /// Spinner tick characters.
    pub ticks: &'static str,
}

const UNICODE: GlyphSet = GlyphSet {
    horizontal: '─',
    vertical: '│',
    top_left: '╭',
    top_right: '╮',
    bottom_left: '╰',
    bottom_right: '╯',
    check: "✓",
    cross: "✗",
    timeout: "◷",
    separator: " · ",
    ellipsis: "…",
    bar_chars: "█▉░",
    ticks: "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏",
};

const ASCII: GlyphSet = GlyphSet {
    horizontal: '-',
    vertical: '|',
    top_left: '+',
    top_right: '+',
    bottom_left: '+',
    bottom_right: '+',
    check: "v",
    cross: "x",
    timeout: "!",
    separator: " - ",
    ellipsis: "...",
    bar_chars: "#=-",
    ticks: "-\\|/",
};

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
    /// Box drawing and status glyphs.
    pub glyphs: &'static GlyphSet,
}

impl Theme {
    /// Creates the theme with the given glyph set.
    pub const fn new(glyphs: Glyphs) -> Self {
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
            glyphs: match glyphs {
                Glyphs::Unicode => &UNICODE,
                Glyphs::Ascii => &ASCII,
            },
        }
    }
}

/// Returns the clap help styles, matching the application theme.
pub const fn clap_styles() -> clap::builder::styling::Styles {
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

        assert_eq!(unicode.glyphs.horizontal, '─');
        assert_eq!(ascii.glyphs.horizontal, '-');
        assert_eq!(unicode.glyphs.top_left, '╭');
        assert_eq!(ascii.glyphs.top_left, '+');
        assert_eq!(unicode.glyphs.vertical, '│');
        assert_eq!(ascii.glyphs.vertical, '|');
        assert_eq!(unicode.glyphs.check, "✓");
        assert_eq!(ascii.glyphs.check, "v");
        assert_eq!(unicode.glyphs.ellipsis, "…");
        assert_eq!(ascii.glyphs.ellipsis, "...");
        assert_eq!(unicode.glyphs.bar_chars, "█▉░");
        assert_eq!(ascii.glyphs.bar_chars, "#=-");
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
