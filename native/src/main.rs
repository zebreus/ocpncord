use core::convert::Infallible;
use core::net::IpAddr;
use std::fs::OpenOptions;
use std::io::{stdout, Write};
use std::sync::Mutex;
use std::time::Instant;

use clap::Parser;
use crossterm::cursor::{Hide, Show};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, size,
};
use crossterm::{execute, queue};
use embedded_io_async::{ErrorType, Read};
use embedded_nal_async::{AddrType, Dns, TcpConnect};
use opencode_backend::BackendEvent;
use opencode_backend_opencode::{OpenCodeBackend, SseParser};
use opencode_tui::Event;
use opencode_tui::{App, KeyEvent, Modifiers, Scancode};
use ratatui_core::backend::Backend;
use ratatui_core::backend::TestBackend;
use ratatui_core::buffer::Cell;
use ratatui_core::layout::{Position, Size};
use ratatui_core::terminal::Terminal;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

static LOG: Mutex<Option<std::fs::File>> = Mutex::new(None);

fn log(msg: &str) {
    if let Ok(mut guard) = LOG.lock() {
        if guard.is_none() {
            *guard = OpenOptions::new()
                .create(true)
                .append(true)
                .open("/tmp/opencode-rust-client.log")
                .ok();
        }
        if let Some(ref mut f) = *guard {
            let _ = writeln!(f, "{msg}");
        }
    }
}

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
            if !symbol.is_empty() {
                let _ = queue!(
                    stdout,
                    crossterm::cursor::MoveTo(x, y),
                    crossterm::style::Print(symbol),
                );
            } else {
                let _ = queue!(
                    stdout,
                    crossterm::cursor::MoveTo(x, y),
                    crossterm::style::Print(" "),
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
        crossterm::event::Event::Resize(_w, _h) => Some(Event::Tick),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScreenshotTrigger {
    Start,
    Part,
    Done,
    Error,
    SessionCreated,
    MessageSent,
    StreamStart,
    StreamEnd,
}

#[derive(Parser)]
#[command(name = "opencode-native", about = "Native TUI client for opencode")]
struct Cli {
    /// OpenCode server URL
    #[arg(long = "url", default_value = "http://localhost:4096")]
    url: String,

    /// Working directory for sessions
    #[arg(long = "cwd", default_value = ".")]
    cwd: String,

    /// Run in headless mode (TestBackend, no real terminal, read keystrokes from file)
    #[arg(long = "headless")]
    headless: bool,

    /// File to read keystrokes from (one per line, append-only). Requires --headless.
    #[arg(long = "keystroke-file")]
    keystroke_file: Option<String>,

    /// Directory to write screenshot files (disabled if not set)
    #[arg(long = "screenshot-dir")]
    screenshot_dir: Option<String>,

    /// Comma-separated list of events that trigger automatic screenshots.
    /// Values: start, part, done, error, session-created, message-sent, stream-start, stream-end
    #[arg(long = "screenshot-on", value_delimiter = ',')]
    screenshot_on: Vec<String>,

    /// Screenshot size in WxH format (e.g. "80x24"). Default: 80x24
    #[arg(long = "screenshot-size", default_value = "80x24")]
    screenshot_size: String,
}

fn parse_screenshot_triggers(raw: &[String]) -> Vec<ScreenshotTrigger> {
    raw.iter()
        .filter_map(|s| match s.to_lowercase().as_str() {
            "start" => Some(ScreenshotTrigger::Start),
            "part" => Some(ScreenshotTrigger::Part),
            "done" => Some(ScreenshotTrigger::Done),
            "error" => Some(ScreenshotTrigger::Error),
            "session-created" => Some(ScreenshotTrigger::SessionCreated),
            "message-sent" => Some(ScreenshotTrigger::MessageSent),
            "stream-start" => Some(ScreenshotTrigger::StreamStart),
            "stream-end" => Some(ScreenshotTrigger::StreamEnd),
            _ => {
                eprintln!("warning: unknown screenshot trigger: {s}");
                None
            }
        })
        .collect()
}

// --- Keystroke file reader ------------------------------------------------

/// Reads keystrokes from an append-only file. Exits with error if the file
/// is truncated or modified (append-only violation).
struct KeystrokeReader {
    path: String,
    byte_pos: u64,
    partial: String,
}

impl KeystrokeReader {
    fn new(path: String) -> Self {
        Self { path, byte_pos: 0, partial: String::new() }
    }

    fn read_new(&mut self) -> Result<Vec<Event>, String> {
        use std::io::{Read, Seek, SeekFrom};

        let mut file = std::fs::File::open(&self.path)
            .map_err(|e| format!("cannot open keystroke file '{}': {e}", self.path))?;

        let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);

        if file_len < self.byte_pos {
            return Err(format!(
                "keystroke file '{}' was truncated! \
                 Previously read {} bytes, now {file_len}. \
                 Keystroke file must be append-only — do not modify or truncate already-sent commands.",
                self.path, self.byte_pos,
            ));
        }

        if file_len == self.byte_pos {
            return Ok(Vec::new());
        }

        file.seek(SeekFrom::Start(self.byte_pos))
            .map_err(|e| format!("seek in keystroke file: {e}"))?;

        let mut buf = String::new();
        file.read_to_string(&mut buf)
            .map_err(|e| format!("read keystroke file: {e}"))?;
        self.byte_pos = file_len;

        let full = format!("{}{}", self.partial, buf);
        let mut lines: Vec<&str> = full.split('\n').collect();

        // The last element is the current partial line (no trailing \n yet)
        // If the last char was \n, the last element is empty string
        if let Some(last) = lines.last() {
            if full.ends_with('\n') {
                // Everything including last is complete
                self.partial.clear();
            } else {
                // Last line is incomplete — save it
                self.partial = last.to_string();
                lines.pop();
            }
        }

        let mut events = Vec::new();
        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some(event) = parse_keystroke(trimmed) {
                events.push(event);
            }
        }

        Ok(events)
    }
}

fn parse_keystroke(s: &str) -> Option<Event> {
    use Scancode::*;

    let plain = || Modifiers::default();
    let ctrl_mod = || Modifiers { ctrl: true, shift: false, alt: false, meta: false };

    match s {
        "Enter"    => Some(Event::Key(KeyEvent { scancode: Enter,    modifiers: plain() })),
        "Esc"      => Some(Event::Key(KeyEvent { scancode: Escape,   modifiers: plain() })),
        "Backspace"=> Some(Event::Key(KeyEvent { scancode: Backspace,modifiers: plain() })),
        "Tab"      => Some(Event::Key(KeyEvent { scancode: Tab,      modifiers: plain() })),
        "Space"    => Some(Event::Key(KeyEvent { scancode: Char(' '),modifiers: plain() })),
        "Up"       => Some(Event::Key(KeyEvent { scancode: Up,       modifiers: plain() })),
        "Down"     => Some(Event::Key(KeyEvent { scancode: Down,     modifiers: plain() })),
        "Left"     => Some(Event::Key(KeyEvent { scancode: Left,     modifiers: plain() })),
        "Right"    => Some(Event::Key(KeyEvent { scancode: Right,    modifiers: plain() })),
        "Home"     => Some(Event::Key(KeyEvent { scancode: Home,     modifiers: plain() })),
        "End"      => Some(Event::Key(KeyEvent { scancode: End,      modifiers: plain() })),
        "PageUp"   => Some(Event::Key(KeyEvent { scancode: PageUp,   modifiers: plain() })),
        "PageDown" => Some(Event::Key(KeyEvent { scancode: PageDown, modifiers: plain() })),
        "Delete"   => Some(Event::Key(KeyEvent { scancode: Delete,   modifiers: plain() })),
        _ if s.starts_with("Ctrl+") && s.len() == 6 => {
            let ch = s.as_bytes()[5] as char;
            Some(Event::Key(KeyEvent {
                scancode: Char(ch.to_ascii_lowercase()),
                modifiers: ctrl_mod(),
            }))
        }
        _ if s.starts_with("Shift+") && s.len() == 7 => {
            let ch = s.as_bytes()[6] as char;
            Some(Event::Key(KeyEvent {
                scancode: Char(ch),
                modifiers: Modifiers { ctrl: false, shift: true, alt: false, meta: false },
            }))
        }
        _ if s.len() == 1 => {
            let ch = s.chars().next().unwrap();
            Some(Event::Key(KeyEvent {
                scancode: Char(ch),
                modifiers: Modifiers { ctrl: false, shift: ch.is_ascii_uppercase(), alt: false, meta: false },
            }))
        }
        _ if s.starts_with('F') && s.len() > 1 => {
            let n: u8 = s[1..].parse().ok()?;
            Some(Event::Key(KeyEvent { scancode: F(n), modifiers: plain() }))
        }
        _ => {
            eprintln!("warning: unrecognized keystroke: '{s}'");
            None
        }
    }
}

// --- Persistent SSE background task ---

/// Background task that maintains a persistent SSE connection to /global/event.
/// Reconnects automatically with Last-Event-ID tracking.
async fn sse_background_task(
    base_url: String,
    event_tx: mpsc::UnboundedSender<Option<Event>>,
) {
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
            let status_end = buf[..pos]
                .iter()
                .position(|&b| b == b'\r')
                .unwrap_or(pos);
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
        log(&format!("[SSE] fed {} bytes, got {} events", n, events.len()));
        send_events(events, event_tx);
    }
}

fn send_events(
    events: Vec<core::result::Result<opencode_backend::BackendEvent, opencode_backend::BackendError>>,
    event_tx: &mpsc::UnboundedSender<Option<Event>>,
) {
    for event in events {
        match event {
            Ok(ref be) => {
                log(&format!("[SSE] sending: {be:?}"));
                let _ = event_tx.send(Some(Event::Backend(be.clone())));
            }
            Err(ref e) => {
                log(&format!("[SSE] parse error: {e}"));
                let _ = event_tx.send(Some(Event::Backend(BackendEvent::Error {
                    message: format!("SSE parse: {e}"),
                })));
            }
        }
    }
}

fn parse_http_url(url: &str) -> core::result::Result<(String, u16, String), Box<dyn std::error::Error>> {
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

// --- Render target: either real terminal or offscreen TestBackend ----------

enum RenderTarget {
    Interactive(Terminal<CrosstermBackend>),
    Headless(Terminal<TestBackend>),
}

impl RenderTarget {
    fn draw(&mut self, app: &App<OpenCodeBackend<StdTcp, StdDns>>) {
        match self {
            RenderTarget::Interactive(t) => {
                let _ = t.draw(|frame| app.render(frame));
            }
            RenderTarget::Headless(t) => {
                let _ = t.draw(|frame| app.render(frame));
            }
        }
    }
}

fn make_headless_terminal(w: u16, h: u16) -> RenderTarget {
    let tb = TestBackend::new(w, h);
    RenderTarget::Headless(Terminal::new(tb).unwrap())
}

fn make_interactive_terminal() -> RenderTarget {
    let _ = enable_raw_mode();
    let _ = execute!(stdout(), EnterAlternateScreen);
    RenderTarget::Interactive(Terminal::new(CrosstermBackend::new()).unwrap())
}

// --- Screenshot helpers ---------------------------------------------------

fn screenshot_header(
    app: &App<OpenCodeBackend<StdTcp, StdDns>>,
    sw: u16,
    sh: u16,
    ms: u64,
    screen_text: &str,
) -> String {
    let error = app.error().unwrap_or("none");
    let session_id = app
        .active_session()
        .map(|s| s.id.as_str())
        .unwrap_or("none");
    let agent = app.active_agent_name();
    let screen_name = format!("{:?}", app.active_screen());

    format!(
        "Screen: {screen_name}\n\
         Dimensions: {sw}x{sh}\n\
         Tick: {}\n\
         Streaming: {}\n\
         Error: {error}\n\
         ActiveSession: {session_id}\n\
         ActiveAgent: {agent}\n\
         ScreenshotMs: {ms:07}\n\
         ---\n\
         {screen_text}",
        app.tick(),
        app.is_streaming(),
    )
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if cli.keystroke_file.is_some() && !cli.headless {
        eprintln!("error: --keystroke-file requires --headless");
        std::process::exit(1);
    }

    static TCP: StdTcp = StdTcp;
    static DNS: StdDns = StdDns;
    let backend = OpenCodeBackend::new(&cli.url, &TCP, &DNS);
    let mut app = App::new(backend);

    app.set_cwd(cli.cwd);
    app.init().await;

    // Parse screenshot size
    let screenshot_size: (u16, u16) = {
        let parts: Vec<&str> = cli.screenshot_size.split('x').collect();
        let w = parts.first().and_then(|s| s.parse().ok()).unwrap_or(80);
        let h = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(24);
        (w.max(1), h.max(1))
    };

    // Initialize render target
    let mut render_target = if cli.headless {
        make_headless_terminal(screenshot_size.0, screenshot_size.1)
    } else {
        make_interactive_terminal()
    };

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<Option<Event>>();

    // Persistent SSE background task — all backend events flow through here
    let sse_event_tx = event_tx.clone();
    let sse_url = cli.url.clone();

    tokio::spawn(async move {
        sse_background_task(sse_url, sse_event_tx).await;
    });

    // Input source: keyboard (interactive) or keystroke file (headless)
    if cli.headless {
        if let Some(path) = cli.keystroke_file {
            let keystroke_event_tx = event_tx.clone();
            tokio::spawn(async move {
                let mut reader = KeystrokeReader::new(path);
                loop {
                    match reader.read_new() {
                        Ok(events) => {
                            for event in events {
                                log(&format!("[KEYSTROKE] {event:?}"));
                                if keystroke_event_tx.send(Some(event)).is_err() {
                                    return;
                                }
                            }
                        }
                        Err(msg) => {
                            eprintln!("{msg}");
                            std::process::exit(1);
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            });
        }
    } else {
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
    }

    // Screenshot machinery ------------------------------------------------

    let screenshot_dir = cli.screenshot_dir;
    let screenshot_triggers = parse_screenshot_triggers(&cli.screenshot_on);
    let mut pending_screenshot = screenshot_triggers.contains(&ScreenshotTrigger::Start);
    let program_start = Instant::now();

    let mut sigusr1 = if screenshot_dir.is_some() {
        signal(SignalKind::user_defined1()).ok()
    } else {
        None
    };

    // Main loop -----------------------------------------------------------

    let mut running = true;
    let mut tick_interval = interval(Duration::from_millis(50));

    while running {
        tokio::select! {
            maybe_event = event_rx.recv() => {
                if let Some(Some(ref event)) = maybe_event {
                    log(&format!("[DEBUG] event: {event:?}"));
                    if matches!(event, Event::Backend(BackendEvent::Done)) {
                        log(&format!("[DEBUG] Done received — is_streaming={} partial_parts={} messages={}", app.is_streaming(), app.partial_parts().len(), app.messages().len()));
                    }
                }

                if let Some(Some(ref event)) = maybe_event {
                    // Check event-based auto-triggers (before processing)
                    if screenshot_dir.is_some() {
                        match event {
                            Event::Backend(BackendEvent::Part { .. }) if screenshot_triggers.contains(&ScreenshotTrigger::Part) => {
                                pending_screenshot = true;
                            }
                            Event::Backend(BackendEvent::Done) if screenshot_triggers.contains(&ScreenshotTrigger::Done) => {
                                pending_screenshot = true;
                            }
                            Event::Backend(BackendEvent::Error { .. }) if screenshot_triggers.contains(&ScreenshotTrigger::Error) => {
                                pending_screenshot = true;
                            }
                            Event::Backend(BackendEvent::SessionCreated { .. }) if screenshot_triggers.contains(&ScreenshotTrigger::SessionCreated) => {
                                pending_screenshot = true;
                            }
                            Event::Key(ref key) if key.scancode == Scancode::Enter
                                && !app.prompt_text().is_empty()
                                && screenshot_triggers.contains(&ScreenshotTrigger::MessageSent) =>
                            {
                                pending_screenshot = true;
                            }
                            _ => {}
                        }
                    }
                }

                if let Some(Some(event)) = maybe_event {
                    let was_streaming = app.is_streaming();
                    running = app.handle_event(event).await;

                    // Check streaming-transition auto-triggers (after processing, before draw)
                    if screenshot_dir.is_some() {
                        if !was_streaming && app.is_streaming()
                            && screenshot_triggers.contains(&ScreenshotTrigger::StreamStart)
                        {
                            pending_screenshot = true;
                        }
                        if was_streaming && !app.is_streaming()
                            && screenshot_triggers.contains(&ScreenshotTrigger::StreamEnd)
                        {
                            pending_screenshot = true;
                        }
                    }
                }
            }
            _ = tick_interval.tick() => {
                running = app.handle_event(Event::Tick).await;
            }
            _ = async {
                let sig = sigusr1.as_mut()?;
                let _: Option<()> = sig.recv().await;
                Some(())
            }, if screenshot_dir.is_some() => {
                pending_screenshot = true;
            }
        }

        log(&format!("[RENDER] screen={:?} is_streaming={} partial_parts={} messages={} tick={}", app.active_screen(), app.is_streaming(), app.partial_parts().len(), app.messages().len(), app.tick()));
        render_target.draw(&app);

        // Screenshot after draw (captures the fresh frame)
        if pending_screenshot {
            if let Some(ref dir) = screenshot_dir {
                let ms = program_start.elapsed().as_millis();
                let ms = core::cmp::min(ms, 9_999_999) as u64;
                let path = format!("{dir}/{ms:07}-screenshot.txt");

                let screen_text = match &render_target {
                    // In headless mode, the render target IS a TestBackend — read it directly
                    RenderTarget::Headless(t) => format!("{}", t.backend()),
                    // In interactive mode, render a fresh TestBackend
                    RenderTarget::Interactive(_) => {
                        let tb = TestBackend::new(screenshot_size.0, screenshot_size.1);
                        let mut t = match Terminal::new(tb) {
                            Ok(t) => t,
                            Err(_) => continue,
                        };
                        let _ = t.draw(|frame| app.render(frame));
                        format!("{}", t.backend())
                    }
                };

                let header = screenshot_header(&app, screenshot_size.0, screenshot_size.1, ms, &screen_text);
                let _ = std::fs::write(&path, header);
            }
            pending_screenshot = false;
        }
    }

    if !cli.headless {
        let _ = execute!(stdout(), LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}
