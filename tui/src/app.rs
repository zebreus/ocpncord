use opencode_backend::Backend;

/// Top-level application state.
pub struct App<B: Backend> {
    backend: B,
    screen: Screen,
}

pub enum Screen {
    SessionList,
    Chat,
}

impl<B: Backend> App<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            screen: Screen::SessionList,
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }
}
