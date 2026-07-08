//! Command-line argument parsing (hand-rolled — no dependency).
//!
//!   plume                 open the current directory as a project
//!   plume path/to/dir     open that directory as a project
//!   plume a.rs b.rs       open those files (project root = current dir)
//!   plume -r | --resume   reopen your most recent session
//!   plume -n | --new      open fresh (don't restore a saved session)
//!   plume -h | --help     print help
//!   plume -v | --version  print version

use std::path::PathBuf;

use crate::{paths, session};

pub struct Invocation {
    pub root: PathBuf,
    /// Explicit files to open (in addition to any restored session).
    pub files: Vec<PathBuf>,
    /// Restore the saved session for `root` before opening `files`.
    pub restore: bool,
}

pub enum Cli {
    Run(Invocation),
    Exit(i32),
}

const HELP: &str = "\
plume — a feather-light terminal IDE

USAGE:
    plume [OPTIONS] [PATH]...

ARGS:
    <PATH>...    Files to open, or a single directory to open as the project.
                 With no path, opens the current directory.

OPTIONS:
    -r, --resume     Reopen your most recent session (project + tabs + cursors)
    -n, --new        Start fresh; do not restore a saved session
    -h, --help       Print this help and exit
    -v, --version    Print version and exit

Opening a folder restores its last session automatically (tabs, cursor
positions, sidebar). Use --new to start clean, or --resume to jump back into
whatever you had open last, from anywhere.";

pub fn parse<I: IntoIterator<Item = String>>(args: I, cwd: PathBuf) -> Cli {
    let mut resume = false;
    let mut fresh = false;
    let mut positionals: Vec<String> = Vec::new();
    let mut opts_done = false;

    for a in args {
        if !opts_done && a == "--" {
            opts_done = true;
            continue;
        }
        if !opts_done && a.starts_with('-') && a != "-" {
            match a.as_str() {
                "-h" | "--help" => {
                    println!("{HELP}");
                    print_paths();
                    return Cli::Exit(0);
                }
                "-v" | "--version" => {
                    println!("plume {}", env!("CARGO_PKG_VERSION"));
                    return Cli::Exit(0);
                }
                "-r" | "--resume" => resume = true,
                "-n" | "--new" => fresh = true,
                other => {
                    eprintln!("plume: unknown option '{other}' (try --help)");
                    return Cli::Exit(2);
                }
            }
        } else {
            positionals.push(a);
        }
    }

    let abs = |p: &str| {
        let pb = PathBuf::from(p);
        if pb.is_absolute() {
            pb
        } else {
            cwd.join(pb)
        }
    };

    // --resume: reopen the most recent project, ignoring positionals.
    if resume {
        let root = session::last_project().unwrap_or_else(|| cwd.clone());
        return Cli::Run(Invocation { root, files: Vec::new(), restore: !fresh });
    }

    // Partition positionals into a directory (project root) and files.
    let mut dir: Option<PathBuf> = None;
    let mut files: Vec<PathBuf> = Vec::new();
    for p in &positionals {
        let path = abs(p);
        if path.is_dir() {
            if dir.is_none() {
                dir = Some(path);
            } else {
                eprintln!("plume: only one directory may be opened at a time");
                return Cli::Exit(2);
            }
        } else if path.is_file() {
            files.push(path);
        } else if positionals.len() == 1 {
            // A lone non-existent path: treat as a new file to create on save,
            // rooted at the current directory.
            files.push(path);
        } else {
            eprintln!("plume: no such file or directory: {p}");
            return Cli::Exit(1);
        }
    }

    let (root, restore) = match (dir, files.is_empty()) {
        // opened a folder (or nothing) → restore its session unless --new
        (Some(d), _) => (d, !fresh),
        (None, true) => (cwd, !fresh),
        // opened explicit files → root is cwd, don't auto-restore tabs
        (None, false) => (cwd, false),
    };
    Cli::Run(Invocation { root, files, restore })
}

fn print_paths() {
    let cfg = paths::config_file()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unavailable>".into());
    let st = paths::sessions_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unavailable>".into());
    println!("\nConfig:   {cfg}");
    println!("Sessions: {st}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str], cwd: &str) -> Invocation {
        match parse(args.iter().map(|s| s.to_string()), PathBuf::from(cwd)) {
            Cli::Run(i) => i,
            Cli::Exit(c) => panic!("unexpected exit {c}"),
        }
    }

    #[test]
    fn no_args_opens_cwd_and_restores() {
        let i = run(&[], "/tmp");
        assert_eq!(i.root, PathBuf::from("/tmp"));
        assert!(i.files.is_empty());
        assert!(i.restore);
    }

    #[test]
    fn new_flag_disables_restore() {
        let i = run(&["--new"], "/tmp");
        assert!(!i.restore);
    }

    #[test]
    fn nonexistent_single_path_becomes_new_file() {
        let i = run(&["newfile.txt"], "/tmp");
        assert_eq!(i.files, vec![PathBuf::from("/tmp/newfile.txt")]);
        assert!(!i.restore); // explicit file open, don't restore tabs
    }
}
