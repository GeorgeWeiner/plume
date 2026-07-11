//! Text buffer: lines of text, cursor, selection, undo/redo, edit ops.
//!
//! Performance model: every edit records a *delta* (the replaced line range),
//! never a whole-buffer snapshot, and the block-comment highlight state is
//! updated incrementally from the edited row until it converges. Both keep
//! per-keystroke cost O(changed lines), independent of file size.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::syntax::{self, Language};

pub const TAB_STOP: usize = 4;
const MAX_FIND_MATCHES: usize = 10_000;
const MAX_UNDO_ENTRIES: usize = 1_000;
/// Cap on characters visited when matching the bracket pair around the cursor,
/// so an unbalanced file can't make a frame scan the whole buffer.
const MAX_BRACKET_SCAN: usize = 50_000;

/// The matching bracket pair enclosing the cursor: opener and closer positions
/// as (row, char column).
#[derive(Clone, Copy)]
pub struct BracketPair {
    pub open: (usize, usize),
    pub close: (usize, usize),
}

/// Classify a bracket char as `(type index, is_opening)`: 0 = `()`, 1 = `[]`,
/// 2 = `{}`. Non-brackets return `None`.
fn bracket_kind(c: char) -> Option<(usize, bool)> {
    match c {
        '(' => Some((0, true)),
        ')' => Some((0, false)),
        '[' => Some((1, true)),
        ']' => Some((1, false)),
        '{' => Some((2, true)),
        '}' => Some((2, false)),
        _ => None,
    }
}

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

/// Detect a file's newline style from its first line ending. A file with no
/// newline falls back to the platform default, so fresh content is CRLF on
/// Windows and LF elsewhere.
fn detect_crlf(s: &str) -> bool {
    match s.find('\n') {
        Some(i) => i > 0 && s.as_bytes()[i - 1] == b'\r',
        None => cfg!(windows),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum EditKind {
    InsertChar,
    Backspace,
    Other,
}

/// One recorded edit: rows [row, row+new_len) currently hold what replaced
/// `old`. Multiple changes made by a single user action share a `group` and
/// are undone together.
struct Change {
    group: u64,
    kind: EditKind,
    row: usize,
    old: Vec<String>,
    new_len: usize,
    cursor_before: (usize, usize),
    cursor_after: (usize, usize),
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
    /// File uses CRLF line endings (preserved on save).
    pub crlf: bool,
    pub language: Language,
    /// Per line: starts inside a block comment.
    pub line_states: Vec<bool>,
    undo_stack: Vec<Change>,
    redo_stack: Vec<Change>,
    group_counter: u64,
}

impl Buffer {
    pub fn untitled() -> Buffer {
        Buffer {
            path: None,
            lines: vec![String::new()],
            cursor: (0, 0),
            anchor: None,
            pref_col: 0,
            scroll_row: 0,
            scroll_col: 0,
            modified: false,
            crlf: cfg!(windows),
            language: Language::Plain,
            line_states: vec![false],
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            group_counter: 0,
        }
    }

    pub fn from_path(path: &Path) -> io::Result<Buffer> {
        let bytes = fs::read(path)?;
        let raw = String::from_utf8_lossy(&bytes);
        let crlf = detect_crlf(&raw);
        let text = raw.replace("\r\n", "\n").replace('\r', "\n");
        let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
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
        b.crlf = crlf;
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
        let newline = if self.crlf { "\r\n" } else { "\n" };
        let mut text = self.lines.join(newline);
        text.push_str(newline);
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

    /// Visual width of `row`'s leading whitespace, or `None` if the line is
    /// blank (all whitespace) — used to place indentation guides.
    fn content_indent(&self, row: usize) -> Option<usize> {
        let mut w = 0;
        for c in self.line(row).chars() {
            match c {
                ' ' => w += 1,
                '\t' => w += TAB_STOP - w % TAB_STOP,
                _ => return Some(w),
            }
        }
        None
    }

    /// Effective indentation (visual columns) for drawing guides on `row`. A
    /// blank line borrows the shallower indent of its nearest non-blank
    /// neighbors, so guides run through blank lines inside a block but stop at
    /// its edges. Neighbor search is bounded so blank spans stay cheap.
    pub fn guide_indent(&self, row: usize) -> usize {
        if let Some(w) = self.content_indent(row) {
            return w;
        }
        const LOOK: usize = 500;
        let up = (0..row).rev().take(LOOK).find_map(|r| self.content_indent(r));
        let down = (row + 1..self.lines.len()).take(LOOK).find_map(|r| self.content_indent(r));
        match (up, down) {
            (Some(a), Some(b)) => a.min(b),
            _ => 0,
        }
    }

    // ---- bracket matching ----

    /// Char positions (char index) that are inside a string or comment token on
    /// `row`, so bracket matching can ignore brackets that are really text.
    fn skip_mask(&self, row: usize) -> Vec<bool> {
        let line = self.line(row);
        let n = line.chars().count();
        let mut mask = vec![false; n];
        let in_block = self.line_states.get(row).copied().unwrap_or(false);
        for (s, e, tok) in syntax::scan_line(self.language, line, in_block).0 {
            if matches!(tok, syntax::Tok::String | syntax::Tok::Comment) {
                for slot in mask.iter_mut().take(e.min(n)).skip(s.min(n)) {
                    *slot = true;
                }
            }
        }
        mask
    }

    /// The bracket pair `()`, `[]` or `{}` that encloses the cursor, ignoring
    /// brackets inside strings and comments. Returns the opener/closer positions
    /// so the UI can highlight them and draw a guide between them. Bounded work:
    /// gives up after scanning `MAX_BRACKET_SCAN` characters in either direction.
    pub fn enclosing_brackets(&self) -> Option<BracketPair> {
        let (crow, ccol) = self.cursor;

        // Walk backward from just left of the cursor for the nearest opener that
        // isn't already closed before the cursor (one pending counter per type).
        let mut pending = [0i32; 3];
        let mut steps = 0usize;
        let (mut open_pos, mut open_kind) = (None, 0usize);
        let mut r = crow as isize;
        'back: while r >= 0 {
            let row = r as usize;
            let chars: Vec<char> = self.line(row).chars().collect();
            let mask = self.skip_mask(row);
            let upper = if row == crow { ccol.min(chars.len()) } else { chars.len() };
            let mut c = upper as isize - 1;
            while c >= 0 {
                steps += 1;
                if steps > MAX_BRACKET_SCAN {
                    return None;
                }
                let ci = c as usize;
                if !mask.get(ci).copied().unwrap_or(false) {
                    if let Some((k, is_open)) = bracket_kind(chars[ci]) {
                        if is_open {
                            if pending[k] > 0 {
                                pending[k] -= 1;
                            } else {
                                open_pos = Some((row, ci));
                                open_kind = k;
                                break 'back;
                            }
                        } else {
                            pending[k] += 1;
                        }
                    }
                }
                c -= 1;
            }
            r -= 1;
        }
        let (or, oc) = open_pos?;

        // Walk forward from the opener for its matching closer (depth on this
        // bracket type only).
        let mut depth = 0i32;
        steps = 0;
        for row in or..self.lines.len() {
            let chars: Vec<char> = self.line(row).chars().collect();
            let mask = self.skip_mask(row);
            let lo = if row == or { oc } else { 0 };
            for (ci, &ch) in chars.iter().enumerate().skip(lo) {
                steps += 1;
                if steps > MAX_BRACKET_SCAN {
                    return None;
                }
                if mask.get(ci).copied().unwrap_or(false) {
                    continue;
                }
                if let Some((k, is_open)) = bracket_kind(ch) {
                    if k != open_kind {
                        continue;
                    }
                    if is_open {
                        depth += 1;
                    } else {
                        depth -= 1;
                        if depth == 0 {
                            return Some(BracketPair { open: (or, oc), close: (row, ci) });
                        }
                    }
                }
            }
        }
        None
    }

    // ---- highlight state (block comments) ----

    fn lang_has_block(&self) -> bool {
        matches!(
            self.language,
            Language::Rust
                | Language::JavaScript
                | Language::TypeScript
                | Language::C
                | Language::Glsl
                | Language::Hlsl
                | Language::Go
                | Language::Css
                | Language::Html
        )
    }

    /// Full recompute — used on load and language change only.
    pub fn recompute_states(&mut self) {
        if !self.lang_has_block() {
            self.line_states = vec![false; self.lines.len()];
            return;
        }
        let mut state = false;
        self.line_states = Vec::with_capacity(self.lines.len());
        for line in &self.lines {
            self.line_states.push(state);
            let (_, next) = syntax::scan_line(self.language, line, state);
            state = next;
        }
    }

    /// Incremental update after rows [row, row+old_count) were replaced by
    /// `new_count` rows: rescan forward only until the carried state matches
    /// the cached one again.
    fn update_states(&mut self, row: usize, old_count: usize, new_count: usize) {
        if !self.lang_has_block() {
            self.line_states.resize(self.lines.len(), false);
            return;
        }
        let start = self.line_states.get(row).copied().unwrap_or(false);
        let end_old = (row + old_count).min(self.line_states.len());
        self.line_states
            .splice(row..end_old, std::iter::repeat(start).take(new_count));
        if self.line_states.len() != self.lines.len() {
            // bookkeeping went out of sync somewhere — self-heal
            self.recompute_states();
            return;
        }
        let mut st = start;
        let mut i = row;
        while i < self.lines.len() {
            if i >= row + new_count && self.line_states[i] == st {
                break; // converged: everything below is unaffected
            }
            self.line_states[i] = st;
            let (_, next) = syntax::scan_line(self.language, &self.lines[i], st);
            st = next;
            i += 1;
        }
    }

    // ---- undo plumbing ----

    fn next_group(&mut self) -> u64 {
        self.group_counter += 1;
        self.group_counter
    }

    /// Record a completed change. Call AFTER the mutation, with `old` holding
    /// the pre-mutation lines of the replaced range and `cursor_before` the
    /// cursor at the start of this sub-change.
    fn record(
        &mut self,
        group: u64,
        kind: EditKind,
        row: usize,
        old: Vec<String>,
        new_len: usize,
        cursor_before: (usize, usize),
    ) {
        self.modified = true;
        self.redo_stack.clear();
        let old_len = old.len();
        // coalesce plain typing / backspacing on the same line
        let coalesce = kind != EditKind::Other
            && old_len == 1
            && new_len == 1
            && matches!(
                self.undo_stack.last(),
                Some(l) if l.kind == kind && l.row == row && l.new_len == 1
                    && l.cursor_after == cursor_before
            );
        if coalesce {
            let last = self.undo_stack.last_mut().unwrap();
            last.cursor_after = self.cursor;
        } else {
            self.undo_stack.push(Change {
                group,
                kind,
                row,
                old,
                new_len,
                cursor_before,
                cursor_after: self.cursor,
            });
            if self.undo_stack.len() > MAX_UNDO_ENTRIES {
                let g0 = self.undo_stack[0].group;
                let n = self.undo_stack.iter().take_while(|c| c.group == g0).count();
                self.undo_stack.drain(..n);
            }
        }
        self.update_states(row, old_len, new_len);
    }

    /// Apply the inverse of a change and return the change that undoes it.
    fn apply_change(&mut self, ch: &Change) -> Change {
        let current: Vec<String> = self.lines[ch.row..ch.row + ch.new_len].to_vec();
        self.lines
            .splice(ch.row..ch.row + ch.new_len, ch.old.iter().cloned());
        self.modified = true;
        self.update_states(ch.row, ch.new_len, ch.old.len());
        Change {
            group: ch.group,
            kind: EditKind::Other,
            row: ch.row,
            old: current,
            new_len: ch.old.len(),
            cursor_before: ch.cursor_after,
            cursor_after: ch.cursor_before,
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(last) = self.undo_stack.last() else {
            return false;
        };
        let group = last.group;
        let mut applied = false;
        while let Some(top) = self.undo_stack.last() {
            if top.group != group {
                break;
            }
            let ch = self.undo_stack.pop().unwrap();
            let inv = self.apply_change(&ch);
            self.redo_stack.push(inv);
            self.cursor = ch.cursor_before;
            applied = true;
        }
        if applied {
            self.anchor = None;
            self.clamp_cursor();
            self.pref_col = visual_col(self.line(self.cursor.0), self.cursor.1);
        }
        applied
    }

    pub fn redo(&mut self) -> bool {
        let Some(last) = self.redo_stack.last() else {
            return false;
        };
        let group = last.group;
        let mut applied = false;
        while let Some(top) = self.redo_stack.last() {
            if top.group != group {
                break;
            }
            let ch = self.redo_stack.pop().unwrap();
            let inv = self.apply_change(&ch);
            self.undo_stack.push(inv);
            self.cursor = ch.cursor_before;
            applied = true;
        }
        if applied {
            self.anchor = None;
            self.clamp_cursor();
            self.pref_col = visual_col(self.line(self.cursor.0), self.cursor.1);
        }
        applied
    }

    fn clamp_cursor(&mut self) {
        let row = self.cursor.0.min(self.lines.len().saturating_sub(1));
        let col = self.cursor.1.min(self.line_len(row));
        self.cursor = (row, col);
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

    /// Remove the selection as part of undo group `g`. Returns true if there
    /// was a selection.
    fn delete_selection_grouped(&mut self, g: u64) -> bool {
        let Some((a, b)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        let cb = self.cursor;
        let old = self.lines[a.0..=b.0].to_vec();
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
        self.pref_col = visual_col(self.line(a.0), a.1);
        self.record(g, EditKind::Other, a.0, old, 1, cb);
        true
    }

    pub fn delete_selection(&mut self) {
        let g = self.next_group();
        self.delete_selection_grouped(g);
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
        let g = self.next_group();
        let had_sel = self.delete_selection_grouped(g);
        let cb = self.cursor;
        let (r, col) = self.cursor;
        let old = vec![self.lines[r].clone()];
        let b = bidx(&self.lines[r], col);
        self.lines[r].insert(b, c);
        self.cursor = (r, col + 1);
        self.pref_col = visual_col(self.line(r), self.cursor.1);
        let kind = if had_sel { EditKind::Other } else { EditKind::InsertChar };
        self.record(g, kind, r, old, 1, cb);
    }

    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let g = self.next_group();
        self.delete_selection_grouped(g);
        let cb = self.cursor;
        let (r, col) = self.cursor;
        let old = vec![self.lines[r].clone()];
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
        self.record(g, EditKind::Other, r, old, parts.len(), cb);
    }

    pub fn newline(&mut self) {
        let g = self.next_group();
        self.delete_selection_grouped(g);
        let cb = self.cursor;
        let (r, col) = self.cursor;
        let old = vec![self.lines[r].clone()];
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
        self.record(g, EditKind::Other, r, old, 2, cb);
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
        let g = self.next_group();
        let cb = self.cursor;
        if col > 0 {
            let old = vec![self.lines[r].clone()];
            let b = bidx(&self.lines[r], col - 1);
            self.lines[r].remove(b);
            self.cursor = (r, col - 1);
            self.pref_col = visual_col(self.line(r), self.cursor.1);
            self.record(g, EditKind::Backspace, r, old, 1, cb);
        } else {
            let old = self.lines[r - 1..=r].to_vec();
            let cur = self.lines.remove(r);
            let prev_len = self.line_len(r - 1);
            self.lines[r - 1].push_str(&cur);
            self.cursor = (r - 1, prev_len);
            self.pref_col = visual_col(self.line(r - 1), prev_len);
            self.record(g, EditKind::Other, r - 1, old, 1, cb);
        }
    }

    pub fn delete_forward(&mut self) {
        if self.selection().is_some() {
            self.delete_selection();
            return;
        }
        let (r, col) = self.cursor;
        if col < self.line_len(r) {
            let g = self.next_group();
            let cb = self.cursor;
            let old = vec![self.lines[r].clone()];
            let b = bidx(&self.lines[r], col);
            self.lines[r].remove(b);
            self.record(g, EditKind::Other, r, old, 1, cb);
        } else if r + 1 < self.lines.len() {
            let g = self.next_group();
            let cb = self.cursor;
            let old = self.lines[r..=r + 1].to_vec();
            let next = self.lines.remove(r + 1);
            self.lines[r].push_str(&next);
            self.record(g, EditKind::Other, r, old, 1, cb);
        }
    }

    pub fn duplicate_line(&mut self) {
        let g = self.next_group();
        let cb = self.cursor;
        let r = self.cursor.0;
        let old = vec![self.lines[r].clone()];
        let copy = self.lines[r].clone();
        self.lines.insert(r + 1, copy);
        self.cursor.0 = r + 1;
        self.anchor = None;
        self.record(g, EditKind::Other, r, old, 2, cb);
    }

    /// Delete the current line, or every line spanned by the selection.
    pub fn delete_line(&mut self) {
        let (start, end) = match self.selection() {
            Some((a, b)) => (a.0, b.0),
            None => (self.cursor.0, self.cursor.0),
        };
        let g = self.next_group();
        let cb = self.cursor;
        if end - start + 1 >= self.lines.len() {
            // deleting all lines -> leave a single empty line
            let old = std::mem::replace(&mut self.lines, vec![String::new()]);
            self.cursor = (0, 0);
            self.anchor = None;
            self.record(g, EditKind::Other, 0, old, 1, cb);
        } else {
            let old = self.lines[start..=end].to_vec();
            self.lines.drain(start..=end);
            let new_row = start.min(self.lines.len().saturating_sub(1));
            self.cursor = (new_row, 0);
            self.anchor = None;
            self.record(g, EditKind::Other, start, old, 0, cb);
        }
        self.clamp_cursor();
    }

    pub fn move_line(&mut self, down: bool) {
        let r = self.cursor.0;
        if down && r + 1 < self.lines.len() {
            let g = self.next_group();
            let cb = self.cursor;
            let old = self.lines[r..=r + 1].to_vec();
            self.lines.swap(r, r + 1);
            self.cursor.0 = r + 1;
            self.anchor = None;
            self.record(g, EditKind::Other, r, old, 2, cb);
        } else if !down && r > 0 {
            let g = self.next_group();
            let cb = self.cursor;
            let old = self.lines[r - 1..=r].to_vec();
            self.lines.swap(r, r - 1);
            self.cursor.0 = r - 1;
            self.anchor = None;
            self.record(g, EditKind::Other, r - 1, old, 2, cb);
        }
    }

    /// Indent (or dedent) the selected lines / current line.
    pub fn indent(&mut self, dedent: bool) {
        let (start, end) = match self.selection() {
            Some((a, b)) => (a.0, b.0),
            None => (self.cursor.0, self.cursor.0),
        };
        let g = self.next_group();
        let cb = self.cursor;
        let old = self.lines[start..=end].to_vec();
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
        self.record(g, EditKind::Other, start, old, end - start + 1, cb);
    }

    pub fn toggle_comment(&mut self, prefix: &str) {
        let (start, end) = match self.selection() {
            Some((a, b)) => (a.0, b.0),
            None => (self.cursor.0, self.cursor.0),
        };
        let g = self.next_group();
        let cb = self.cursor;
        let old = self.lines[start..=end].to_vec();
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
        self.record(g, EditKind::Other, start, old, end - start + 1, cb);
        self.clamp_cursor();
    }

    pub fn trim_trailing_whitespace(&mut self) -> usize {
        let g = self.next_group();
        let cb = self.cursor;
        let old = self.lines.clone();
        let mut count = 0;
        for line in self.lines.iter_mut() {
            let trimmed = line.trim_end();
            if trimmed.len() != line.len() {
                count += 1;
                let t = trimmed.to_string();
                *line = t;
            }
        }
        if count == 0 {
            return 0; // nothing changed; don't pollute the undo stack
        }
        let n = self.lines.len();
        self.record(g, EditKind::Other, 0, old, n, cb);
        self.clamp_cursor();
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

    /// All matches of `query` as (row, char col), sorted row-major and capped.
    /// ASCII case-insensitive unless the query contains uppercase (smart case).
    pub fn find_matches(&self, query: &str) -> Vec<(usize, usize)> {
        if query.is_empty() {
            return Vec::new();
        }
        let sensitive = query.chars().any(|c| c.is_uppercase());
        let q: Vec<char> = query.chars().collect();
        let mut out = Vec::new();
        let mut chars: Vec<char> = Vec::new(); // reused across lines
        for (r, line) in self.lines.iter().enumerate() {
            chars.clear();
            chars.extend(line.chars());
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
                    if out.len() >= MAX_FIND_MATCHES {
                        return out;
                    }
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
    pub fn rename_word(&mut self, old_word: &str, new_word: &str) -> usize {
        if old_word.is_empty() {
            return 0;
        }
        let g = self.next_group();
        let cb = self.cursor;
        let old = self.lines.clone();
        let oldc: Vec<char> = old_word.chars().collect();
        let mut count = 0;
        for line in self.lines.iter_mut() {
            if !line.contains(old_word) && old_word.is_ascii() {
                continue; // fast reject for the common case
            }
            let chars: Vec<char> = line.chars().collect();
            let mut result = String::new();
            let mut i = 0;
            while i < chars.len() {
                let end = i + oldc.len();
                let boundary_ok = (i == 0 || !is_word(chars[i - 1]))
                    && (end >= chars.len() || !is_word(chars[end]));
                if end <= chars.len() && chars[i..end] == oldc[..] && boundary_ok {
                    result.push_str(new_word);
                    count += 1;
                    i = end;
                } else {
                    result.push(chars[i]);
                    i += 1;
                }
            }
            *line = result;
        }
        if count == 0 {
            return 0;
        }
        self.anchor = None;
        let n = self.lines.len();
        self.record(g, EditKind::Other, 0, old, n, cb);
        self.clamp_cursor();
        count
    }
}

#[cfg(test)]
mod bench {
    use super::*;
    use std::time::Instant;

    /// Build a syntactically Rust-ish buffer of `n` lines.
    fn big_buffer(n: usize) -> Buffer {
        let mut b = Buffer::untitled();
        b.language = Language::Rust;
        b.lines = (0..n)
            .map(|i| match i % 5 {
                0 => format!("fn handler_{i}(x: u64) -> u64 {{"),
                1 => format!("    // step {i}: accumulate the widget value"),
                2 => format!("    let value_{i} = x.wrapping_mul({i}) + 0x{i:x};"),
                3 => format!("    value_{i} /* running total */"),
                _ => "}".to_string(),
            })
            .collect();
        b.recompute_states();
        b
    }

    // Run with: cargo test --release bench_edit_cost -- --ignored --nocapture
    #[test]
    #[ignore]
    fn bench_edit_cost() {
        let n = 120_000;
        let mut b = big_buffer(n);
        b.goto(n - 1, 0); // worst case: edits at end of file

        let iters = 2000;
        let t0 = Instant::now();
        for _ in 0..iters {
            b.insert_char('x');
        }
        let per_insert = t0.elapsed().as_secs_f64() * 1e6 / iters as f64;

        let t1 = Instant::now();
        for _ in 0..iters {
            b.undo();
        }
        let per_undo = t1.elapsed().as_secs_f64() * 1e6 / iters as f64;

        // find that saturates the 10k cap
        let t2 = Instant::now();
        let m = b.find_matches("value_");
        let find_ms = t2.elapsed().as_secs_f64() * 1e3;

        println!("\n=== plume buffer micro-bench ({n} lines, edits at EOF) ===");
        println!("insert_char (delta undo + incremental highlight): {per_insert:8.2} µs/edit");
        println!("undo:                                             {per_undo:8.2} µs/edit");
        println!("find_matches('value_') -> {} hits:               {find_ms:8.2} ms", m.len());
        println!("(a whole-buffer snapshot per edit would clone ~4 MB each time)\n");

        // Regression guard: an O(file) edit on 120k lines would be
        // hundreds of µs to milliseconds; incremental must stay tiny.
        assert!(per_insert < 50.0, "insert_char too slow: {per_insert} µs");
        assert!(per_undo < 50.0, "undo too slow: {per_undo} µs");
    }
}

#[cfg(test)]
mod bracket_tests {
    use super::*;

    fn buf(lines: &[&str], lang: Language) -> Buffer {
        let mut b = Buffer::untitled();
        b.language = lang;
        b.lines = lines.iter().map(|s| s.to_string()).collect();
        b.recompute_states();
        b
    }

    #[test]
    fn matches_across_lines() {
        let mut b = buf(&["fn f() {", "    body();", "}"], Language::Rust);
        b.goto(1, 4); // inside the block
        let p = b.enclosing_brackets().expect("should find {}");
        assert_eq!(p.open, (0, 7));
        assert_eq!(p.close, (2, 0));
    }

    #[test]
    fn picks_innermost_pair() {
        let mut b = buf(&["a(b[c] )"], Language::Rust);
        b.goto(0, 5); // between [ and ] region: cursor after c
        let p = b.enclosing_brackets().expect("innermost is []");
        assert_eq!((p.open, p.close), ((0, 3), (0, 5)));
    }

    #[test]
    fn ignores_brackets_in_strings_and_comments() {
        // The only real pair is the outer (); the "(" in the string and the
        // "]" in the comment must be ignored.
        let mut b = buf(&["f(\"a ( b\"); // ]", "x"], Language::Rust);
        b.goto(0, 5);
        let p = b.enclosing_brackets().expect("outer ()");
        assert_eq!((p.open, p.close), ((0, 1), (0, 9)));
    }

    #[test]
    fn none_when_unbalanced() {
        let mut b = buf(&["let x = 1;"], Language::Rust);
        b.goto(0, 4);
        assert!(b.enclosing_brackets().is_none());
    }
}

#[cfg(test)]
mod io_tests {
    use super::*;
    use std::env;

    fn tmp(name: &str) -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!("plume_test_{}_{name}", std::process::id()));
        p
    }

    /// A file's original CRLF/LF style survives a load/save round-trip.
    #[test]
    fn preserves_line_endings() {
        let crlf = tmp("crlf.txt");
        fs::write(&crlf, b"one\r\ntwo\r\nthree\r\n").unwrap();
        let mut b = Buffer::from_path(&crlf).unwrap();
        assert!(b.crlf, "should detect CRLF");
        assert_eq!(b.lines, vec!["one", "two", "three"]);
        b.save().unwrap();
        assert_eq!(fs::read(&crlf).unwrap(), b"one\r\ntwo\r\nthree\r\n");

        let lf = tmp("lf.txt");
        fs::write(&lf, b"a\nb\n").unwrap();
        let mut b = Buffer::from_path(&lf).unwrap();
        assert!(!b.crlf, "should detect LF");
        b.save().unwrap();
        assert_eq!(fs::read(&lf).unwrap(), b"a\nb\n");

        let _ = fs::remove_file(&crlf);
        let _ = fs::remove_file(&lf);
    }
}
