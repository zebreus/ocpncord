use crate::app::Action;
use crate::event::{KeyEvent, Modifiers, Scancode};
use alloc::string::String;

const LEADER_TIMEOUT_TICKS: u64 = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChord {
    leader_tick: Option<u64>,
}

impl KeyChord {
    pub fn new() -> Self {
        Self { leader_tick: None }
    }

    pub fn handle(&mut self, key: &KeyEvent, tick: u64) -> Option<Action> {
        self.check_timeout(tick);

        match self.leader_tick {
            None => self.handle_no_leader(key, tick),
            Some(_) => self.handle_with_leader(key),
        }
    }

    pub fn tick(&mut self, tick: u64) -> Option<Action> {
        self.check_timeout(tick);
        None
    }

    pub fn is_leader_active(&self) -> bool {
        self.leader_tick.is_some()
    }

    fn check_timeout(&mut self, tick: u64) {
        if let Some(lt) = self.leader_tick {
            if tick.saturating_sub(lt) >= LEADER_TIMEOUT_TICKS {
                self.leader_tick = None;
            }
        }
    }

    fn handle_no_leader(&mut self, key: &KeyEvent, tick: u64) -> Option<Action> {
        if key.modifiers.ctrl {
            match key.scancode {
                Scancode::Char('c') | Scancode::Char('C') => return Some(Action::Quit),
                Scancode::Char('x') | Scancode::Char('X') => {
                    self.leader_tick = Some(tick);
                    return None;
                }
                Scancode::Char('p') | Scancode::Char('P') => {
                    return Some(Action::OpenPalette);
                }
                _ => {}
            }
        }
        None
    }

    fn handle_with_leader(&mut self, key: &KeyEvent) -> Option<Action> {
        if key.modifiers != Modifiers::default() {
            return None;
        }
        self.leader_tick = None;
        match key.scancode {
            Scancode::Char('q') | Scancode::Char('Q') => Some(Action::Quit),
            Scancode::Char('l') | Scancode::Char('L') => {
                Some(Action::ExecuteCommand("/sessions".into()))
            }
            Scancode::Char('m') | Scancode::Char('M') => {
                Some(Action::ExecuteCommand("/models".into()))
            }
            Scancode::Char('h') | Scancode::Char('H') => {
                Some(Action::ExecuteCommand("/help".into()))
            }
            Scancode::Char('n') | Scancode::Char('N') => {
                Some(Action::ExecuteCommand("/new".into()))
            }
            Scancode::Char('t') | Scancode::Char('T') => Some(Action::OpenTerminal(String::new())),
            Scancode::Char('d') | Scancode::Char('D') => {
                Some(Action::ExecuteCommand("/diagnostics".into()))
            }
            Scancode::Char('o') | Scancode::Char('O') => {
                Some(Action::ExecuteCommand("/todos".into()))
            }
            _ => None,
        }
    }
}

impl Default for KeyChord {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl(key: char) -> KeyEvent {
        KeyEvent {
            scancode: Scancode::Char(key),
            modifiers: Modifiers {
                ctrl: true,
                shift: false,
                alt: false,
                meta: false,
            },
        }
    }

    fn key(key: char) -> KeyEvent {
        KeyEvent {
            scancode: Scancode::Char(key),
            modifiers: Modifiers::default(),
        }
    }

    #[test]
    fn ctrl_c_returns_quit() {
        let mut chord = KeyChord::new();
        assert_eq!(chord.handle(&ctrl('c'), 0), Some(Action::Quit));
    }

    #[test]
    fn ctrl_c_capital_also_quits() {
        let mut chord = KeyChord::new();
        assert_eq!(chord.handle(&ctrl('C'), 0), Some(Action::Quit));
    }

    #[test]
    fn ctrl_x_enters_leader_mode() {
        let mut chord = KeyChord::new();
        assert_eq!(chord.handle(&ctrl('x'), 0), None);
        assert!(chord.leader_tick.is_some());
    }

    #[test]
    fn leader_q_completes_quit_chord() {
        let mut chord = KeyChord::new();
        chord.handle(&ctrl('x'), 0);
        assert_eq!(chord.handle(&key('q'), 1), Some(Action::Quit));
        assert!(chord.leader_tick.is_none());
    }

    #[test]
    fn leader_capital_q_also_quits() {
        let mut chord = KeyChord::new();
        chord.handle(&ctrl('x'), 0);
        assert_eq!(chord.handle(&key('Q'), 1), Some(Action::Quit));
    }

    #[test]
    fn leader_times_out() {
        let mut chord = KeyChord::new();
        chord.handle(&ctrl('x'), 0);
        assert!(chord.leader_tick.is_some());

        chord.tick(40);
        assert!(
            chord.leader_tick.is_none(),
            "leader should timeout at tick 40"
        );
    }

    #[test]
    fn leader_does_not_timeout_early() {
        let mut chord = KeyChord::new();
        chord.handle(&ctrl('x'), 0);
        chord.tick(39);
        assert!(
            chord.leader_tick.is_some(),
            "leader should still be active at tick 39"
        );
    }

    #[test]
    fn handle_also_checks_timeout() {
        let mut chord = KeyChord::new();
        chord.handle(&ctrl('x'), 0);
        chord.handle(&key('q'), 41);
        assert!(
            chord.leader_tick.is_none(),
            "leader should have timed out before key press"
        );
    }

    #[test]
    fn modifier_during_leader_keeps_leader_active() {
        let mut chord = KeyChord::new();
        chord.handle(&ctrl('x'), 0);
        let shift_key = KeyEvent {
            scancode: Scancode::Char('q'),
            modifiers: Modifiers {
                ctrl: false,
                shift: true,
                alt: false,
                meta: false,
            },
        };
        assert_eq!(chord.handle(&shift_key, 1), None);
        assert!(
            chord.leader_tick.is_some(),
            "leader should remain active after modifier"
        );
    }

    #[test]
    fn leader_h_opens_help_modal() {
        let mut chord = KeyChord::new();
        chord.handle(&ctrl('x'), 0);
        assert_eq!(
            chord.handle(&key('h'), 1),
            Some(Action::ExecuteCommand("/help".into()))
        );
        assert!(chord.leader_tick.is_none());
    }

    #[test]
    fn leader_d_toggles_diagnostics_panel() {
        let mut chord = KeyChord::new();
        chord.handle(&ctrl('x'), 0);
        assert_eq!(
            chord.handle(&key('d'), 1),
            Some(Action::ExecuteCommand("/diagnostics".into()))
        );
    }

    #[test]
    fn leader_o_toggles_todos_panel() {
        let mut chord = KeyChord::new();
        chord.handle(&ctrl('x'), 0);
        assert_eq!(
            chord.handle(&key('o'), 1),
            Some(Action::ExecuteCommand("/todos".into()))
        );
    }

    #[test]
    fn leader_capital_h_also_opens_help_modal() {
        let mut chord = KeyChord::new();
        chord.handle(&ctrl('x'), 0);
        assert_eq!(
            chord.handle(&key('H'), 1),
            Some(Action::ExecuteCommand("/help".into()))
        );
    }

    #[test]
    fn leader_n_starts_new_session_command() {
        let mut chord = KeyChord::new();
        chord.handle(&ctrl('x'), 0);
        assert_eq!(
            chord.handle(&key('n'), 1),
            Some(Action::ExecuteCommand("/new".into()))
        );
    }

    #[test]
    fn unknown_keypress_does_nothing() {
        let mut chord = KeyChord::new();
        assert_eq!(chord.handle(&key('a'), 0), None);
        assert!(chord.leader_tick.is_none());
    }
}
