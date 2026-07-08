//! Project-wide search and file listing (used by global search, quick open,
//! and find-references).

use std::fs;
use std::path::{Path, PathBuf};

use crate::explorer::SKIP_DIRS;

const MAX_MATCHES: usize = 500;
const MAX_FILES: usize = 5000;
const MAX_FILE_SIZE: u64 = 1_000_000;

pub struct SearchMatch {
    pub path: PathBuf,
    pub line_no: usize, // 0-based
    pub col: usize,     // char index
    pub len: usize,     // match length in chars
    pub text: String,   // the whole line, trimmed for display
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

/// Smart-case substring search across the project. Capped at 500 matches.
pub fn search_project(root: &Path, query: &str) -> Vec<SearchMatch> {
    let mut out = Vec::new();
    if query.is_empty() {
        return out;
    }
    let sensitive = query.chars().any(|c| c.is_uppercase());
    let q: Vec<char> = query.chars().collect();

    for path in list_files(root) {
        if out.len() >= MAX_MATCHES {
            break;
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
            let chars: Vec<char> = line.chars().collect();
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
                    out.push(SearchMatch {
                        path: path.clone(),
                        line_no,
                        col: start,
                        len: q.len(),
                        text: display,
                    });
                    if out.len() >= MAX_MATCHES {
                        return out;
                    }
                    break; // one match per line is enough for the results list
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
