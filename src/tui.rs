use std::io::{self, Stdout};

use color_eyre::eyre::Result;
use crossterm::{
    cursor::Show,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

pub type CrosstermTerminal = Terminal<CrosstermBackend<Stdout>>;

/// Initialize the terminal: raw mode, alternate screen, hidden cursor.
pub fn init(mouse: bool) -> Result<CrosstermTerminal> {
    install_panic_hook(mouse);
    enable_raw_mode()?;
    let rollback = std::mem::DropGuard::new(mouse, |mouse| {
        let _ = restore(mouse);
    });
    execute!(io::stdout(), EnterAlternateScreen)?;
    if mouse {
        execute!(io::stdout(), EnableMouseCapture)?;
    }
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    terminal.clear()?;
    std::mem::DropGuard::dismiss(rollback);
    Ok(terminal)
}

/// Restore the terminal to its original state.
pub fn restore(mouse: bool) -> Result<()> {
    let mouse_result = if mouse {
        execute!(io::stdout(), DisableMouseCapture)
    } else {
        Ok(())
    };
    let screen_result = execute!(io::stdout(), LeaveAlternateScreen, Show);
    let raw_result = disable_raw_mode();
    mouse_result.and(screen_result).and(raw_result)?;
    Ok(())
}

/// Install a panic hook that restores the terminal before printing the panic.
fn install_panic_hook(mouse: bool) {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore(mouse);
        original_hook(panic_info);
    }));
}
