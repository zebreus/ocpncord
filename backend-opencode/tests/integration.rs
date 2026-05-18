//! Integration tests against a running opencode server.
//!
//! Start the server with: `opencode serve --port 4096`
//! Then run: `cargo test -p opencode-backend-opencode --test integration -- --nocapture`

use core::net::IpAddr;
use core::result::Result as CoreResult;

use embedded_io_async::{ErrorType, Read};
use embedded_nal_async::{AddrType, Dns, TcpConnect};
use ocpncord_backend::*;
use ocpncord_backend_opencode::OpenCodeBackend;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// --- Transport types (mirror opencode-native) ---

struct StdTcp;

impl TcpConnect for StdTcp {
    type Error = std::io::Error;
    type Connection<'a> = StdTcpStream;

    async fn connect<'a>(
        &'a self,
        remote: core::net::SocketAddr,
    ) -> CoreResult<Self::Connection<'a>, Self::Error> {
        let stream = tokio::net::TcpStream::connect(remote).await?;
        Ok(StdTcpStream(stream))
    }
}

struct StdDns;

impl Dns for StdDns {
    type Error = std::io::Error;

    async fn get_host_by_name(
        &self,
        host: &str,
        addr_type: AddrType,
    ) -> CoreResult<IpAddr, Self::Error> {
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
    ) -> CoreResult<usize, Self::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "reverse DNS not supported",
        ))
    }
}

struct StdTcpStream(tokio::net::TcpStream);

impl ErrorType for StdTcpStream {
    type Error = std::io::Error;
}

impl Read for StdTcpStream {
    async fn read(&mut self, buf: &mut [u8]) -> CoreResult<usize, Self::Error> {
        self.0.read(buf).await
    }
}

impl embedded_io_async::Write for StdTcpStream {
    async fn write(&mut self, buf: &[u8]) -> CoreResult<usize, Self::Error> {
        self.0.write(buf).await
    }

    async fn flush(&mut self) -> CoreResult<(), Self::Error> {
        self.0.flush().await
    }
}

fn backend() -> OpenCodeBackend<StdTcp, StdDns> {
    static TCP: StdTcp = StdTcp;
    static DNS: StdDns = StdDns;
    OpenCodeBackend::new("http://localhost:4096", &TCP, &DNS)
}

#[tokio::test]
async fn health() {
    let mut b = backend();
    let h = b.health().await.expect("health should succeed");
    assert!(h.healthy);
}

#[tokio::test]
async fn create_and_get_session() {
    let mut b = backend();

    let s = b
        .create_session("integration-test", "/tmp")
        .await
        .expect("create session should succeed");

    assert!(!s.id.is_empty());
    assert_eq!(s.title, "integration-test");
    assert!(!s.slug.is_empty());
    assert!(!s.version.is_empty());
    assert!(s.time.created > 0);

    let fetched = b
        .get_session(&s.id)
        .await
        .expect("get session should succeed");
    assert_eq!(fetched.id, s.id);
    assert_eq!(fetched.title, s.title);
}

#[tokio::test]
async fn list_sessions() {
    let mut b = backend();
    // Ensure at least one session exists
    let _ = b.create_session("list-test", "/tmp").await;

    let sessions = b
        .list_sessions()
        .await
        .expect("list sessions should succeed");
    assert!(!sessions.is_empty());
    // Our session should be in the list
    assert!(sessions.iter().any(|s| s.title == "list-test"));
}

#[tokio::test]
async fn update_session() {
    let mut b = backend();
    let s = b.create_session("update-test", "/tmp").await.unwrap();

    let updated = b
        .update_session(&s.id, "updated-title")
        .await
        .expect("update should succeed");

    assert_eq!(updated.title, "updated-title");
    assert_eq!(updated.id, s.id);
}

#[tokio::test]
async fn children_sessions() {
    let mut b = backend();
    let parent = b.create_session("parent-test", "/tmp").await.unwrap();
    let children = b
        .children_sessions(&parent.id)
        .await
        .expect("children should succeed");
    // No children for a fresh session
    assert!(children.is_empty());
}

#[tokio::test]
async fn abort_session() {
    let mut b = backend();
    let s = b.create_session("abort-test", "/tmp").await.unwrap();
    b.abort_session(&s.id).await.expect("abort should succeed");
}

#[tokio::test]
async fn find_text() {
    let mut b = backend();
    let matches = b
        .find_text("integration")
        .await
        .expect("find should succeed");
    assert!(!matches.is_empty());
    assert!(!matches[0].path.text.is_empty());
    assert!(matches[0].line_number > 0);
    assert!(!matches[0].lines.text.is_empty());
}

#[tokio::test]
async fn get_config() {
    let mut b = backend();
    let config = b.get_config().await.expect("get config should succeed");
    // Server always has a username
    assert!(config.username.is_some());
}

#[tokio::test]
async fn set_auth() {
    let mut b = backend();
    b.set_auth("integration-test-provider", "sk-test-key")
        .await
        .expect("set auth should succeed");
}

#[tokio::test]
async fn prompt_and_messages() {
    use futures::StreamExt;

    let mut b = backend();
    let s = b.create_session("prompt-test", "/tmp").await.unwrap();

    // Send a prompt (will actually call the AI)
    let mut stream = b
        .prompt(&s.id, "say hello in one word", None)
        .await
        .expect("prompt should succeed");

    // Consume the stream until Done to let the server complete processing
    while let Some(event) = stream.next().await {
        match event {
            Ok(ocpncord_backend::BackendEvent::Done) => break,
            Ok(_) => {}
            Err(e) => panic!("unexpected error in prompt stream: {e}"),
        }
    }

    // List messages
    let messages = b
        .list_messages(&s.id)
        .await
        .expect("list messages should succeed");
    assert!(
        !messages.is_empty(),
        "prompt should create at least one message"
    );

    // Check the message has expected fields
    let msg = &messages[0];
    assert!(!msg.id.is_empty());
    assert!(!msg.session_id.is_empty());
    assert!(msg.time.created > 0);
}

#[tokio::test]
async fn send_command() {
    use futures::StreamExt;

    let mut b = backend();
    let s = b.create_session("command-test", "/tmp").await.unwrap();
    let mut stream = b
        .command(&s.id, "init", None)
        .await
        .expect("command should succeed");

    // Consume the stream until Done
    while let Some(event) = stream.next().await {
        match event {
            Ok(ocpncord_backend::BackendEvent::Done) => break,
            Ok(_) => {}
            Err(e) => panic!("unexpected error in command stream: {e}"),
        }
    }
}

#[tokio::test]
async fn delete_session() {
    let mut b = backend();
    let s = b.create_session("delete-test", "/tmp").await.unwrap();
    b.delete_session(&s.id)
        .await
        .expect("delete should succeed");

    // Verify it's gone
    let sessions = b.list_sessions().await.unwrap();
    assert!(!sessions.iter().any(|sess| sess.id == s.id));
}

#[tokio::test]
async fn subscribe_connects() {
    let mut b = backend();
    let stream = b.subscribe().await.expect("subscribe should succeed");
    // Connection established, drop it
    drop(stream);
}

#[tokio::test]
async fn full_roundtrip() {
    let mut b = backend();

    // Health
    let h = b.health().await.unwrap();
    assert!(h.healthy);

    // Create
    let s = b.create_session("roundtrip", "/tmp").await.unwrap();
    assert!(!s.id.is_empty());

    // Get
    let fetched = b.get_session(&s.id).await.unwrap();
    assert_eq!(fetched.id, s.id);

    // Update
    let updated = b.update_session(&s.id, "roundtrip-updated").await.unwrap();
    assert_eq!(updated.title, "roundtrip-updated");

    // Find
    let matches = b.find_text("roundtrip").await.unwrap();
    assert!(matches.len() >= 1);

    // Config
    let config = b.get_config().await.unwrap();
    assert!(config.username.is_some());

    // Delete
    b.delete_session(&s.id).await.unwrap();

    // Verify deleted
    let sessions = b.list_sessions().await.unwrap();
    assert!(!sessions.iter().any(|sess| sess.id == s.id));
}
