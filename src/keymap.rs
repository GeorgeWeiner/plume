//! Data-driven keybindings: chords, presets (VS Code / Visual Studio /
//! JetBrains / Sublime), parsing, and reverse lookup for display.
//!
//! Only *commands* are keymapped. Cursor movement, text entry, and the
//! overlay / explorer / panel navigation keys are universal and handled
//! directly in `keys.rs` — those don't differ between editors and users
//! don't rebind them via presets.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::CommandId as C;
use crate::app::CommandId;

/// A single normalized key press: a key code plus Ctrl/Alt/Shift.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Chord {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

/// Presets offered in the keymap picker: (id, display name).
pub const PRESETS: &[(&str, &str)] = &[
    ("vscode", "VS Code"),
    ("visual-studio", "Visual Studio"),
    ("jetbrains", "JetBrains"),
    ("sublime", "Sublime Text"),
];

pub fn preset_name(id: &str) -> &'static str {
    PRESETS.iter().find(|(k, _)| *k == id).map(|(_, n)| *n).unwrap_or("VS Code")
}

// ---- normalization ----

/// Turn a raw key event into a canonical chord, or None if it carries no
/// usable key. Alphabetic keys fold case into the Shift modifier so bindings
/// compare consistently across terminals; symbol keys drop Shift (terminals
/// already fold it into the character).
pub fn normalize(e: &KeyEvent) -> Option<Chord> {
    let mut mods = KeyModifiers::empty();
    if e.modifiers.contains(KeyModifiers::CONTROL) {
        mods |= KeyModifiers::CONTROL;
    }
    if e.modifiers.contains(KeyModifiers::ALT) {
        mods |= KeyModifiers::ALT;
    }
    let mut shift = e.modifiers.contains(KeyModifiers::SHIFT);
    let mut code = e.code;

    if let KeyCode::Char(c) = code {
        if c.is_ascii_alphabetic() {
            if c.is_ascii_uppercase() {
                shift = true;
            }
            code = KeyCode::Char(c.to_ascii_lowercase());
            if shift {
                mods |= KeyModifiers::SHIFT;
            }
        } else {
            // Ctrl+/ and Ctrl+_ collapse to the same control byte on many
            // terminals; treat both as "/" so comment-toggle is reliable.
            if mods.contains(KeyModifiers::CONTROL) && c == '_' {
                code = KeyCode::Char('/');
            }
            // symbol/digit: Shift is already folded into the char
        }
    } else if code != KeyCode::BackTab {
        if shift {
            mods |= KeyModifiers::SHIFT;
        }
    }

    match code {
        KeyCode::Null | KeyCode::Modifier(_) => None,
        _ => Some(Chord { code, mods }),
    }
}

// ---- parsing ----

fn name_to_code(name: &str) -> Option<KeyCode> {
    let n = name.to_ascii_lowercase();
    if let Some(rest) = n.strip_prefix('f') {
        if let Ok(num) = rest.parse::<u8>() {
            if (1..=24).contains(&num) {
                return Some(KeyCode::F(num));
            }
        }
    }
    Some(match n.as_str() {
        "enter" | "return" | "cr" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "esc" | "escape" => KeyCode::Esc,
        "space" => KeyCode::Char(' '),
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "pgup" => KeyCode::PageUp,
        "pagedown" | "pgdn" | "pgdown" => KeyCode::PageDown,
        "delete" | "del" => KeyCode::Delete,
        "backspace" | "bs" => KeyCode::Backspace,
        "insert" | "ins" => KeyCode::Insert,
        "slash" => KeyCode::Char('/'),
        "backslash" => KeyCode::Char('\\'),
        "grave" | "backtick" | "tilde" => KeyCode::Char('`'),
        "comma" => KeyCode::Char(','),
        "period" | "dot" => KeyCode::Char('.'),
        "semicolon" => KeyCode::Char(';'),
        "minus" | "dash" => KeyCode::Char('-'),
        "equal" | "equals" => KeyCode::Char('='),
        "plus" => KeyCode::Char('+'),
        "lbracket" | "leftbracket" => KeyCode::Char('['),
        "rbracket" | "rightbracket" => KeyCode::Char(']'),
        "quote" | "apostrophe" => KeyCode::Char('\''),
        s if s.chars().count() == 1 => KeyCode::Char(s.chars().next().unwrap()),
        _ => return None,
    })
}

/// Parse one chord like `ctrl+shift+p` or `f3` or `ctrl+slash`.
pub fn parse_chord(token: &str) -> Option<Chord> {
    let parts: Vec<&str> = token.split('+').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }
    let (key_part, mod_parts) = parts.split_last().unwrap();
    let mut mods = KeyModifiers::empty();
    for m in mod_parts {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "c" => mods |= KeyModifiers::CONTROL,
            "alt" | "option" | "meta" | "m" => mods |= KeyModifiers::ALT,
            "shift" | "s" => mods |= KeyModifiers::SHIFT,
            _ => return None,
        }
    }
    let mut code = name_to_code(key_part)?;

    // shift+tab is delivered as BackTab with no shift modifier
    if code == KeyCode::Tab && mods.contains(KeyModifiers::SHIFT) {
        code = KeyCode::BackTab;
        mods.remove(KeyModifiers::SHIFT);
    }
    // mirror normalize(): symbols carry no Shift, letters keep it lowercased
    if let KeyCode::Char(c) = code {
        if c.is_ascii_alphabetic() {
            code = KeyCode::Char(c.to_ascii_lowercase());
        } else {
            mods.remove(KeyModifiers::SHIFT);
        }
    }
    Some(Chord { code, mods })
}

/// Parse a full binding: one or two space-separated chords.
pub fn parse_binding(s: &str) -> Option<Vec<Chord>> {
    let chords: Vec<Chord> = s.split_whitespace().filter_map(parse_chord).collect();
    if chords.is_empty() || chords.len() > 2 {
        None
    } else {
        Some(chords)
    }
}

// ---- formatting (for display in the palette / status bar) ----

fn code_label(code: KeyCode) -> String {
    match code {
        KeyCode::Char(' ') => "Space".into(),
        KeyCode::Char('`') => "`".into(),
        KeyCode::Char(c) => c.to_ascii_uppercase().to_string(),
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::Enter => "Enter".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::BackTab => "Shift+Tab".into(),
        KeyCode::Esc => "Esc".into(),
        KeyCode::Up => "Up".into(),
        KeyCode::Down => "Down".into(),
        KeyCode::Left => "Left".into(),
        KeyCode::Right => "Right".into(),
        KeyCode::Home => "Home".into(),
        KeyCode::End => "End".into(),
        KeyCode::PageUp => "PgUp".into(),
        KeyCode::PageDown => "PgDn".into(),
        KeyCode::Delete => "Del".into(),
        KeyCode::Backspace => "Backspace".into(),
        KeyCode::Insert => "Ins".into(),
        _ => "?".into(),
    }
}

fn chord_label(c: &Chord) -> String {
    let mut s = String::new();
    if c.mods.contains(KeyModifiers::CONTROL) {
        s.push_str("Ctrl+");
    }
    if c.mods.contains(KeyModifiers::ALT) {
        s.push_str("Alt+");
    }
    if c.mods.contains(KeyModifiers::SHIFT) {
        s.push_str("Shift+");
    }
    s.push_str(&code_label(c.code));
    s
}

pub fn format_binding(chords: &[Chord]) -> String {
    chords.iter().map(chord_label).collect::<Vec<_>>().join(" ")
}

// ---- action names (config file) ----

pub fn action_from_str(name: &str) -> Option<CommandId> {
    Some(match name.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "new_file" => C::NewFile,
        "quick_open" | "go_to_file" => C::QuickOpen,
        "save" => C::Save,
        "save_as" => C::SaveAs,
        "close_tab" => C::CloseTab,
        "next_tab" => C::NextTab,
        "prev_tab" | "previous_tab" => C::PrevTab,
        "find" => C::Find,
        "find_next" => C::FindNext,
        "find_prev" | "find_previous" => C::FindPrev,
        "global_search" | "search_in_project" => C::GlobalSearch,
        "goto_line" | "go_to_line" => C::GotoLine,
        "toggle_sidebar" => C::ToggleSidebar,
        "toggle_terminal" => C::ToggleTerminal,
        "theme_picker" | "color_theme" => C::ThemePicker,
        "keymap_picker" | "keymap" => C::KeymapPicker,
        "rename_symbol" | "rename" => C::RenameSymbol,
        "format_document" | "format" => C::FormatDocument,
        "find_references" => C::FindReferences,
        "extract_variable" => C::ExtractVariable,
        "focus_explorer" => C::FocusExplorer,
        "command_palette" => C::CommandPalette,
        "undo" => C::Undo,
        "redo" => C::Redo,
        "cut" => C::Cut,
        "copy" => C::Copy,
        "paste" => C::Paste,
        "select_all" => C::SelectAll,
        "duplicate_line" => C::DuplicateLine,
        "delete_line" => C::DeleteLine,
        "move_lines_up" | "move_line_up" => C::MoveLinesUp,
        "move_lines_down" | "move_line_down" => C::MoveLinesDown,
        "toggle_comment" | "comment" => C::ToggleComment,
        "indent" => C::Indent,
        "outdent" | "dedent" => C::Outdent,
        "quit" => C::Quit,
        _ => return None,
    })
}

// ---- presets ----

/// Bindings shared by every preset. A chord listed here is only used if a
/// preset (or a user override) hasn't already claimed it — later entries lose.
const COMMON: &[(CommandId, &str)] = &[
    (C::Save, "ctrl+s"),
    (C::NewFile, "ctrl+n"),
    (C::CloseTab, "ctrl+w"),
    (C::NextTab, "ctrl+pagedown"),
    (C::PrevTab, "ctrl+pageup"),
    (C::Find, "ctrl+f"),
    (C::FindNext, "f3"),
    (C::FindPrev, "shift+f3"),
    (C::GlobalSearch, "ctrl+shift+f"),
    (C::GotoLine, "ctrl+g"),
    (C::ToggleSidebar, "ctrl+b"),
    (C::ToggleTerminal, "ctrl+grave"),
    (C::ToggleTerminal, "ctrl+j"),
    (C::FocusExplorer, "ctrl+e"),
    (C::CommandPalette, "f1"),
    (C::ThemePicker, "ctrl+k ctrl+t"),
    (C::KeymapPicker, "ctrl+k ctrl+m"),
    (C::Undo, "ctrl+z"),
    (C::Redo, "ctrl+y"),
    (C::Cut, "ctrl+x"),
    (C::Copy, "ctrl+c"),
    (C::Paste, "ctrl+v"),
    (C::SelectAll, "ctrl+a"),
    (C::Indent, "ctrl+rbracket"),
    (C::Outdent, "ctrl+lbracket"),
    (C::Quit, "ctrl+q"),
];

const VSCODE: &[(CommandId, &str)] = &[
    (C::CommandPalette, "ctrl+shift+p"),
    (C::QuickOpen, "ctrl+p"),
    (C::FocusExplorer, "ctrl+shift+e"),
    (C::Redo, "ctrl+shift+z"),
    (C::DuplicateLine, "shift+alt+down"),
    (C::DeleteLine, "ctrl+shift+k"),
    (C::MoveLinesUp, "alt+up"),
    (C::MoveLinesDown, "alt+down"),
    (C::ToggleComment, "ctrl+slash"),
    (C::RenameSymbol, "f2"),
    (C::FormatDocument, "shift+alt+f"),
    (C::FindReferences, "shift+f12"),
];

const VISUAL_STUDIO: &[(CommandId, &str)] = &[
    (C::CommandPalette, "ctrl+shift+p"),
    (C::QuickOpen, "ctrl+comma"),
    (C::QuickOpen, "ctrl+p"),
    (C::FocusExplorer, "ctrl+alt+l"),
    (C::DuplicateLine, "ctrl+d"),
    (C::DeleteLine, "ctrl+shift+l"),
    (C::MoveLinesUp, "alt+up"),
    (C::MoveLinesDown, "alt+down"),
    (C::ToggleComment, "ctrl+k ctrl+c"),
    (C::RenameSymbol, "ctrl+r ctrl+r"),
    (C::RenameSymbol, "f2"),
    (C::FormatDocument, "ctrl+k ctrl+d"),
    (C::FindReferences, "shift+f12"),
];

const JETBRAINS: &[(CommandId, &str)] = &[
    (C::CommandPalette, "ctrl+shift+a"),
    (C::QuickOpen, "ctrl+shift+n"),
    (C::FocusExplorer, "alt+1"),
    (C::Redo, "ctrl+shift+z"),
    (C::DuplicateLine, "ctrl+d"),
    (C::DeleteLine, "ctrl+y"),
    (C::MoveLinesUp, "shift+alt+up"),
    (C::MoveLinesDown, "shift+alt+down"),
    (C::ToggleComment, "ctrl+slash"),
    (C::RenameSymbol, "shift+f6"),
    (C::FormatDocument, "ctrl+alt+l"),
    (C::FindReferences, "alt+f7"),
];

const SUBLIME: &[(CommandId, &str)] = &[
    (C::CommandPalette, "ctrl+shift+p"),
    (C::QuickOpen, "ctrl+p"),
    (C::FocusExplorer, "ctrl+shift+e"),
    (C::DuplicateLine, "ctrl+shift+d"),
    (C::DeleteLine, "ctrl+shift+k"),
    (C::MoveLinesUp, "ctrl+shift+up"),
    (C::MoveLinesDown, "ctrl+shift+down"),
    (C::ToggleComment, "ctrl+slash"),
    (C::RenameSymbol, "f2"),
    (C::FindReferences, "shift+f12"),
];

fn preset_entries(id: &str) -> &'static [(CommandId, &'static str)] {
    match id {
        "visual-studio" | "vs" => VISUAL_STUDIO,
        "jetbrains" | "intellij" => JETBRAINS,
        "sublime" | "sublime-text" => SUBLIME,
        _ => VSCODE,
    }
}

// ---- the keymap ----

pub struct Keymap {
    pub id: String,
    pub name: String,
    /// (chord sequence, command); earlier entries win on conflict.
    entries: Vec<(Vec<Chord>, CommandId)>,
    prefixes: Vec<Chord>,
}

impl Keymap {
    /// Build a keymap from a preset id, with user overrides taking priority.
    pub fn preset(id: &str, overrides: &[(CommandId, Vec<Chord>)]) -> Keymap {
        let mut entries: Vec<(Vec<Chord>, CommandId)> = Vec::new();
        for (cmd, chords) in overrides {
            entries.push((chords.clone(), *cmd));
        }
        for (cmd, s) in preset_entries(id) {
            if let Some(cs) = parse_binding(s) {
                entries.push((cs, *cmd));
            }
        }
        for (cmd, s) in COMMON {
            if let Some(cs) = parse_binding(s) {
                entries.push((cs, *cmd));
            }
        }
        let prefixes = entries
            .iter()
            .filter(|(c, _)| c.len() >= 2)
            .map(|(c, _)| c[0].clone())
            .collect();
        Keymap {
            id: id.to_string(),
            name: preset_name(id).to_string(),
            entries,
            prefixes,
        }
    }

    pub fn lookup_single(&self, c: &Chord) -> Option<CommandId> {
        self.entries
            .iter()
            .find(|(cs, _)| cs.len() == 1 && &cs[0] == c)
            .map(|(_, cmd)| *cmd)
    }

    pub fn lookup_seq(&self, a: &Chord, b: &Chord) -> Option<CommandId> {
        self.entries
            .iter()
            .find(|(cs, _)| cs.len() == 2 && &cs[0] == a && &cs[1] == b)
            .map(|(_, cmd)| *cmd)
    }

    pub fn is_prefix(&self, c: &Chord) -> bool {
        self.prefixes.iter().any(|p| p == c)
    }

    /// First binding for a command, formatted for display (e.g. "Ctrl+P").
    pub fn format_for(&self, cmd: CommandId) -> Option<String> {
        self.entries
            .iter()
            .find(|(_, c)| *c == cmd)
            .map(|(cs, _)| format_binding(cs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEventKind, KeyEventState};

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: mods,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    #[test]
    fn normalize_folds_uppercase_into_shift() {
        let up = normalize(&ev(KeyCode::Char('P'), KeyModifiers::CONTROL)).unwrap();
        let lo = parse_chord("ctrl+shift+p").unwrap();
        assert_eq!(up, lo);
    }

    #[test]
    fn ctrl_underscore_aliases_slash() {
        let u = normalize(&ev(KeyCode::Char('_'), KeyModifiers::CONTROL)).unwrap();
        let s = parse_chord("ctrl+slash").unwrap();
        assert_eq!(u, s);
    }

    #[test]
    fn presets_resolve_expected_commands() {
        let vscode = Keymap::preset("vscode", &[]);
        assert_eq!(
            vscode.lookup_single(&parse_chord("ctrl+p").unwrap()),
            Some(C::QuickOpen)
        );
        let jb = Keymap::preset("jetbrains", &[]);
        // JetBrains: Ctrl+Y deletes the line (not redo)
        assert_eq!(
            jb.lookup_single(&parse_chord("ctrl+y").unwrap()),
            Some(C::DeleteLine)
        );
        assert_eq!(
            jb.lookup_single(&parse_chord("ctrl+shift+n").unwrap()),
            Some(C::QuickOpen)
        );
    }

    #[test]
    fn two_chord_sequence_and_override() {
        let vs = Keymap::preset("visual-studio", &[]);
        let k = parse_chord("ctrl+k").unwrap();
        let c = parse_chord("ctrl+c").unwrap();
        assert!(vs.is_prefix(&k));
        assert_eq!(vs.lookup_seq(&k, &c), Some(C::ToggleComment));

        // user override wins over the preset
        let ov = vec![(C::Save, parse_binding("ctrl+enter").unwrap())];
        let km = Keymap::preset("vscode", &ov);
        assert_eq!(
            km.lookup_single(&parse_chord("ctrl+enter").unwrap()),
            Some(C::Save)
        );
    }
}
