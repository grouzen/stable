use std::io::{stdout, Stdout};
use std::panic;

use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

pub fn enter_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    Ok(terminal)
}

pub fn leave_terminal() -> Result<()> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    Ok(())
}

pub fn install_panic_hook() {
    let original = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = leave_terminal();
        original(info);
    }));
}

pub fn run<F>(mut f: F) -> Result<()>
where
    F: FnMut(&mut Tui) -> Result<bool>,
{
    install_panic_hook();
    let mut terminal = enter_terminal()?;
    loop {
        let should_quit = f(&mut terminal)?;
        if should_quit {
            break;
        }
    }
    leave_terminal()?;
    Ok(())
}
