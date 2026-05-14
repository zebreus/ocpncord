use alloc::string::String;
use alloc::string::ToString;

use opencode_backend::Backend;

use crate::chat::Chat;
use crate::event::Event;
use crate::key_chord::KeyChord;
use crate::prompt_bar::PromptBar;
use crate::screen::{Action, Screen, ScreenId};
use crate::start_page::StartPage;
use crate::theme::Theme;
use ratatui_core::layout::Rect;
use ratatui_core::text::Text;
use ratatui_core::widgets::Widget;

/// Top-level application state.
pub struct App<B: Backend> {
    backend: B,
    active_screen: ScreenId,
    theme: Theme,
    key_chord: KeyChord,
    tick: u64,
    prompt_bar: PromptBar,
    active_session: Option<opencode_backend::Session>,
    draft: Option<String>,
    error: Option<String>,
}

impl<B: Backend> App<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            active_screen: ScreenId::StartPage,
            theme: Theme::default(),
            key_chord: KeyChord::new(),
            tick: 0,
            prompt_bar: PromptBar::new(),
            active_session: None,
            draft: None,
            error: None,
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

    pub fn draft(&self) -> Option<&str> {
        self.draft.as_deref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn active_session(&self) -> Option<&opencode_backend::Session> {
        self.active_session.as_ref()
    }

    /// Returns `false` when the application should quit.
    pub async fn handle_event(&mut self, event: Event) -> bool {
        match event {
            Event::Key(ref key) => {
                self.error = None;

                if let Some(action) = self.key_chord.handle(key, self.tick) {
                    return self.apply_action(Some(action)).await;
                }

                if let Some(action) = self.prompt_bar.handle_key(key) {
                    match action {
                        Action::SendMessage => {
                            return self.handle_send_message().await;
                        }
                        _ => {}
                    }
                }
            }
            Event::Tick => {
                self.tick = self.tick.wrapping_add(1);
                if let Some(action) = self.key_chord.tick(self.tick) {
                    return self.apply_action(Some(action)).await;
                }
            }
            Event::Quit => return false,
        }
        true
    }

    async fn handle_send_message(&mut self) -> bool {
        let text = self.prompt_bar.text().to_string();

        if text == "/new" {
            self.prompt_bar.clear();
            self.active_session = None;
            self.draft = None;
            self.active_screen = ScreenId::StartPage;
            return true;
        }

        if self.active_session.is_none() {
            match self.backend.create_session("Chat", "").await {
                Ok(session) => {
                    self.active_session = Some(session);
                }
                Err(e) => {
                    self.error = Some(alloc::format!("{}", e));
                    return true;
                }
            }
        }

        self.draft = Some(text);
        self.prompt_bar.clear();
        self.active_screen = ScreenId::Chat;
        true
    }

    async fn apply_action(&mut self, action: Option<Action>) -> bool {
        match action {
            Some(Action::Quit) => return false,
            Some(Action::SwitchScreen(id)) => self.active_screen = id,
            _ => {}
        }
        true
    }

    pub fn render(&self, frame: &mut ratatui_core::terminal::Frame) {
        match self.active_screen {
            ScreenId::StartPage => {
                StartPage.render(frame, &self.theme);
                let area = frame.area();
                let prompt_area = Rect::new(
                    area.x + area.width.saturating_sub(50) / 2,
                    area.height.saturating_sub(8),
                    50.min(area.width),
                    1,
                );
                self.prompt_bar.render(prompt_area, frame, &self.theme);
            }
            ScreenId::Chat => {
                Chat.render(frame, &self.theme);
                let area = frame.area();
                let prompt_area = Rect::new(area.x, area.height.saturating_sub(1), area.width, 1);
                self.prompt_bar.render(prompt_area, frame, &self.theme);
            }
        }

        if let Some(ref err) = self.error {
            let area = frame.area();
            let msg = alloc::format!(" Error: {} ", err);
            let msg_width = msg.len() as u16;
            let x = (area.width.saturating_sub(msg_width)) / 2;
            let err_area = Rect::new(x, area.height / 2, msg_width.min(area.width), 1);
            Text::from(msg.as_str())
                .style(self.theme.text_error)
                .render(err_area, frame.buffer_mut());
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

    fn run<B: Backend>(app: &mut App<B>, event: Event) -> bool {
        futures::executor::block_on(app.handle_event(event))
    }

    #[test]
    fn ctrl_c_quits() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        assert!(!run(&mut app, ctrl('c')));
    }

    #[test]
    fn ctrl_x_q_quits() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        run(&mut app, ctrl('x'));
        assert!(!run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Char('q'),
                modifiers: Modifiers::default(),
            })
        ));
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
        assert!(run(&mut app, char_key('a')));
        assert!(run(&mut app, char_key('b')));
    }

    #[test]
    fn leader_times_out_after_ticks() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        run(&mut app, ctrl('x'));
        for _ in 0..40 {
            run(&mut app, Event::Tick);
        }
        assert!(
            run(&mut app, char_key('q')),
            "q after timeout should not quit"
        );
    }

    #[test]
    fn explicit_quit_event_quits() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        assert!(!run(&mut app, Event::Quit));
    }

    #[test]
    fn session_creation_error_shows_error_and_stays_on_start_page() {
        let mut backend = MockBackend::default();
        backend.fail_create_session = Some(opencode_backend::BackendError::Api {
            status: 500,
            message: "server error".into(),
        });
        let mut app = App::new(backend);

        run(&mut app, char_key('h'));
        let running = run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Enter,
                modifiers: Modifiers::default(),
            }),
        );
        assert!(running);
        assert_eq!(
            app.active_screen(),
            ScreenId::StartPage,
            "should stay on StartPage on error"
        );
        assert!(
            app.error().unwrap_or("").contains("server error"),
            "error should contain failure message"
        );
    }

    #[test]
    fn typing_enter_creates_session_and_switches_to_chat() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        assert_eq!(app.active_screen(), ScreenId::StartPage);

        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        let running = run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Enter,
                modifiers: Modifiers::default(),
            }),
        );
        assert!(running);
        assert_eq!(
            app.backend().sessions.len(),
            1,
            "a session should have been created"
        );
        assert_eq!(app.active_screen(), ScreenId::Chat);
        assert_eq!(app.draft(), Some("hi"));
    }

    #[test]
    fn enter_on_empty_input_does_nothing() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        let running = run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Enter,
                modifiers: Modifiers::default(),
            }),
        );
        assert!(running);

        assert_eq!(app.active_screen(), ScreenId::StartPage);
        assert_eq!(app.backend().sessions.len(), 0);
    }

    #[test]
    fn new_command_resets_to_start_page() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Enter,
                modifiers: Modifiers::default(),
            }),
        );
        assert_eq!(app.active_screen(), ScreenId::Chat);

        run(&mut app, char_key('/'));
        run(&mut app, char_key('n'));
        run(&mut app, char_key('e'));
        run(&mut app, char_key('w'));
        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Enter,
                modifiers: Modifiers::default(),
            }),
        );

        assert_eq!(app.active_screen(), ScreenId::StartPage);
    }
}
