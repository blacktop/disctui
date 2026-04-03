use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::action::Action;
use crate::app::{FocusPane, InputMode};

/// Map a terminal event to an `Action`, given the current app mode and focus.
pub fn map_terminal_event(event: &Event, mode: InputMode, focus: FocusPane) -> Option<Action> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => map_key_event(key, mode, focus),
        Event::Resize(w, h) => Some(Action::Resize {
            width: *w,
            height: *h,
        }),
        _ => None,
    }
}

fn map_key_event(key: &KeyEvent, mode: InputMode, focus: FocusPane) -> Option<Action> {
    // Ctrl-C always quits
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(Action::Quit);
    }

    match mode {
        InputMode::Normal => map_normal_mode(key, focus),
        InputMode::Insert => map_insert_mode(key),
    }
}

fn map_normal_mode(key: &KeyEvent, _focus: FocusPane) -> Option<Action> {
    // Ctrl-D / Ctrl-U for half-page scroll
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('d') => Some(Action::ScrollDown(15)),
            KeyCode::Char('u') => Some(Action::ScrollUp(15)),
            _ => None,
        };
    }

    match key.code {
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('j') | KeyCode::Down => Some(Action::MoveDown),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::MoveUp),
        KeyCode::PageDown => Some(Action::ScrollDown(30)),
        KeyCode::PageUp => Some(Action::ScrollUp(30)),
        KeyCode::Char('g') => Some(Action::JumpTop),
        KeyCode::Char('G') => Some(Action::JumpBottom),
        KeyCode::Tab => Some(Action::FocusNext),
        KeyCode::Char('1') => Some(Action::SetFocus(FocusPane::Guilds)),
        KeyCode::Char('2') => Some(Action::SetFocus(FocusPane::Channels)),
        KeyCode::Char('3') => Some(Action::SetFocus(FocusPane::Messages)),
        KeyCode::BackTab => Some(Action::FocusPrev),
        KeyCode::Enter => Some(Action::OpenSelected),
        KeyCode::Char('i') => Some(Action::EnterInsert),
        KeyCode::Char('?') => Some(Action::ToggleHelp),
        KeyCode::Char('R') => Some(Action::MarkAllRead),
        KeyCode::Char('s') => Some(Action::RequestSummary),
        KeyCode::Char('m') | KeyCode::Esc => Some(Action::ShowMessages),
        _ => None,
    }
}

fn map_insert_mode(key: &KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc => Some(Action::ExitInsert),
        KeyCode::Enter if !key.modifiers.contains(KeyModifiers::SHIFT) => {
            Some(Action::SendCurrentMessage)
        }
        // All other keys are handled by the textarea widget directly
        _ => None,
    }
}
