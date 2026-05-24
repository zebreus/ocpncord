use core::convert::Infallible;
use core::future::Future;
use core::net::IpAddr;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::fs::OpenOptions;
use std::io::{stdout, Write};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use futures::Stream;
use log::{LevelFilter, Log, Metadata, Record};

use clap::Parser;
use crossterm::cursor::{Hide, Show};
use crossterm::style::{
    Attribute, Color as CrosstermColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, size, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{execute, queue};
use embedded_io_async::{ErrorType, Read};
use embedded_nal_async::{AddrType, Dns, TcpConnect};
use ocpncord_backend_opencode::OpenCodeBackend;
use ocpncord_tui::Event;
use ocpncord_tui::{App, KeyEvent, Modifiers, Scancode};
use ratatui_core::backend::Backend;
use ratatui_core::buffer::Cell;
use ratatui_core::layout::{Position, Size};
use ratatui_core::style::{Color, Modifier};
use ratatui_core::terminal::Terminal;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration, Interval};

struct TuiLogger {
    file: Mutex<Option<std::fs::File>>,
}

impl Log for TuiLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if let Ok(mut guard) = self.file.lock() {
            if guard.is_none() {
                *guard = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("/tmp/ocpncord.log")
                    .ok();
            }
            if let Some(ref mut f) = *guard {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default();
                let h = (now.as_secs() / 3600) % 24;
                let m = (now.as_secs() / 60) % 60;
                let s = now.as_secs() % 60;
                let ns = now.subsec_nanos() / 100_000;
                let _ = writeln!(
                    f,
                    "[{h:02}:{m:02}:{s:02}.{ns:04} TUI] [{}] {}",
                    record.level(),
                    record.args()
                );
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.file.lock() {
            if let Some(ref mut f) = *guard {
                let _ = f.flush();
            }
        }
    }
}

static LOGGER: TuiLogger = TuiLogger {
    file: Mutex::new(None),
};

/// A TCP transport over tokio `TcpStream`.
struct StdTcp;

impl TcpConnect for StdTcp {
    type Error = std::io::Error;
    type Connection<'a> = StdTcpStream;

    async fn connect<'a>(
        &'a self,
        remote: core::net::SocketAddr,
    ) -> Result<Self::Connection<'a>, Self::Error> {
        let stream = tokio::net::TcpStream::connect(remote).await?;
        Ok(StdTcpStream(stream))
    }
}

/// A DNS resolver over tokio.
struct StdDns;

impl Dns for StdDns {
    type Error = std::io::Error;

    async fn get_host_by_name(
        &self,
        host: &str,
        addr_type: AddrType,
    ) -> Result<IpAddr, Self::Error> {
        let addrs = tokio::net::lookup_host((host, 0)).await?;
        let addrs: Vec<std::net::SocketAddr> = addrs.collect();
        let addr = match addr_type {
            AddrType::IPv4 => addrs.iter().find(|a| a.is_ipv4()),
            AddrType::IPv6 => addrs.iter().find(|a| a.is_ipv6()),
            AddrType::Either => addrs
                .iter()
                .find(|a| a.is_ipv4())
                .or_else(|| addrs.iter().find(|a| a.is_ipv6())),
        };
        match addr {
            Some(a) => Ok(a.ip()),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no address found for host",
            )),
        }
    }

    async fn get_host_by_address(
        &self,
        _addr: IpAddr,
        _result: &mut [u8],
    ) -> Result<usize, Self::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "reverse DNS not supported",
        ))
    }
}

/// Wraps a tokio `TcpStream` to implement `embedded-io-async` traits.
struct StdTcpStream(tokio::net::TcpStream);

impl ErrorType for StdTcpStream {
    type Error = std::io::Error;
}

impl Read for StdTcpStream {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.read(buf).await
    }
}

impl embedded_io_async::Write for StdTcpStream {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.0.write(buf).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.0.flush().await
    }
}

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
            let modifier = cell.modifier;
            if !symbol.is_empty() {
                let _ = queue!(
                    stdout,
                    crossterm::cursor::MoveTo(x, y),
                    SetAttribute(Attribute::Reset),
                    SetForegroundColor(to_crossterm_color(cell.fg)),
                    SetBackgroundColor(to_crossterm_color(cell.bg)),
                );
                queue_modifiers(&mut stdout, modifier);
                let _ = queue!(stdout, crossterm::style::Print(symbol),);
            } else {
                let _ = queue!(
                    stdout,
                    crossterm::cursor::MoveTo(x, y),
                    SetAttribute(Attribute::Reset),
                    SetForegroundColor(to_crossterm_color(cell.fg)),
                    SetBackgroundColor(to_crossterm_color(cell.bg)),
                );
                queue_modifiers(&mut stdout, modifier);
                let _ = queue!(stdout, crossterm::style::Print(" "),);
            }
        }
        let _ = queue!(stdout, SetAttribute(Attribute::Reset));
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
        let _ = execute!(
            stdout(),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
        );
        Ok(())
    }

    fn clear_region(
        &mut self,
        clear_type: ratatui_core::backend::ClearType,
    ) -> Result<(), Self::Error> {
        let ct = match clear_type {
            ratatui_core::backend::ClearType::All => crossterm::terminal::ClearType::All,
            ratatui_core::backend::ClearType::AfterCursor => {
                crossterm::terminal::ClearType::FromCursorDown
            }
            ratatui_core::backend::ClearType::BeforeCursor => {
                crossterm::terminal::ClearType::FromCursorUp
            }
            ratatui_core::backend::ClearType::CurrentLine => {
                crossterm::terminal::ClearType::CurrentLine
            }
            ratatui_core::backend::ClearType::UntilNewLine => {
                crossterm::terminal::ClearType::UntilNewLine
            }
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

fn to_crossterm_color(color: Color) -> CrosstermColor {
    match color {
        Color::Reset => CrosstermColor::Reset,
        Color::Black => CrosstermColor::Black,
        Color::Red => CrosstermColor::DarkRed,
        Color::Green => CrosstermColor::DarkGreen,
        Color::Yellow => CrosstermColor::DarkYellow,
        Color::Blue => CrosstermColor::DarkBlue,
        Color::Magenta => CrosstermColor::DarkMagenta,
        Color::Cyan => CrosstermColor::DarkCyan,
        Color::Gray => CrosstermColor::Grey,
        Color::DarkGray => CrosstermColor::DarkGrey,
        Color::LightRed => CrosstermColor::Red,
        Color::LightGreen => CrosstermColor::Green,
        Color::LightYellow => CrosstermColor::Yellow,
        Color::LightBlue => CrosstermColor::Blue,
        Color::LightMagenta => CrosstermColor::Magenta,
        Color::LightCyan => CrosstermColor::Cyan,
        Color::White => CrosstermColor::White,
        Color::Rgb(r, g, b) => CrosstermColor::Rgb { r, g, b },
        Color::Indexed(index) => CrosstermColor::AnsiValue(index),
    }
}

fn queue_modifiers(stdout: &mut std::io::Stdout, modifier: Modifier) {
    if modifier.contains(Modifier::BOLD) {
        let _ = queue!(stdout, SetAttribute(Attribute::Bold));
    }
    if modifier.contains(Modifier::DIM) {
        let _ = queue!(stdout, SetAttribute(Attribute::Dim));
    }
    if modifier.contains(Modifier::ITALIC) {
        let _ = queue!(stdout, SetAttribute(Attribute::Italic));
    }
    if modifier.contains(Modifier::UNDERLINED) {
        let _ = queue!(stdout, SetAttribute(Attribute::Underlined));
    }
    if modifier.contains(Modifier::SLOW_BLINK) || modifier.contains(Modifier::RAPID_BLINK) {
        let _ = queue!(stdout, SetAttribute(Attribute::SlowBlink));
    }
    if modifier.contains(Modifier::REVERSED) {
        let _ = queue!(stdout, SetAttribute(Attribute::Reverse));
    }
    if modifier.contains(Modifier::HIDDEN) {
        let _ = queue!(stdout, SetAttribute(Attribute::Hidden));
    }
    if modifier.contains(Modifier::CROSSED_OUT) {
        let _ = queue!(stdout, SetAttribute(Attribute::CrossedOut));
    }
}

fn translate_crossterm_event(event: crossterm::event::Event) -> Option<Event> {
    match event {
        crossterm::event::Event::Key(key) => {
            let modifiers = Modifiers {
                ctrl: key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL),
                shift: key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::SHIFT),
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
            Some(Event::Key(KeyEvent {
                scancode,
                modifiers,
            }))
        }
        crossterm::event::Event::Resize(_w, _h) => Some(Event::Tick),
        _ => None,
    }
}

struct NativeEvents {
    tick_interval: Interval,
    input: Option<JoinHandle<Option<Event>>>,
}

impl NativeEvents {
    fn new() -> Self {
        Self {
            tick_interval: interval(Duration::from_millis(50)),
            input: Some(spawn_input_read()),
        }
    }
}

fn spawn_input_read() -> JoinHandle<Option<Event>> {
    tokio::task::spawn_blocking(|| {
        if !crossterm::event::poll(Duration::from_millis(50)).unwrap_or(false) {
            return None;
        }

        crossterm::event::read()
            .ok()
            .and_then(translate_crossterm_event)
    })
}

impl Stream for NativeEvents {
    type Item = Event;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.input.is_none() {
            self.input = Some(spawn_input_read());
        }

        if let Some(input) = &mut self.input {
            match Pin::new(input).poll(cx) {
                Poll::Ready(Ok(Some(event))) => {
                    self.input = Some(spawn_input_read());
                    return Poll::Ready(Some(event));
                }
                Poll::Ready(Ok(None)) | Poll::Ready(Err(_)) => {
                    self.input = Some(spawn_input_read());
                }
                Poll::Pending => {}
            }
        }

        if Pin::new(&mut self.tick_interval).poll_tick(cx).is_ready() {
            return Poll::Ready(Some(Event::Tick));
        }

        Poll::Pending
    }
}

#[derive(Parser)]
#[command(
    name = "ocpncord-native",
    about = "ocpncord — native TUI client for opencode"
)]
struct Cli {
    /// OpenCode server URL
    #[arg(long = "url", default_value = "http://localhost:4096")]
    url: String,

    /// Optional server-side working directory for new sessions
    #[arg(long = "session-directory")]
    session_directory: Option<String>,
}

// --- Render target setup --------------------------------------------------

fn setup_terminal() -> Terminal<CrosstermBackend> {
    let _ = enable_raw_mode();
    let _ = execute!(stdout(), EnterAlternateScreen);
    Terminal::new(CrosstermBackend::new()).unwrap()
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(LevelFilter::Debug));

    static TCP: StdTcp = StdTcp;
    static DNS: StdDns = StdDns;
    let backend = OpenCodeBackend::new(&cli.url, &TCP, &DNS);
    let terminal = setup_terminal();
    let events = NativeEvents::new();
    let mut app = App::new(backend, events, terminal);

    if let Some(session_directory) = cli.session_directory {
        app.set_session_directory_override(session_directory);
    }
    app.run().await;

    let _ = execute!(stdout(), LeaveAlternateScreen);
    let _ = disable_raw_mode();
}
