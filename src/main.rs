mod app;
mod buffer;
mod explorer;
mod keys;
mod palette;
mod search;
mod syntax;
mod theme;
mod ui;

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement, EnterAlternateScreen,
    LeaveAlternateScreen,
};
use ratatui::Terminal;

use crate::app::App;

fn restore_terminal(kitty: bool) {
    let mut stdout = io::stdout();
    if kitty {
        let _ = execute!(stdout, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(stdout, DisableBracketedPaste, DisableMouseCapture, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

fn main() -> io::Result<()> {
    // Resolve project root (and optionally a file to open) from argv.
    let arg = std::env::args().nth(1);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (root, open_file) = match arg {
        Some(a) => {
            let p = PathBuf::from(&a);
            let p = if p.is_absolute() { p } else { cwd.join(p) };
            if p.is_file() {
                let parent = p.parent().map(PathBuf::from).unwrap_or(cwd);
                (parent, Some(p))
            } else if p.is_dir() {
                (p, None)
            } else {
                eprintln!("plume: no such file or directory: {a}");
                std::process::exit(1);
            }
        }
        None => (cwd, None),
    };

    let mut app = App::new(root);
    if let Some(f) = open_file {
        app.open_file(&f);
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    // Kitty keyboard protocol (when supported) disambiguates e.g.
    // Ctrl+Shift+F from Ctrl+F.
    let kitty = supports_keyboard_enhancement().unwrap_or(false);
    if kitty {
        let _ = execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }

    // Restore the terminal even if we panic.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal(kitty);
        default_hook(info);
    }));

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run(&mut terminal, &mut app);

    restore_terminal(kitty);
    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    // Responsiveness model: draw at most once per loop iteration, and only
    // when something actually changed. All queued input events are drained
    // before drawing, so a burst (mouse drag, key repeat, paste) costs one
    // redraw instead of one per event.
    let mut dirty = true;
    loop {
        if dirty {
            terminal.draw(|f| ui::draw(f, app))?;
            dirty = false;
        }
        if event::poll(Duration::from_millis(100))? {
            let mut budget = 256; // cap so a continuous flood can't starve rendering
            loop {
                match event::read()? {
                    Event::Key(key) => dirty |= keys::handle_key(app, key),
                    Event::Mouse(m) => dirty |= keys::handle_mouse(app, m),
                    Event::Resize(..) => dirty = true,
                    Event::Paste(text) => {
                        if let Some(buf) = app.buf_mut() {
                            buf.insert_str(&text);
                            app.after_editor_action();
                        }
                        dirty = true;
                    }
                    _ => {}
                }
                budget -= 1;
                if budget == 0 || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        if app.should_quit {
            return Ok(());
        }
        dirty |= app.drain_async();
        dirty |= app.tick();
    }
}
