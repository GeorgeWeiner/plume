//! Per-project session persistence: which files were open, where the cursor
//! was in each, the active tab, sidebar visibility, and expanded tree folders.
//! Saved on exit, restored when the same folder is opened again.
//!
//! Sessions live under the state dir, one file per project (named by a stable
//! hash of the absolute root path). A `last-project` marker records the most
//! recent root so `plume --resume` can find it.

use std::fs;
use std::path::{Path, PathBuf};

use crate::paths;

pub struct OpenFile {
    pub path: PathBuf,
    pub row: usize,
    pub col: usize,
    pub scroll: usize,
}

#[derive(Default)]
pub struct Session {
    pub root: PathBuf,
    pub files: Vec<OpenFile>,
    pub active: usize,
    pub sidebar: bool,
    pub expanded: Vec<PathBuf>,
}

/// Stable FNV-1a hash → hex, used to name a project's session file.
fn hash_path(p: &Path) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in p.to_string_lossy().as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn session_file(root: &Path) -> Option<PathBuf> {
    paths::sessions_dir().map(|d| d.join(format!("{}.session", hash_path(root))))
}

pub fn set_last_project(root: &Path) {
    if let Some(f) = paths::last_project_file() {
        if let Some(dir) = f.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(&f, root.to_string_lossy().as_bytes());
    }
}

pub fn last_project() -> Option<PathBuf> {
    let f = paths::last_project_file()?;
    let s = fs::read_to_string(f).ok()?;
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let p = PathBuf::from(s);
    p.is_dir().then_some(p)
}

pub fn save(s: &Session) {
    let Some(path) = session_file(&s.root) else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let mut out = String::new();
    out.push_str(&format!("root\t{}\n", s.root.display()));
    out.push_str(&format!("active\t{}\n", s.active));
    out.push_str(&format!("sidebar\t{}\n", if s.sidebar { 1 } else { 0 }));
    for f in &s.files {
        // tab-separated; paths practically never contain tabs
        out.push_str(&format!(
            "file\t{}\t{}\t{}\t{}\n",
            f.path.display(),
            f.row,
            f.col,
            f.scroll
        ));
    }
    for d in &s.expanded {
        out.push_str(&format!("expanded\t{}\n", d.display()));
    }
    let _ = fs::write(&path, out);
}

pub fn load(root: &Path) -> Option<Session> {
    let path = session_file(root)?;
    let text = fs::read_to_string(path).ok()?;
    let mut s = Session {
        root: root.to_path_buf(),
        sidebar: true,
        ..Default::default()
    };
    for line in text.lines() {
        let mut parts = line.split('\t');
        match parts.next() {
            Some("active") => {
                if let Some(v) = parts.next() {
                    s.active = v.parse().unwrap_or(0);
                }
            }
            Some("sidebar") => {
                s.sidebar = parts.next() == Some("1");
            }
            Some("file") => {
                let p = parts.next().map(PathBuf::from);
                let row = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                let col = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                let scroll = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                if let Some(path) = p {
                    s.files.push(OpenFile { path, row, col, scroll });
                }
            }
            Some("expanded") => {
                if let Some(p) = parts.next() {
                    s.expanded.push(PathBuf::from(p));
                }
            }
            _ => {}
        }
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_and_distinct() {
        let a = hash_path(Path::new("/home/u/proj"));
        assert_eq!(a, hash_path(Path::new("/home/u/proj")));
        assert_ne!(a, hash_path(Path::new("/home/u/other")));
        assert_eq!(a.len(), 16);
    }
}
