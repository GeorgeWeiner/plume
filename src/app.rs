//! Central application state and actions.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use ratatui::layout::Rect;

use crate::buffer::Buffer;
use crate::explorer::FileTree;
use crate::palette::{InputLine, PaletteAction, PaletteItem, PaletteMode, PaletteState};
use crate::search::{self, SearchMatch};
use crate::theme::Theme;

#[derive(Clone, Copy, PartialEq)]
pub enum Focus {
    Editor,
    Explorer,
    Panel,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

pub struct Notification {
    pub text: String,
    pub level: Level,
    pub at: Instant,
}

#[derive(Clone, Copy, PartialEq)]
pub enum CommandId {
    NewFile,
    QuickOpen,
    Save,
    SaveAs,
    CloseTab,
    NextTab,
    PrevTab,
    Find,
    GlobalSearch,
    GotoLine,
    ToggleSidebar,
    ToggleTerminal,
    ThemePicker,
    RenameSymbol,
    FormatDocument,
    FindReferences,
    ExtractVariable,
    FocusExplorer,
    Quit,
}

pub fn command_list() -> Vec<(CommandId, &'static str, &'static str)> {
    vec![
        (CommandId::QuickOpen, "Go to File…", "Ctrl+P"),
        (CommandId::GlobalSearch, "Search in Project…", "Ctrl+Shift+F"),
        (CommandId::Find, "Find in File", "Ctrl+F"),
        (CommandId::GotoLine, "Go to Line…", "Ctrl+G"),
        (CommandId::NewFile, "New Untitled File", "Ctrl+N"),
        (CommandId::Save, "Save File", "Ctrl+S"),
        (CommandId::SaveAs, "Save As…", ""),
        (CommandId::CloseTab, "Close Editor Tab", "Ctrl+W"),
        (CommandId::NextTab, "Next Tab", "Ctrl+PgDn"),
        (CommandId::PrevTab, "Previous Tab", "Ctrl+PgUp"),
        (CommandId::ToggleSidebar, "Toggle Sidebar", "Ctrl+B"),
        (CommandId::ToggleTerminal, "Toggle Terminal Panel", "Ctrl+`"),
        (CommandId::FocusExplorer, "Focus Explorer", "Ctrl+E"),
        (CommandId::ThemePicker, "Color Theme…", "Ctrl+K Ctrl+T"),
        (CommandId::RenameSymbol, "Rename Symbol (naive)", "F2"),
        (CommandId::FormatDocument, "Format Document (basic)", "Shift+Alt+F"),
        (CommandId::FindReferences, "Find All References (naive)", "Shift+F12"),
        (CommandId::ExtractVariable, "Extract Variable (naive)", ""),
        (CommandId::Quit, "Quit Plume", "Ctrl+Q"),
    ]
}

pub enum PromptKind {
    NewFile { dir: PathBuf },
    NewDir { dir: PathBuf },
    RenamePath { path: PathBuf },
    DeletePath { path: PathBuf },
    RenameSymbol { old: String },
    GotoLine,
    SaveAs,
    GlobalSearch,
    QuitConfirm,
}

pub struct PromptState {
    pub title: String,
    pub input: InputLine,
    pub kind: PromptKind,
}

pub enum Overlay {
    Palette(PaletteState),
    Prompt(PromptState),
}

pub struct FindState {
    pub input: InputLine,
    pub matches: Vec<(usize, usize)>,
    pub idx: usize,
}

pub struct SearchPane {
    pub title: String,
    pub query: String,
    pub matches: Vec<SearchMatch>,
    pub selected: usize,
    pub scroll: usize,
}

pub enum Panel {
    None,
    Terminal,
    Search(SearchPane),
}

#[derive(Default, Clone, Copy)]
pub struct LayoutInfo {
    pub tabbar: Rect,
    pub sidebar: Rect,
    pub sidebar_list: Rect,
    pub editor: Rect,
    /// Text area inside the editor block, after the gutter.
    pub text: Rect,
    pub panel: Rect,
    pub panel_list: Rect,
    pub status: Rect,
}

pub struct App {
    pub root: PathBuf,
    pub themes: Vec<Theme>,
    pub theme_idx: usize,
    pub theme_before_preview: Option<usize>,
    pub tree: FileTree,
    pub buffers: Vec<Buffer>,
    pub active: usize,
    pub focus: Focus,
    pub sidebar: bool,
    pub overlay: Option<Overlay>,
    pub panel: Panel,
    pub find: Option<FindState>,
    pub find_typing: bool,
    pub clipboard: String,
    pub notices: Vec<Notification>,
    pub chord_k: bool,
    pub should_quit: bool,
    pub layout: LayoutInfo,
    pub tab_hits: Vec<(u16, u16, usize)>,
    pub mouse_sel: bool,
    /// When true, the next draw scrolls the editor so the cursor is visible.
    pub follow: bool,
}

impl App {
    pub fn new(root: PathBuf) -> App {
        App {
            tree: FileTree::new(root.clone()),
            root,
            themes: Theme::all(),
            theme_idx: 0,
            theme_before_preview: None,
            buffers: Vec::new(),
            active: 0,
            focus: Focus::Editor,
            sidebar: true,
            overlay: None,
            panel: Panel::None,
            find: None,
            find_typing: false,
            clipboard: String::new(),
            notices: Vec::new(),
            chord_k: false,
            should_quit: false,
            layout: LayoutInfo::default(),
            tab_hits: Vec::new(),
            mouse_sel: false,
            follow: true,
        }
    }

    /// Called after any editor keystroke: keep the cursor on screen and keep
    /// find-in-file matches in sync with the buffer contents.
    pub fn after_editor_action(&mut self) {
        self.follow = true;
        if self.find.is_some() {
            let query = self.find.as_ref().map(|f| f.input.text.clone()).unwrap_or_default();
            let matches = self.buf().map(|b| b.find_matches(&query)).unwrap_or_default();
            if let Some(f) = self.find.as_mut() {
                f.matches = matches;
                f.idx = f.idx.min(f.matches.len().saturating_sub(1));
            }
        }
    }

    pub fn theme(&self) -> &Theme {
        &self.themes[self.theme_idx]
    }

    pub fn buf(&self) -> Option<&Buffer> {
        self.buffers.get(self.active)
    }

    pub fn buf_mut(&mut self) -> Option<&mut Buffer> {
        self.buffers.get_mut(self.active)
    }

    pub fn notify(&mut self, text: impl Into<String>, level: Level) {
        self.notices.push(Notification { text: text.into(), level, at: Instant::now() });
        if self.notices.len() > 4 {
            self.notices.remove(0);
        }
    }

    pub fn tick(&mut self) {
        self.notices.retain(|n| n.at.elapsed().as_secs_f32() < 4.0);
    }

    // ---- files & buffers ----

    pub fn open_file(&mut self, path: &Path) {
        if let Some(idx) = self
            .buffers
            .iter()
            .position(|b| b.path.as_deref() == Some(path))
        {
            self.active = idx;
            self.focus = Focus::Editor;
            self.follow = true;
            self.refresh_find();
            return;
        }
        match Buffer::from_path(path) {
            Ok(b) => {
                self.buffers.push(b);
                self.active = self.buffers.len() - 1;
                self.focus = Focus::Editor;
                self.refresh_find();
                self.tree.reveal(path);
            }
            Err(e) => self.notify(format!("Cannot open {}: {e}", path.display()), Level::Error),
        }
    }

    pub fn open_untitled(&mut self) {
        self.buffers.push(Buffer::untitled());
        self.active = self.buffers.len() - 1;
        self.focus = Focus::Editor;
    }

    pub fn save_active(&mut self) {
        let Some(buf) = self.buffers.get_mut(self.active) else {
            return;
        };
        if buf.path.is_none() {
            self.open_prompt(PromptKind::SaveAs, "Save As (path relative to project)", "");
            return;
        }
        let name = buf.display_name();
        let lines = buf.lines.len();
        match buf.save() {
            Ok(()) => self.notify(format!("Saved {name} ({lines} lines)"), Level::Info),
            Err(e) => self.notify(format!("Save failed: {e}"), Level::Error),
        }
    }

    pub fn close_tab(&mut self) {
        if self.buffers.is_empty() {
            return;
        }
        let buf = &self.buffers[self.active];
        if buf.modified {
            self.notify(
                format!("{} has unsaved changes (Ctrl+S to save; close again to discard)", buf.display_name()),
                Level::Warn,
            );
            self.buffers[self.active].modified = false; // next close discards
            return;
        }
        self.buffers.remove(self.active);
        if self.active >= self.buffers.len() && self.active > 0 {
            self.active -= 1;
        }
        self.find = None;
    }

    pub fn cycle_tab(&mut self, delta: isize) {
        if self.buffers.len() < 2 {
            return;
        }
        let n = self.buffers.len() as isize;
        self.active = (self.active as isize + delta).rem_euclid(n) as usize;
        self.refresh_find();
    }

    // ---- command execution ----

    pub fn execute(&mut self, cmd: CommandId) {
        match cmd {
            CommandId::NewFile => self.open_untitled(),
            CommandId::QuickOpen => self.open_quick_open(),
            CommandId::Save => self.save_active(),
            CommandId::SaveAs => {
                if self.buf().is_some() {
                    self.open_prompt(PromptKind::SaveAs, "Save As (path relative to project)", "");
                }
            }
            CommandId::CloseTab => self.close_tab(),
            CommandId::NextTab => self.cycle_tab(1),
            CommandId::PrevTab => self.cycle_tab(-1),
            CommandId::Find => self.open_find(),
            CommandId::GlobalSearch => {
                self.open_prompt(PromptKind::GlobalSearch, "Search in project", "")
            }
            CommandId::GotoLine => self.open_prompt(PromptKind::GotoLine, "Go to line", ""),
            CommandId::ToggleSidebar => self.sidebar = !self.sidebar,
            CommandId::ToggleTerminal => self.toggle_terminal(),
            CommandId::ThemePicker => self.open_theme_picker(),
            CommandId::RenameSymbol => self.rename_symbol_prompt(),
            CommandId::FormatDocument => self.format_document(),
            CommandId::FindReferences => self.find_references(),
            CommandId::ExtractVariable => self.extract_variable(),
            CommandId::FocusExplorer => {
                self.sidebar = true;
                self.focus = if self.focus == Focus::Explorer { Focus::Editor } else { Focus::Explorer };
            }
            CommandId::Quit => self.request_quit(),
        }
    }

    pub fn request_quit(&mut self) {
        if self.buffers.iter().any(|b| b.modified) {
            self.open_prompt(PromptKind::QuitConfirm, "Unsaved changes — type y to quit anyway", "");
        } else {
            self.should_quit = true;
        }
    }

    // ---- overlays ----

    pub fn open_command_palette(&mut self) {
        let items = command_list()
            .into_iter()
            .map(|(id, label, hint)| PaletteItem {
                label: label.to_string(),
                hint: hint.to_string(),
                action: PaletteAction::Command(id),
            })
            .collect();
        self.overlay = Some(Overlay::Palette(PaletteState::new(PaletteMode::Commands, items)));
    }

    pub fn open_quick_open(&mut self) {
        let files = search::list_files(&self.root);
        let items = files
            .into_iter()
            .map(|p| {
                let rel = p.strip_prefix(&self.root).unwrap_or(&p).to_path_buf();
                PaletteItem {
                    label: rel.display().to_string(),
                    hint: String::new(),
                    action: PaletteAction::OpenFile(p),
                }
            })
            .collect();
        self.overlay = Some(Overlay::Palette(PaletteState::new(PaletteMode::Files, items)));
    }

    pub fn open_theme_picker(&mut self) {
        let items = self
            .themes
            .iter()
            .enumerate()
            .map(|(i, t)| PaletteItem {
                label: t.name.to_string(),
                hint: if i == self.theme_idx { "current".into() } else { String::new() },
                action: PaletteAction::SetTheme(i),
            })
            .collect();
        self.theme_before_preview = Some(self.theme_idx);
        let mut st = PaletteState::new(PaletteMode::Themes, items);
        st.selected = self.theme_idx;
        self.overlay = Some(Overlay::Palette(st));
    }

    pub fn open_prompt(&mut self, kind: PromptKind, title: &str, initial: &str) {
        self.overlay = Some(Overlay::Prompt(PromptState {
            title: title.to_string(),
            input: InputLine::with_text(initial),
            kind,
        }));
    }

    pub fn close_overlay(&mut self) {
        if let Some(prev) = self.theme_before_preview.take() {
            self.theme_idx = prev;
        }
        self.overlay = None;
    }

    /// Live-preview themes as the picker selection moves.
    pub fn palette_preview(&mut self) {
        if let Some(Overlay::Palette(p)) = &self.overlay {
            if p.mode == PaletteMode::Themes {
                if let Some(PaletteAction::SetTheme(i)) = p.current().map(|it| it.action.clone()) {
                    self.theme_idx = i;
                }
            }
        }
    }

    pub fn palette_accept(&mut self) {
        let Some(Overlay::Palette(p)) = self.overlay.take() else {
            return;
        };
        let Some(item) = p.current() else {
            self.theme_before_preview = None;
            return;
        };
        match item.action.clone() {
            PaletteAction::Command(cmd) => {
                self.theme_before_preview = None;
                self.execute(cmd);
            }
            PaletteAction::OpenFile(path) => {
                self.theme_before_preview = None;
                self.open_file(&path);
            }
            PaletteAction::SetTheme(i) => {
                self.theme_idx = i;
                self.theme_before_preview = None;
                let name = self.themes[i].name;
                self.notify(format!("Theme: {name}"), Level::Info);
            }
        }
    }

    // ---- prompt submission ----

    pub fn submit_prompt(&mut self) {
        let Some(Overlay::Prompt(p)) = self.overlay.take() else {
            return;
        };
        let text = p.input.text.trim().to_string();
        match p.kind {
            PromptKind::NewFile { dir } => {
                if text.is_empty() {
                    return;
                }
                let path = dir.join(&text);
                if path.exists() {
                    self.notify("File already exists", Level::Warn);
                    return;
                }
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                match fs::write(&path, "") {
                    Ok(()) => {
                        self.tree.refresh();
                        self.open_file(&path);
                        self.notify(format!("Created {text}"), Level::Info);
                    }
                    Err(e) => self.notify(format!("Create failed: {e}"), Level::Error),
                }
            }
            PromptKind::NewDir { dir } => {
                if text.is_empty() {
                    return;
                }
                let path = dir.join(&text);
                match fs::create_dir_all(&path) {
                    Ok(()) => {
                        self.tree.refresh();
                        self.tree.reveal(&path);
                        self.notify(format!("Created {text}/"), Level::Info);
                    }
                    Err(e) => self.notify(format!("Create failed: {e}"), Level::Error),
                }
            }
            PromptKind::RenamePath { path } => {
                if text.is_empty() {
                    return;
                }
                let new_path = path.parent().unwrap_or(&self.root).join(&text);
                match fs::rename(&path, &new_path) {
                    Ok(()) => {
                        for b in self.buffers.iter_mut() {
                            if b.path.as_deref() == Some(path.as_path()) {
                                b.path = Some(new_path.clone());
                                b.language = crate::syntax::Language::from_path(&new_path);
                                b.recompute_states();
                            }
                        }
                        self.tree.refresh();
                        self.tree.reveal(&new_path);
                        self.notify(format!("Renamed to {text}"), Level::Info);
                    }
                    Err(e) => self.notify(format!("Rename failed: {e}"), Level::Error),
                }
            }
            PromptKind::DeletePath { path } => {
                if text != "y" && text != "yes" {
                    self.notify("Delete cancelled", Level::Info);
                    return;
                }
                let res = if path.is_dir() {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_file(&path)
                };
                match res {
                    Ok(()) => {
                        self.buffers.retain(|b| {
                            b.path.as_deref().map(|p| !p.starts_with(&path)).unwrap_or(true)
                        });
                        if self.active >= self.buffers.len() {
                            self.active = self.buffers.len().saturating_sub(1);
                        }
                        self.tree.refresh();
                        self.notify(
                            format!("Deleted {}", path.file_name().unwrap_or_default().to_string_lossy()),
                            Level::Info,
                        );
                    }
                    Err(e) => self.notify(format!("Delete failed: {e}"), Level::Error),
                }
            }
            PromptKind::RenameSymbol { old } => {
                if text.is_empty() || text == old {
                    return;
                }
                if let Some(buf) = self.buffers.get_mut(self.active) {
                    let count = buf.rename_word(&old, &text);
                    self.notify(
                        format!("Renamed {count} occurrence(s) of '{old}' in this file (naive rename)"),
                        Level::Info,
                    );
                }
            }
            PromptKind::GotoLine => {
                if let Ok(n) = text.parse::<usize>() {
                    if let Some(buf) = self.buffers.get_mut(self.active) {
                        buf.goto(n.saturating_sub(1), 0);
                    }
                    self.follow = true;
                }
            }
            PromptKind::SaveAs => {
                if text.is_empty() {
                    return;
                }
                let path = self.root.join(&text);
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if let Some(buf) = self.buffers.get_mut(self.active) {
                    buf.path = Some(path.clone());
                    buf.language = crate::syntax::Language::from_path(&path);
                    buf.recompute_states();
                    match buf.save() {
                        Ok(()) => {
                            self.tree.refresh();
                            self.tree.reveal(&path);
                            self.notify(format!("Saved {text}"), Level::Info);
                        }
                        Err(e) => self.notify(format!("Save failed: {e}"), Level::Error),
                    }
                }
            }
            PromptKind::GlobalSearch => {
                if !text.is_empty() {
                    self.run_global_search(&text, None);
                }
            }
            PromptKind::QuitConfirm => {
                if text == "y" || text == "yes" {
                    self.should_quit = true;
                }
            }
        }
    }

    // ---- find in file ----

    pub fn open_find(&mut self) {
        if self.buf().is_none() {
            return;
        }
        let initial = self
            .buf()
            .and_then(|b| b.selected_text())
            .filter(|t| !t.contains('\n') && t.len() < 64)
            .unwrap_or_default();
        self.find = Some(FindState {
            input: InputLine::with_text(&initial),
            matches: Vec::new(),
            idx: 0,
        });
        self.find_typing = true;
        self.refresh_find();
        if !initial.is_empty() {
            self.find_jump(true, false);
        }
    }

    pub fn close_find(&mut self) {
        self.find = None;
        self.find_typing = false;
    }

    /// Recompute matches for the active buffer.
    pub fn refresh_find(&mut self) {
        let Some(query) = self.find.as_ref().map(|f| f.input.text.clone()) else {
            return;
        };
        let matches = self.buf().map(|b| b.find_matches(&query)).unwrap_or_default();
        let cursor = self.buf().map(|b| b.cursor).unwrap_or((0, 0));
        if let Some(f) = self.find.as_mut() {
            f.idx = matches
                .iter()
                .position(|&m| m >= cursor)
                .unwrap_or(0);
            f.matches = matches;
        }
    }

    /// Jump to next/prev match. `advance` moves before jumping.
    pub fn find_jump(&mut self, forward: bool, advance: bool) {
        let Some(f) = self.find.as_mut() else { return };
        if f.matches.is_empty() {
            return;
        }
        if advance {
            let n = f.matches.len() as isize;
            let d = if forward { 1 } else { -1 };
            f.idx = ((f.idx as isize + d).rem_euclid(n)) as usize;
        } else {
            f.idx = f.idx.min(f.matches.len() - 1);
        }
        let (row, col) = f.matches[f.idx];
        let len = f.input.text.chars().count();
        if let Some(buf) = self.buffers.get_mut(self.active) {
            buf.select_range(row, col, len);
        }
        self.follow = true;
    }

    // ---- global search / references ----

    pub fn run_global_search(&mut self, query: &str, title: Option<String>) {
        let matches = search::search_project(&self.root, query);
        let count = matches.len();
        let title = title.unwrap_or_else(|| "SEARCH".to_string());
        self.panel = Panel::Search(SearchPane {
            title,
            query: query.to_string(),
            matches,
            selected: 0,
            scroll: 0,
        });
        self.focus = Focus::Panel;
        if count == 0 {
            self.notify(format!("No results for '{query}'"), Level::Warn);
        }
    }

    pub fn find_references(&mut self) {
        let Some((word, _, _)) = self.buf().and_then(|b| b.word_under_cursor()) else {
            self.notify("Place the cursor on a symbol first", Level::Warn);
            return;
        };
        self.run_global_search(&word, Some(format!("REFERENCES: {word}")));
    }

    /// Open the selected search result in the editor.
    pub fn open_search_result(&mut self) {
        let Panel::Search(pane) = &self.panel else { return };
        let Some(m) = pane.matches.get(pane.selected) else { return };
        let (path, row, col, len) = (m.path.clone(), m.line_no, m.col, m.len);
        self.open_file(&path);
        if let Some(buf) = self.buffers.get_mut(self.active) {
            buf.select_range(row, col, len);
        }
        self.focus = Focus::Editor;
        self.follow = true;
    }

    // ---- refactoring (naive prototype implementations) ----

    pub fn rename_symbol_prompt(&mut self) {
        let Some((word, _, _)) = self.buf().and_then(|b| b.word_under_cursor()) else {
            self.notify("Place the cursor on a symbol first", Level::Warn);
            return;
        };
        let title = format!("Rename symbol '{word}' (this file only)");
        self.open_prompt(PromptKind::RenameSymbol { old: word.clone() }, &title, &word);
    }

    pub fn format_document(&mut self) {
        let Some(buf) = self.buffers.get_mut(self.active) else {
            return;
        };
        let n = buf.trim_trailing_whitespace();
        self.notify(
            format!("Formatted: trimmed trailing whitespace on {n} line(s). Full formatting needs an LSP."),
            Level::Info,
        );
    }

    pub fn extract_variable(&mut self) {
        let lang = self.buf().map(|b| b.language);
        let Some(buf) = self.buffers.get_mut(self.active) else {
            return;
        };
        let Some(sel) = buf.selected_text().filter(|s| !s.contains('\n')) else {
            self.notify("Select a single-line expression first", Level::Warn);
            return;
        };
        use crate::syntax::Language;
        let decl = match lang {
            Some(Language::Rust) => format!("let extracted = {sel};"),
            Some(Language::Python) => format!("extracted = {sel}"),
            Some(Language::Go) => format!("extracted := {sel}"),
            _ => format!("const extracted = {sel};"),
        };
        let row = buf.selection().map(|(a, _)| a.0).unwrap_or(buf.cursor.0);
        let indent: String = buf
            .line(row)
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();
        buf.insert_str("extracted"); // replaces the selection
        let save_cursor = buf.cursor;
        buf.goto(row, 0);
        buf.insert_str(&format!("{indent}{decl}\n"));
        buf.goto(save_cursor.0 + 1, save_cursor.1);
        self.notify("Extracted to 'extracted' — rename it with F2 (naive refactor)", Level::Info);
    }

    // ---- panels ----

    pub fn toggle_terminal(&mut self) {
        self.panel = match self.panel {
            Panel::Terminal => Panel::None,
            _ => Panel::Terminal,
        };
        if matches!(self.panel, Panel::None) && self.focus == Focus::Panel {
            self.focus = Focus::Editor;
        }
    }

    pub fn close_panel(&mut self) {
        self.panel = Panel::None;
        if self.focus == Focus::Panel {
            self.focus = Focus::Editor;
        }
    }

    // ---- clipboard ----

    pub fn copy(&mut self) {
        let Some(buf) = self.buf() else { return };
        let text = buf
            .selected_text()
            .unwrap_or_else(|| format!("{}\n", buf.line(buf.cursor.0)));
        self.clipboard = text;
        self.notify("Copied", Level::Info);
    }

    pub fn cut(&mut self) {
        let Some(buf) = self.buffers.get_mut(self.active) else { return };
        if buf.selection().is_some() {
            self.clipboard = buf.selected_text().unwrap_or_default();
            buf.delete_selection();
        } else {
            // cut whole line
            let r = buf.cursor.0;
            self.clipboard = format!("{}\n", buf.line(r));
            if buf.lines.len() > 1 {
                buf.select_range(r, 0, buf.line_len(r));
                buf.anchor = Some((r, 0));
                buf.cursor = if r + 1 < buf.lines.len() { (r + 1, 0) } else { (r, buf.line_len(r)) };
                buf.delete_selection();
            } else {
                buf.select_range(0, 0, buf.line_len(0));
                buf.delete_selection();
            }
        }
        self.refresh_find();
    }

    pub fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        let text = self.clipboard.clone();
        if let Some(buf) = self.buffers.get_mut(self.active) {
            buf.insert_str(&text);
        }
        self.refresh_find();
    }

    // ---- explorer actions ----

    pub fn explorer_open(&mut self) {
        let Some(item) = self.tree.selected_item() else { return };
        if item.is_dir {
            self.tree.toggle();
        } else {
            let path = item.path.clone();
            self.open_file(&path);
        }
    }

    pub fn explorer_new_file(&mut self) {
        let dir = self.tree.target_dir();
        let rel = dir.strip_prefix(&self.root).unwrap_or(&dir).display().to_string();
        let title = if rel.is_empty() {
            "New file".to_string()
        } else {
            format!("New file in {rel}/")
        };
        self.open_prompt(PromptKind::NewFile { dir }, &title, "");
    }

    pub fn explorer_new_dir(&mut self) {
        let dir = self.tree.target_dir();
        self.open_prompt(PromptKind::NewDir { dir }, "New folder", "");
    }

    pub fn explorer_rename(&mut self) {
        let Some(item) = self.tree.selected_item() else { return };
        let path = item.path.clone();
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        self.open_prompt(PromptKind::RenamePath { path }, "Rename", &name);
    }

    pub fn explorer_delete(&mut self) {
        let Some(item) = self.tree.selected_item() else { return };
        let path = item.path.clone();
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let title = format!("Delete '{name}'? type y to confirm");
        self.open_prompt(PromptKind::DeletePath { path }, &title, "");
    }
}
