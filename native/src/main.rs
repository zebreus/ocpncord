use core::convert::Infallible;
use core::net::IpAddr;
use std::fs::OpenOptions;
use std::io::{stdout, Write};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

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
use ocpncord_backend::BackendEvent;
use ocpncord_backend_opencode::{OpenCodeBackend, SseParser};
use ocpncord_tui::Event;
use ocpncord_tui::{App, KeyEvent, Modifiers, Scancode};
use ratatui_core::backend::Backend;
use ratatui_core::buffer::Cell;
use ratatui_core::layout::{Position, Size};
use ratatui_core::style::{Color, Modifier};
use ratatui_core::terminal::Terminal;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

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

#[derive(Parser)]
#[command(
    name = "ocpncord-native",
    about = "ocpncord — native TUI client for opencode"
)]
struct Cli {
    /// OpenCode server URL
    #[arg(long = "url", default_value = "http://localhost:4096")]
    url: String,

    /// Working directory for sessions
    #[arg(long = "cwd", default_value = ".")]
    cwd: String,
}

// --- Persistent SSE background task ---

/// Background task that maintains a persistent SSE connection to /global/event.
/// Reconnects automatically with Last-Event-ID tracking.
async fn sse_background_task(base_url: String, event_tx: mpsc::UnboundedSender<Option<Event>>) {
    static TCP: StdTcp = StdTcp;
    static DNS: StdDns = StdDns;

    let mut parser = SseParser::new();

    loop {
        if let Err(e) = connect_and_read_sse(&base_url, &TCP, &DNS, &mut parser, &event_tx).await {
            let _ = event_tx.send(Some(Event::Backend(BackendEvent::Error {
                message: format!("SSE: {e}"),
            })));
        }

        tokio::time::sleep(Duration::from_millis(parser.retry_ms())).await;
    }
}

/// Connect to /global/event, send HTTP request, read SSE events in a loop.
async fn connect_and_read_sse(
    base_url: &str,
    _tcp: &'static StdTcp,
    dns: &'static StdDns,
    parser: &mut SseParser,
    event_tx: &mpsc::UnboundedSender<Option<Event>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let url = format!("{}/global/event", base_url.trim_end_matches('/'));
    let (host, port, path) = parse_http_url(&url)?;

    let addr = dns.get_host_by_name(&host, AddrType::Either).await?;
    let socket_addr = std::net::SocketAddr::new(addr, port);
    let mut stream = tokio::net::TcpStream::connect(socket_addr).await?;

    let mut request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nAccept: text/event-stream\r\n",
        path, host,
    );
    let last_id = parser.last_event_id();
    if !last_id.is_empty() {
        request.push_str(&format!("Last-Event-ID: {}\r\n", last_id));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await?;

    let mut buf = vec![0u8; 8192];
    let mut pos = 0;

    loop {
        let n = stream.read(&mut buf[pos..]).await?;
        if n == 0 {
            return Err("connection closed during headers".into());
        }
        pos += n;
        if let Some(header_end) = buf[..pos].windows(4).position(|w| w == b"\r\n\r\n") {
            let status_end = buf[..pos].iter().position(|&b| b == b'\r').unwrap_or(pos);
            let status_line =
                core::str::from_utf8(&buf[..status_end]).map_err(|_| "invalid utf-8")?;
            if !status_line.contains("200") {
                return Err(format!("non-200 response: {status_line}").into());
            }

            let body_start = header_end + 4;
            if body_start < pos {
                send_events(parser.feed(&buf[body_start..pos]), event_tx);
            }
            break;
        }
        if pos >= buf.len() {
            buf.resize(buf.len() * 2, 0);
        }
    }

    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        let events = parser.feed(&buf[..n]);
        log::info!("[SSE] fed {} bytes, got {} events", n, events.len());
        send_events(events, event_tx);
    }
}

fn send_events(
    events: Vec<
        core::result::Result<ocpncord_backend::BackendEvent, ocpncord_backend::BackendError>,
    >,
    event_tx: &mpsc::UnboundedSender<Option<Event>>,
) {
    for event in events {
        match event {
            Ok(ref be) => {
                log::debug!("[SSE] sending: {be:?}");
                let _ = event_tx.send(Some(Event::Backend(be.clone())));
            }
            Err(ref e) => {
                log::error!("[SSE] parse error: {e}");
                let _ = event_tx.send(Some(Event::Backend(BackendEvent::Error {
                    message: format!("SSE parse: {e}"),
                })));
            }
        }
    }
}

fn parse_http_url(
    url: &str,
) -> core::result::Result<(String, u16, String), Box<dyn std::error::Error>> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);
    let (host_part, path) = match rest.split_once('/') {
        Some((h, p)) => (h, format!("/{}", p)),
        None => (rest, String::new()),
    };
    let (host, port) = match host_part.split_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().unwrap_or(80)),
        None => (host_part.to_string(), 80u16),
    };
    Ok((host, port, path))
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
    let mut app = App::new(backend);

    app.set_cwd(cli.cwd);
    app.init().await;

    let mut terminal = setup_terminal();

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<Option<Event>>();

    // Persistent SSE background task — all backend events flow through here
    let sse_event_tx = event_tx.clone();
    let sse_url = cli.url.clone();

    tokio::spawn(async move {
        sse_background_task(sse_url, sse_event_tx).await;
    });

    // Keyboard input
    tokio::spawn(async move {
        loop {
            let event = tokio::task::spawn_blocking(|| {
                crossterm::event::read()
                    .ok()
                    .and_then(translate_crossterm_event)
            })
            .await
            .ok()
            .flatten();
            if event_tx.send(event).is_err() {
                break;
            }
        }
    });

    // Main loop -----------------------------------------------------------

    let mut running = true;
    let mut tick_interval = interval(Duration::from_millis(50));

    while running {
        tokio::select! {
            maybe_event = event_rx.recv() => {
                if let Some(Some(ref event)) = maybe_event {
                    log::debug!("[DEBUG] event: {event:?}");
                    if matches!(event, Event::Backend(BackendEvent::Done)) {
                        log::debug!("[DEBUG] Done received — is_streaming={} partial_parts={} messages={}", app.is_streaming(), app.partial_parts().len(), app.messages().len());
                    }
                }

                if let Some(Some(event)) = maybe_event {
                    running = app.handle_event(event).await;
                }
            }
            _ = tick_interval.tick() => {
                running = app.handle_event(Event::Tick).await;
            }
        }

        log::trace!(
            "[RENDER] screen={:?} is_streaming={} partial_parts={} messages={} tick={}",
            app.active_screen(),
            app.is_streaming(),
            app.partial_parts().len(),
            app.messages().len(),
            app.tick()
        );
        let _ = terminal.draw(|frame| app.render(frame));
    }

    let _ = execute!(stdout(), LeaveAlternateScreen);
    let _ = disable_raw_mode();
}
