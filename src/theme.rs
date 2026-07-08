//! Theme definitions: UI chrome colors + syntax palette.

use ratatui::style::Color;

use crate::syntax::Tok;

const fn hex(h: u32) -> Color {
    Color::Rgb(((h >> 16) & 0xff) as u8, ((h >> 8) & 0xff) as u8, (h & 0xff) as u8)
}

#[derive(Clone)]
pub struct Theme {
    pub name: &'static str,

    // chrome
    pub bg: Color,
    pub fg: Color,
    pub panel_bg: Color,
    pub popup_bg: Color,
    pub accent: Color,
    pub accent_fg: Color,
    pub border: Color,
    pub border_active: Color,
    pub selection: Color,
    pub cursor_line: Color,
    pub line_nr: Color,
    pub line_nr_active: Color,
    pub dim: Color,
    pub status_bg: Color,
    pub match_bg: Color,
    pub match_current_bg: Color,

    // accents
    pub green: Color,
    pub yellow: Color,
    pub red: Color,
    pub magenta: Color,
    pub cyan: Color,
    pub orange: Color,

    // syntax
    pub s_keyword: Color,
    pub s_type: Color,
    pub s_fn: Color,
    pub s_string: Color,
    pub s_comment: Color,
    pub s_number: Color,
    pub s_constant: Color,
    pub s_punct: Color,
    pub s_attr: Color,
}

impl Theme {
    pub fn tok(&self, t: Tok) -> Color {
        match t {
            Tok::Keyword => self.s_keyword,
            Tok::Type => self.s_type,
            Tok::Fn => self.s_fn,
            Tok::String => self.s_string,
            Tok::Comment => self.s_comment,
            Tok::Number => self.s_number,
            Tok::Constant => self.s_constant,
            Tok::Punct => self.s_punct,
            Tok::Attr => self.s_attr,
        }
    }

    pub fn all() -> Vec<Theme> {
        vec![midnight_ocean(), graphite(), solar_dawn(), synthwave()]
    }
}

/// Deep blue dark theme (Tokyonight-inspired). The default.
fn midnight_ocean() -> Theme {
    Theme {
        name: "Midnight Ocean",
        bg: hex(0x0d1120),
        fg: hex(0xc8d3f5),
        panel_bg: hex(0x0a0e1a),
        popup_bg: hex(0x141a30),
        accent: hex(0x7aa2f7),
        accent_fg: hex(0x0d1120),
        border: hex(0x24304f),
        border_active: hex(0x7aa2f7),
        selection: hex(0x2d3f66),
        cursor_line: hex(0x161d33),
        line_nr: hex(0x3b4666),
        line_nr_active: hex(0x7aa2f7),
        dim: hex(0x565f89),
        status_bg: hex(0x10162a),
        match_bg: hex(0x3d4f21),
        match_current_bg: hex(0x6a5419),
        green: hex(0x9ece6a),
        yellow: hex(0xe0af68),
        red: hex(0xf7768e),
        magenta: hex(0xbb9af7),
        cyan: hex(0x2ac3de),
        orange: hex(0xff9e64),
        s_keyword: hex(0x9d7cd8),
        s_type: hex(0x2ac3de),
        s_fn: hex(0x7aa2f7),
        s_string: hex(0x9ece6a),
        s_comment: hex(0x565f89),
        s_number: hex(0xff9e64),
        s_constant: hex(0xff757f),
        s_punct: hex(0x89ddff),
        s_attr: hex(0xe0af68),
    }
}

/// Neutral gray dark theme (OneDark / NvChad-inspired).
fn graphite() -> Theme {
    Theme {
        name: "Graphite",
        bg: hex(0x1e222a),
        fg: hex(0xabb2bf),
        panel_bg: hex(0x1a1e26),
        popup_bg: hex(0x252a33),
        accent: hex(0x61afef),
        accent_fg: hex(0x1e222a),
        border: hex(0x32384a),
        border_active: hex(0x61afef),
        selection: hex(0x3e4451),
        cursor_line: hex(0x24292f),
        line_nr: hex(0x495162),
        line_nr_active: hex(0x61afef),
        dim: hex(0x5c6370),
        status_bg: hex(0x21252d),
        match_bg: hex(0x4a4520),
        match_current_bg: hex(0x6b5b1e),
        green: hex(0x98c379),
        yellow: hex(0xe5c07b),
        red: hex(0xe06c75),
        magenta: hex(0xc678dd),
        cyan: hex(0x56b6c2),
        orange: hex(0xd19a66),
        s_keyword: hex(0xc678dd),
        s_type: hex(0xe5c07b),
        s_fn: hex(0x61afef),
        s_string: hex(0x98c379),
        s_comment: hex(0x5c6370),
        s_number: hex(0xd19a66),
        s_constant: hex(0xe06c75),
        s_punct: hex(0x8fa3b8),
        s_attr: hex(0xe5c07b),
    }
}

/// Clean light theme (One Light-inspired).
fn solar_dawn() -> Theme {
    Theme {
        name: "Solar Dawn",
        bg: hex(0xfafafa),
        fg: hex(0x383a42),
        panel_bg: hex(0xf0f0f1),
        popup_bg: hex(0xffffff),
        accent: hex(0x4078f2),
        accent_fg: hex(0xfafafa),
        border: hex(0xd4d4d6),
        border_active: hex(0x4078f2),
        selection: hex(0xcfe0ff),
        cursor_line: hex(0xf0f1f3),
        line_nr: hex(0xb4b6bd),
        line_nr_active: hex(0x4078f2),
        dim: hex(0xa0a1a7),
        status_bg: hex(0xededee),
        match_bg: hex(0xfdf0b0),
        match_current_bg: hex(0xf8d878),
        green: hex(0x50a14f),
        yellow: hex(0xc18401),
        red: hex(0xe45649),
        magenta: hex(0xa626a4),
        cyan: hex(0x0184bc),
        orange: hex(0x986801),
        s_keyword: hex(0xa626a4),
        s_type: hex(0xc18401),
        s_fn: hex(0x4078f2),
        s_string: hex(0x50a14f),
        s_comment: hex(0xa0a1a7),
        s_number: hex(0x986801),
        s_constant: hex(0xe45649),
        s_punct: hex(0x526069),
        s_attr: hex(0xc18401),
    }
}

/// Neon retrowave dark theme.
fn synthwave() -> Theme {
    Theme {
        name: "Synthwave",
        bg: hex(0x262335),
        fg: hex(0xdfd9f7),
        panel_bg: hex(0x1f1c2c),
        popup_bg: hex(0x2f2a45),
        accent: hex(0xff7edb),
        accent_fg: hex(0x262335),
        border: hex(0x463465),
        border_active: hex(0xff7edb),
        selection: hex(0x4a3f70),
        cursor_line: hex(0x2c2841),
        line_nr: hex(0x5b537d),
        line_nr_active: hex(0xff7edb),
        dim: hex(0x848bbd),
        status_bg: hex(0x232038),
        match_bg: hex(0x50406a),
        match_current_bg: hex(0x7a5a2a),
        green: hex(0x72f1b8),
        yellow: hex(0xfede5d),
        red: hex(0xfe4450),
        magenta: hex(0xff7edb),
        cyan: hex(0x36f9f6),
        orange: hex(0xf97e72),
        s_keyword: hex(0xfede5d),
        s_type: hex(0xff8b39),
        s_fn: hex(0x36f9f6),
        s_string: hex(0x72f1b8),
        s_comment: hex(0x848bbd),
        s_number: hex(0xf97e72),
        s_constant: hex(0xfe4450),
        s_punct: hex(0xb6b1d8),
        s_attr: hex(0xfede5d),
    }
}
