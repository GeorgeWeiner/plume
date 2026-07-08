//! Sidebar file tree: a flattened view of the project rebuilt from an
//! expanded-directories set.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", "__pycache__", ".venv"];

pub struct TreeItem {
    pub path: PathBuf,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

pub struct FileTree {
    pub root: PathBuf,
    expanded: HashSet<PathBuf>,
    pub items: Vec<TreeItem>,
    pub selected: usize,
    pub scroll: usize,
}

impl FileTree {
    pub fn new(root: PathBuf) -> FileTree {
        let mut t = FileTree {
            root,
            expanded: HashSet::new(),
            items: Vec::new(),
            selected: 0,
            scroll: 0,
        };
        t.refresh();
        t
    }

    pub fn refresh(&mut self) {
        let prev = self.items.get(self.selected).map(|i| i.path.clone());
        self.items.clear();
        let root = self.root.clone();
        self.walk(&root, 0);
        if let Some(p) = prev {
            if let Some(idx) = self.items.iter().position(|i| i.path == p) {
                self.selected = idx;
            }
        }
        self.selected = self.selected.min(self.items.len().saturating_sub(1));
    }

    fn walk(&mut self, dir: &Path, depth: usize) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        let mut entries: Vec<(PathBuf, bool, String)> = rd
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                let is_dir = e.file_type().ok()?.is_dir();
                if is_dir && SKIP_DIRS.contains(&name.as_str()) {
                    return None;
                }
                Some((path, is_dir, name.to_lowercase()))
            })
            .collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
        for (path, is_dir, _) in entries {
            let expanded = is_dir && self.expanded.contains(&path);
            self.items.push(TreeItem { path: path.clone(), depth, is_dir, expanded });
            if expanded {
                self.walk(&path, depth + 1);
            }
        }
    }

    pub fn selected_item(&self) -> Option<&TreeItem> {
        self.items.get(self.selected)
    }

    pub fn move_sel(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let n = self.items.len() as isize;
        self.selected = (self.selected as isize + delta).clamp(0, n - 1) as usize;
    }

    pub fn select_first(&mut self) {
        self.selected = 0;
    }

    pub fn select_last(&mut self) {
        self.selected = self.items.len().saturating_sub(1);
    }

    /// Toggle a directory open/closed. Returns true if the item was a dir.
    pub fn toggle(&mut self) -> bool {
        let Some(item) = self.items.get(self.selected) else { return false };
        if !item.is_dir {
            return false;
        }
        let path = item.path.clone();
        if !self.expanded.remove(&path) {
            self.expanded.insert(path);
        }
        self.refresh();
        true
    }

    pub fn expand(&mut self) {
        if let Some(item) = self.items.get(self.selected) {
            if item.is_dir && !item.expanded {
                self.expanded.insert(item.path.clone());
                self.refresh();
            }
        }
    }

    /// Collapse the selected dir, or jump to the parent dir.
    pub fn collapse(&mut self) {
        let Some(item) = self.items.get(self.selected) else { return };
        if item.is_dir && item.expanded {
            self.expanded.remove(&item.path);
            self.refresh();
            return;
        }
        if let Some(parent) = item.path.parent() {
            if let Some(idx) = self.items.iter().position(|i| i.path == parent) {
                self.selected = idx;
            }
        }
    }

    /// Directory that new files should land in, based on the selection.
    pub fn target_dir(&self) -> PathBuf {
        match self.selected_item() {
            Some(item) if item.is_dir => item.path.clone(),
            Some(item) => item.path.parent().map(Path::to_path_buf).unwrap_or_else(|| self.root.clone()),
            None => self.root.clone(),
        }
    }

    /// Currently expanded directories (for session persistence).
    pub fn expanded_paths(&self) -> Vec<PathBuf> {
        self.expanded.iter().cloned().collect()
    }

    /// Restore a set of expanded directories, then rebuild the flattened view.
    pub fn set_expanded(&mut self, dirs: Vec<PathBuf>) {
        self.expanded = dirs.into_iter().filter(|p| p.is_dir()).collect();
        self.refresh();
    }

    /// Expand ancestors of `path` and select it (reveal in explorer).
    pub fn reveal(&mut self, path: &Path) {
        let mut anc = path.parent();
        while let Some(p) = anc {
            if p == self.root {
                break;
            }
            if !p.starts_with(&self.root) {
                return;
            }
            self.expanded.insert(p.to_path_buf());
            anc = p.parent();
        }
        self.refresh();
        if let Some(idx) = self.items.iter().position(|i| i.path == path) {
            self.selected = idx;
        }
    }
}
