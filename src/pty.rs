//! A real embedded terminal: a PTY running the user's shell, its byte stream
//! parsed by vt100 into a screen grid that the bottom panel renders. Kept
//! cross-platform via portable-pty (ConPTY on Windows, forkpty on unix).
//!
//! A background thread pumps the PTY's output into the parser (behind a mutex
//! shared with the renderer) and flips a `dirty` flag so the main loop knows to
//! redraw. Input flows the other way: `key_to_bytes` turns a key event into the
//! escape sequence a terminal would send, and `write_input` feeds it to the shell.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Lines of history kept above the visible screen.
const SCROLLBACK: usize = 5000;

pub struct PtyTerminal {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    /// Set by the reader thread when new output arrives; cleared each draw.
    dirty: Arc<AtomicBool>,
    /// Set once the shell exits (or the PTY errors out).
    dead: Arc<AtomicBool>,
    rows: u16,
    cols: u16,
}

impl PtyTerminal {
    /// Spawn the platform default shell, rooted at `cwd`, sized `rows`×`cols`.
    pub fn spawn(rows: u16, cols: u16, cwd: &Path) -> std::io::Result<PtyTerminal> {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        let mut cmd = CommandBuilder::new_default_prog();
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        // The child owns the slave now; drop ours so the master reader sees EOF
        // the instant the shell exits.
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, SCROLLBACK)));
        let dirty = Arc::new(AtomicBool::new(true));
        let dead = Arc::new(AtomicBool::new(false));

        {
            let parser = Arc::clone(&parser);
            let dirty = Arc::clone(&dirty);
            let dead = Arc::clone(&dead);
            thread::spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => {
                            dead.store(true, Ordering::Release);
                            dirty.store(true, Ordering::Release);
                            break;
                        }
                        Ok(n) => {
                            if let Ok(mut p) = parser.lock() {
                                p.process(&buf[..n]);
                            }
                            dirty.store(true, Ordering::Release);
                        }
                    }
                }
            });
        }

        Ok(PtyTerminal { parser, writer, master: pair.master, child, dirty, dead, rows, cols })
    }

    pub fn is_dead(&self) -> bool {
        self.dead.load(Ordering::Acquire)
    }

    /// Returns whether new output has arrived since the last call, clearing the flag.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    /// Feed bytes (already terminal-encoded) to the shell.
    pub fn write_input(&mut self, bytes: &[u8]) {
        if self.writer.write_all(bytes).and_then(|_| self.writer.flush()).is_err() {
            self.dead.store(true, Ordering::Release);
        }
    }

    /// Match the PTY and parser to the panel's current inner size.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        let _ = self
            .master
            .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        self.parser().screen_mut().set_size(rows, cols);
        self.dirty.store(true, Ordering::Release);
    }

    /// Lock the parser for rendering. Poisoning is recovered from — a panicked
    /// reader thread shouldn't take the whole editor down.
    pub fn parser(&self) -> MutexGuard<'_, vt100::Parser> {
        self.parser.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Move the viewport through scrollback. Positive `lines` scrolls up into
    /// history, negative scrolls back down toward the live bottom (offset 0).
    /// vt100 clamps to the real scrollback length for us.
    pub fn scroll(&self, lines: isize) {
        let mut p = self.parser();
        let cur = p.screen().scrollback() as isize;
        let next = (cur + lines).max(0) as usize;
        if next != cur as usize {
            p.screen_mut().set_scrollback(next);
            self.dirty.store(true, Ordering::Release);
        }
    }

    /// Snap the viewport back to the live bottom of the output.
    pub fn scroll_to_bottom(&self) {
        let mut p = self.parser();
        if p.screen().scrollback() != 0 {
            p.screen_mut().set_scrollback(0);
            self.dirty.store(true, Ordering::Release);
        }
    }

    /// Rows currently scrolled up from the live bottom (0 = at the bottom).
    pub fn scrollback(&self) -> usize {
        self.parser().screen().scrollback()
    }
}

impl Drop for PtyTerminal {
    fn drop(&mut self) {
        // Hang up the shell so it doesn't linger after the panel/app closes.
        let _ = self.child.kill();
    }
}

/// Translate a key event into the bytes a real terminal would send to the PTY.
/// Returns `None` for keys with no terminal meaning (e.g. bare modifiers).
pub fn key_to_bytes(key: &KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    // xterm modifier parameter: 1 + (shift) + 2·(alt) + 4·(ctrl).
    let modn = 1 + shift as u8 + (alt as u8) * 2 + (ctrl as u8) * 4;

    let mut out: Vec<u8> = Vec::new();
    match key.code {
        KeyCode::Char(c) => {
            if ctrl {
                // Control byte for the @A–Z[\]^_ group, plus Space and ?.
                let up = (c.to_ascii_uppercase() as u32) & 0x7f;
                let ctl = match up {
                    0x40..=0x5f => Some((up & 0x1f) as u8), // @ A..Z [ \ ] ^ _
                    0x20 => Some(0x00),                     // Ctrl+Space -> NUL
                    0x3f => Some(0x7f),                     // Ctrl+? -> DEL
                    _ => None,
                };
                if let Some(byte) = ctl {
                    if alt {
                        out.push(0x1b);
                    }
                    out.push(byte);
                    return Some(out);
                }
                // Unrecognized Ctrl+<char>: fall through and send the char itself.
            }
            if alt {
                out.push(0x1b); // Alt = ESC prefix
            }
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            Some(out)
        }
        KeyCode::Enter => Some(b"\r".to_vec()),
        KeyCode::Backspace => Some(if alt { vec![0x1b, 0x7f] } else { vec![0x7f] }),
        KeyCode::Tab => Some(b"\t".to_vec()),
        KeyCode::BackTab => Some(b"\x1b[Z".to_vec()),
        KeyCode::Esc => Some(b"\x1b".to_vec()),
        KeyCode::Delete => Some(b"\x1b[3~".to_vec()),
        KeyCode::Insert => Some(b"\x1b[2~".to_vec()),
        KeyCode::Up => Some(csi_cursor(b'A', modn)),
        KeyCode::Down => Some(csi_cursor(b'B', modn)),
        KeyCode::Right => Some(csi_cursor(b'C', modn)),
        KeyCode::Left => Some(csi_cursor(b'D', modn)),
        KeyCode::Home => Some(csi_cursor(b'H', modn)),
        KeyCode::End => Some(csi_cursor(b'F', modn)),
        KeyCode::PageUp => Some(csi_tilde(5, modn)),
        KeyCode::PageDown => Some(csi_tilde(6, modn)),
        KeyCode::F(n) => function_key(n),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn press(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn shell_echoes_typed_command() {
        let mut term =
            PtyTerminal::spawn(24, 80, Path::new(".")).expect("spawn shell");
        // Type `printf plumeok` then Enter, character by character.
        for ch in "printf plumeok".chars() {
            let bytes = key_to_bytes(&press(KeyCode::Char(ch), KeyModifiers::NONE)).unwrap();
            term.write_input(&bytes);
        }
        term.write_input(&key_to_bytes(&press(KeyCode::Enter, KeyModifiers::NONE)).unwrap());

        // Poll the parsed screen until the output shows up (or we give up).
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = String::new();
        while Instant::now() < deadline {
            seen = term.parser().screen().contents();
            if seen.contains("plumeok") {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        assert!(seen.contains("plumeok"), "shell output never appeared; got:\n{seen}");
    }

    fn wait_until(term: &PtyTerminal, pred: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = String::new();
        while Instant::now() < deadline {
            seen = term.parser().screen().contents();
            if pred(&seen) {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        seen
    }

    fn type_line(term: &mut PtyTerminal, text: &str) {
        for ch in text.chars() {
            let b = key_to_bytes(&press(KeyCode::Char(ch), KeyModifiers::NONE)).unwrap();
            term.write_input(&b);
        }
        term.write_input(&key_to_bytes(&press(KeyCode::Enter, KeyModifiers::NONE)).unwrap());
    }

    #[test]
    fn scrollback_reveals_earlier_output() {
        // A short screen forces most output into scrollback. HEADXYZ prints
        // first (scrolls off the top), TAILXYZ last (stays at the bottom).
        let mut term = PtyTerminal::spawn(6, 40, Path::new(".")).expect("spawn shell");
        type_line(&mut term, "echo HEADXYZ; seq 1 60; echo TAILXYZ");

        // Completion = tail is on screen and the echoed command has scrolled off.
        let bottom = wait_until(&term, |s| s.contains("TAILXYZ") && !s.contains("; seq 1 60;"));
        assert!(bottom.contains("TAILXYZ"), "tail missing; got:\n{bottom}");
        assert!(!bottom.contains("HEADXYZ"), "head should have scrolled off the bottom view:\n{bottom}");
        assert_eq!(term.scrollback(), 0);

        // Scroll all the way up; the oldest output (HEADXYZ) reappears.
        term.scroll(1000);
        assert!(term.scrollback() > 0, "nothing was scrolled back");
        let top = term.parser().screen().contents();
        assert!(top.contains("HEADXYZ"), "early output not revealed after scrolling up; got:\n{top}");

        // Snapping to bottom returns to the live tail.
        term.scroll_to_bottom();
        assert_eq!(term.scrollback(), 0);
        assert!(term.parser().screen().contents().contains("TAILXYZ"));
    }

    #[test]
    fn ctrl_c_maps_to_etx() {
        let bytes = key_to_bytes(&press(KeyCode::Char('c'), KeyModifiers::CONTROL)).unwrap();
        assert_eq!(bytes, vec![0x03]);
    }

    #[test]
    fn arrow_keys_send_csi() {
        let up = key_to_bytes(&press(KeyCode::Up, KeyModifiers::NONE)).unwrap();
        assert_eq!(up, b"\x1b[A");
        let ctrl_right =
            key_to_bytes(&press(KeyCode::Right, KeyModifiers::CONTROL)).unwrap();
        assert_eq!(ctrl_right, b"\x1b[1;5C");
    }
}

/// Cursor/edit keys: `ESC [ <final>`, or `ESC [ 1 ; <mod> <final>` when modified.
fn csi_cursor(final_byte: u8, modn: u8) -> Vec<u8> {
    if modn > 1 {
        format!("\x1b[1;{}{}", modn, final_byte as char).into_bytes()
    } else {
        vec![0x1b, b'[', final_byte]
    }
}

/// Tilde keys (PageUp/Down etc.): `ESC [ <n> ~`, or `ESC [ <n> ; <mod> ~`.
fn csi_tilde(n: u8, modn: u8) -> Vec<u8> {
    if modn > 1 {
        format!("\x1b[{};{}~", n, modn).into_bytes()
    } else {
        format!("\x1b[{}~", n).into_bytes()
    }
}

/// xterm sequences for F1–F12 (modifiers ignored — rarely used in a shell).
fn function_key(n: u8) -> Option<Vec<u8>> {
    let seq: &[u8] = match n {
        1 => b"\x1bOP",
        2 => b"\x1bOQ",
        3 => b"\x1bOR",
        4 => b"\x1bOS",
        5 => b"\x1b[15~",
        6 => b"\x1b[17~",
        7 => b"\x1b[18~",
        8 => b"\x1b[19~",
        9 => b"\x1b[20~",
        10 => b"\x1b[21~",
        11 => b"\x1b[23~",
        12 => b"\x1b[24~",
        _ => return None,
    };
    Some(seq.to_vec())
}
