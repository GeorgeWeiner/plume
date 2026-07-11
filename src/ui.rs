//! All rendering: layout, tabs, sidebar, editor, panels, status bar, overlays.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, Focus, Level, Overlay, Panel};
use crate::buffer::{visual_col, Buffer, TAB_STOP};
use crate::keymap;
use crate::palette::PaletteMode;
use crate::search;
use crate::syntax;
use crate::theme::Theme;

pub fn draw(f: &mut Frame, app: &mut App) {
    let t = app.theme().clone();
    let area = f.area();
    f.render_widget(Block::default().style(Style::default().bg(t.bg).fg(t.fg)), area);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);
    app.layout.tabbar = rows[0];
    app.layout.status = rows[2];

    let sidebar_w: u16 = if app.sidebar {
        if area.width < 100 {
            26
        } else {
            32
        }
    } else {
        0
    };
    let cols = Layout::horizontal([Constraint::Length(sidebar_w), Constraint::Min(20)]).split(rows[1]);
    app.layout.sidebar = cols[0];

    let mut editor_area = cols[1];
    let panel_h: u16 = match &app.panel {
        Panel::None => 0,
        // A real shell wants room; take roughly a third of the editor height.
        Panel::Terminal => (editor_area.height / 3).clamp(8, 20),
        Panel::Search(p) => (p.matches.len() as u16 + 2).clamp(5, 12),
    };
    if panel_h > 0 && editor_area.height > panel_h + 3 {
        let v = Layout::vertical([Constraint::Min(3), Constraint::Length(panel_h)]).split(editor_area);
        editor_area = v[0];
        app.layout.panel = v[1];
    } else {
        app.layout.panel = Rect::default();
        app.layout.panel_list = Rect::default();
    }
    app.layout.editor = editor_area;

    draw_tabs(f, app, &t);
    if app.sidebar && sidebar_w > 0 {
        draw_sidebar(f, app, &t);
    }
    draw_editor(f, app, &t);
    if app.layout.panel.height > 0 {
        draw_panel(f, app, &t);
    }
    draw_status(f, app, &t);
    draw_find_bar(f, app, &t);
    draw_notices(f, app, &t);
    draw_overlay(f, app, &t);
}

// ---- tab bar ----

fn draw_tabs(f: &mut Frame, app: &mut App, t: &Theme) {
    let area = app.layout.tabbar;
    app.tab_hits.clear();
    let mut spans: Vec<Span> = Vec::new();
    let mut x = area.x;

    let logo = " ❯ plume ";
    spans.push(Span::styled(
        logo,
        Style::default().fg(t.accent).bg(t.panel_bg).add_modifier(Modifier::BOLD),
    ));
    x += logo.chars().count() as u16;

    for (i, b) in app.buffers.iter().enumerate() {
        let name = b.display_name();
        let active = i == app.active;
        let (tab_bg, tab_fg) = if active { (t.bg, t.fg) } else { (t.panel_bg, t.dim) };
        let marker = if active { "▎" } else { " " };
        let dot = if b.modified { " ●" } else { "" };
        let w = (1 + name.chars().count() + dot.chars().count() + 1) as u16;

        spans.push(Span::styled(marker, Style::default().fg(t.accent).bg(tab_bg)));
        let mut name_style = Style::default().fg(tab_fg).bg(tab_bg);
        if active {
            name_style = name_style.add_modifier(Modifier::BOLD);
        }
        spans.push(Span::styled(name, name_style));
        if !dot.is_empty() {
            spans.push(Span::styled(dot, Style::default().fg(t.yellow).bg(tab_bg)));
        }
        spans.push(Span::styled(" ", Style::default().bg(tab_bg)));

        app.tab_hits.push((x, x + w, i));
        x += w;
    }

    let para = Paragraph::new(Line::from(spans)).style(Style::default().bg(t.panel_bg));
    f.render_widget(para, area);
}

// ---- sidebar ----

fn file_color(t: &Theme, name: &str) -> Color {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "rs" => t.orange,
        "py" => t.green,
        "js" | "jsx" | "mjs" | "cjs" | "json" => t.yellow,
        "ts" | "tsx" | "c" | "h" | "cpp" | "hpp" => t.accent,
        "go" | "css" | "scss" => t.cyan,
        "md" | "markdown" => t.magenta,
        "html" | "htm" | "sh" | "bash" => t.red,
        "toml" | "yaml" | "yml" | "lock" | "gitignore" => t.dim,
        _ => t.dim,
    }
}

fn draw_sidebar(f: &mut Frame, app: &mut App, t: &Theme) {
    let area = app.layout.sidebar;
    let focused = app.focus == Focus::Explorer;
    let root_name = app
        .root
        .file_name()
        .map(|n| n.to_string_lossy().to_uppercase())
        .unwrap_or_else(|| "PROJECT".into());

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { t.border_active } else { t.border }))
        .title(Line::from(vec![Span::styled(
            format!(" ⛁ {root_name} "),
            Style::default().fg(if focused { t.accent } else { t.dim }).add_modifier(Modifier::BOLD),
        )]))
        .style(Style::default().bg(t.panel_bg));
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.layout.sidebar_list = inner;

    let h = inner.height as usize;
    if h == 0 {
        return;
    }
    if app.tree.selected < app.tree.scroll {
        app.tree.scroll = app.tree.selected;
    } else if app.tree.selected >= app.tree.scroll + h {
        app.tree.scroll = app.tree.selected + 1 - h;
    }

    let width = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    let end = (app.tree.scroll + h).min(app.tree.items.len());
    for i in app.tree.scroll..end {
        let item = &app.tree.items[i];
        let selected = i == app.tree.selected;
        let row_bg = if selected {
            if focused {
                t.selection
            } else {
                t.cursor_line
            }
        } else {
            t.panel_bg
        };
        let name = item
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let indent = "  ".repeat(item.depth);
        let (icon, icon_color) = if item.is_dir {
            (if item.expanded { "▾ " } else { "▸ " }, t.accent)
        } else {
            ("● ", file_color(t, &name))
        };
        let mut name_style = Style::default().fg(t.fg).bg(row_bg);
        if selected {
            name_style = name_style.add_modifier(Modifier::BOLD);
        }
        let used = 1 + indent.chars().count() + 2 + name.chars().count();
        let filler = " ".repeat(width.saturating_sub(used));
        lines.push(Line::from(vec![
            Span::styled(format!(" {indent}"), Style::default().bg(row_bg)),
            Span::styled(icon, Style::default().fg(icon_color).bg(row_bg)),
            Span::styled(name, name_style),
            Span::styled(filler, Style::default().bg(row_bg)),
        ]));
    }
    if app.tree.items.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (empty project)",
            Style::default().fg(t.dim).bg(t.panel_bg),
        )));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

// ---- editor ----

fn draw_editor(f: &mut Frame, app: &mut App, t: &Theme) {
    let area = app.layout.editor;
    let focused = app.focus == Focus::Editor;
    let title = match app.buf() {
        Some(b) => {
            let dot = if b.modified { " ●" } else { "" };
            format!(" {}{dot} ", b.display_name())
        }
        None => " welcome ".to_string(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { t.border_active } else { t.border }))
        .title(Line::from(vec![Span::styled(
            title,
            Style::default()
                .fg(if focused { t.accent } else { t.dim })
                .add_modifier(Modifier::BOLD),
        )]))
        .style(Style::default().bg(t.bg));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.buffers.is_empty() {
        app.layout.text = Rect::default();
        draw_dashboard(f, app, t, inner);
        return;
    }

    let gutter_w = {
        let n = app.buf().map(|b| b.lines.len()).unwrap_or(1);
        n.max(1).to_string().len() as u16 + 2
    };

    // Carve a minimap column off the right edge when enabled and there's room
    // to keep a usable text area.
    let mut mm_w: u16 = 0;
    if app.minimap && inner.width > 70 {
        let w = (inner.width / 7).clamp(12, 22);
        if inner.width.saturating_sub(gutter_w + w + 1) >= 40 {
            mm_w = w;
        }
    }
    let sep_w = if mm_w > 0 { 1 } else { 0 };
    let text_rect = Rect {
        x: inner.x + gutter_w,
        y: inner.y,
        width: inner.width.saturating_sub(gutter_w + mm_w + sep_w),
        height: inner.height,
    };
    app.layout.text = text_rect;
    let minimap_rect = if mm_w > 0 {
        Rect { x: inner.x + inner.width - mm_w, y: inner.y, width: mm_w, height: inner.height }
    } else {
        Rect::default()
    };
    app.layout.minimap = minimap_rect;

    let follow = app.follow;
    app.follow = false;
    let overlay_open = app.overlay.is_some();
    let find_typing = app.find_typing;
    let Some(buf) = app.buffers.get_mut(app.active) else {
        return;
    };

    buf.scroll_row = buf.scroll_row.min(buf.lines.len().saturating_sub(1));
    if follow {
        let h = text_rect.height as usize;
        let w = text_rect.width.saturating_sub(1).max(1) as usize;
        let (crow, ccol) = buf.cursor;
        if h > 0 {
            if crow < buf.scroll_row {
                buf.scroll_row = crow;
            } else if crow >= buf.scroll_row + h {
                buf.scroll_row = crow + 1 - h;
            }
        }
        let vc = visual_col(buf.line(crow), ccol);
        if vc < buf.scroll_col {
            buf.scroll_col = vc;
        } else if vc >= buf.scroll_col + w {
            buf.scroll_col = vc + 1 - w;
        }
    }

    let (crow, ccol) = buf.cursor;
    let scroll_row = buf.scroll_row;
    let scroll_col = buf.scroll_col;
    let sel = buf.selection();
    let width = text_rect.width as usize;

    // Bracket pair enclosing the cursor: its two brackets get accent-highlighted
    // and a vertical guide is drawn down the closing bracket's column to connect
    // them across the block.
    let pair = buf.enclosing_brackets();
    let guide_color = blend(t.accent, t.bg, 0.62);
    let guide_vcol = pair.map(|p| visual_col(buf.line(p.close.0), p.close.1));

    // Only the find matches on visible rows matter for rendering; matches are
    // row-major sorted, so slice them out instead of cloning the whole list.
    let (vis_matches, cur_match, qlen): (Vec<(usize, usize)>, Option<(usize, usize)>, usize) =
        match app.find.as_ref() {
            Some(fs) => {
                let h = text_rect.height as usize;
                let s = fs.matches.partition_point(|m| m.0 < scroll_row);
                let e = fs.matches.partition_point(|m| m.0 < scroll_row + h);
                (
                    fs.matches[s..e].to_vec(),
                    fs.matches.get(fs.idx).copied(),
                    fs.input.text.chars().count(),
                )
            }
            None => (Vec::new(), None, 0),
        };

    let mut out: Vec<Line> = Vec::new();
    for vy in 0..text_rect.height as usize {
        let row = scroll_row + vy;
        if row >= buf.lines.len() {
            out.push(Line::from(Span::styled(
                " ".repeat(inner.width as usize),
                Style::default().bg(t.bg),
            )));
            continue;
        }
        let row_bg = if row == crow { t.cursor_line } else { t.bg };
        let nr_style = if row == crow {
            Style::default().fg(t.line_nr_active).bg(row_bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.line_nr).bg(row_bg)
        };
        let mut spans = vec![Span::styled(
            format!("{:>w$} ", row + 1, w = gutter_w as usize - 1),
            nr_style,
        )];
        let mut bracket_cols: Vec<usize> = Vec::new();
        let mut guide = None;
        if let Some(p) = pair {
            if p.open.0 == row {
                bracket_cols.push(p.open.1);
            }
            if p.close.0 == row {
                bracket_cols.push(p.close.1);
            }
            if p.open.0 < row && row < p.close.0 {
                guide = guide_vcol.map(|v| (v, guide_color));
            }
        }
        spans.extend(line_spans(
            t, buf, row, row_bg, scroll_col, width, sel, &vis_matches, cur_match, qlen,
            &bracket_cols, guide,
        ));
        out.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(out), inner);

    // hardware cursor
    if focused && !overlay_open && !find_typing {
        let vc = visual_col(buf.line(crow), ccol);
        if crow >= scroll_row && vc >= scroll_col {
            let cy = inner.y as usize + (crow - scroll_row);
            let cx = text_rect.x as usize + (vc - scroll_col);
            if cy < (inner.y + inner.height) as usize && cx < (text_rect.x + text_rect.width) as usize {
                f.set_cursor_position((cx as u16, cy as u16));
            }
        }
    }

    if minimap_rect.width > 0 {
        // dim separator between the text and the minimap
        let sep = Rect { x: minimap_rect.x - 1, y: inner.y, width: 1, height: inner.height };
        let sep_lines: Vec<Line> = (0..inner.height)
            .map(|_| Line::from(Span::styled("│", Style::default().fg(t.border).bg(t.bg))))
            .collect();
        f.render_widget(Paragraph::new(sep_lines), sep);
        draw_minimap(f, t, buf, minimap_rect, text_rect.height);
    }
}

// ---- minimap ----

fn rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (128, 128, 128),
    }
}

/// Linear blend: t=0 → a, t=1 → b.
fn blend(a: Color, b: Color, t: f32) -> Color {
    let (ar, ag, ab) = rgb(a);
    let (br, bg, bb) = rgb(b);
    let m = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t).round() as u8;
    Color::Rgb(m(ar, br), m(ag, bg), m(ab, bb))
}

fn token_color_at(t: &Theme, ranges: &[(usize, usize, syntax::Tok)], col: usize) -> Option<Color> {
    ranges
        .iter()
        .find(|(s, e, _)| col >= *s && col < *e)
        .map(|(_, _, tok)| t.tok(*tok))
}

/// Dominant (most frequent) color in a slice, ties broken by first seen.
/// Returns `None` for an all-empty slice.
fn dominant_color(px: &[Option<Color>]) -> Option<Color> {
    let mut best: Option<(Color, usize)> = None;
    for (i, &c) in px.iter().enumerate() {
        let Some(c) = c else { continue };
        let count = px[i..].iter().filter(|o| **o == Some(c)).count();
        if best.is_none_or(|(_, n)| count > n) {
            best = Some((c, count));
        }
    }
    best.map(|(c, _)| c)
}

/// Braille dot bit for sub-cell (row 0..4, col 0..2), offset from U+2800.
/// Layout is the standard 2×4 matrix: dots 1-3,7 down the left, 4-6,8 the right.
const BRAILLE: [[u8; 2]; 4] = [
    [0x01, 0x08],
    [0x02, 0x10],
    [0x04, 0x20],
    [0x40, 0x80],
];

/// Zoomed-out overview of the whole file. Each terminal cell is a Braille glyph
/// packing a 2×4 grid of source "pixels" (2 columns × 4 rows), so the map has
/// double the resolution of a half-block rendering in both axes. Each pixel
/// samples one source column; the cell is tinted by its dominant syntax color.
/// Sampling is bounded by the panel size, so cost is independent of file length.
fn draw_minimap(f: &mut Frame, t: &Theme, buf: &Buffer, area: Rect, text_h: u16) {
    let (w, h) = (area.width as usize, area.height as usize);
    if w == 0 || h == 0 {
        return;
    }
    // Sub-pixel grid: 2 columns and 4 rows per terminal cell.
    let px_w = w * 2;
    let px_h = h * 4;
    let n = buf.lines.len();

    let line_at = |px: usize| -> Option<usize> {
        if n == 0 {
            return None;
        }
        let idx = if n <= px_h { px } else { px * n / px_h };
        (idx < n).then_some(idx)
    };
    let px_at = |line: usize| -> usize {
        if n <= px_h {
            line
        } else {
            line * px_h / n
        }
    };
    let band_top = px_at(buf.scroll_row);
    let band_bot = px_at((buf.scroll_row + text_h as usize).min(n)).max(band_top + 1);

    // Precompute a color (or none) for every sub-pixel: one source column each.
    let mut grid: Vec<Vec<Option<Color>>> = Vec::with_capacity(px_h);
    for px in 0..px_h {
        let mut row = vec![None; px_w];
        if let Some(li) = line_at(px) {
            let clipped: String = buf.line(li).chars().take(px_w).collect();
            let in_block = buf.line_states.get(li).copied().unwrap_or(false);
            let ranges = syntax::scan_line(buf.language, &clipped, in_block).0;
            for (sc, ch) in clipped.chars().enumerate() {
                if !ch.is_whitespace() {
                    row[sc] = Some(token_color_at(t, &ranges, sc).unwrap_or(t.fg));
                }
            }
        }
        grid.push(row);
    }

    let band_tint = blend(t.accent, t.bg, 0.80);
    let mut lines: Vec<Line> = Vec::with_capacity(h);
    for ty in 0..h {
        let mut spans: Vec<Span> = Vec::with_capacity(w);
        for cx in 0..w {
            // Assemble the 2×4 dot pattern and collect lit colors for the cell.
            let mut pattern: u8 = 0;
            let mut lit: [Option<Color>; 8] = [None; 8];
            let mut k = 0;
            let mut in_band = false;
            for (ry, dots) in BRAILLE.iter().enumerate() {
                let py = ty * 4 + ry;
                in_band |= py >= band_top && py < band_bot;
                for (rx, &bit) in dots.iter().enumerate() {
                    if let Some(col) = grid[py][cx * 2 + rx] {
                        pattern |= bit;
                        lit[k] = Some(col);
                        k += 1;
                    }
                }
            }
            let bg = if in_band { band_tint } else { t.bg };
            match dominant_color(&lit[..k]) {
                Some(col) => {
                    let fg = blend(col, t.bg, if in_band { 0.10 } else { 0.35 });
                    let ch = char::from_u32(0x2800 + pattern as u32).unwrap_or(' ');
                    spans.push(Span::styled(ch.to_string(), Style::default().fg(fg).bg(bg)));
                }
                None => spans.push(Span::styled(" ", Style::default().bg(bg))),
            }
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), area);
}

#[allow(clippy::too_many_arguments)]
fn line_spans(
    t: &Theme,
    buf: &Buffer,
    row: usize,
    row_bg: Color,
    scroll_col: usize,
    width: usize,
    sel: Option<((usize, usize), (usize, usize))>,
    matches: &[(usize, usize)],
    cur_match: Option<(usize, usize)>,
    qlen: usize,
    // Char indices on this row that are a matched bracket (accent-highlighted),
    // and an optional (visual column, color) for the bracket-pair guide line.
    bracket_cols: &[usize],
    guide: Option<(usize, Color)>,
) -> Vec<Span<'static>> {
    let line = buf.line(row);
    // Every char is at least one column wide, so nothing past this index can
    // be visible — bounds per-line render cost regardless of line length.
    let take_n = scroll_col + width + 1;
    let chars: Vec<char> = line.chars().take(take_n).collect();
    let n = chars.len();

    // Skip tokenization on absurdly long lines (minified files): scan_line is
    // O(full line) and would run for every visible frame.
    let ranges = if line.len() <= 8192 {
        let in_block = buf.line_states.get(row).copied().unwrap_or(false);
        syntax::scan_line(buf.language, line, in_block).0
    } else {
        Vec::new()
    };

    let mut fgs = vec![t.fg; n];
    for (s, e, tok) in ranges {
        for slot in fgs.iter_mut().take(e.min(n)).skip(s) {
            *slot = t.tok(tok);
        }
    }
    let mut bgs = vec![row_bg; n];

    for &(mr, mc) in matches {
        if mr != row {
            continue;
        }
        let bg = if cur_match == Some((mr, mc)) { t.match_current_bg } else { t.match_bg };
        for slot in bgs.iter_mut().take((mc + qlen).min(n)).skip(mc.min(n)) {
            *slot = bg;
        }
    }
    if let Some((a, b)) = sel {
        if row >= a.0 && row <= b.0 {
            let from = if row == a.0 { a.1 } else { 0 };
            let to = if row == b.0 { b.1 } else { n };
            for slot in bgs.iter_mut().take(to.min(n)).skip(from.min(n)) {
                *slot = t.selection;
            }
        }
    }
    // The matched brackets around the cursor: accent fg over a faint accent wash.
    for &bc in bracket_cols {
        if bc < n {
            fgs[bc] = t.accent;
            bgs[bc] = blend(t.accent, bgs[bc], 0.72);
        }
    }

    // Paint into a visual-cell grid (tabs expanded, one cell per column), then
    // overlay the guide and coalesce equal styles into spans.
    let mut cells: Vec<(char, Color, Color)> = vec![(' ', t.fg, row_bg); width];
    let mut v = 0usize;
    for (i, &c) in chars.iter().enumerate() {
        let w = if c == '\t' { TAB_STOP - v % TAB_STOP } else { 1 };
        let end_v = v + w;
        if end_v <= scroll_col {
            v = end_v;
            continue;
        }
        if v >= scroll_col + width {
            break;
        }
        if c == '\t' {
            for vv in v.max(scroll_col)..end_v.min(scroll_col + width) {
                cells[vv - scroll_col] = (' ', fgs[i], bgs[i]);
            }
        } else if v >= scroll_col {
            cells[v - scroll_col] = (c, fgs[i], bgs[i]);
        }
        v = end_v;
    }

    // Draw the guide only over blank space so it never clobbers code.
    if let Some((gv, color)) = guide {
        if gv >= scroll_col && gv < scroll_col + width {
            let x = gv - scroll_col;
            if cells[x].0 == ' ' {
                cells[x] = ('│', color, cells[x].2);
            }
        }
    }

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut cur_text = String::new();
    let mut cur_style = Style::default();
    for (c, fg, bg) in cells {
        let style = Style::default().fg(fg).bg(bg);
        if style != cur_style && !cur_text.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut cur_text), cur_style));
        }
        cur_style = style;
        cur_text.push(c);
    }
    if !cur_text.is_empty() {
        spans.push(Span::styled(cur_text, cur_style));
    }
    spans
}

// ---- welcome dashboard ----

const LOGO: [&str; 6] = [
    "██████╗ ██╗     ██╗   ██╗███╗   ███╗███████╗",
    "██╔══██╗██║     ██║   ██║████╗ ████║██╔════╝",
    "██████╔╝██║     ██║   ██║██╔████╔██║█████╗  ",
    "██╔═══╝ ██║     ██║   ██║██║╚██╔╝██║██╔══╝  ",
    "██║     ███████╗╚██████╔╝██║ ╚═╝ ██║███████╗",
    "╚═╝     ╚══════╝ ╚═════╝ ╚═╝     ╚═╝╚══════╝",
];

fn draw_dashboard(f: &mut Frame, app: &App, t: &Theme, inner: Rect) {
    let hints: [(&str, &str); 7] = [
        ("Ctrl+P        ", "open a file"),
        ("Ctrl+E        ", "browse the file tree"),
        ("Ctrl+Shift+P  ", "command palette"),
        ("Ctrl+Shift+F  ", "search the project"),
        ("Ctrl+N        ", "new untitled file"),
        ("Ctrl+K Ctrl+T ", "change theme"),
        ("Ctrl+Q        ", "quit"),
    ];
    let mut lines: Vec<Line> = Vec::new();
    for l in LOGO {
        lines.push(
            Line::from(Span::styled(
                l,
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            ))
            .centered(),
        );
    }
    lines.push(Line::default());
    lines.push(
        Line::from(Span::styled(
            "v0.1.0  ·  a feather-light terminal IDE",
            Style::default().fg(t.dim),
        ))
        .centered(),
    );
    lines.push(
        Line::from(Span::styled(
            format!("project: {}", app.root.display()),
            Style::default().fg(t.dim),
        ))
        .centered(),
    );
    lines.push(Line::default());
    for (key, label) in hints {
        lines.push(
            Line::from(vec![
                Span::styled(key, Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {label}"), Style::default().fg(t.dim)),
            ])
            .centered(),
        );
    }
    let total = lines.len() as u16;
    let top = inner.y + inner.height.saturating_sub(total) / 2;
    let rect = Rect {
        x: inner.x,
        y: top,
        width: inner.width,
        height: total.min(inner.height),
    };
    f.render_widget(Paragraph::new(lines), rect);
}

// ---- bottom panel ----

fn draw_panel(f: &mut Frame, app: &mut App, t: &Theme) {
    let area = app.layout.panel;
    let focused = app.focus == Focus::Panel;
    let border_style = Style::default().fg(if focused { t.border_active } else { t.border });

    match &mut app.panel {
        Panel::None => {}
        Panel::Terminal => {
            let dead = app.terminal.as_ref().is_some_and(|term| term.is_dead());
            let scrollback = app.terminal.as_ref().map_or(0, |term| term.scrollback());
            let hint: String = if dead {
                " ↵ restart · Ctrl+` close ".into()
            } else if scrollback > 0 {
                format!(" ▲ {scrollback} · Shift+PgDn to bottom ")
            } else {
                " Ctrl+` hide · Shift+PgUp scroll ".into()
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(border_style)
                .title(Line::from(vec![Span::styled(
                    " TERMINAL ",
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                )]))
                .title(
                    Line::from(Span::styled(hint, Style::default().fg(t.dim))).right_aligned(),
                )
                .style(Style::default().bg(t.bg));
            let inner = block.inner(area);
            f.render_widget(block, area);
            app.layout.panel_list = inner;
            if inner.width == 0 || inner.height == 0 {
                return;
            }

            match app.terminal.as_mut() {
                Some(term) => {
                    // Keep the shell's view matched to the panel's inner area.
                    term.resize(inner.height, inner.width);
                    // Hide the cursor while scrolled back — it's not the live view.
                    let show_cursor = focused && !term.is_dead() && scrollback == 0;
                    let guard = term.parser();
                    let mut lines = render_terminal(guard.screen(), t, show_cursor);
                    drop(guard);
                    if dead {
                        lines.push(Line::from(Span::styled(
                            "[process exited]",
                            Style::default().fg(t.dim).add_modifier(Modifier::ITALIC),
                        )));
                    }
                    f.render_widget(Paragraph::new(lines), inner);
                }
                None => {
                    f.render_widget(
                        Paragraph::new(Line::from(Span::styled(
                            " Could not start a shell here.",
                            Style::default().fg(t.red),
                        ))),
                        inner,
                    );
                }
            }
        }
        Panel::Search(pane) => {
            let title = if pane.done {
                format!(" {}  ·  {} results for '{}' ", pane.title, pane.matches.len(), pane.query)
            } else {
                format!(" {}  ·  searching… {} found ", pane.title, pane.matches.len())
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(border_style)
                .title(Line::from(vec![Span::styled(
                    title,
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                )]))
                .title(Line::from(Span::styled(
                    " ↵ open · q close ",
                    Style::default().fg(t.dim),
                )).right_aligned())
                .style(Style::default().bg(t.panel_bg));
            let inner = block.inner(area);
            f.render_widget(block, area);
            app.layout.panel_list = inner;

            let h = inner.height as usize;
            if h == 0 {
                return;
            }
            if pane.selected < pane.scroll {
                pane.scroll = pane.selected;
            } else if pane.selected >= pane.scroll + h {
                pane.scroll = pane.selected + 1 - h;
            }
            let width = inner.width as usize;
            let mut lines: Vec<Line> = Vec::new();
            let end = (pane.scroll + h).min(pane.matches.len());
            for i in pane.scroll..end {
                let m = &pane.matches[i];
                let selected = i == pane.selected;
                let row_bg = if selected { t.selection } else { t.panel_bg };
                let rel = m.path.strip_prefix(&app.root).unwrap_or(&m.path);
                let loc = format!(" {}:{} ", rel.display(), m.line_no + 1);
                let text = m.text.trim_start();
                let mut spans = vec![Span::styled(
                    loc.clone(),
                    Style::default().fg(t.accent).bg(row_bg),
                )];
                let mut used = loc.chars().count();
                match search::find_in_line(text, &pane.query) {
                    Some(col) => {
                        let chars: Vec<char> = text.chars().collect();
                        let qlen = pane.query.chars().count();
                        let pre: String = chars[..col].iter().collect();
                        let hit: String = chars[col..(col + qlen).min(chars.len())].iter().collect();
                        let post: String = chars[(col + qlen).min(chars.len())..].iter().collect();
                        used += pre.chars().count() + hit.chars().count() + post.chars().count();
                        spans.push(Span::styled(pre, Style::default().fg(t.dim).bg(row_bg)));
                        spans.push(Span::styled(
                            hit,
                            Style::default().fg(t.yellow).bg(row_bg).add_modifier(Modifier::BOLD),
                        ));
                        spans.push(Span::styled(post, Style::default().fg(t.dim).bg(row_bg)));
                    }
                    None => {
                        used += text.chars().count();
                        spans.push(Span::styled(
                            text.to_string(),
                            Style::default().fg(t.dim).bg(row_bg),
                        ));
                    }
                }
                spans.push(Span::styled(
                    " ".repeat(width.saturating_sub(used)),
                    Style::default().bg(row_bg),
                ));
                lines.push(Line::from(spans));
            }
            if pane.matches.is_empty() {
                lines.push(Line::from(Span::styled(
                    if pane.done { "  no matches" } else { "  searching…" },
                    Style::default().fg(t.dim).bg(t.panel_bg),
                )));
            }
            f.render_widget(Paragraph::new(lines), inner);
        }
    }
}

/// Turn a vt100 screen grid into styled lines, one span per cell. When
/// `show_cursor` is set, the cell under the cursor gets a block highlight.
fn render_terminal(screen: &vt100::Screen, t: &Theme, show_cursor: bool) -> Vec<Line<'static>> {
    let (rows, cols) = screen.size();
    let (cur_row, cur_col) = screen.cursor_position();
    let cursor_on = show_cursor && !screen.hide_cursor();

    let mut lines = Vec::with_capacity(rows as usize);
    for row in 0..rows {
        let mut spans: Vec<Span> = Vec::with_capacity(cols as usize);
        let mut col = 0u16;
        while col < cols {
            let cell = screen.cell(row, col);
            let wide = cell.map(|c| c.is_wide()).unwrap_or(false);
            let text = match cell {
                Some(c) if c.has_contents() => c.contents().to_string(),
                _ => " ".to_string(),
            };
            let mut style = cell.map(|c| cell_style(c, t)).unwrap_or_default();
            if cursor_on && row == cur_row && col == cur_col {
                style = style.bg(t.accent).fg(t.bg).add_modifier(Modifier::BOLD);
            }
            spans.push(Span::styled(text, style));
            col += if wide { 2 } else { 1 };
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn cell_style(cell: &vt100::Cell, t: &Theme) -> Style {
    let mut fg = conv_color(cell.fgcolor(), t, true);
    let mut bg = conv_color(cell.bgcolor(), t, false);
    if cell.inverse() {
        std::mem::swap(&mut fg, &mut bg);
    }
    let mut m = Modifier::empty();
    if cell.bold() {
        m |= Modifier::BOLD;
    }
    if cell.italic() {
        m |= Modifier::ITALIC;
    }
    if cell.underline() {
        m |= Modifier::UNDERLINED;
    }
    Style::default().fg(fg).bg(bg).add_modifier(m)
}

fn conv_color(c: vt100::Color, t: &Theme, fg: bool) -> Color {
    match c {
        vt100::Color::Default => {
            if fg {
                t.fg
            } else {
                t.bg
            }
        }
        vt100::Color::Idx(i) => ansi_color(i, t),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Map the 16 ANSI colors onto the active theme so the shell blends in; leave
/// the 6×6×6 cube and grayscale (16–255) to the host terminal's own palette.
fn ansi_color(i: u8, t: &Theme) -> Color {
    match i {
        0 => t.bg,      // black
        1 | 9 => t.red,
        2 | 10 => t.green,
        3 => t.yellow,
        11 => t.orange, // bright yellow
        4 | 12 => t.accent,
        5 | 13 => t.magenta,
        6 | 14 => t.cyan,
        7 => t.fg,      // white
        8 => t.dim,     // bright black
        15 => t.fg,     // bright white
        _ => Color::Indexed(i),
    }
}

// ---- status bar ----

fn draw_status(f: &mut Frame, app: &App, t: &Theme) {
    let area = app.layout.status;
    let mode = if app.find_typing {
        "FIND"
    } else {
        match app.focus {
            Focus::Editor => "EDIT",
            Focus::Explorer => "TREE",
            Focus::Panel => "PANEL",
        }
    };

    let chip = Style::default().fg(t.accent_fg).bg(t.accent).add_modifier(Modifier::BOLD);
    let base = Style::default().fg(t.fg).bg(t.status_bg);
    let dim = Style::default().fg(t.dim).bg(t.status_bg);

    let mut left: Vec<Span> = vec![Span::styled(format!(" {mode} "), chip)];
    let mut right: Vec<Span> = Vec::new();

    // Pending two-key chord (e.g. after Ctrl+K), shown prominently.
    if let Some(chord) = &app.pending_chord {
        let label = keymap::format_binding(std::slice::from_ref(chord));
        left.push(Span::styled(
            format!(" {label} … "),
            Style::default().fg(t.accent_fg).bg(t.yellow).add_modifier(Modifier::BOLD),
        ));
    }

    match app.buf() {
        Some(b) => {
            let path = b
                .path
                .as_ref()
                .map(|p| p.strip_prefix(&app.root).unwrap_or(p).display().to_string())
                .unwrap_or_else(|| "untitled".into());
            left.push(Span::styled(format!("  {path}"), base));
            if b.modified {
                left.push(Span::styled(" ●", Style::default().fg(t.yellow).bg(t.status_bg)));
            }
            let (r, c) = b.cursor;
            let vcol = visual_col(b.line(r), c) + 1;
            let pct = (r + 1) * 100 / b.lines.len().max(1);
            right.push(Span::styled(format!("{}  ", b.language.name()), dim));
            right.push(Span::styled("UTF-8  ", dim));
            right.push(Span::styled(format!("Ln {}, Col {}  ", r + 1, vcol), base));
            right.push(Span::styled(format!("{pct}%  "), dim));
        }
        None => {
            left.push(Span::styled(format!("  {}", app.root.display()), dim));
        }
    }
    right.push(Span::styled(format!("⌨ {}  ", app.keymap.name), dim));
    right.push(Span::styled(format!(" ✦ {} ", t.name), chip));

    let lw: usize = left.iter().map(|s| s.content.chars().count()).sum();
    let rw: usize = right.iter().map(|s| s.content.chars().count()).sum();
    let pad = (area.width as usize).saturating_sub(lw + rw);
    let mut spans = left;
    spans.push(Span::styled(" ".repeat(pad), base));
    spans.extend(right);
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(t.status_bg)),
        area,
    );
}

// ---- find bar ----

fn draw_find_bar(f: &mut Frame, app: &App, t: &Theme) {
    let Some(fs) = app.find.as_ref() else { return };
    let area = app.layout.editor;
    if area.width < 30 || area.height < 4 {
        return;
    }
    let w: u16 = 36.min(area.width - 4);
    let rect = Rect {
        x: area.x + area.width - w - 2,
        y: area.y + 1,
        width: w,
        height: 3,
    };
    f.render_widget(Clear, rect);
    let border = if app.find_typing { t.border_active } else { t.border };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .title(Line::from(vec![Span::styled(
            " FIND ",
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        )]))
        .style(Style::default().bg(t.popup_bg));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let counter = if fs.matches.is_empty() {
        "0/0".to_string()
    } else {
        format!("{}/{}", fs.idx.min(fs.matches.len() - 1) + 1, fs.matches.len())
    };
    let counter_style = if fs.matches.is_empty() && !fs.input.text.is_empty() {
        Style::default().fg(t.red).bg(t.popup_bg)
    } else {
        Style::default().fg(t.dim).bg(t.popup_bg)
    };
    let qw = fs.input.text.chars().count();
    let pad = (inner.width as usize).saturating_sub(qw + counter.chars().count() + 2);
    let spans = vec![
        Span::styled(" ", Style::default().bg(t.popup_bg)),
        Span::styled(fs.input.text.clone(), Style::default().fg(t.fg).bg(t.popup_bg)),
        Span::styled(" ".repeat(pad), Style::default().bg(t.popup_bg)),
        Span::styled(counter, counter_style),
        Span::styled(" ", Style::default().bg(t.popup_bg)),
    ];
    f.render_widget(Paragraph::new(Line::from(spans)), inner);
    if app.find_typing {
        f.set_cursor_position((inner.x + 1 + fs.input.cursor as u16, inner.y));
    }
}

// ---- notifications ----

fn draw_notices(f: &mut Frame, app: &App, t: &Theme) {
    let area = f.area();
    for (i, n) in app.notices.iter().rev().enumerate() {
        let w: u16 = 50.min(area.width.saturating_sub(4));
        if w < 12 {
            return;
        }
        let y = app
            .layout
            .status
            .y
            .saturating_sub((i as u16 + 1) * 3);
        if y < 2 {
            return;
        }
        let rect = Rect { x: area.x + area.width - w - 2, y, width: w, height: 3 };
        let (color, tag) = match n.level {
            Level::Info => (t.accent, " info "),
            Level::Warn => (t.yellow, " warn "),
            Level::Error => (t.red, " error "),
        };
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(color))
            .title(Line::from(vec![Span::styled(
                tag,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )]))
            .style(Style::default().bg(t.popup_bg));
        let inner = block.inner(rect);
        f.render_widget(block, rect);
        let text: String = n.text.chars().take(inner.width as usize).collect();
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                text,
                Style::default().fg(t.fg).bg(t.popup_bg),
            ))),
            inner,
        );
    }
}

// ---- overlays: palette & prompt ----

fn draw_overlay(f: &mut Frame, app: &App, t: &Theme) {
    let area = f.area();
    match app.overlay.as_ref() {
        None => {}
        Some(Overlay::Prompt(p)) => {
            let w: u16 = 60.min(area.width.saturating_sub(4));
            let rect = Rect {
                x: area.x + (area.width - w) / 2,
                y: area.y + area.height / 3,
                width: w,
                height: 3,
            };
            f.render_widget(Clear, rect);
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(t.border_active))
                .title(Line::from(vec![Span::styled(
                    format!(" {} ", p.title),
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                )]))
                .style(Style::default().bg(t.popup_bg));
            let inner = block.inner(rect);
            f.render_widget(block, rect);
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(" ", Style::default().bg(t.popup_bg)),
                    Span::styled(p.input.text.clone(), Style::default().fg(t.fg).bg(t.popup_bg)),
                ]))
                .style(Style::default().bg(t.popup_bg)),
                inner,
            );
            f.set_cursor_position((inner.x + 1 + p.input.cursor as u16, inner.y));
        }
        Some(Overlay::Palette(p)) => {
            let w: u16 = 66.min(area.width.saturating_sub(6));
            let list_h = (p.filtered.len().max(1) as u16).min(12);
            let h = list_h + 4;
            let rect = Rect {
                x: area.x + (area.width - w) / 2,
                y: area.y + 2,
                width: w,
                height: h.min(area.height.saturating_sub(3)),
            };
            f.render_widget(Clear, rect);
            let title = match p.mode {
                PaletteMode::Commands => " COMMAND PALETTE ",
                PaletteMode::Files => " GO TO FILE ",
                PaletteMode::Themes => " COLOR THEME ",
                PaletteMode::Keymaps => " KEYMAP ",
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(t.border_active))
                .title(Line::from(vec![Span::styled(
                    title,
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                )]))
                .style(Style::default().bg(t.popup_bg));
            let inner = block.inner(rect);
            f.render_widget(block, rect);
            if inner.height < 2 {
                return;
            }

            let width = inner.width as usize;
            let mut lines: Vec<Line> = Vec::new();
            // input row
            let input_used = 2 + p.input.text.chars().count();
            lines.push(Line::from(vec![
                Span::styled("❯ ", Style::default().fg(t.accent).bg(t.popup_bg).add_modifier(Modifier::BOLD)),
                Span::styled(p.input.text.clone(), Style::default().fg(t.fg).bg(t.popup_bg)),
                Span::styled(
                    " ".repeat(width.saturating_sub(input_used)),
                    Style::default().bg(t.popup_bg),
                ),
            ]));

            let list_rows = (inner.height as usize).saturating_sub(2);
            let start = if p.selected >= list_rows && list_rows > 0 {
                p.selected + 1 - list_rows
            } else {
                0
            };
            for vis in 0..list_rows {
                let idx = start + vis;
                let Some(&item_idx) = p.filtered.get(idx) else {
                    if idx == 0 {
                        let msg = if p.mode == PaletteMode::Files && app.files_loading() {
                            "  scanning project…"
                        } else {
                            "  no matches"
                        };
                        lines.push(Line::from(Span::styled(
                            msg,
                            Style::default().fg(t.dim).bg(t.popup_bg),
                        )));
                    }
                    continue;
                };
                let item = &p.items[item_idx];
                let selected = idx == p.selected;
                let row_bg = if selected { t.selection } else { t.popup_bg };
                let marker = if selected { "▌" } else { " " };
                let label: String = item.label.chars().take(width.saturating_sub(4 + item.hint.chars().count())).collect();
                let used = 2 + label.chars().count() + item.hint.chars().count() + 1;
                let mut label_style = Style::default().fg(t.fg).bg(row_bg);
                if selected {
                    label_style = label_style.add_modifier(Modifier::BOLD);
                }
                lines.push(Line::from(vec![
                    Span::styled(marker, Style::default().fg(t.accent).bg(row_bg)),
                    Span::styled(" ", Style::default().bg(row_bg)),
                    Span::styled(label, label_style),
                    Span::styled(
                        " ".repeat(width.saturating_sub(used)),
                        Style::default().bg(row_bg),
                    ),
                    Span::styled(item.hint.clone(), Style::default().fg(t.dim).bg(row_bg)),
                    Span::styled(" ", Style::default().bg(row_bg)),
                ]));
            }
            // footer
            lines.push(
                Line::from(Span::styled(
                    "↑↓ navigate · ↵ select · esc cancel",
                    Style::default().fg(t.dim).bg(t.popup_bg),
                ))
                .centered(),
            );
            f.render_widget(Paragraph::new(lines), inner);
            f.set_cursor_position((inner.x + 2 + p.input.cursor as u16, inner.y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    fn app_with_lines(n: usize) -> App {
        let mut app = App::new(PathBuf::from("."));
        app.open_untitled();
        {
            let b = app.buf_mut().unwrap();
            b.lines = (0..n).map(|i| format!("    let value_{i} = compute({i});")).collect();
            b.recompute_states();
        }
        app
    }

    /// Count Braille glyphs (U+2800..U+28FF) rendered in the buffer.
    fn braille_count(term: &Terminal<TestBackend>) -> usize {
        term.backend()
            .buffer()
            .content()
            .iter()
            .filter(|c| {
                c.symbol()
                    .chars()
                    .next()
                    .is_some_and(|ch| ('\u{2800}'..='\u{28FF}').contains(&ch))
            })
            .count()
    }

    #[test]
    fn minimap_renders_and_toggles() {
        let mut app = app_with_lines(200);
        let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();

        app.minimap = true;
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert!(app.layout.minimap.width > 0, "minimap column should be carved out");
        let on = braille_count(&term);
        assert!(on > 20, "minimap should render Braille glyphs, got {on}");

        app.minimap = false;
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert_eq!(app.layout.minimap.width, 0, "minimap rect cleared when off");
        assert_eq!(braille_count(&term), 0, "no minimap glyphs when disabled");
    }

    #[test]
    fn minimap_hidden_on_narrow_terminals() {
        let mut app = app_with_lines(200);
        app.minimap = true;
        let mut term = Terminal::new(TestBackend::new(60, 24)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert_eq!(app.layout.minimap.width, 0, "no minimap when there isn't room");
        assert_eq!(braille_count(&term), 0);
    }

    // Count guide glyphs only — `│` also draws block borders and the minimap
    // separator, so match on the guide's distinctive accent-blend color.
    fn guide_count(term: &Terminal<TestBackend>, color: Color) -> usize {
        term.backend()
            .buffer()
            .content()
            .iter()
            .filter(|c| c.symbol() == "│" && c.style().fg == Some(color))
            .count()
    }

    #[test]
    fn bracket_guide_connects_the_pair() {
        let mut app = App::new(PathBuf::from("."));
        app.open_untitled();
        {
            let b = app.buf_mut().unwrap();
            b.language = crate::syntax::Language::Rust;
            b.lines = vec![
                "fn f() {".into(),
                "    a();".into(),
                "    b();".into(),
                "}".into(),
            ];
            b.recompute_states();
            b.goto(1, 4); // put the cursor inside the block
        }
        let mut app_off = App::new(PathBuf::from("."));
        app_off.open_untitled();
        {
            let b = app_off.buf_mut().unwrap();
            b.language = crate::syntax::Language::Rust;
            b.lines = app.buf().unwrap().lines.clone();
            b.recompute_states();
            b.goto(0, 0); // cursor not enclosed by any pair
        }

        let guide_color = blend(app.theme().accent, app.theme().bg, 0.62);
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        // The two interior rows each get one guide glyph in the indent column.
        assert_eq!(guide_count(&term, guide_color), 2, "guide should span the interior rows");

        term.draw(|f| draw(f, &mut app_off)).unwrap();
        assert_eq!(guide_count(&term, guide_color), 0, "no guide when the cursor isn't enclosed");
    }
}
