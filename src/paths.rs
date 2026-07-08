//! Platform-appropriate config and state directories.
//!
//! Config (user settings) and state (session/history) are separated per the
//! XDG spec on Linux, and mapped to the conventional locations on macOS and
//! Windows. An explicit `XDG_*` env var always wins, so the same layout can be
//! forced anywhere (and is what the tests use).

use std::env;
use std::path::PathBuf;

fn home() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

/// `%USERPROFILE%\.config\plume` — a last-resort fallback if the usual Windows
/// app-data variables are somehow unset.
fn win_profile_fallback() -> Option<PathBuf> {
    env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".config").join("plume"))
}

/// Directory for `config.toml` (keymap, theme, keybindings).
pub fn config_dir() -> Option<PathBuf> {
    if let Some(x) = env::var_os("XDG_CONFIG_HOME") {
        if !x.is_empty() {
            return Some(PathBuf::from(x).join("plume"));
        }
    }
    match env::consts::OS {
        "macos" => home().map(|h| h.join("Library/Application Support/plume")),
        "windows" => env::var_os("APPDATA")
            .map(|a| PathBuf::from(a).join("plume"))
            .or_else(win_profile_fallback),
        _ => home().map(|h| h.join(".config/plume")),
    }
}

/// Directory for session/state data (open tabs, cursor positions, last project).
pub fn state_dir() -> Option<PathBuf> {
    if let Some(x) = env::var_os("XDG_STATE_HOME") {
        if !x.is_empty() {
            return Some(PathBuf::from(x).join("plume"));
        }
    }
    match env::consts::OS {
        "macos" => home().map(|h| h.join("Library/Application Support/plume")),
        "windows" => env::var_os("LOCALAPPDATA")
            .map(|a| PathBuf::from(a).join("plume"))
            .or_else(win_profile_fallback),
        _ => home().map(|h| h.join(".local/state/plume")),
    }
}

pub fn config_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.toml"))
}

pub fn sessions_dir() -> Option<PathBuf> {
    state_dir().map(|d| d.join("sessions"))
}

pub fn last_project_file() -> Option<PathBuf> {
    state_dir().map(|d| d.join("last-project"))
}
