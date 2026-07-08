//! Text buffer: lines of text, cursor, selection, undo/redo, edit ops.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::syntax::{self, Language};

pub const TAB_STOP: usize = 4;

/// Visual column of char index `col` (tabs expand to the next tab stop).
pub fn visual_col(line: &str, col: usize) -> usize {
    let mut v = 0;
    for (i, c) in line.chars().enumerate() {
        if i >= col {
            break;
        }
        v += if c == '\t' { TAB_STOP - v % TAB_STOP } else { 1 };
    }
    v
}

/// Char index whose visual column is closest to `target`.
pub fn col_at_visual(line: &str, target: usize) -> usize {
    let mut v = 0;
    for (i, c) in line.chars().enumerate() {
        if v >= target {
            return i;
        }
        v += if c == '\t' { TAB_STOP - v % TAB_STOP } else { 1 };
    }
    line.chars().count()
}

fn bidx(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map(|(i, _)| i).unwrap_or(s.len())
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[derive(Clone, Copy, PartialEq)]
enum EditKind {
    InsertChar,
    Backspace,
    Other,
}

#[derive(Clone)]
struct Snapshot {
    lines: Vec<String>,
    cursor: (usize, usize),
}

pub struct Buffer {
    pub path: Option<PathBuf>,
    pub lines: Vec<String>,
    /// (row, char column)
    pub cursor: (usize, usize),
    /// Selection anchor; selection = anchor..cursor.
    pub anchor: Option<(usize, usize)>,
    pub pref_col: usize,
    pub scroll_row: usize,
    pub scroll_col: usize,
    pub modified: bool,
    pub language: Language,
    /// Per line: starts inside a block comment.
    pub line_states: Vec<bool>,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    last_edit: EditKind,
}

impl Buffer {
    pub fn untitled() -> Buffer {
        let mut b = Buffer {
            path: None,
            lines: vec![String::new()],
            cursor: (0, 0),
            anchor: None,
            pref_col: 0,
            scroll_row: 0,
            scroll_col: 0,
            modified: false,
            language: Language::Plain,
            line_states: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_edit: EditKind::Other,
        };
        b.recompute_states();
        b
    }

    pub fn from_path(path: &Path) -> io::Result<Buffer> {
        let bytes = fs::read(path)?;
        let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n").replace('\r', "\n");
        let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
        // A trailing newline produces one phantom empty line; keep it as the
        // final editable line but drop nothing (matches most editors).
        if lines.is_empty() {
            lines.push(String::new());
        }
        if lines.len() > 1 && lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        let mut b = Buffer::untitled();
        b.language = Language::from_path(path);
        b.path = Some(path.to_path_buf());
        b.lines = lines;
        b.recompute_states();
        Ok(b)
    }

    pub fn display_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".into())
    }

    pub fn save(&mut self) -> io::Result<()> {
        let path = self.path.clone().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "buffer has no path")
        })?;
        let mut text = self.lines.join("\n");
        text.push('\n');
        fs::write(&path, text)?;
        self.modified = false;
        Ok(())
    }

    pub fn line(&self, row: usize) -> &str {
        self.lines.get(row).map(|s| s.as_str()).unwrap_or("")
    }

    pub fn line_len(&self, row: usize) -> usize {
        self.line(row).chars().count()
    }

    pub fn recompute_states(&mut self) {
        let mut state = false;
        self.line_states = Vec::with_capacity(self.lines.len());
        for line in &self.lines {
            self.line_states.push(state);
            let (_, next) = syntax::scan_line(self.language, line, state);
            state = next;
        }
    }

    fn clamp_cursor(&mut self) {
        let row = self.cursor.0.min(self.lines.len().saturating_sub(1));
        let col = self.cursor.1.min(self.line_len(row));
        self.cursor = (row, col);
    }

    // ---- undo ----

    fn snapshot(&self) -> Snapshot {
        Snapshot { lines: self.lines.clone(), cursor: self.cursor }
    }

    fn edit_begin(&mut self, kind: EditKind) {
        let coalesce = kind != EditKind::Other
            && kind == self.last_edit
            && self.anchor.is_none()
            && !self.undo_stack.is_empty();
        if !coalesce {
            self.undo_stack.push(self.snapshot());
            if self.undo_stack.len() > 200 {
                self.undo_stack.remove(0);
            }
        }
        self.redo_stack.clear();
        self.last_edit = kind;
    }

    fn edit_end(&mut self) {
        self.modified = true;
        self.recompute_states();
        self.clamp_cursor();
    }

    pub fn undo(&mut self) -> bool {
        match self.undo_stack.pop() {
            Some(s) => {
                self.redo_stack.push(self.snapshot());
                self.lines = s.lines;
                self.cursor = s.cursor;
                self.anchor = None;
                self.last_edit = EditKind::Other;
                self.modified = true;
                self.recompute_states();
                self.clamp_cursor();
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo_stack.pop() {
            Some(s) => {
                self.undo_stack.push(self.snapshot());
                self.lines = s.lines;
                self.cursor = s.cursor;
                self.anchor = None;
                self.last_edit = EditKind::Other;
                self.modified = true;
                self.recompute_states();
                self.clamp_cursor();
                true
            }
            None => false,
        }
    }

    // ---- selection ----

    /// Ordered selection range ((start),(end)), if any.
    pub fn selection(&self) -> Option<((usize, usize), (usize, usize))> {
        let a = self.anchor?;
        let c = self.cursor;
        if a == c {
            return None;
        }
        Some(if a < c { (a, c) } else { (c, a) })
    }

    pub fn selected_text(&self) -> Option<String> {
        let (a, b) = self.selection()?;
        if a.0 == b.0 {
            let l = self.line(a.0);
            return Some(l[bidx(l, a.1)..bidx(l, b.1)].to_string());
        }
        let mut out = String::new();
        let first = self.line(a.0);
        out.push_str(&first[bidx(first, a.1)..]);
        for r in a.0 + 1..b.0 {
            out.push('\n');
            out.push_str(self.line(r));
        }
        out.push('\n');
        let last = self.line(b.0);
        out.push_str(&last[..bidx(last, b.1)]);
        Some(out)
    }

    fn remove_selection(&mut self) {
        let Some((a, b)) = self.selection() else {
            self.anchor = None;
            return;
        };
        if a.0 == b.0 {
            let l = &self.lines[a.0];
            let (s, e) = (bidx(l, a.1), bidx(l, b.1));
            self.lines[a.0].replace_range(s..e, "");
        } else {
            let first_keep = {
                let l = &self.lines[a.0];
                l[..bidx(l, a.1)].to_string()
            };
            let last_keep = {
                let l = &self.lines[b.0];
                l[bidx(l, b.1)..].to_string()
            };
            self.lines[a.0] = first_keep + &last_keep;
            self.lines.drain(a.0 + 1..=b.0);
        }
        self.cursor = a;
        self.anchor = None;
    }

    pub fn delete_selection(&mut self) {
        if self.selection().is_some() {
            self.edit_begin(EditKind::Other);
            self.remove_selection();
            self.edit_end();
        }
    }

    pub fn select_all(&mut self) {
        self.anchor = Some((0, 0));
        let last = self.lines.len() - 1;
        self.cursor = (last, self.line_len(last));
    }

    /// Select `len` chars starting at (row, col) — used to highlight a jumped-to match.
    pub fn select_range(&mut self, row: usize, col: usize, len: usize) {
        self.anchor = Some((row, col));
        self.cursor = (row, (col + len).min(self.line_len(row)));
        self.pref_col = visual_col(self.line(row), self.cursor.1);
    }

    // ---- editing ----

    pub fn insert_char(&mut self, c: char) {
        self.edit_begin(EditKind::InsertChar);
        self.remove_selection();
        let (r, col) = self.cursor;
        let b = bidx(&self.lines[r], col);
        self.lines[r].insert(b, c);
        self.cursor.1 += 1;
        self.pref_col = visual_col(self.line(r), self.cursor.1);
        self.edit_end();
    }

    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.edit_begin(EditKind::Other);
        self.remove_selection();
        let (r, col) = self.cursor;
        let tail = {
            let l = &self.lines[r];
            l[bidx(l, col)..].to_string()
        };
        let head = {
            let l = &self.lines[r];
            l[..bidx(l, col)].to_string()
        };
        let parts: Vec<&str> = text.split('\n').collect();
        if parts.len() == 1 {
            self.lines[r] = head + parts[0] + &tail;
            self.cursor = (r, col + parts[0].chars().count());
        } else {
            self.lines[r] = head + parts[0];
            let mut insert_at = r + 1;
            for part in &parts[1..parts.len() - 1] {
                self.lines.insert(insert_at, part.to_string());
                insert_at += 1;
            }
            let last = parts[parts.len() - 1];
            self.lines.insert(insert_at, last.to_string() + &tail);
            self.cursor = (insert_at, last.chars().count());
        }
        self.pref_col = visual_col(self.line(self.cursor.0), self.cursor.1);
        self.edit_end();
    }

    pub fn newline(&mut self) {
        self.edit_begin(EditKind::Other);
        self.remove_selection();
        let (r, col) = self.cursor;
        let b = bidx(&self.lines[r], col);
        let tail = self.lines[r][b..].to_string();
        self.lines[r].truncate(b);
        // auto-indent: copy leading whitespace of the current line
        let indent: String = self.lines[r]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let indent_len = indent.chars().count();
        self.lines.insert(r + 1, indent + &tail);
        self.cursor = (r + 1, indent_len);
        self.pref_col = visual_col(self.line(r + 1), indent_len);
        self.edit_end();
    }

    pub fn backspace(&mut self) {
        if self.selection().is_some() {
            self.delete_selection();
            return;
        }
        let (r, col) = self.cursor;
        if r == 0 && col == 0 {
            return;
        }
        self.edit_begin(EditKind::Backspace);
        if col > 0 {
            let b = bidx(&self.lines[r], col - 1);
            self.lines[r].remove(b);
            self.cursor.1 -= 1;
        } else {
            let cur = self.lines.remove(r);
            let prev_len = self.line_len(r - 1);
            self.lines[r - 1].push_str(&cur);
            self.cursor = (r - 1, prev_len);
        }
        self.pref_col = visual_col(self.line(self.cursor.0), self.cursor.1);
        self.edit_end();
    }

    pub fn delete_forward(&mut self) {
        if self.selection().is_some() {
            self.delete_selection();
            return;
        }
        let (r, col) = self.cursor;
        if col < self.line_len(r) {
            self.edit_begin(EditKind::Other);
            let b = bidx(&self.lines[r], col);
            self.lines[r].remove(b);
            self.edit_end();
        } else if r + 1 < self.lines.len() {
            self.edit_begin(EditKind::Other);
            let next = self.lines.remove(r + 1);
            self.lines[r].push_str(&next);
            self.edit_end();
        }
    }

    pub fn duplicate_line(&mut self) {
        self.edit_begin(EditKind::Other);
        let r = self.cursor.0;
        let copy = self.lines[r].clone();
        self.lines.insert(r + 1, copy);
        self.cursor.0 = r + 1;
        self.anchor = None;
        self.edit_end();
    }

    pub fn move_line(&mut self, down: bool) {
        let r = self.cursor.0;
        if down && r + 1 < self.lines.len() {
            self.edit_begin(EditKind::Other);
            self.lines.swap(r, r + 1);
            self.cursor.0 = r + 1;
            self.anchor = None;
            self.edit_end();
        } else if !down && r > 0 {
            self.edit_begin(EditKind::Other);
            self.lines.swap(r, r - 1);
            self.cursor.0 = r - 1;
            self.anchor = None;
            self.edit_end();
        }
    }

    /// Indent (or dedent) the selected lines / current line.
    pub fn indent(&mut self, dedent: bool) {
        self.edit_begin(EditKind::Other);
        let (start, end) = match self.selection() {
            Some((a, b)) => (a.0, b.0),
            None => (self.cursor.0, self.cursor.0),
        };
        for r in start..=end {
            if dedent {
                let strip = self.lines[r]
                    .chars()
                    .take(TAB_STOP)
                    .take_while(|c| *c == ' ')
                    .count();
                let strip = if strip == 0 && self.lines[r].starts_with('\t') { 1 } else { strip };
                if strip > 0 {
                    let b = bidx(&self.lines[r], strip);
                    self.lines[r].replace_range(..b, "");
                    if r == self.cursor.0 {
                        self.cursor.1 = self.cursor.1.saturating_sub(strip);
                    }
                    if let Some(a) = self.anchor.as_mut() {
                        if a.0 == r {
                            a.1 = a.1.saturating_sub(strip);
                        }
                    }
                }
            } else {
                self.lines[r].insert_str(0, "    ");
                if r == self.cursor.0 {
                    self.cursor.1 += TAB_STOP;
                }
                if let Some(a) = self.anchor.as_mut() {
                    if a.0 == r {
                        a.1 += TAB_STOP;
                    }
                }
            }
        }
        self.edit_end();
    }

    pub fn toggle_comment(&mut self, prefix: &str) {
        self.edit_begin(EditKind::Other);
        let (start, end) = match self.selection() {
            Some((a, b)) => (a.0, b.0),
            None => (self.cursor.0, self.cursor.0),
        };
        let all_commented = (start..=end)
            .filter(|r| !self.lines[*r].trim().is_empty())
            .all(|r| self.lines[r].trim_start().starts_with(prefix));
        for r in start..=end {
            if self.lines[r].trim().is_empty() {
                continue;
            }
            let ws = self.lines[r].len() - self.lines[r].trim_start().len();
            if all_commented {
                let mut rest = self.lines[r][ws..].strip_prefix(prefix).unwrap_or("").to_string();
                if rest.starts_with(' ') {
                    rest.remove(0);
                }
                let head = self.lines[r][..ws].to_string();
                self.lines[r] = head + &rest;
            } else {
                self.lines[r].insert_str(ws, &format!("{prefix} "));
            }
        }
        self.anchor = None;
        self.edit_end();
    }

    pub fn trim_trailing_whitespace(&mut self) -> usize {
        self.edit_begin(EditKind::Other);
        let mut count = 0;
        for line in self.lines.iter_mut() {
            let trimmed = line.trim_end();
            if trimmed.len() != line.len() {
                count += 1;
                let t = trimmed.to_string();
                *line = t;
            }
        }
        self.edit_end();
        count
    }

    // ---- movement ----

    fn begin_move(&mut self, select: bool) {
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
    }

    fn set_pref(&mut self) {
        self.pref_col = visual_col(self.line(self.cursor.0), self.cursor.1);
    }

    pub fn move_left(&mut self, select: bool) {
        if !select {
            if let Some((a, _)) = self.selection() {
                self.anchor = None;
                self.cursor = a;
                self.set_pref();
                return;
            }
        }
        self.begin_move(select);
        let (r, c) = self.cursor;
        self.cursor = if c > 0 {
            (r, c - 1)
        } else if r > 0 {
            (r - 1, self.line_len(r - 1))
        } else {
            (r, c)
        };
        self.set_pref();
    }

    pub fn move_right(&mut self, select: bool) {
        if !select {
            if let Some((_, b)) = self.selection() {
                self.anchor = None;
                self.cursor = b;
                self.set_pref();
                return;
            }
        }
        self.begin_move(select);
        let (r, c) = self.cursor;
        self.cursor = if c < self.line_len(r) {
            (r, c + 1)
        } else if r + 1 < self.lines.len() {
            (r + 1, 0)
        } else {
            (r, c)
        };
        self.set_pref();
    }

    pub fn move_up(&mut self, select: bool) {
        self.begin_move(select);
        let r = self.cursor.0;
        if r > 0 {
            let col = col_at_visual(self.line(r - 1), self.pref_col);
            self.cursor = (r - 1, col);
        } else {
            self.cursor = (0, 0);
            self.set_pref();
        }
    }

    pub fn move_down(&mut self, select: bool) {
        self.begin_move(select);
        let r = self.cursor.0;
        if r + 1 < self.lines.len() {
            let col = col_at_visual(self.line(r + 1), self.pref_col);
            self.cursor = (r + 1, col);
        } else {
            self.cursor = (r, self.line_len(r));
            self.set_pref();
        }
    }

    pub fn move_word(&mut self, right: bool, select: bool) {
        self.begin_move(select);
        let (mut r, c) = self.cursor;
        let chars: Vec<char> = self.line(r).chars().collect();
        if right {
            let n = chars.len();
            if c >= n {
                if r + 1 < self.lines.len() {
                    self.cursor = (r + 1, 0);
                }
            } else {
                let mut i = c;
                while i < n && !is_word(chars[i]) {
                    i += 1;
                }
                while i < n && is_word(chars[i]) {
                    i += 1;
                }
                self.cursor = (r, i);
            }
        } else if c == 0 {
            if r > 0 {
                r -= 1;
                self.cursor = (r, self.line_len(r));
            }
        } else {
            let mut i = c;
            while i > 0 && !is_word(chars[i - 1]) {
                i -= 1;
            }
            while i > 0 && is_word(chars[i - 1]) {
                i -= 1;
            }
            self.cursor = (r, i);
        }
        self.set_pref();
    }

    pub fn move_home(&mut self, select: bool) {
        self.begin_move(select);
        let r = self.cursor.0;
        // smart home: first non-whitespace, then column 0
        let first = self.line(r).chars().take_while(|c| c.is_whitespace()).count();
        self.cursor.1 = if self.cursor.1 == first { 0 } else { first };
        self.set_pref();
    }

    pub fn move_end(&mut self, select: bool) {
        self.begin_move(select);
        self.cursor.1 = self.line_len(self.cursor.0);
        self.set_pref();
    }

    pub fn move_doc(&mut self, end: bool, select: bool) {
        self.begin_move(select);
        self.cursor = if end {
            let last = self.lines.len() - 1;
            (last, self.line_len(last))
        } else {
            (0, 0)
        };
        self.set_pref();
    }

    pub fn move_page(&mut self, down: bool, page: usize, select: bool) {
        self.begin_move(select);
        let r = self.cursor.0;
        let nr = if down {
            (r + page).min(self.lines.len() - 1)
        } else {
            r.saturating_sub(page)
        };
        let col = col_at_visual(self.line(nr), self.pref_col);
        self.cursor = (nr, col);
    }

    pub fn goto(&mut self, row: usize, col: usize) {
        let row = row.min(self.lines.len().saturating_sub(1));
        let col = col.min(self.line_len(row));
        self.cursor = (row, col);
        self.anchor = None;
        self.set_pref();
    }

    // ---- search / words ----

    /// All matches of `query` as (row, char col). ASCII case-insensitive
    /// unless the query contains an uppercase letter (smart case).
    pub fn find_matches(&self, query: &str) -> Vec<(usize, usize)> {
        if query.is_empty() {
            return Vec::new();
        }
        let sensitive = query.chars().any(|c| c.is_uppercase());
        let q: Vec<char> = query.chars().collect();
        let mut out = Vec::new();
        for (r, line) in self.lines.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            if chars.len() < q.len() {
                continue;
            }
            for start in 0..=chars.len() - q.len() {
                let hit = (0..q.len()).all(|k| {
                    let (a, b) = (chars[start + k], q[k]);
                    if sensitive {
                        a == b
                    } else {
                        a.eq_ignore_ascii_case(&b)
                    }
                });
                if hit {
                    out.push((r, start));
                }
            }
        }
        out
    }

    pub fn word_under_cursor(&self) -> Option<(String, usize, usize)> {
        let (r, c) = self.cursor;
        let chars: Vec<char> = self.line(r).chars().collect();
        if chars.is_empty() {
            return None;
        }
        let mut i = c.min(chars.len().saturating_sub(1));
        if !is_word(chars[i]) {
            if i > 0 && is_word(chars[i - 1]) {
                i -= 1;
            } else {
                return None;
            }
        }
        let mut start = i;
        while start > 0 && is_word(chars[start - 1]) {
            start -= 1;
        }
        let mut end = i;
        while end < chars.len() && is_word(chars[end]) {
            end += 1;
        }
        Some((chars[start..end].iter().collect(), start, end))
    }

    /// Naive whole-word rename across this buffer. Returns replacement count.
    pub fn rename_word(&mut self, old: &str, new: &str) -> usize {
        if old.is_empty() {
            return 0;
        }
        self.edit_begin(EditKind::Other);
        let oldc: Vec<char> = old.chars().collect();
        let mut count = 0;
        for line in self.lines.iter_mut() {
            let chars: Vec<char> = line.chars().collect();
            let mut result = String::new();
            let mut i = 0;
            while i < chars.len() {
                let end = i + oldc.len();
                let boundary_ok = (i == 0 || !is_word(chars[i - 1]))
                    && (end >= chars.len() || !is_word(chars[end]));
                if end <= chars.len() && chars[i..end] == oldc[..] && boundary_ok {
                    result.push_str(new);
                    count += 1;
                    i = end;
                } else {
                    result.push(chars[i]);
                    i += 1;
                }
            }
            *line = result;
        }
        self.anchor = None;
        self.edit_end();
        count
    }
}
