//! Keyboard and mouse event handling (VS Code-style bindings).

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::app::{App, CommandId, Focus, Overlay, Panel};
use crate::buffer::col_at_visual;

fn hit(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// Returns true if the event may have changed state (redraw needed).
pub fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    if key.kind == KeyEventKind::Release {
        return false;
    }
    handle_key_inner(app, key);
    true
}

fn handle_key_inner(app: &mut App, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    // Ctrl+K chord (Ctrl+K Ctrl+T = theme picker)
    if app.chord_k {
        app.chord_k = false;
        if let KeyCode::Char(c) = key.code {
            if c.eq_ignore_ascii_case(&'t') {
                app.execute(CommandId::ThemePicker);
            }
        }
        return;
    }
    if ctrl && !alt && app.overlay.is_none() {
        if let KeyCode::Char(c) = key.code {
            if c.eq_ignore_ascii_case(&'k') {
                app.chord_k = true;
                return;
            }
        }
    }

    if app.overlay.is_some() {
        handle_overlay(app, key, ctrl);
        return;
    }
    if app.find_typing && app.find.is_some() {
        handle_find_bar(app, key, ctrl);
        return;
    }
    if handle_global(app, key, ctrl, alt, shift) {
        return;
    }
    match app.focus {
        Focus::Explorer => handle_explorer(app, key, shift),
        Focus::Panel => handle_panel(app, key),
        Focus::Editor => handle_editor(app, key, ctrl, alt, shift),
    }
}

fn ctrl_char(key: KeyEvent, target: char) -> bool {
    matches!(key.code, KeyCode::Char(c) if c.eq_ignore_ascii_case(&target))
}

fn handle_global(app: &mut App, key: KeyEvent, ctrl: bool, alt: bool, shift: bool) -> bool {
    if ctrl && !alt {
        let cmd = if ctrl_char(key, 'q') {
            Some(CommandId::Quit)
        } else if ctrl_char(key, 's') {
            Some(CommandId::Save)
        } else if ctrl_char(key, 'n') {
            Some(CommandId::NewFile)
        } else if ctrl_char(key, 'p') && shift {
            app.open_command_palette();
            return true;
        } else if ctrl_char(key, 'p') {
            Some(CommandId::QuickOpen)
        } else if ctrl_char(key, 'f') && shift {
            Some(CommandId::GlobalSearch)
        } else if ctrl_char(key, 'f') {
            Some(CommandId::Find)
        } else if ctrl_char(key, 'g') {
            Some(CommandId::GotoLine)
        } else if ctrl_char(key, 'b') {
            Some(CommandId::ToggleSidebar)
        } else if ctrl_char(key, 'e') {
            Some(CommandId::FocusExplorer)
        } else if ctrl_char(key, 'j') || ctrl_char(key, '`') {
            Some(CommandId::ToggleTerminal)
        } else if ctrl_char(key, 'w') {
            Some(CommandId::CloseTab)
        } else if key.code == KeyCode::PageDown {
            Some(CommandId::NextTab)
        } else if key.code == KeyCode::PageUp {
            Some(CommandId::PrevTab)
        } else {
            None
        };
        if let Some(cmd) = cmd {
            app.execute(cmd);
            return true;
        }
    }
    match key.code {
        KeyCode::F(1) => {
            app.open_command_palette();
            true
        }
        KeyCode::F(3) => {
            if app.find.is_some() {
                app.find_jump(!shift, true);
            } else {
                app.execute(CommandId::Find);
            }
            true
        }
        KeyCode::F(12) if shift => {
            app.execute(CommandId::FindReferences);
            true
        }
        KeyCode::Char(c) if alt && shift && c.eq_ignore_ascii_case(&'f') => {
            app.execute(CommandId::FormatDocument);
            true
        }
        KeyCode::Esc => {
            if app.find.is_some() {
                app.close_find();
            } else if app.focus == Focus::Panel {
                app.close_panel();
            } else if app.focus == Focus::Explorer {
                app.focus = Focus::Editor;
            } else if let Some(buf) = app.buf_mut() {
                buf.anchor = None;
            }
            true
        }
        _ => false,
    }
}

fn handle_overlay(app: &mut App, key: KeyEvent, ctrl: bool) {
    if key.code == KeyCode::Esc {
        app.close_overlay();
        return;
    }
    let is_palette = matches!(app.overlay, Some(Overlay::Palette(_)));
    let mut accept = false;
    let mut preview = false;
    match app.overlay.as_mut() {
        Some(Overlay::Palette(p)) => match key.code {
            KeyCode::Enter => accept = true,
            KeyCode::Up => {
                p.nav(-1);
                preview = true;
            }
            KeyCode::Down | KeyCode::Tab => {
                p.nav(1);
                preview = true;
            }
            KeyCode::PageUp => {
                p.nav(-8);
                preview = true;
            }
            KeyCode::PageDown => {
                p.nav(8);
                preview = true;
            }
            KeyCode::Backspace => {
                p.input.backspace();
                p.refilter();
                preview = true;
            }
            KeyCode::Delete => {
                p.input.delete();
                p.refilter();
                preview = true;
            }
            KeyCode::Left => p.input.left(),
            KeyCode::Right => p.input.right(),
            KeyCode::Home => p.input.home(),
            KeyCode::End => p.input.end(),
            KeyCode::Char(c) if !ctrl => {
                p.input.insert(c);
                p.refilter();
                preview = true;
            }
            _ => {}
        },
        Some(Overlay::Prompt(p)) => match key.code {
            KeyCode::Enter => accept = true,
            KeyCode::Backspace => p.input.backspace(),
            KeyCode::Delete => p.input.delete(),
            KeyCode::Left => p.input.left(),
            KeyCode::Right => p.input.right(),
            KeyCode::Home => p.input.home(),
            KeyCode::End => p.input.end(),
            KeyCode::Char(c) if !ctrl => p.input.insert(c),
            _ => {}
        },
        None => {}
    }
    if preview {
        app.palette_preview();
    }
    if accept {
        if is_palette {
            app.palette_accept();
        } else {
            app.submit_prompt();
        }
    }
}

fn handle_find_bar(app: &mut App, key: KeyEvent, ctrl: bool) {
    match key.code {
        KeyCode::Esc => app.close_find(),
        KeyCode::Enter | KeyCode::Down => app.find_jump(true, true),
        KeyCode::Up => app.find_jump(false, true),
        KeyCode::Backspace => {
            if let Some(f) = app.find.as_mut() {
                f.input.backspace();
            }
            app.refresh_find();
            app.find_jump(true, false);
        }
        KeyCode::Left => {
            if let Some(f) = app.find.as_mut() {
                f.input.left();
            }
        }
        KeyCode::Right => {
            if let Some(f) = app.find.as_mut() {
                f.input.right();
            }
        }
        KeyCode::Char(c) if !ctrl => {
            if let Some(f) = app.find.as_mut() {
                f.input.insert(c);
            }
            app.refresh_find();
            app.find_jump(true, false);
        }
        _ => {}
    }
}

fn handle_explorer(app: &mut App, key: KeyEvent, shift: bool) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => app.tree.move_sel(-1),
        KeyCode::Down | KeyCode::Char('j') => app.tree.move_sel(1),
        KeyCode::Char('g') => app.tree.select_first(),
        KeyCode::Char('G') => app.tree.select_last(),
        KeyCode::Enter | KeyCode::Char('l') => app.explorer_open(),
        KeyCode::Right => app.tree.expand(),
        KeyCode::Left | KeyCode::Char('h') => app.tree.collapse(),
        KeyCode::Char('a') | KeyCode::Char('n') if !shift => app.explorer_new_file(),
        KeyCode::Char('A') | KeyCode::Char('N') => app.explorer_new_dir(),
        KeyCode::Char('r') | KeyCode::F(2) => app.explorer_rename(),
        KeyCode::Char('d') | KeyCode::Delete => app.explorer_delete(),
        KeyCode::Char('R') => {
            app.tree.refresh();
        }
        _ => {}
    }
}

fn handle_panel(app: &mut App, key: KeyEvent) {
    let mut open = false;
    let mut close = false;
    match &mut app.panel {
        Panel::Search(pane) => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                pane.selected = pane.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if pane.selected + 1 < pane.matches.len() {
                    pane.selected += 1;
                }
            }
            KeyCode::Enter => open = true,
            KeyCode::Char('q') => close = true,
            _ => {}
        },
        Panel::Terminal => {
            if matches!(key.code, KeyCode::Char('q') | KeyCode::Enter) {
                close = true;
            }
        }
        Panel::None => app.focus = Focus::Editor,
    }
    if open {
        app.open_search_result();
    }
    if close {
        app.close_panel();
    }
}

fn handle_editor(app: &mut App, key: KeyEvent, ctrl: bool, alt: bool, shift: bool) {
    if app.buffers.is_empty() {
        return;
    }
    let page = app.layout.text.height.saturating_sub(1).max(1) as usize;

    // app-level clipboard / undo actions
    if ctrl && !alt {
        if ctrl_char(key, 'c') {
            app.copy();
            return;
        }
        if ctrl_char(key, 'x') {
            app.cut();
            app.follow = true;
            return;
        }
        if ctrl_char(key, 'v') {
            app.paste();
            app.follow = true;
            return;
        }
    }
    if ctrl && ctrl_char(key, 'z') && shift {
        if let Some(b) = app.buf_mut() {
            b.redo();
        }
        app.after_editor_action();
        return;
    }

    let lang_comment = app.buf().and_then(|b| b.language.comment_prefix());
    let Some(buf) = app.buffers.get_mut(app.active) else {
        return;
    };
    let mut edited = true; // most branches move or edit; used to refresh find state

    match key.code {
        // movement
        KeyCode::Left if ctrl => buf.move_word(false, shift),
        KeyCode::Right if ctrl => buf.move_word(true, shift),
        KeyCode::Left => buf.move_left(shift),
        KeyCode::Right => buf.move_right(shift),
        KeyCode::Up if alt => buf.move_line(false),
        KeyCode::Down if alt => buf.move_line(true),
        KeyCode::Up => buf.move_up(shift),
        KeyCode::Down => buf.move_down(shift),
        KeyCode::Home if ctrl => buf.move_doc(false, shift),
        KeyCode::End if ctrl => buf.move_doc(true, shift),
        KeyCode::Home => buf.move_home(shift),
        KeyCode::End => buf.move_end(shift),
        KeyCode::PageUp => buf.move_page(false, page, shift),
        KeyCode::PageDown => buf.move_page(true, page, shift),

        // editing
        KeyCode::Enter => buf.newline(),
        KeyCode::Backspace => buf.backspace(),
        KeyCode::Delete => buf.delete_forward(),
        KeyCode::Tab => {
            let multiline = buf.selection().map(|(a, b)| a.0 != b.0).unwrap_or(false);
            if multiline {
                buf.indent(false);
            } else {
                buf.insert_str("    ");
            }
        }
        KeyCode::BackTab => buf.indent(true),
        KeyCode::Char(c) if ctrl && !alt => {
            match c.to_ascii_lowercase() {
                'a' => buf.select_all(),
                'z' => {
                    buf.undo();
                }
                'y' => {
                    buf.redo();
                }
                'd' => buf.duplicate_line(),
                '/' | '_' | '7' => {
                    if let Some(prefix) = lang_comment {
                        buf.toggle_comment(prefix);
                    }
                }
                _ => edited = false,
            }
        }
        KeyCode::F(2) => {
            app.rename_symbol_prompt();
            return;
        }
        KeyCode::Char(c) if !ctrl && !alt => buf.insert_char(c),
        _ => edited = false,
    }
    if edited {
        app.after_editor_action();
    }
}

// ---- mouse ----

/// Returns true if the event may have changed state (redraw needed).
/// Plain mouse motion (no button) is ignored — terminals flood those.
pub fn handle_mouse(app: &mut App, m: MouseEvent) -> bool {
    match m.kind {
        MouseEventKind::Down(_)
        | MouseEventKind::Drag(_)
        | MouseEventKind::Up(_)
        | MouseEventKind::ScrollUp
        | MouseEventKind::ScrollDown => {
            handle_mouse_inner(app, m);
            true
        }
        _ => false,
    }
}

fn handle_mouse_inner(app: &mut App, m: MouseEvent) {
    let (x, y) = (m.column, m.row);
    match m.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if app.overlay.is_some() {
                return;
            }
            if hit(app.layout.tabbar, x, y) {
                let hits = app.tab_hits.clone();
                for (x0, x1, idx) in hits {
                    if x >= x0 && x < x1 {
                        app.active = idx;
                        app.focus = Focus::Editor;
                        app.refresh_find();
                        break;
                    }
                }
            } else if app.sidebar && hit(app.layout.sidebar_list, x, y) {
                app.focus = Focus::Explorer;
                let row = (y - app.layout.sidebar_list.y) as usize + app.tree.scroll;
                if row < app.tree.items.len() {
                    app.tree.selected = row;
                    app.explorer_open();
                }
            } else if hit(app.layout.text, x, y) {
                app.focus = Focus::Editor;
                click_editor(app, x, y, false);
                app.mouse_sel = true;
            } else if hit(app.layout.panel_list, x, y) {
                let mut open = false;
                if let Panel::Search(pane) = &mut app.panel {
                    app.focus = Focus::Panel;
                    let row = (y - app.layout.panel_list.y) as usize + pane.scroll;
                    if row < pane.matches.len() {
                        pane.selected = row;
                        open = true;
                    }
                }
                if open {
                    app.open_search_result();
                }
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.mouse_sel && hit(app.layout.text, x, y) {
                click_editor(app, x, y, true);
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            app.mouse_sel = false;
        }
        MouseEventKind::ScrollUp => scroll_at(app, x, y, -3),
        MouseEventKind::ScrollDown => scroll_at(app, x, y, 3),
        _ => {}
    }
}

fn click_editor(app: &mut App, x: u16, y: u16, drag: bool) {
    let text = app.layout.text;
    let Some(buf) = app.buffers.get_mut(app.active) else {
        return;
    };
    let row = (buf.scroll_row + (y - text.y) as usize).min(buf.lines.len().saturating_sub(1));
    let vcol = buf.scroll_col + (x - text.x) as usize;
    let col = col_at_visual(buf.line(row), vcol);
    if drag {
        if buf.anchor.is_none() {
            buf.anchor = Some(buf.cursor);
        }
    } else {
        buf.anchor = None;
    }
    buf.cursor = (row, col);
    buf.pref_col = vcol;
}

fn scroll_at(app: &mut App, x: u16, y: u16, delta: isize) {
    if app.sidebar && hit(app.layout.sidebar, x, y) {
        app.tree.move_sel(delta.signum() * 2);
    } else if hit(app.layout.panel, x, y) {
        if let Panel::Search(pane) = &mut app.panel {
            let n = pane.matches.len();
            if n > 0 {
                let cur = pane.selected as isize + delta.signum() * 2;
                pane.selected = cur.clamp(0, n as isize - 1) as usize;
            }
        }
    } else if let Some(buf) = app.buffers.get_mut(app.active) {
        let max = buf.lines.len().saturating_sub(1);
        let cur = buf.scroll_row as isize + delta;
        buf.scroll_row = cur.clamp(0, max as isize) as usize;
    }
}

#[cfg(test)]
mod mouse_tests {
    use super::*;
    use crate::app::App;
    use ratatui::crossterm::event::KeyModifiers;
    use ratatui::layout::Rect;
    use std::path::PathBuf;

    fn ev(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
        MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE }
    }

    #[test]
    fn drag_selects_expected_text_and_coalesces() {
        let mut app = App::new(PathBuf::from("."));
        app.open_untitled();
        {
            let b = app.buf_mut().unwrap();
            b.lines = vec!["hello world foo".to_string()];
            b.recompute_states();
        }
        // editor text area starts at (5,2)
        app.layout.text = Rect { x: 5, y: 2, width: 40, height: 10 };

        // press at col 0 of the line
        assert!(handle_mouse(&mut app, ev(MouseEventKind::Down(MouseButton::Left), 5, 2)));
        // 200 drag events ending at visual col 11 ("hello world" = 11 chars)
        for i in 1..=200u16 {
            let x = 5 + (i % 12); // wander, final lands at col 11
            handle_mouse(&mut app, ev(MouseEventKind::Drag(MouseButton::Left), x, 2));
        }
        // final deterministic drag to exactly col 11
        handle_mouse(&mut app, ev(MouseEventKind::Drag(MouseButton::Left), 16, 2));
        handle_mouse(&mut app, ev(MouseEventKind::Up(MouseButton::Left), 16, 2));

        let sel = app.buf().unwrap().selected_text();
        assert_eq!(sel.as_deref(), Some("hello world"), "drag selection text");

        // bare Moved events must be ignored (no redraw requested)
        assert!(!handle_mouse(&mut app, ev(MouseEventKind::Moved, 20, 2)));
    }
}
