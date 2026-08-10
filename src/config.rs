//! Config file parsing and persistence — a tiny hand-parsed TOML-ish subset
//! with no external dependency. Holds the default keymap, an optional theme,
//! and per-action keybinding overrides. Its location is resolved by `paths`.

use std::fs;

use crate::app::CommandId;
use crate::keymap::{self, Chord};
use crate::paths::config_file;

#[derive(Default)]
pub struct Config {
    pub keymap: Option<String>,
    pub theme: Option<String>,
    pub minimap: Option<bool>,
    pub indent_guides: Option<String>,
    pub indent_guide_offset: Option<f32>,
    pub overrides: Vec<(CommandId, Vec<Chord>)>,
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "on" | "yes" => Some(true),
        "false" | "0" | "off" | "no" => Some(false),
        _ => None,
    }
}

fn strip_comment(line: &str) -> &str {
    // A '#' outside of a quoted string starts a comment. Our values never
    // legitimately contain '#', so a first-hit split is enough, except we
    // keep '#' inside the first quote pair.
    let bytes = line.as_bytes();
    let mut in_quote = false;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'"' => in_quote = !in_quote,
            b'#' if !in_quote => return &line[..i],
            _ => {}
        }
    }
    line
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && (s.starts_with('"') && s.ends_with('"')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

pub fn load() -> Config {
    let mut cfg = Config::default();
    let Some(path) = config_file() else { return cfg };
    let Ok(text) = fs::read_to_string(&path) else { return cfg };

    let mut section = String::new();
    for raw in text.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_ascii_lowercase();
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let key = k.trim().to_ascii_lowercase();
        let val = unquote(v);
        match section.as_str() {
            "" => match key.as_str() {
                "keymap" => cfg.keymap = Some(val.to_ascii_lowercase()),
                "theme" => cfg.theme = Some(val),
                "minimap" => cfg.minimap = parse_bool(&val),
                "indent_guides" | "indent-guides" => {
                    cfg.indent_guides = Some(val.to_ascii_lowercase())
                }
                "indent_guide_offset" | "indent-guide-offset" => {
                    cfg.indent_guide_offset = val.trim().parse().ok()
                }
                _ => {}
            },
            "keybindings" | "keys" => {
                if let (Some(cmd), Some(chords)) =
                    (keymap::action_from_str(&key), keymap::parse_binding(&val))
                {
                    cfg.overrides.push((cmd, chords));
                }
            }
            _ => {}
        }
    }
    cfg
}

/// Persist a single top-level `key = "value"`, preserving the rest of the
/// file (comments, the `[keybindings]` section, everything). Creates the file
/// from a template if it doesn't exist yet.
pub fn set_value(key: &str, val: &str) {
    let Some(path) = config_file() else { return };
    let existing = fs::read_to_string(&path).unwrap_or_default();
    if existing.is_empty() {
        write_template(Some((key, val)));
        return;
    }

    let mut lines: Vec<String> = existing.lines().map(String::from).collect();
    let mut in_top = true;
    let mut replaced = false;
    for line in lines.iter_mut() {
        let t = line.trim_start();
        if t.starts_with('[') {
            in_top = false;
        }
        if in_top && !t.starts_with('#') {
            if let Some((k, _)) = t.split_once('=') {
                if k.trim().eq_ignore_ascii_case(key) {
                    *line = format!("{key} = \"{val}\"");
                    replaced = true;
                    break;
                }
            }
        }
    }
    if !replaced {
        let at = lines
            .iter()
            .position(|l| l.trim_start().starts_with('['))
            .unwrap_or(lines.len());
        lines.insert(at, format!("{key} = \"{val}\""));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(&path, out);
}

/// Write the documented default config if none exists. `seed` optionally
/// overrides one top-level value in the template.
pub fn write_template(seed: Option<(&str, &str)>) {
    let Some(path) = config_file() else { return };
    let (mut keymap_val, mut theme_val) = ("vscode", "Midnight Ocean");
    if let Some((k, v)) = seed {
        match k {
            "keymap" => keymap_val = v,
            "theme" => theme_val = v,
            _ => {}
        }
    }
    let body = format!(
        "# Plume configuration\n\
         #\n\
         # Base keymap preset: vscode | visual-studio | jetbrains | sublime\n\
         keymap = \"{keymap_val}\"\n\
         \n\
         # Default color theme by name. Browse all of them (57 built in — Nord,\n\
         # Dracula, Gruvbox, Tokyo Night, Catppuccin, Solarized, …) with the\n\
         # in-app picker: Ctrl+K Ctrl+T.\n\
         theme = \"{theme_val}\"\n\
         \n\
         # Show the zoomed-out minimap on the right (toggle in-app with F9):\n\
         minimap = true\n\
         \n\
         # Indentation / bracket-pair guides:  all | context | off\n\
         #   all     = faint dotted guides at every indent level (JetBrains-style)\n\
         #   context = only the guide for the block enclosing the cursor\n\
         #   off     = no guides\n\
         indent_guides = \"all\"\n\
         # Shift guides left of each indent stop. 0 sits on the stop, 1 one\n\
         # column left, and 0.5 draws a thin bar exactly on the column boundary\n\
         # between them (solid rather than dotted, since it hugs a cell edge).\n\
         indent_guide_offset = 0\n\
         \n\
         # Override individual keybindings. Format:  action = \"chord\"\n\
         # A chord is like  ctrl+shift+p  /  f3  /  ctrl+slash  and a two-key\n\
         # sequence is space-separated:  theme_picker = \"ctrl+k ctrl+t\".\n\
         #\n\
         # Actions: save quick_open command_palette find find_next find_prev\n\
         #   global_search goto_line new_file close_tab next_tab prev_tab\n\
         #   toggle_sidebar toggle_terminal focus_explorer theme_picker\n\
         #   keymap_picker rename_symbol format_document find_references\n\
         #   go_to_definition extract_variable undo redo cut copy paste\n\
         #   select_all\n\
         #   duplicate_line delete_line move_lines_up move_lines_down\n\
         #   toggle_comment indent outdent quit\n\
         [keybindings]\n\
         # duplicate_line = \"ctrl+shift+d\"\n\
         # toggle_comment = \"ctrl+shift+c\"\n"
    );
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(&path, body);
}

/// Create the template on first run (never overwrites an existing file).
pub fn ensure_template() {
    if let Some(path) = config_file() {
        if !path.exists() {
            write_template(None);
        }
    }
}
