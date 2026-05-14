use core::convert::Infallible;

use crossterm::cursor::{Hide, Show};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, size,
};
use crossterm::{execute, queue};
use opencode_backend_opencode::OpenCodeBackend;
use opencode_tui::Event;
use opencode_tui::{App, KeyEvent, Modifiers, Scancode};
use ratatui_core::backend::Backend;
use ratatui_core::buffer::Cell;
use ratatui_core::layout::{Position, Size};
use ratatui_core::terminal::Terminal;
use std::io::{stdout, Write};
use tokio::time::{interval, Duration};

struct CrosstermBackend;

impl CrosstermBackend {
    fn new() -> Self {
        Self
    }
}

impl Backend for CrosstermBackend {
    type Error = Infallible;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let mut stdout = stdout();
        for (x, y, cell) in content {
            let symbol = cell.symbol();
            if !symbol.is_empty() && symbol != " " {
                let _ = queue!(
                    stdout,
                    crossterm::cursor::MoveTo(x, y),
                    crossterm::style::Print(symbol),
                );
            }
        }
        let _ = stdout.flush();
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        let _ = execute!(stdout(), Hide);
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        let _ = execute!(stdout(), Show);
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        let (x, y) = crossterm::cursor::position().unwrap_or((0, 0));
        Ok(Position { x, y })
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        let pos = position.into();
        let _ = execute!(stdout(), crossterm::cursor::MoveTo(pos.x, pos.y));
        Ok(())
    }

    fn size(&self) -> Result<Size, Self::Error> {
        let (w, h) = size().unwrap_or((80, 24));
        Ok(Size::new(w, h))
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        let _ = stdout().flush();
        Ok(())
    }

    fn set_cursor(&mut self, x: u16, y: u16) -> Result<(), Self::Error> {
        let _ = execute!(stdout(), crossterm::cursor::MoveTo(x, y));
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        let _ = execute!(stdout(), crossterm::terminal::Clear(crossterm::terminal::ClearType::All));
        Ok(())
    }

    fn clear_region(&mut self, clear_type: ratatui_core::backend::ClearType) -> Result<(), Self::Error> {
        let ct = match clear_type {
            ratatui_core::backend::ClearType::All => crossterm::terminal::ClearType::All,
            ratatui_core::backend::ClearType::AfterCursor => crossterm::terminal::ClearType::FromCursorDown,
            ratatui_core::backend::ClearType::BeforeCursor => crossterm::terminal::ClearType::FromCursorUp,
            ratatui_core::backend::ClearType::CurrentLine => crossterm::terminal::ClearType::CurrentLine,
            ratatui_core::backend::ClearType::UntilNewLine => crossterm::terminal::ClearType::UntilNewLine,
        };
        let _ = execute!(stdout(), crossterm::terminal::Clear(ct));
        Ok(())
    }

    fn window_size(&mut self) -> Result<ratatui_core::backend::WindowSize, Self::Error> {
        let (w, h) = size().unwrap_or((80, 24));
        Ok(ratatui_core::backend::WindowSize {
            columns_rows: Size::new(w, h),
            pixels: Size::new(0, 0),
        })
    }
}

fn translate_crossterm_event(event: crossterm::event::Event) -> Option<Event> {
    match event {
        crossterm::event::Event::Key(key) => {
            let modifiers = Modifiers {
                ctrl: key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL),
                shift: key.modifiers.contains(crossterm::event::KeyModifiers::SHIFT),
                alt: key.modifiers.contains(crossterm::event::KeyModifiers::ALT),
                meta: false,
            };
            let scancode = match key.code {
                crossterm::event::KeyCode::Char(c) => Scancode::Char(c),
                crossterm::event::KeyCode::Enter => Scancode::Enter,
                crossterm::event::KeyCode::Esc => Scancode::Escape,
                crossterm::event::KeyCode::Backspace => Scancode::Backspace,
                crossterm::event::KeyCode::Tab => Scancode::Tab,
                crossterm::event::KeyCode::Up => Scancode::Up,
                crossterm::event::KeyCode::Down => Scancode::Down,
                crossterm::event::KeyCode::Left => Scancode::Left,
                crossterm::event::KeyCode::Right => Scancode::Right,
                crossterm::event::KeyCode::Home => Scancode::Home,
                crossterm::event::KeyCode::End => Scancode::End,
                crossterm::event::KeyCode::PageUp => Scancode::PageUp,
                crossterm::event::KeyCode::PageDown => Scancode::PageDown,
                crossterm::event::KeyCode::Delete => Scancode::Delete,
                crossterm::event::KeyCode::F(n) => Scancode::F(n),
                _ => return None,
            };
            Some(Event::Key(KeyEvent { scancode, modifiers }))
        }
        crossterm::event::Event::Resize(_w, _h) => {
            Some(Event::Tick)
        }
        _ => None,
    }
}

#[tokio::main]
async fn main() {
    let backend = OpenCodeBackend::new_std("http://localhost:4096");
    let app = App::new(backend);

    let _ = enable_raw_mode();
    let _ = execute!(stdout(), EnterAlternateScreen);
    let crossterm_backend = CrosstermBackend::new();
    let mut terminal = Terminal::new(crossterm_backend).unwrap();

    let mut app = app;
    let mut running = true;
    let mut tick_interval = interval(Duration::from_millis(50));

    while running {
        tokio::select! {
            maybe_event = read_crossterm_event() => {
                if let Some(event) = maybe_event {
                    running = app.handle_event(event).await;
                }
            }
            _ = tick_interval.tick() => {
                running = app.handle_event(Event::Tick).await;
            }
        }

        let _ = terminal.draw(|frame| {
            app.render(frame);
        });
    }

    let _ = execute!(stdout(), LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

async fn read_crossterm_event() -> Option<Event> {
    tokio::task::spawn_blocking(|| {
        match crossterm::event::read() {
            Ok(event) => translate_crossterm_event(event),
            Err(_) => None,
        }
    })
    .await
    .ok()
    .flatten()
}
