//! Central application state and actions.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::time::Instant;

use ratatui::layout::Rect;

use crate::buffer::Buffer;
use crate::config;
use crate::explorer::FileTree;
use crate::keymap::{Chord, Keymap};
use crate::palette::{InputLine, PaletteAction, PaletteItem, PaletteMode, PaletteState};
use crate::search::{self, SearchMatch, SearchMsg};
use crate::session;
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommandId {
    NewFile,
    QuickOpen,
    Save,
    SaveAs,
    CloseTab,
    NextTab,
    PrevTab,
    Find,
    FindNext,
    FindPrev,
    GlobalSearch,
    GotoLine,
    ToggleSidebar,
    ToggleTerminal,
    ToggleMinimap,
    ThemePicker,
    KeymapPicker,
    CommandPalette,
    RenameSymbol,
    FormatDocument,
    FindReferences,
    GoToDefinition,
    ExtractVariable,
    FocusExplorer,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    DuplicateLine,
    DeleteLine,
    MoveLinesUp,
    MoveLinesDown,
    ToggleComment,
    Indent,
    Outdent,
    Quit,
}

/// (command, palette label). The key hint shown alongside is derived from the
/// active keymap at display time, so it always reflects the current preset.
pub fn command_list() -> Vec<(CommandId, &'static str)> {
    vec![
        (CommandId::QuickOpen, "Go to File…"),
        (CommandId::GlobalSearch, "Search in Project…"),
        (CommandId::Find, "Find in File"),
        (CommandId::GotoLine, "Go to Line…"),
        (CommandId::NewFile, "New Untitled File"),
        (CommandId::Save, "Save File"),
        (CommandId::SaveAs, "Save As…"),
        (CommandId::CloseTab, "Close Editor Tab"),
        (CommandId::NextTab, "Next Tab"),
        (CommandId::PrevTab, "Previous Tab"),
        (CommandId::ToggleSidebar, "Toggle Sidebar"),
        (CommandId::ToggleMinimap, "Toggle Minimap"),
        (CommandId::ToggleTerminal, "Toggle Terminal Panel"),
        (CommandId::FocusExplorer, "Focus Explorer"),
        (CommandId::ThemePicker, "Color Theme…"),
        (CommandId::KeymapPicker, "Keymap: Select Preset…"),
        (CommandId::ToggleComment, "Toggle Line Comment"),
        (CommandId::DuplicateLine, "Duplicate Line"),
        (CommandId::DeleteLine, "Delete Line"),
        (CommandId::MoveLinesUp, "Move Line Up"),
        (CommandId::MoveLinesDown, "Move Line Down"),
        (CommandId::Undo, "Undo"),
        (CommandId::Redo, "Redo"),
        (CommandId::RenameSymbol, "Rename Symbol (naive)"),
        (CommandId::FormatDocument, "Format Document (basic)"),
        (CommandId::GoToDefinition, "Go to Definition (naive)"),
        (CommandId::FindReferences, "Find All References (naive)"),
        (CommandId::ExtractVariable, "Extract Variable (naive)"),
        (CommandId::Quit, "Quit Plume"),
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
    /// False while a worker thread is still streaming results in.
    pub done: bool,
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
    /// Minimap column at the right of the editor (empty when disabled).
    pub minimap: Rect,
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
    /// The embedded shell, spawned on first open and kept alive across toggles.
    pub terminal: Option<crate::pty::PtyTerminal>,
    pub find: Option<FindState>,
    pub find_typing: bool,
    pub clipboard: String,
    pub notices: Vec<Notification>,
    pub keymap: Keymap,
    /// First chord of a pending two-key sequence (e.g. after Ctrl+K).
    pub pending_chord: Option<Chord>,
    /// User keybinding overrides from the config file (survive keymap switches).
    key_overrides: Vec<(CommandId, Vec<Chord>)>,
    pub should_quit: bool,
    pub layout: LayoutInfo,
    pub tab_hits: Vec<(u16, u16, usize)>,
    pub mouse_sel: bool,
    /// A minimap scrub drag is in progress.
    pub mouse_minimap: bool,
    /// Show the zoomed-out minimap column at the right of the editor.
    pub minimap: bool,
    /// When true, the next draw scrolls the editor so the cursor is visible.
    pub follow: bool,
    /// Streaming results from the current global-search worker, if any.
    search_rx: Option<Receiver<SearchMsg>>,
    search_gen: u64,
    /// Pending file listing for quick open, if a scan is in flight.
    files_rx: Option<Receiver<(u64, Vec<PathBuf>)>>,
    files_gen: u64,
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
            terminal: None,
            find: None,
            find_typing: false,
            clipboard: String::new(),
            notices: Vec::new(),
            keymap: Keymap::preset("vscode", &[]),
            pending_chord: None,
            key_overrides: Vec::new(),
            should_quit: false,
            layout: LayoutInfo::default(),
            tab_hits: Vec::new(),
            mouse_sel: false,
            mouse_minimap: false,
            minimap: true,
            follow: true,
            search_rx: None,
            search_gen: 0,
            files_rx: None,
            files_gen: 0,
        }
    }

    /// True while quick open is waiting for the project file scan.
    pub fn files_loading(&self) -> bool {
        self.files_rx.is_some()
    }

    /// Pull in any results background workers have produced. Returns true if
    /// something changed and the UI should redraw.
    pub fn drain_async(&mut self) -> bool {
        let mut changed = false;

        // global search results
        let mut finished = false;
        let mut no_results_query: Option<String> = None;
        if let Some(rx) = &self.search_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    SearchMsg::Batch(gen, batch) if gen == self.search_gen => {
                        if let Panel::Search(p) = &mut self.panel {
                            p.matches.extend(batch);
                            changed = true;
                        }
                    }
                    SearchMsg::Done(gen) if gen == self.search_gen => {
                        if let Panel::Search(p) = &mut self.panel {
                            p.done = true;
                            if p.matches.is_empty() {
                                no_results_query = Some(p.query.clone());
                            }
                        }
                        finished = true;
                        changed = true;
                    }
                    _ => {} // stale generation — a newer search superseded it
                }
            }
        }
        if finished {
            self.search_rx = None;
        }
        if let Some(q) = no_results_query {
            self.notify(format!("No results for '{q}'"), Level::Warn);
        }

        // quick-open file listing
        let mut files: Option<Vec<PathBuf>> = None;
        if let Some(rx) = &self.files_rx {
            while let Ok((gen, list)) = rx.try_recv() {
                if gen == self.files_gen {
                    files = Some(list);
                }
            }
        }
        if let Some(list) = files {
            self.files_rx = None;
            if let Some(Overlay::Palette(p)) = &mut self.overlay {
                if p.mode == PaletteMode::Files {
                    p.items = list
                        .into_iter()
                        .map(|path| {
                            let rel = path.strip_prefix(&self.root).unwrap_or(&path).to_path_buf();
                            PaletteItem {
                                label: rel.display().to_string(),
                                hint: String::new(),
                                action: PaletteAction::OpenFile(path),
                            }
                        })
                        .collect();
                    p.refilter();
                    changed = true;
                }
            }
        }
        changed
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

    // ---- keymap & config ----

    /// Apply a loaded config: overrides first, then keymap, then theme.
    pub fn apply_config(&mut self, cfg: config::Config) {
        self.key_overrides = cfg.overrides;
        let id = cfg.keymap.as_deref().unwrap_or("vscode");
        self.keymap = Keymap::preset(id, &self.key_overrides);
        if let Some(name) = cfg.theme {
            self.set_theme_by_name(&name);
        }
        if let Some(m) = cfg.minimap {
            self.minimap = m;
        }
    }

    pub fn set_theme_by_name(&mut self, name: &str) {
        if let Some(i) = self
            .themes
            .iter()
            .position(|t| t.name.eq_ignore_ascii_case(name))
        {
            self.theme_idx = i;
        }
    }

    /// Switch keymap preset from the picker: rebuild and persist to config.
    pub fn set_keymap(&mut self, id: &str) {
        self.keymap = Keymap::preset(id, &self.key_overrides);
        config::set_value("keymap", id);
        let name = self.keymap.name.clone();
        self.notify(format!("Keymap: {name} (saved to config)"), Level::Info);
    }

    // ---- session persistence ----

    /// Capture the current workspace state for saving on exit.
    pub fn session_snapshot(&self) -> session::Session {
        let mut files = Vec::new();
        let mut active = 0;
        for (i, b) in self.buffers.iter().enumerate() {
            if let Some(p) = &b.path {
                if i == self.active {
                    active = files.len();
                }
                files.push(session::OpenFile {
                    path: p.clone(),
                    row: b.cursor.0,
                    col: b.cursor.1,
                    scroll: b.scroll_row,
                });
            }
        }
        session::Session {
            root: self.root.clone(),
            files,
            active,
            sidebar: self.sidebar,
            expanded: self.tree.expanded_paths(),
        }
    }

    /// Reopen the files, cursors, and view state from a saved session.
    pub fn restore_session(&mut self, s: session::Session) {
        for f in &s.files {
            if !f.path.is_file() {
                continue; // file moved or deleted since last time
            }
            if self.buffers.iter().any(|b| b.path.as_deref() == Some(f.path.as_path())) {
                continue;
            }
            if let Ok(mut buf) = Buffer::from_path(&f.path) {
                buf.goto(f.row, f.col);
                buf.scroll_row = f.scroll.min(buf.lines.len().saturating_sub(1));
                self.buffers.push(buf);
            }
        }
        if !self.buffers.is_empty() {
            self.active = s.active.min(self.buffers.len() - 1);
            self.focus = Focus::Editor;
        }
        self.sidebar = s.sidebar;
        if !s.expanded.is_empty() {
            self.tree.set_expanded(s.expanded);
        }
        if let Some(p) = self.buffers.get(self.active).and_then(|b| b.path.clone()) {
            self.tree.reveal(&p);
        }
        self.follow = true;
        self.refresh_find();
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

    /// Expire old notifications. Returns true if the UI should redraw.
    pub fn tick(&mut self) -> bool {
        let before = self.notices.len();
        self.notices.retain(|n| n.at.elapsed().as_secs_f32() < 4.0);
        self.notices.len() != before
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
            CommandId::FindNext => {
                if self.find.is_some() {
                    self.find_jump(true, true);
                } else {
                    self.open_find();
                }
            }
            CommandId::FindPrev => {
                if self.find.is_some() {
                    self.find_jump(false, true);
                } else {
                    self.open_find();
                }
            }
            CommandId::GlobalSearch => {
                self.open_prompt(PromptKind::GlobalSearch, "Search in project", "")
            }
            CommandId::GotoLine => self.open_prompt(PromptKind::GotoLine, "Go to line", ""),
            CommandId::ToggleSidebar => self.sidebar = !self.sidebar,
            CommandId::ToggleTerminal => self.toggle_terminal(),
            CommandId::ToggleMinimap => {
                self.minimap = !self.minimap;
                config::set_value("minimap", if self.minimap { "true" } else { "false" });
                self.notify(
                    if self.minimap { "Minimap on" } else { "Minimap off" },
                    Level::Info,
                );
            }
            CommandId::ThemePicker => self.open_theme_picker(),
            CommandId::KeymapPicker => self.open_keymap_picker(),
            CommandId::CommandPalette => self.open_command_palette(),
            CommandId::RenameSymbol => self.rename_symbol_prompt(),
            CommandId::FormatDocument => self.format_document(),
            CommandId::FindReferences => self.find_references(),
            CommandId::GoToDefinition => self.go_to_definition(),
            CommandId::ExtractVariable => self.extract_variable(),
            CommandId::FocusExplorer => {
                self.sidebar = true;
                self.focus = if self.focus == Focus::Explorer { Focus::Editor } else { Focus::Explorer };
            }
            CommandId::Undo => {
                if let Some(b) = self.buf_mut() {
                    b.undo();
                }
                self.after_editor_action();
            }
            CommandId::Redo => {
                if let Some(b) = self.buf_mut() {
                    b.redo();
                }
                self.after_editor_action();
            }
            CommandId::Cut => self.cut(),
            CommandId::Copy => self.copy(),
            CommandId::Paste => self.paste(),
            CommandId::SelectAll => {
                if let Some(b) = self.buf_mut() {
                    b.select_all();
                }
                self.follow = true;
            }
            CommandId::DuplicateLine => {
                if let Some(b) = self.buf_mut() {
                    b.duplicate_line();
                }
                self.after_editor_action();
            }
            CommandId::DeleteLine => {
                if let Some(b) = self.buf_mut() {
                    b.delete_line();
                }
                self.after_editor_action();
            }
            CommandId::MoveLinesUp => {
                if let Some(b) = self.buf_mut() {
                    b.move_line(false);
                }
                self.after_editor_action();
            }
            CommandId::MoveLinesDown => {
                if let Some(b) = self.buf_mut() {
                    b.move_line(true);
                }
                self.after_editor_action();
            }
            CommandId::ToggleComment => {
                if let Some(prefix) = self.buf().and_then(|b| b.language.comment_prefix()) {
                    if let Some(b) = self.buf_mut() {
                        b.toggle_comment(prefix);
                    }
                    self.after_editor_action();
                }
            }
            CommandId::Indent => {
                if let Some(b) = self.buf_mut() {
                    b.indent(false);
                }
                self.after_editor_action();
            }
            CommandId::Outdent => {
                if let Some(b) = self.buf_mut() {
                    b.indent(true);
                }
                self.after_editor_action();
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
            .map(|(id, label)| PaletteItem {
                label: label.to_string(),
                hint: self.keymap.format_for(id).unwrap_or_default(),
                action: PaletteAction::Command(id),
            })
            .collect();
        self.overlay = Some(Overlay::Palette(PaletteState::new(PaletteMode::Commands, items)));
    }

    pub fn open_keymap_picker(&mut self) {
        use crate::keymap::PRESETS;
        let items = PRESETS
            .iter()
            .map(|(id, name)| PaletteItem {
                label: name.to_string(),
                hint: if *id == self.keymap.id { "current".into() } else { String::new() },
                action: PaletteAction::SetKeymap(id.to_string()),
            })
            .collect();
        let mut st = PaletteState::new(PaletteMode::Keymaps, items);
        st.selected = PRESETS.iter().position(|(id, _)| *id == self.keymap.id).unwrap_or(0);
        self.overlay = Some(Overlay::Palette(st));
    }

    pub fn open_quick_open(&mut self) {
        // Scan the project on a worker thread; the palette opens instantly
        // and fills in when the listing arrives (see drain_async).
        self.files_gen += 1;
        let gen = self.files_gen;
        let (tx, rx) = channel();
        self.files_rx = Some(rx);
        let root = self.root.clone();
        thread::spawn(move || {
            let files = search::list_files(&root);
            let _ = tx.send((gen, files));
        });
        self.overlay = Some(Overlay::Palette(PaletteState::new(PaletteMode::Files, Vec::new())));
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
                config::set_value("theme", name);
                self.notify(format!("Theme: {name}"), Level::Info);
            }
            PaletteAction::SetKeymap(id) => {
                self.theme_before_preview = None;
                self.set_keymap(&id);
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

    /// Kick off a project search on a worker thread; results stream into the
    /// panel via drain_async so the UI stays responsive.
    pub fn run_global_search(&mut self, query: &str, title: Option<String>) {
        self.search_gen += 1;
        let gen = self.search_gen;
        let (tx, rx) = channel();
        self.search_rx = Some(rx);
        let root = self.root.clone();
        let q = query.to_string();
        thread::spawn(move || search::search_project_streaming(&root, &q, gen, tx));

        self.panel = Panel::Search(SearchPane {
            title: title.unwrap_or_else(|| "SEARCH".to_string()),
            query: query.to_string(),
            matches: Vec::new(),
            selected: 0,
            scroll: 0,
            done: false,
        });
        self.focus = Focus::Panel;
    }

    pub fn find_references(&mut self) {
        let Some((word, _, _)) = self.buf().and_then(|b| b.word_under_cursor()) else {
            self.notify("Place the cursor on a symbol first", Level::Warn);
            return;
        };
        self.run_global_search(&word, Some(format!("REFERENCES: {word}")));
    }

    /// Naive "go to definition" (no LSP): jump to a definition of the symbol
    /// under the cursor — the current file first, then the project. If the
    /// cursor is already sitting on the only definition, fall back to showing
    /// usages, à la JetBrains' "declaration or usages".
    pub fn go_to_definition(&mut self) {
        let Some((word, wcol, _)) = self.buf().and_then(|b| b.word_under_cursor()) else {
            self.notify("Place the cursor on a symbol first", Level::Warn);
            return;
        };
        let wrow = self.buf().map(|b| b.cursor.0).unwrap_or(0);
        let cur_path = self.buf().and_then(|b| b.path.clone());
        let len = word.chars().count();

        // 1) Definitions in the live buffer (catches unsaved edits).
        let in_file: Vec<(usize, usize)> = self
            .buf()
            .map(|b| {
                b.lines
                    .iter()
                    .enumerate()
                    .filter_map(|(row, line)| {
                        search::definition_col_in_line(line, &word).map(|col| (row, col))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let cursor_is_def = in_file.iter().any(|&(r, c)| r == wrow && c == wcol);
        if let Some(&(r, c)) = in_file.iter().find(|&&(r, c)| (r, c) != (wrow, wcol)) {
            if let Some(buf) = self.buf_mut() {
                buf.select_range(r, c, len);
            }
            self.focus = Focus::Editor;
            self.follow = true;
            self.notify(format!("Definition of '{word}' (naive, this file)"), Level::Info);
            return;
        }

        // 2) Definitions elsewhere in the project.
        let defs = search::find_definitions(&self.root, &word, cur_path.as_deref());
        if let Some(first) = defs.first() {
            let (path, row, col) = (first.path.clone(), first.line_no, first.col);
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let more = if defs.len() > 1 {
                format!(" (+{} more)", defs.len() - 1)
            } else {
                String::new()
            };
            self.open_file(&path);
            if let Some(buf) = self.buf_mut() {
                buf.select_range(row, col, len);
            }
            self.focus = Focus::Editor;
            self.follow = true;
            self.notify(format!("Definition of '{word}' in {name}{more}"), Level::Info);
            return;
        }

        // 3) Nothing. If we're already on the sole definition, show usages.
        if cursor_is_def {
            self.find_references();
        } else {
            self.notify(format!("No definition found for '{word}' (naive search)"), Level::Warn);
        }
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
        match self.panel {
            Panel::Terminal => {
                // Hide the panel but leave the shell running so scrollback and
                // any in-flight command survive until the next open.
                self.panel = Panel::None;
                if self.focus == Focus::Panel {
                    self.focus = Focus::Editor;
                }
            }
            _ => {
                self.ensure_terminal();
                self.panel = Panel::Terminal;
                self.focus = Focus::Panel;
            }
        }
    }

    /// Spawn the shell the first time the terminal is opened. On failure the
    /// panel still opens and shows an error, so the user learns why.
    fn ensure_terminal(&mut self) {
        if self.terminal.is_none() {
            match crate::pty::PtyTerminal::spawn(24, 80, &self.root) {
                Ok(t) => self.terminal = Some(t),
                Err(e) => self.notify(format!("Terminal: {e}"), Level::Error),
            }
        }
    }

    /// Replace an exited shell with a fresh one (invoked from the dead panel).
    pub fn restart_terminal(&mut self) {
        self.terminal = None;
        self.ensure_terminal();
    }

    /// A terminal exists and its shell is still running.
    pub fn terminal_live(&self) -> bool {
        self.terminal.as_ref().is_some_and(|t| !t.is_dead())
    }

    /// True if the terminal has produced output since the last draw.
    pub fn terminal_take_dirty(&mut self) -> bool {
        self.terminal.as_ref().is_some_and(|t| t.take_dirty())
    }

    /// Forward already-encoded bytes to the shell.
    pub fn terminal_input(&mut self, bytes: &[u8]) {
        if let Some(t) = self.terminal.as_mut() {
            t.write_input(bytes);
        }
    }

    /// Scroll the terminal viewport through its scrollback (positive = up into
    /// history). No-op when there's no terminal.
    pub fn terminal_scroll(&mut self, lines: isize) {
        if let Some(t) = self.terminal.as_ref() {
            t.scroll(lines);
        }
    }

    /// Jump the terminal viewport back to the live bottom.
    pub fn terminal_scroll_to_bottom(&mut self) {
        if let Some(t) = self.terminal.as_ref() {
            t.scroll_to_bottom();
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

#[cfg(test)]
mod goto_tests {
    use super::*;

    fn app_with(lines: &[&str]) -> App {
        let mut app = App::new(PathBuf::from("."));
        app.open_untitled();
        let b = app.buf_mut().unwrap();
        b.lines = lines.iter().map(|s| s.to_string()).collect();
        b.recompute_states();
        app
    }

    #[test]
    fn jumps_from_use_to_definition_in_file() {
        let mut app = app_with(&["fn helper() {}", "", "fn main() {", "    helper();", "}"]);
        app.buf_mut().unwrap().goto(3, 6); // on the `helper` call
        app.go_to_definition();
        let b = app.buf().unwrap();
        assert_eq!(b.cursor.0, 0, "should land on the definition line");
        assert_eq!(b.anchor, Some((0, 3)), "should select the definition name");
    }

    #[test]
    fn on_sole_definition_shows_usages() {
        let mut app = app_with(&["fn helper() {}", "fn main() { helper(); }"]);
        // Empty root so the project scan finds nothing and we fall back to usages.
        app.root = PathBuf::from("/plume-nonexistent-test-root");
        app.buf_mut().unwrap().goto(0, 3); // on the definition itself
        app.go_to_definition();
        // No other definition, so it falls back to a references search panel.
        assert!(matches!(app.panel, Panel::Search(_)), "should open a usages search");
    }

    #[test]
    fn no_symbol_no_jump() {
        let mut app = app_with(&["    ", "x"]);
        app.buf_mut().unwrap().goto(0, 2); // on whitespace
        app.go_to_definition();
        assert_eq!(app.buf().unwrap().cursor, (0, 2), "cursor unmoved");
    }
}
