mod app;
mod buffer;
mod cli;
mod config;
mod explorer;
mod keymap;
mod keys;
mod palette;
mod paths;
mod search;
mod session;
mod syntax;
mod theme;
mod ui;

use std::io;
use std::path::PathBuf;
use std::process::ExitCode;
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

fn main() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let inv = match cli::parse(std::env::args().skip(1), cwd) {
        cli::Cli::Run(inv) => inv,
        cli::Cli::Exit(code) => return ExitCode::from(code as u8),
    };
    match tui(inv) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("plume: {e}");
            ExitCode::FAILURE
        }
    }
}

fn tui(inv: cli::Invocation) -> io::Result<()> {
    let mut app = App::new(inv.root.clone());
    // First run writes a documented default config; otherwise load the user's.
    config::ensure_template();
    app.apply_config(config::load());

    // Restore the saved session for this project (tabs, cursors, tree state),
    // then open any files named explicitly on the command line.
    if inv.restore {
        if let Some(sess) = session::load(&inv.root) {
            app.restore_session(sess);
        }
    }
    for f in &inv.files {
        app.open_file(f);
    }
    session::set_last_project(&inv.root);

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

    // Persist the workspace so the next launch in this folder resumes here.
    session::save(&app.session_snapshot());
    session::set_last_project(&app.root);

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
