#![cfg_attr(not(test), no_std)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![allow(async_fn_in_trait)]

extern crate alloc;

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use core::future::Future;
use core::pin::Pin;
use ocpncord_backend::*;
use reqwless::client::HttpClient;
use reqwless::headers::ContentType;
use reqwless::request::{Method, RequestBuilder};

mod stream;
pub use stream::{BufferedStream, SseParser};

const RX_BUF_SIZE: usize = 65536;

/// An HTTP client for the opencode server REST API.
///
/// Generic over the TCP transport and DNS resolver, allowing it to work
/// with any platform that implements `embedded-nal-async` traits.
pub struct OpenCodeBackend<
    T: embedded_nal_async::TcpConnect + 'static,
    D: embedded_nal_async::Dns + 'static,
> {
    base_url: String,
    rx_buf: [u8; RX_BUF_SIZE],
    transport: &'static T,
    dns: &'static D,
}

impl<T: embedded_nal_async::TcpConnect + 'static, D: embedded_nal_async::Dns + 'static>
    OpenCodeBackend<T, D>
{
    pub fn new(base_url: &str, transport: &'static T, dns: &'static D) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            rx_buf: [0; RX_BUF_SIZE],
            transport,
            dns,
        }
    }

    fn make_client(&self) -> HttpClient<'static, T, D> {
        HttpClient::new(self.transport, self.dns)
    }
}

// --- Error helpers ---

fn conn_err(e: impl fmt::Display) -> BackendError {
    BackendError::Connection {
        message: alloc::format!("{e}"),
    }
}

fn parse_err(e: impl fmt::Display) -> BackendError {
    BackendError::Parse {
        message: alloc::format!("{e}"),
    }
}

fn api_err(status: u16, body: &[u8]) -> BackendError {
    let msg = core::str::from_utf8(body).unwrap_or("unknown error");
    BackendError::Api {
        status,
        message: msg.into(),
    }
}

// --- Helper: send + check status + return body (blocking within the async fn) ---

impl<T: embedded_nal_async::TcpConnect + 'static, D: embedded_nal_async::Dns + 'static>
    OpenCodeBackend<T, D>
{
    async fn send_get_body(
        &mut self,
        method: Method,
        url: &str,
        body_bytes: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        let mut client = self.make_client();
        if let Some(bytes) = body_bytes {
            let handle = client.request(method, url).await.map_err(conn_err)?;
            let mut handle = handle
                .body(bytes)
                .content_type(ContentType::ApplicationJson);
            let response = handle.send(&mut self.rx_buf).await.map_err(conn_err)?;
            if !response.status.is_successful() {
                let status = response.status.0;
                let b = response.body().read_to_end().await.map_err(conn_err)?;
                return Err(api_err(status, b));
            }
            let body = response.body().read_to_end().await.map_err(conn_err)?;
            Ok(body.to_vec())
        } else {
            let mut handle = client.request(method, url).await.map_err(conn_err)?;
            let response = handle.send(&mut self.rx_buf).await.map_err(conn_err)?;
            if !response.status.is_successful() {
                let status = response.status.0;
                let b = response.body().read_to_end().await.map_err(conn_err)?;
                return Err(api_err(status, b));
            }
            let body = response.body().read_to_end().await.map_err(conn_err)?;
            Ok(body.to_vec())
        }
    }
}

// --- Non-blocking HTTP POST for streaming endpoints (allocates own buffer) ---

/// POST JSON to the given URL and return the response body.
/// Allocates its own rx buffer so it can be used in a boxed future without `&mut` access.
async fn http_post_json<
    T: embedded_nal_async::TcpConnect + 'static,
    D: embedded_nal_async::Dns + 'static,
>(
    transport: &'static T,
    dns: &'static D,
    url: &str,
    json: &[u8],
) -> Result<Vec<u8>> {
    let mut rx_buf = alloc::vec![0u8; RX_BUF_SIZE];
    let mut client = HttpClient::new(transport, dns);
    let handle = client.request(Method::POST, url).await.map_err(conn_err)?;
    let mut handle = handle.body(json).content_type(ContentType::ApplicationJson);
    let response = handle.send(&mut rx_buf).await.map_err(conn_err)?;
    if !response.status.is_successful() {
        let status = response.status.0;
        let b = response.body().read_to_end().await.map_err(conn_err)?;
        return Err(api_err(status, b));
    }
    let body = response.body().read_to_end().await.map_err(conn_err)?;
    Ok(body.to_vec())
}

/// Send a POST request and return immediately after receiving the status code.
/// The response body is discarded — used for fire-and-forget triggers where
/// the actual response arrives via the SSE event stream.
async fn http_post_fire_and_forget<
    T: embedded_nal_async::TcpConnect + 'static,
    D: embedded_nal_async::Dns + 'static,
>(
    transport: &'static T,
    dns: &'static D,
    url: &str,
    json: &[u8],
) -> Result<()> {
    let mut rx_buf = alloc::vec![0u8; RX_BUF_SIZE];
    let mut client = HttpClient::new(transport, dns);
    let handle = client.request(Method::POST, url).await.map_err(conn_err)?;
    let mut handle = handle.body(json).content_type(ContentType::ApplicationJson);
    let response = handle.send(&mut rx_buf).await.map_err(conn_err)?;
    if !response.status.is_successful() {
        let status = response.status.0;
        return Err(BackendError::Api {
            status,
            message: alloc::format!("POST {url} failed with status {status}"),
        });
    }
    Ok(())
}

/// GET the given URL and return the response body (non-blocking, own buffer).
async fn http_get<
    T: embedded_nal_async::TcpConnect + 'static,
    D: embedded_nal_async::Dns + 'static,
>(
    transport: &'static T,
    dns: &'static D,
    url: &str,
) -> Result<Vec<u8>> {
    let mut rx_buf = alloc::vec![0u8; RX_BUF_SIZE];
    let mut client = HttpClient::new(transport, dns);
    let mut handle = client.request(Method::GET, url).await.map_err(conn_err)?;
    let response = handle.send(&mut rx_buf).await.map_err(conn_err)?;
    if !response.status.is_successful() {
        let status = response.status.0;
        let b = response.body().read_to_end().await.map_err(conn_err)?;
        return Err(api_err(status, b));
    }
    let body = response.body().read_to_end().await.map_err(conn_err)?;
    Ok(body.to_vec())
}

// --- Backend trait implementation ---

impl<T: embedded_nal_async::TcpConnect + 'static, D: embedded_nal_async::Dns + 'static> Backend
    for OpenCodeBackend<T, D>
{
    type PromptStream = BufferedStream;
    type EventStream = BufferedStream;

    async fn health(&mut self) -> Result<Health> {
        let url = alloc::format!("{}/global/health", self.base_url);
        let body = self.send_get_body(Method::GET, &url, None).await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn list_sessions(&mut self) -> Result<Vec<Session>> {
        let url = alloc::format!("{}/session", self.base_url);
        let body = self.send_get_body(Method::GET, &url, None).await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn get_session(&mut self, id: &SessionId) -> Result<Session> {
        let url = alloc::format!("{}/session/{id}", self.base_url);
        let body = self.send_get_body(Method::GET, &url, None).await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn create_session(&mut self, title: &str, cwd: &str) -> Result<Session> {
        let url = alloc::format!("{}/session", self.base_url);
        let body = ocpncord_types::CreateSessionBody {
            title,
            directory: cwd,
        };
        let json = serde_json::to_string(&body).map_err(parse_err)?;
        let body = self
            .send_get_body(Method::POST, &url, Some(json.as_bytes()))
            .await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn delete_session(&mut self, id: &SessionId) -> Result<()> {
        let url = alloc::format!("{}/session/{id}", self.base_url);
        self.send_get_body(Method::DELETE, &url, None).await?;
        Ok(())
    }

    async fn update_session(&mut self, id: &SessionId, title: &str) -> Result<Session> {
        let url = alloc::format!("{}/session/{id}", self.base_url);
        let body = ocpncord_types::UpdateSessionBody { title };
        let json = serde_json::to_string(&body).map_err(parse_err)?;
        let body = self
            .send_get_body(Method::PATCH, &url, Some(json.as_bytes()))
            .await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn children_sessions(&mut self, id: &SessionId) -> Result<Vec<Session>> {
        let url = alloc::format!("{}/session/{id}/children", self.base_url);
        let body = self.send_get_body(Method::GET, &url, None).await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn abort_session(&mut self, id: &SessionId) -> Result<()> {
        let url = alloc::format!("{}/session/{id}/abort", self.base_url);
        self.send_get_body(Method::POST, &url, None).await?;
        Ok(())
    }

    async fn list_messages(&mut self, id: &SessionId) -> Result<Vec<MessageSummary>> {
        let url = alloc::format!("{}/session/{id}/message", self.base_url);
        let body = self.send_get_body(Method::GET, &url, None).await?;
        let details: Vec<MessageDetail> = serde_json::from_slice(&body).map_err(parse_err)?;
        Ok(details.into_iter().map(|d| d.info).collect())
    }

    async fn get_message(
        &mut self,
        session_id: &SessionId,
        message_id: &MessageId,
    ) -> Result<MessageDetail> {
        let url = alloc::format!(
            "{}/session/{}/message/{}",
            self.base_url,
            session_id,
            message_id
        );
        let body = self.send_get_body(Method::GET, &url, None).await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn prompt(
        &mut self,
        id: &SessionId,
        text: &str,
        agent: Option<&str>,
    ) -> Result<Self::PromptStream> {
        let url = alloc::format!("{}/session/{id}/message", self.base_url);
        let prompt_body = ocpncord_types::PromptBody {
            parts: &[ocpncord_types::TextPartBody {
                type_: "text",
                text,
            }],
            agent,
        };
        let json = serde_json::to_string(&prompt_body).map_err(parse_err)?;
        http_post_fire_and_forget(self.transport, self.dns, &url, json.as_bytes()).await?;
        Ok(BufferedStream::empty())
    }

    async fn command(
        &mut self,
        id: &SessionId,
        text: &str,
        agent: Option<&str>,
    ) -> Result<Self::PromptStream> {
        let url = alloc::format!("{}/session/{id}/command", self.base_url);
        let cmd_body = ocpncord_types::CommandBody {
            command: text,
            arguments: "",
            agent,
        };
        let json = serde_json::to_string(&cmd_body).map_err(parse_err)?;
        http_post_fire_and_forget(self.transport, self.dns, &url, json.as_bytes()).await?;
        Ok(BufferedStream::empty())
    }

    async fn list_agents(&mut self) -> Result<Vec<Agent>> {
        let url = alloc::format!("{}/agent", self.base_url);
        let body = self.send_get_body(Method::GET, &url, None).await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn find_text(&mut self, pattern: &str) -> Result<Vec<TextMatch>> {
        let encoded: String = pattern
            .chars()
            .flat_map(|c| match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => {
                    alloc::vec![c]
                }
                ' ' => alloc::vec!['+'],
                c => alloc::format!("%{:02X}", c as u8).chars().collect(),
            })
            .collect();
        let url = alloc::format!("{}/find?pattern={}", self.base_url, encoded);
        let body = self.send_get_body(Method::GET, &url, None).await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn subscribe(&mut self) -> Result<Self::EventStream> {
        let url = alloc::format!("{}/global/event", self.base_url);
        let transport = self.transport;
        let dns = self.dns;
        let url2 = url;
        let fut: Pin<Box<dyn Future<Output = Vec<Result<BackendEvent>>>>> = Box::pin(async move {
            match http_get(transport, dns, &url2).await {
                Ok(raw) => BufferedStream::parse_sse(&raw),
                Err(e) => vec![Err(e)],
            }
        });
        Ok(BufferedStream::from_pending(fut))
    }

    async fn get_config(&mut self) -> Result<Config> {
        let url = alloc::format!("{}/config", self.base_url);
        let body = self.send_get_body(Method::GET, &url, None).await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn set_auth(&mut self, provider: &str, api_key: &str) -> Result<()> {
        let url = alloc::format!("{}/auth/{provider}", self.base_url);
        let body = ocpncord_types::AuthBody {
            type_: "api",
            key: api_key,
        };
        let json = serde_json::to_string(&body).map_err(parse_err)?;
        self.send_get_body(Method::PUT, &url, Some(json.as_bytes()))
            .await?;
        Ok(())
    }

    async fn sync_events(&mut self) -> Result<Self::EventStream> {
        let url = alloc::format!("{}/global/sync-event", self.base_url);
        let transport = self.transport;
        let dns = self.dns;
        let url2 = url;
        let fut: Pin<Box<dyn Future<Output = Vec<Result<BackendEvent>>>>> = Box::pin(async move {
            match http_get(transport, dns, &url2).await {
                Ok(raw) => BufferedStream::parse_sse(&raw),
                Err(e) => vec![Err(e)],
            }
        });
        Ok(BufferedStream::from_pending(fut))
    }

    async fn set_config(&mut self, config: &Config) -> Result<Config> {
        let url = alloc::format!("{}/config", self.base_url);
        let body = ocpncord_types::ConfigBody {
            model: config.model.as_deref(),
            username: config.username.as_deref(),
        };
        let json = serde_json::to_string(&body).map_err(parse_err)?;
        let body = self
            .send_get_body(Method::PATCH, &url, Some(json.as_bytes()))
            .await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn dispose(&mut self) -> Result<()> {
        let url = alloc::format!("{}/global/dispose", self.base_url);
        self.send_get_body(Method::POST, &url, None).await?;
        Ok(())
    }

    async fn upgrade(&mut self) -> Result<()> {
        let url = alloc::format!("{}/global/upgrade", self.base_url);
        self.send_get_body(Method::POST, &url, None).await?;
        Ok(())
    }

    async fn log(&mut self, level: &str, message: &str) -> Result<()> {
        let url = alloc::format!("{}/log", self.base_url);
        let body = ocpncord_types::LogBody { level, message };
        let json = serde_json::to_string(&body).map_err(parse_err)?;
        self.send_get_body(Method::POST, &url, Some(json.as_bytes()))
            .await?;
        Ok(())
    }

    async fn remove_auth(&mut self, provider: &str) -> Result<()> {
        let url = alloc::format!("{}/auth/{provider}", self.base_url);
        self.send_get_body(Method::DELETE, &url, None).await?;
        Ok(())
    }
}
