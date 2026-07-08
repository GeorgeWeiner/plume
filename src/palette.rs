//! Command palette / quick open / theme picker state, plus the shared
//! single-line input widget and fuzzy matcher.

use std::path::PathBuf;

use crate::app::CommandId;

/// A minimal editable single-line input (used by palette, prompts, find bar).
#[derive(Default, Clone)]
pub struct InputLine {
    pub text: String,
    pub cursor: usize, // char index
}

impl InputLine {
    pub fn with_text(text: &str) -> InputLine {
        InputLine { text: text.to_string(), cursor: text.chars().count() }
    }

    fn bidx(&self, col: usize) -> usize {
        self.text.char_indices().nth(col).map(|(i, _)| i).unwrap_or(self.text.len())
    }

    pub fn insert(&mut self, c: char) {
        let b = self.bidx(self.cursor);
        self.text.insert(b, c);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let b = self.bidx(self.cursor - 1);
            self.text.remove(b);
            self.cursor -= 1;
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.text.chars().count() {
            let b = self.bidx(self.cursor);
            self.text.remove(b);
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.text.chars().count());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.text.chars().count();
    }

}

/// Subsequence fuzzy match. Higher score = better. None = no match.
pub fn fuzzy(needle: &str, hay: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(0);
    }
    let hay_chars: Vec<char> = hay.chars().collect();
    let mut score: i32 = 0;
    let mut hi = 0usize;
    let mut last_hit: Option<usize> = None;
    let mut first_hit: Option<usize> = None;

    for nc in needle.chars() {
        let nc_low = nc.to_ascii_lowercase();
        let mut found = None;
        while hi < hay_chars.len() {
            if hay_chars[hi].to_ascii_lowercase() == nc_low {
                found = Some(hi);
                break;
            }
            hi += 1;
        }
        let idx = found?;
        score += 1;
        if last_hit == Some(idx.wrapping_sub(1)) {
            score += 3; // consecutive
        }
        let word_start = idx == 0
            || matches!(hay_chars[idx - 1], '/' | '\\' | '_' | '-' | ' ' | '.');
        if word_start {
            score += 2;
        }
        if first_hit.is_none() {
            first_hit = Some(idx);
        }
        last_hit = Some(idx);
        hi = idx + 1;
    }
    // prefer matches that start early and short haystacks
    score -= (first_hit.unwrap_or(0) as i32) / 4;
    score -= (hay_chars.len() as i32) / 16;
    Some(score)
}

#[derive(Clone, Copy, PartialEq)]
pub enum PaletteMode {
    Commands,
    Files,
    Themes,
    Keymaps,
}

#[derive(Clone)]
pub enum PaletteAction {
    Command(CommandId),
    OpenFile(PathBuf),
    SetTheme(usize),
    SetKeymap(String),
}

pub struct PaletteItem {
    pub label: String,
    pub hint: String,
    pub action: PaletteAction,
}

pub struct PaletteState {
    pub mode: PaletteMode,
    pub input: InputLine,
    pub items: Vec<PaletteItem>,
    /// Indices into `items`, filtered + ranked.
    pub filtered: Vec<usize>,
    pub selected: usize,
}

impl PaletteState {
    pub fn new(mode: PaletteMode, items: Vec<PaletteItem>) -> PaletteState {
        let mut p = PaletteState {
            mode,
            input: InputLine::default(),
            items,
            filtered: Vec::new(),
            selected: 0,
        };
        p.refilter();
        p
    }

    pub fn refilter(&mut self) {
        let q = self.input.text.trim();
        if q.is_empty() {
            self.filtered = (0..self.items.len()).collect();
        } else {
            let mut scored: Vec<(i32, usize)> = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, item)| fuzzy(q, &item.label).map(|s| (s, i)))
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            self.filtered = scored.into_iter().map(|(_, i)| i).collect();
        }
        self.selected = self.selected.min(self.filtered.len().saturating_sub(1));
    }

    pub fn nav(&mut self, delta: isize) {
        if self.filtered.is_empty() {
            return;
        }
        let n = self.filtered.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(n) as usize;
    }

    pub fn current(&self) -> Option<&PaletteItem> {
        self.filtered.get(self.selected).map(|&i| &self.items[i])
    }
}
