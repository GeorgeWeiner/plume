//! Project-wide search and file listing. Both run on background threads and
//! stream results back over channels so the UI never blocks.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use crate::explorer::SKIP_DIRS;

const MAX_MATCHES: usize = 500;
const MAX_FILES: usize = 5000;
const MAX_FILE_SIZE: u64 = 1_000_000;
const BATCH: usize = 25;

pub struct SearchMatch {
    pub path: PathBuf,
    pub line_no: usize, // 0-based
    pub col: usize,     // char index
    pub len: usize,     // match length in chars
    pub text: String,   // the whole line, trimmed for display
}

/// Messages streamed from a search worker thread. Tagged with a generation id
/// so results from a superseded search are ignored.
pub enum SearchMsg {
    Batch(u64, Vec<SearchMatch>),
    Done(u64),
}

fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if out.len() >= MAX_FILES {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Ok(ft) = e.file_type() else { continue };
        let name = e.file_name().to_string_lossy().to_string();
        if ft.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                walk_files(&e.path(), out);
            }
        } else if ft.is_file() {
            out.push(e.path());
            if out.len() >= MAX_FILES {
                return;
            }
        }
    }
}

pub fn list_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk_files(root, &mut out);
    out
}

/// Smart-case substring search across the project, streaming batches of
/// matches through `tx`. Runs on a worker thread; stops early if the receiver
/// is gone (a newer search superseded this one, or the app quit).
pub fn search_project_streaming(root: &Path, query: &str, gen: u64, tx: Sender<SearchMsg>) {
    let mut total = 0usize;
    let mut batch: Vec<SearchMatch> = Vec::new();
    if !query.is_empty() {
        let sensitive = query.chars().any(|c| c.is_uppercase());
        let q: Vec<char> = query.chars().collect();
        let mut chars: Vec<char> = Vec::new(); // reused across lines

        'files: for path in list_files(root) {
            let Ok(meta) = path.metadata() else { continue };
            if meta.len() > MAX_FILE_SIZE {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else { continue };
            if bytes.contains(&0) {
                continue; // binary
            }
            let text = String::from_utf8_lossy(&bytes);
            for (line_no, line) in text.lines().enumerate() {
                chars.clear();
                chars.extend(line.chars());
                if chars.len() < q.len() {
                    continue;
                }
                for start in 0..=chars.len() - q.len() {
                    let hit = (0..q.len()).all(|k| {
                        let (a, b) = (chars[start + k], q[k]);
                        if sensitive { a == b } else { a.eq_ignore_ascii_case(&b) }
                    });
                    if hit {
                        let display: String = line.trim_end().chars().take(200).collect();
                        batch.push(SearchMatch {
                            path: path.clone(),
                            line_no,
                            col: start,
                            len: q.len(),
                            text: display,
                        });
                        total += 1;
                        if batch.len() >= BATCH {
                            if tx.send(SearchMsg::Batch(gen, std::mem::take(&mut batch))).is_err() {
                                return; // receiver gone — stop working
                            }
                        }
                        if total >= MAX_MATCHES {
                            break 'files;
                        }
                        break; // one match per line is enough for the results list
                    }
                }
            }
        }
    }
    if !batch.is_empty() {
        let _ = tx.send(SearchMsg::Batch(gen, batch));
    }
    let _ = tx.send(SearchMsg::Done(gen));
}

// ---- naive "go to definition" ----

/// Keywords that introduce a definition when they sit just before the symbol
/// (`fn foo`, `class Foo`, `let x`, …). A language-agnostic superset — enough
/// for a naive, LSP-free jump.
const DEF_KEYWORDS: &[&str] = &[
    "fn", "func", "fun", "def", "struct", "enum", "trait", "impl", "class",
    "interface", "type", "let", "const", "var", "static", "mod", "namespace",
    "val", "package", "function", "record", "object", "protocol", "typedef",
    "module", "abstract", "public", "private", "protected",
];

/// Words that precede a *call* (or statement), so `<kw> name(` is not a C-style
/// definition. Keeps the `<type> name(` heuristic from firing on `return foo(`.
const CALL_KEYWORDS: &[&str] = &[
    "return", "if", "else", "while", "for", "switch", "case", "do", "goto",
    "sizeof", "match", "when", "await", "yield", "in", "and", "or", "not",
    "new", "delete", "throw", "catch", "with", "assert", "del", "raise",
    "elif", "typeof", "print", "await",
];

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// If `line` looks like it *defines* `word` (naively), return the char column of
/// `word` on that line. Recognizes `<keyword> word`, a leading `word =` / `word
/// :=` assignment, and C-style `<type> word(`. Ignores partial-word hits.
pub fn definition_col_in_line(line: &str, word: &str) -> Option<usize> {
    if word.is_empty() {
        return None;
    }
    let chars: Vec<char> = line.chars().collect();
    let w: Vec<char> = word.chars().collect();
    let n = chars.len();
    if n < w.len() {
        return None;
    }
    for start in 0..=n - w.len() {
        let whole = chars[start..start + w.len()] == w[..]
            && (start == 0 || !is_ident(chars[start - 1]))
            && (start + w.len() == n || !is_ident(chars[start + w.len()]));
        if whole && classifies_as_def(&chars, start, w.len()) {
            return Some(start);
        }
    }
    None
}

fn classifies_as_def(chars: &[char], start: usize, len: usize) -> bool {
    // Identifier token immediately to the left (skipping whitespace).
    let mut prev_end = start;
    while prev_end > 0 && chars[prev_end - 1].is_whitespace() {
        prev_end -= 1;
    }
    let mut prev_start = prev_end;
    while prev_start > 0 && is_ident(chars[prev_start - 1]) {
        prev_start -= 1;
    }
    let prev: String = chars[prev_start..prev_end].iter().collect();
    if DEF_KEYWORDS.contains(&prev.as_str()) {
        return true;
    }

    // First char sequence after the symbol (skipping whitespace).
    let mut a = start + len;
    while a < chars.len() && chars[a].is_whitespace() {
        a += 1;
    }
    let next = chars.get(a).copied();
    let next2 = chars.get(a + 1).copied();

    // Leading assignment: `word =` (not `==`) or Go's `word :=`.
    let leading = chars[..start].iter().all(|c| c.is_whitespace());
    if leading {
        if next == Some(':') && next2 == Some('=') {
            return true;
        }
        if next == Some('=') && next2 != Some('=') {
            return true;
        }
    }

    // C-style `<type> word(`: preceded by a plain identifier that isn't a
    // call/keyword and isn't a method receiver (`obj.method(`).
    if next == Some('(') && !prev.is_empty() && !CALL_KEYWORDS.contains(&prev.as_str()) {
        let before_prev = prev_start.checked_sub(1).map(|i| chars[i]);
        if before_prev != Some('.') {
            return true;
        }
    }
    false
}

/// Scan the project for naive definitions of `word`, skipping `skip` (the file
/// scanned from the live buffer instead, so unsaved edits count). Bounded like
/// the text search: file count, size, and total candidates are all capped.
pub fn find_definitions(root: &Path, word: &str, skip: Option<&Path>) -> Vec<SearchMatch> {
    let mut out = Vec::new();
    if word.is_empty() {
        return out;
    }
    let len = word.chars().count();
    for path in list_files(root) {
        if skip == Some(path.as_path()) {
            continue;
        }
        let Ok(meta) = path.metadata() else { continue };
        if meta.len() > MAX_FILE_SIZE {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else { continue };
        if bytes.contains(&0) {
            continue; // binary
        }
        let text = String::from_utf8_lossy(&bytes);
        for (line_no, line) in text.lines().enumerate() {
            if let Some(col) = definition_col_in_line(line, word) {
                out.push(SearchMatch {
                    path: path.clone(),
                    line_no,
                    col,
                    len,
                    text: line.trim_end().chars().take(200).collect(),
                });
                if out.len() >= MAX_MATCHES {
                    return out;
                }
            }
        }
    }
    out
}

/// Find the char position of `query` within `text` (ASCII case-insensitive),
/// for highlighting search results.
pub fn find_in_line(text: &str, query: &str) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let q: Vec<char> = query.chars().collect();
    if q.is_empty() || chars.len() < q.len() {
        return None;
    }
    (0..=chars.len() - q.len()).find(|&start| {
        (0..q.len()).all(|k| chars[start + k].eq_ignore_ascii_case(&q[k]))
    })
}

#[cfg(test)]
mod def_tests {
    use super::definition_col_in_line as def;

    #[test]
    fn recognizes_keyword_definitions() {
        assert_eq!(def("fn compute(x: u64) -> u64 {", "compute"), Some(3));
        assert_eq!(def("    pub fn helper() {", "helper"), Some(11));
        assert_eq!(def("class Widget:", "Widget"), Some(6));
        assert_eq!(def("struct Point { x: i32 }", "Point"), Some(7));
        assert_eq!(def("    let total = 0;", "total"), Some(8));
        assert_eq!(def("def parse(s):", "parse"), Some(4));
    }

    #[test]
    fn recognizes_assignments_and_c_style() {
        assert_eq!(def("count := 0", "count"), Some(0)); // Go
        assert_eq!(def("result = compute()", "result"), Some(0)); // leading assignment
        assert_eq!(def("int add(int a, int b) {", "add"), Some(4)); // C-style
    }

    #[test]
    fn rejects_uses_and_calls() {
        assert_eq!(def("    compute();", "compute"), None); // call
        assert_eq!(def("return compute(x);", "compute"), None); // call after keyword
        assert_eq!(def("obj.method();", "method"), None); // method receiver
        assert_eq!(def("let y = compute;", "compute"), None); // rhs identifier
        assert_eq!(def("precompute(x)", "compute"), None); // partial word
        assert_eq!(def("if compute(x) {", "compute"), None); // call in condition
    }
}
