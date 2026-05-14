use opencode_backend::Backend;

use crate::event::Event;
use crate::key_chord::KeyChord;
use crate::screen::{Action, Screen, ScreenId};
use crate::start_page::StartPage;
use crate::theme::Theme;
use ratatui_core::terminal::Frame;

/// Top-level application state.
pub struct App<B: Backend> {
    backend: B,
    active_screen: ScreenId,
    theme: Theme,
    key_chord: KeyChord,
    tick: u64,
}

impl<B: Backend> App<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            active_screen: ScreenId::StartPage,
            theme: Theme::default(),
            key_chord: KeyChord::new(),
            tick: 0,
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn active_screen(&self) -> ScreenId {
        self.active_screen
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Returns `false` when the application should quit.
    pub fn handle_event(&mut self, event: Event) -> bool {
        match event {
            Event::Key(ref key) => {
                let action = self.key_chord.handle(key, self.tick);
                self.apply_action(action)
            }
            Event::Tick => {
                self.tick = self.tick.wrapping_add(1);
                let action = self.key_chord.tick(self.tick);
                self.apply_action(action)
            }
            Event::Quit => false,
        }
    }

    fn apply_action(&mut self, action: Option<Action>) -> bool {
        match action {
            Some(Action::Quit) => return false,
            Some(Action::SwitchScreen(id)) => self.active_screen = id,
            _ => {}
        }
        true
    }

    pub fn render(&self, frame: &mut Frame) {
        match self.active_screen {
            ScreenId::StartPage => StartPage.render(frame, &self.theme),
            ScreenId::Chat => { /* will be implemented in slice 2 */ }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyEvent, Modifiers, Scancode};
    use opencode_backend::mock::MockBackend;

    fn ctrl(key: char) -> Event {
        Event::Key(KeyEvent {
            scancode: Scancode::Char(key),
            modifiers: Modifiers {
                ctrl: true,
                shift: false,
                alt: false,
                meta: false,
            },
        })
    }

    fn char_key(ch: char) -> Event {
        Event::Key(KeyEvent {
            scancode: Scancode::Char(ch),
            modifiers: Modifiers::default(),
        })
    }

    #[test]
    fn ctrl_c_quits() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        assert!(!app.handle_event(ctrl('c')));
    }

    #[test]
    fn ctrl_x_q_quits() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        app.handle_event(ctrl('x'));
        assert!(!app.handle_event(Event::Key(KeyEvent {
            scancode: Scancode::Char('q'),
            modifiers: Modifiers::default(),
        })));
    }

    #[test]
    fn starts_on_start_page() {
        let backend = MockBackend::default();
        let app = App::new(backend);
        assert_eq!(app.active_screen(), ScreenId::StartPage);
    }

    #[test]
    fn non_quit_events_keep_running() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        assert!(app.handle_event(char_key('a')));
        assert!(app.handle_event(char_key('b')));
    }

    #[test]
    fn leader_times_out_after_ticks() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        app.handle_event(ctrl('x'));
        for _ in 0..40 {
            app.handle_event(Event::Tick);
        }
        assert!(
            app.handle_event(char_key('q')),
            "q after timeout should not quit"
        );
    }

    #[test]
    fn explicit_quit_event_quits() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        assert!(!app.handle_event(Event::Quit));
    }
}
