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

fn map_normal_mode(key: &KeyEvent, focus: FocusPane) -> Option<Action> {
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
        KeyCode::Left => Some(Action::SetFocus(focus_left(focus))),
        KeyCode::Right => Some(Action::SetFocus(focus_right(focus))),
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
        KeyCode::Char('r') => Some(Action::RefreshNow),
        KeyCode::Char('?') => Some(Action::ToggleHelp),
        KeyCode::Char('R') => Some(Action::MarkAllRead),
        KeyCode::Char('s') => Some(Action::RequestSummary),
        KeyCode::Char('m') | KeyCode::Esc => Some(Action::ShowMessages),
        _ => None,
    }
}

fn focus_left(focus: FocusPane) -> FocusPane {
    match focus {
        FocusPane::Guilds => FocusPane::Messages,
        FocusPane::Channels => FocusPane::Guilds,
        FocusPane::Messages | FocusPane::Input => FocusPane::Channels,
    }
}

fn focus_right(focus: FocusPane) -> FocusPane {
    match focus {
        FocusPane::Guilds => FocusPane::Channels,
        FocusPane::Channels => FocusPane::Messages,
        FocusPane::Messages | FocusPane::Input => FocusPane::Guilds,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_arrow_cycles_focus_across_main_panes() {
        assert_eq!(focus_left(FocusPane::Messages), FocusPane::Channels);
        assert_eq!(focus_left(FocusPane::Channels), FocusPane::Guilds);
        assert_eq!(focus_left(FocusPane::Guilds), FocusPane::Messages);
    }

    #[test]
    fn right_arrow_cycles_focus_across_main_panes() {
        assert_eq!(focus_right(FocusPane::Messages), FocusPane::Guilds);
        assert_eq!(focus_right(FocusPane::Guilds), FocusPane::Channels);
        assert_eq!(focus_right(FocusPane::Channels), FocusPane::Messages);
    }

    #[test]
    fn input_focus_uses_message_column_for_horizontal_navigation() {
        assert_eq!(focus_left(FocusPane::Input), FocusPane::Channels);
        assert_eq!(focus_right(FocusPane::Input), FocusPane::Guilds);
    }

    #[test]
    fn r_key_triggers_manual_refresh() {
        let event = Event::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert!(matches!(
            map_terminal_event(&event, InputMode::Normal, FocusPane::Messages),
            Some(Action::RefreshNow)
        ));
    }
}
