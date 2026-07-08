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
