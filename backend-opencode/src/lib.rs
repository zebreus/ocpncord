#![cfg_attr(not(test), no_std)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![allow(async_fn_in_trait)]

extern crate alloc;

use alloc::borrow::ToOwned;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use embedded_io_async::Read;
use ocpncord_backend::*;
use reqwless::client::HttpClient;
use reqwless::headers::ContentType;
use reqwless::request::{Method, RequestBuilder};
use reqwless::response::Response;
use serde::Deserialize;

mod stream;
pub use stream::{BufferedStream, SseParser};

const HTTP_HEADER_BUF_SIZE: usize = 4 * 1024;
const HTTP_BODY_READ_BUF_SIZE: usize = 4 * 1024;
const SSE_READ_BUF_SIZE: usize = 4 * 1024;
const SSE_HEADERS: [(&str, &str); 1] = [("Accept", "text/event-stream")];

/// An HTTP client for the opencode server REST API.
///
/// Generic over the TCP transport and DNS resolver, allowing it to work
/// with any platform that implements `embedded-nal-async` traits.
pub struct OpenCodeBackend<
    T: embedded_nal_async::TcpConnect + 'static,
    D: embedded_nal_async::Dns + 'static,
> {
    base_url: String,
    header_buf: Vec<u8>,
    transport: &'static T,
    dns: &'static D,
}

impl<T: embedded_nal_async::TcpConnect + 'static, D: embedded_nal_async::Dns + 'static>
    OpenCodeBackend<T, D>
{
    pub fn new(base_url: &str, transport: &'static T, dns: &'static D) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            header_buf: vec![0; HTTP_HEADER_BUF_SIZE],
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

fn should_fallback_to_api_models(error: &BackendError) -> bool {
    matches!(error, BackendError::Api { status: 404, .. })
}

async fn read_body_to_vec<C>(response: Response<'_, '_, C>) -> Result<Vec<u8>>
where
    C: Read,
{
    let content_length = response.content_length.unwrap_or(0);
    let mut body = Vec::new();
    body.try_reserve(content_length)
        .map_err(|_| conn_err("response body too large"))?;

    let mut reader = response.body().reader();
    let mut chunk = alloc::vec![0u8; HTTP_BODY_READ_BUF_SIZE];
    loop {
        let read = reader.read(&mut chunk).await.map_err(conn_err)?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }

    Ok(body)
}

fn encode_query_component(input: &str) -> String {
    input
        .chars()
        .flat_map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => alloc::vec![c],
            ' ' => alloc::vec!['+'],
            c => alloc::format!("%{:02X}", c as u8).chars().collect(),
        })
        .collect()
}

fn append_instance_scope_query(
    url: &mut String,
    directory: &Option<String>,
    workspace: &Option<String>,
) {
    let mut first = true;

    if let Some(directory) = directory {
        url.push(if first { '?' } else { '&' });
        first = false;
        url.push_str("directory=");
        url.push_str(&encode_query_component(directory));
    }

    if let Some(workspace) = workspace {
        url.push(if first { '?' } else { '&' });
        url.push_str("workspace=");
        url.push_str(&encode_query_component(workspace));
    }
}

fn global_event_url(base_url: &str) -> String {
    alloc::format!("{base_url}/global/event")
}

fn sync_history_url(base_url: &str, scope: &EventScope) -> String {
    let mut url = alloc::format!("{base_url}/sync/history");
    append_instance_scope_query(&mut url, &scope.directory, &scope.workspace);
    url
}

#[derive(Deserialize)]
struct ConfigProvidersResponse {
    #[serde(default)]
    providers: Vec<ProviderModels>,
}

#[derive(Deserialize)]
struct ProviderModels {
    id: String,
    #[serde(default)]
    models: BTreeMap<String, ProviderModelSummary>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderModelSummary {
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "providerID", default)]
    provider_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    capabilities: Option<ModelCapabilities>,
}

fn parse_config_provider_models(body: &[u8]) -> Result<Vec<ModelSummary>> {
    let response: ConfigProvidersResponse = serde_json::from_slice(body).map_err(parse_err)?;
    let mut models = Vec::new();
    for provider in response.providers {
        for (model_key, model) in provider.models {
            models.push(ModelSummary {
                id: model.id.unwrap_or(model_key),
                provider_id: model.provider_id.unwrap_or_else(|| provider.id.clone()),
                name: model.name,
                family: model.family,
                status: model.status,
                capabilities: model.capabilities,
            });
        }
    }
    Ok(models)
}

fn parse_api_models(body: &[u8]) -> Result<Vec<ModelSummary>> {
    serde_json::from_slice(body).map_err(parse_err)
}

fn parse_submission_receipt(body: &[u8]) -> Result<SubmissionReceipt> {
    serde_json::from_slice(body).map_err(parse_err)
}

fn accepted_submission_receipt(session_id: &SessionId, agent: Option<&str>) -> SubmissionReceipt {
    SubmissionReceipt {
        info: AssistantMessage {
            id: "pending".into(),
            session_id: session_id.clone(),
            role: MessageRole::Assistant,
            time: MessageTime {
                created: 0,
                completed: None,
            },
            parent_id: None,
            model_id: "pending".into(),
            provider_id: "pending".into(),
            mode: "primary".into(),
            agent: agent.unwrap_or("build").into(),
            cost: 0.0,
        },
        parts: Vec::new(),
    }
}

fn parse_sync_history(body: &[u8], scope: &EventScope) -> Result<SyncHistoryBatch> {
    let records: Vec<serde_json::Value> = serde_json::from_slice(body).map_err(parse_err)?;
    let mut batch = SyncHistoryBatch::default();

    for record in records {
        match stream::parse_sync_history_record(&record) {
            Some(Ok(mut envelope)) => {
                if envelope.scope == EventScope::default() {
                    envelope.scope = scope.clone();
                }
                if let Some(cursor) = &envelope.cursor {
                    if let (Some(aggregate_id), Some(seq)) = (&cursor.aggregate_id, cursor.seq) {
                        let entry = batch
                            .known_sequences
                            .entry(aggregate_id.clone())
                            .or_insert(0);
                        *entry = (*entry).max(seq);
                    }
                }
                batch.envelopes.push(envelope);
            }
            Some(Err(error)) => return Err(error),
            None => {}
        }
    }

    Ok(batch)
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
            let response = handle.send(&mut self.header_buf).await.map_err(conn_err)?;
            if !response.status.is_successful() {
                let status = response.status.0;
                let b = read_body_to_vec(response).await?;
                return Err(api_err(status, &b));
            }
            read_body_to_vec(response).await
        } else {
            let mut handle = client.request(method, url).await.map_err(conn_err)?;
            let response = handle.send(&mut self.header_buf).await.map_err(conn_err)?;
            if !response.status.is_successful() {
                let status = response.status.0;
                let b = read_body_to_vec(response).await?;
                return Err(api_err(status, &b));
            }
            read_body_to_vec(response).await
        }
    }
}

fn incremental_sse_stream<
    T: embedded_nal_async::TcpConnect + 'static,
    D: embedded_nal_async::Dns + 'static,
>(
    transport: &'static T,
    dns: &'static D,
    url: String,
) -> BufferedStream {
    BufferedStream::live(move |sink| async move {
        let mut header_buf = alloc::vec![0u8; HTTP_HEADER_BUF_SIZE];
        let mut read_buf = alloc::vec![0u8; SSE_READ_BUF_SIZE];
        let mut parser = SseParser::new();
        let mut client = HttpClient::new(transport, dns);

        let mut handle = match client.request(Method::GET, &url).await {
            Ok(handle) => handle.headers(&SSE_HEADERS),
            Err(error) => {
                sink.push(Err(conn_err(error)));
                sink.finish();
                return;
            }
        };

        let response = match handle.send(&mut header_buf).await {
            Ok(response) => response,
            Err(error) => {
                sink.push(Err(conn_err(error)));
                sink.finish();
                return;
            }
        };

        if !response.status.is_successful() {
            let status = response.status.0;
            match read_body_to_vec(response).await {
                Ok(body) => sink.push(Err(api_err(status, &body))),
                Err(error) => sink.push(Err(conn_err(error))),
            }
            sink.finish();
            return;
        }

        let mut body = response.body().reader();
        loop {
            match body.read(&mut read_buf).await {
                Ok(0) => {
                    sink.finish();
                    return;
                }
                Ok(read) => sink.extend(parser.feed(&read_buf[..read])),
                Err(error) => {
                    sink.push(Err(conn_err(error)));
                    sink.finish();
                    return;
                }
            }
        }
    })
}

// --- Backend trait implementation ---

impl<T: embedded_nal_async::TcpConnect + 'static, D: embedded_nal_async::Dns + 'static> Backend
    for OpenCodeBackend<T, D>
{
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

    async fn create_session(&mut self, title: &str, session_directory: &str) -> Result<Session> {
        let url = alloc::format!("{}/session", self.base_url);
        let body = ocpncord_backend::CreateSessionBody {
            title,
            directory: session_directory,
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
        let body = ocpncord_backend::UpdateSessionBody { title };
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

    async fn submit_prompt(
        &mut self,
        id: &SessionId,
        text: &str,
        agent: Option<&str>,
    ) -> Result<SubmissionReceipt> {
        let url = alloc::format!("{}/session/{id}/prompt_async", self.base_url);
        let prompt_body = ocpncord_backend::PromptBody {
            parts: &[ocpncord_backend::TextPartBody {
                type_: "text",
                text,
            }],
            agent,
        };
        let json = serde_json::to_string(&prompt_body).map_err(parse_err)?;
        self.send_get_body(Method::POST, &url, Some(json.as_bytes()))
            .await?;
        Ok(accepted_submission_receipt(id, agent))
    }

    async fn submit_command(
        &mut self,
        id: &SessionId,
        text: &str,
        agent: Option<&str>,
    ) -> Result<SubmissionReceipt> {
        let url = alloc::format!("{}/session/{id}/command", self.base_url);
        let cmd_body = ocpncord_backend::CommandBody {
            command: text,
            arguments: "",
            agent,
        };
        let json = serde_json::to_string(&cmd_body).map_err(parse_err)?;
        let body = self
            .send_get_body(Method::POST, &url, Some(json.as_bytes()))
            .await?;
        parse_submission_receipt(&body)
    }

    async fn reply_permission(&mut self, reply: &PermissionReply) -> Result<()> {
        let url = alloc::format!("{}/permission/{}/reply", self.base_url, reply.request_id);
        let body = ocpncord_backend::PermissionReplyBody {
            reply: reply.reply.as_str(),
        };
        let json = serde_json::to_string(&body).map_err(parse_err)?;
        self.send_get_body(Method::POST, &url, Some(json.as_bytes()))
            .await?;
        Ok(())
    }

    async fn reply_question(&mut self, reply: &QuestionReply) -> Result<()> {
        let url = alloc::format!("{}/question/{}/reply", self.base_url, reply.request_id);
        let body = ocpncord_backend::QuestionReplyBody {
            answers: reply.answers.as_slice(),
        };
        let json = serde_json::to_string(&body).map_err(parse_err)?;
        self.send_get_body(Method::POST, &url, Some(json.as_bytes()))
            .await?;
        Ok(())
    }

    async fn reject_question(&mut self, request_id: &str) -> Result<()> {
        let url = alloc::format!("{}/question/{request_id}/reject", self.base_url);
        self.send_get_body(Method::POST, &url, None).await?;
        Ok(())
    }

    async fn list_agents(&mut self) -> Result<Vec<Agent>> {
        let url = alloc::format!("{}/agent", self.base_url);
        let body = self.send_get_body(Method::GET, &url, None).await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn find_text(&mut self, pattern: &str) -> Result<Vec<TextMatch>> {
        let encoded = encode_query_component(pattern);
        let url = alloc::format!("{}/find?pattern={}", self.base_url, encoded);
        let body = self.send_get_body(Method::GET, &url, None).await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn subscribe_live(&mut self) -> Result<Self::EventStream> {
        let url = global_event_url(&self.base_url);
        Ok(incremental_sse_stream(self.transport, self.dns, url))
    }

    async fn sync_history(&mut self, request: &SyncHistoryRequest) -> Result<SyncHistoryBatch> {
        let url = sync_history_url(&self.base_url, &request.scope);
        let json = serde_json::to_string(&request.known_sequences).map_err(parse_err)?;
        let body = self
            .send_get_body(Method::POST, &url, Some(json.as_bytes()))
            .await?;
        parse_sync_history(&body, &request.scope)
    }

    async fn get_config(&mut self) -> Result<Config> {
        let url = alloc::format!("{}/global/config", self.base_url);
        let body = self.send_get_body(Method::GET, &url, None).await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn list_models(&mut self) -> Result<Vec<ModelSummary>> {
        let url = alloc::format!("{}/config/providers", self.base_url);
        match self.send_get_body(Method::GET, &url, None).await {
            Ok(body) => parse_config_provider_models(&body),
            Err(error) if should_fallback_to_api_models(&error) => {
                let url = alloc::format!("{}/api/model", self.base_url);
                let body = self.send_get_body(Method::GET, &url, None).await?;
                parse_api_models(&body)
            }
            Err(error) => Err(error),
        }
    }

    async fn set_auth(&mut self, provider: &str, api_key: &str) -> Result<()> {
        let url = alloc::format!("{}/auth/{provider}", self.base_url);
        let body = ocpncord_backend::AuthBody {
            type_: "api",
            key: api_key,
        };
        let json = serde_json::to_string(&body).map_err(parse_err)?;
        self.send_get_body(Method::PUT, &url, Some(json.as_bytes()))
            .await?;
        Ok(())
    }

    async fn set_config(&mut self, config: &Config) -> Result<Config> {
        let url = alloc::format!("{}/global/config", self.base_url);
        let body = ocpncord_backend::ConfigBody {
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
        let body = ocpncord_backend::LogBody { level, message };
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

#[cfg(test)]
mod tests {
    use super::*;
    use core::net::IpAddr;
    use core::result::Result as CoreResult;

    use embedded_io_async::{ErrorType, Read};
    use embedded_nal_async::{AddrType, Dns, TcpConnect};
    use futures::StreamExt;
    use ocpncord_backend::{PermissionReply, QuestionReply};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::oneshot;
    use tokio::time::{timeout, Duration};

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
            if let Ok(ip) = host.parse::<IpAddr>() {
                return Ok(ip);
            }

            let addrs = tokio::net::lookup_host((host, 0)).await?;
            let addrs: Vec<std::net::SocketAddr> = addrs.collect();
            let addr = match addr_type {
                AddrType::IPv4 => addrs.iter().find(|addr| addr.is_ipv4()),
                AddrType::IPv6 => addrs.iter().find(|addr| addr.is_ipv6()),
                AddrType::Either => addrs
                    .iter()
                    .find(|addr| addr.is_ipv4())
                    .or_else(|| addrs.iter().find(|addr| addr.is_ipv6())),
            };
            match addr {
                Some(addr) => Ok(addr.ip()),
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

    fn backend(base_url: &str) -> OpenCodeBackend<StdTcp, StdDns> {
        static TCP: StdTcp = StdTcp;
        static DNS: StdDns = StdDns;
        OpenCodeBackend::new(base_url, &TCP, &DNS)
    }

    fn content_length(headers: &str) -> usize {
        headers
            .split("\r\n")
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.eq_ignore_ascii_case("content-length") {
                    value.trim().parse().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0)
    }

    async fn spawn_capture_server(
        response_body: &'static str,
    ) -> (String, oneshot::Receiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = oneshot::channel();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut total_len = None;

            loop {
                let mut chunk = [0u8; 1024];
                let read = stream.read(&mut chunk).await.unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);

                if total_len.is_none() {
                    if let Some(header_end) =
                        request.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        let header_end = header_end + 4;
                        let headers = String::from_utf8_lossy(&request[..header_end]);
                        total_len = Some(header_end + content_length(&headers));
                    }
                }

                if let Some(total_len) = total_len {
                    if request.len() >= total_len {
                        break;
                    }
                }
            }

            let response = alloc::format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body,
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
            let _ = tx.send(String::from_utf8(request).unwrap());
        });

        (alloc::format!("http://127.0.0.1:{}", addr.port()), rx)
    }

    fn http_response(status_line: &str, body: &str) -> String {
        alloc::format!(
            "{status_line}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        )
    }

    async fn spawn_response_sequence_server(
        responses: Vec<String>,
    ) -> (String, oneshot::Receiver<Vec<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = oneshot::channel();

        tokio::spawn(async move {
            let mut requests = Vec::new();

            for response in responses {
                let accept = timeout(Duration::from_millis(500), listener.accept()).await;
                let Ok(Ok((mut stream, _))) = accept else {
                    break;
                };

                let mut request = Vec::new();
                let mut total_len = None;

                loop {
                    let mut chunk = [0u8; 1024];
                    let read = stream.read(&mut chunk).await.unwrap();
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..read]);

                    if total_len.is_none() {
                        if let Some(header_end) =
                            request.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let header_end = header_end + 4;
                            let headers = String::from_utf8_lossy(&request[..header_end]);
                            total_len = Some(header_end + content_length(&headers));
                        }
                    }

                    if let Some(total_len) = total_len {
                        if request.len() >= total_len {
                            break;
                        }
                    }
                }

                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
                requests.push(String::from_utf8(request).unwrap());
            }

            let _ = tx.send(requests);
        });

        (alloc::format!("http://127.0.0.1:{}", addr.port()), rx)
    }

    async fn spawn_streaming_sse_server(
        first_chunk: &'static str,
    ) -> (String, oneshot::Sender<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = oneshot::channel();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();

            loop {
                let mut chunk = [0u8; 1024];
                let read = stream.read(&mut chunk).await.unwrap();
                if read == 0 {
                    return;
                }
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }

            let response = alloc::format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: keep-alive\r\n\r\n{}",
                first_chunk,
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.flush().await.unwrap();

            let _ = rx.await;
            stream.shutdown().await.unwrap();
        });

        (alloc::format!("http://127.0.0.1:{}", addr.port()), tx)
    }

    async fn spawn_capture_streaming_sse_server(
        first_chunk: &'static str,
    ) -> (String, oneshot::Receiver<String>, oneshot::Sender<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (request_tx, request_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();

            loop {
                let mut chunk = [0u8; 1024];
                let read = stream.read(&mut chunk).await.unwrap();
                if read == 0 {
                    return;
                }
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }

            let _ = request_tx.send(String::from_utf8(request).unwrap());

            let response = alloc::format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: keep-alive\r\n\r\n{}",
                first_chunk,
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.flush().await.unwrap();

            let _ = release_rx.await;
            stream.shutdown().await.unwrap();
        });

        (
            alloc::format!("http://127.0.0.1:{}", addr.port()),
            request_rx,
            release_tx,
        )
    }

    #[test]
    fn model_list_parses_compact_fields_and_ignores_heavy_payloads() {
        let raw = br#"[
            {
                "id": "anthropic/claude-sonnet-4",
                "providerID": "openrouter",
                "name": "Claude Sonnet 4",
                "family": "claude",
                "status": "available",
                "capabilities": {
                    "tools": true,
                    "attachment": false,
                    "input": ["text"],
                    "output": ["text"]
                },
                "options": {
                    "temperature": {"type": "number"},
                    "topP": {"type": "number"}
                }
            }
        ]"#;

        let models = super::parse_api_models(raw).unwrap();

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "anthropic/claude-sonnet-4");
        assert_eq!(models[0].provider_id, "openrouter");
        assert_eq!(models[0].name.as_deref(), Some("Claude Sonnet 4"));
        assert_eq!(
            models[0]
                .capabilities
                .as_ref()
                .and_then(|caps| caps.tool_call),
            Some(true)
        );
    }

    #[test]
    fn config_provider_models_include_opencode_zen_entries() {
        let raw = br#"{
            "default": "opencode/big-pickle",
            "providers": [
                {
                    "id": "opencode",
                    "name": "OpenCode Zen",
                    "models": {
                        "big-pickle": {
                            "id": "big-pickle",
                            "providerID": "opencode",
                            "name": "Big Pickle",
                            "family": "big-pickle",
                            "status": "active",
                            "capabilities": {
                                "reasoning": true,
                                "toolcall": true,
                                "attachment": false,
                                "input": {"text": true},
                                "output": {"text": true}
                            },
                            "options": {},
                            "variants": {}
                        }
                    }
                }
            ]
        }"#;

        let models = super::parse_config_provider_models(raw).unwrap();

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "big-pickle");
        assert_eq!(models[0].provider_id, "opencode");
        assert_eq!(models[0].name.as_deref(), Some("Big Pickle"));
        assert_eq!(models[0].family.as_deref(), Some("big-pickle"));
        assert_eq!(
            models[0]
                .capabilities
                .as_ref()
                .and_then(|caps| caps.tool_call),
            Some(true)
        );
    }

    #[tokio::test]
    async fn list_models_prefers_config_providers_endpoint() {
        let config_providers_body = r#"{
            "providers": [
                {
                    "id": "opencode",
                    "models": {
                        "big-pickle": {
                            "id": "big-pickle",
                            "providerID": "opencode",
                            "name": "Big Pickle",
                            "family": "big-pickle",
                            "status": "active"
                        }
                    }
                }
            ]
        }"#;
        let (base_url, requests_rx) = spawn_response_sequence_server(vec![http_response(
            "HTTP/1.1 200 OK",
            config_providers_body,
        )])
        .await;
        let mut backend = backend(&base_url);

        let models = backend.list_models().await.unwrap();
        let requests = requests_rx.await.unwrap();

        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("GET /config/providers HTTP/1.1"));
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "big-pickle");
        assert_eq!(models[0].provider_id, "opencode");
    }

    #[tokio::test]
    async fn list_models_falls_back_to_api_model_on_404() {
        let api_models_body = r#"[
            {
                "id": "anthropic/claude-sonnet-4",
                "providerID": "openrouter",
                "name": "Claude Sonnet 4",
                "family": "claude",
                "status": "available"
            }
        ]"#;
        let (base_url, requests_rx) = spawn_response_sequence_server(vec![
            http_response("HTTP/1.1 404 Not Found", "missing"),
            http_response("HTTP/1.1 200 OK", api_models_body),
        ])
        .await;
        let mut backend = backend(&base_url);

        let models = backend.list_models().await.unwrap();
        let requests = requests_rx.await.unwrap();

        assert_eq!(requests.len(), 2);
        assert!(requests[0].contains("GET /config/providers HTTP/1.1"));
        assert!(requests[1].contains("GET /api/model HTTP/1.1"));
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "anthropic/claude-sonnet-4");
        assert_eq!(models[0].provider_id, "openrouter");
    }

    #[tokio::test]
    async fn list_models_reads_config_providers_response_larger_than_header_buffer() {
        let model_count = 160;
        let mut large_body = String::from(r#"{"providers":[{"id":"opencode","models":{"#);
        for index in 0..model_count {
            if index > 0 {
                large_body.push(',');
            }
            large_body.push_str(&alloc::format!(
                r#""model-{index}":{{"id":"model-{index}","providerID":"opencode","name":"Model {index}","family":"large","status":"active"}}"#
            ));
        }
        large_body.push_str(r#"}}]}"#);
        assert!(large_body.len() > HTTP_HEADER_BUF_SIZE);

        let (base_url, requests_rx) =
            spawn_response_sequence_server(vec![http_response("HTTP/1.1 200 OK", &large_body)])
                .await;
        let mut backend = backend(&base_url);

        let models = backend.list_models().await.unwrap();
        let requests = requests_rx.await.unwrap();

        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("GET /config/providers HTTP/1.1"));
        assert_eq!(models.len(), model_count);
        assert_eq!(models[0].id, "model-0");
        assert_eq!(models[0].provider_id, "opencode");
        assert!(models.iter().any(|model| model.id == "model-159"));
    }

    #[tokio::test]
    async fn list_models_does_not_fallback_on_malformed_config_providers_response() {
        let malformed_body = "x".repeat(HTTP_HEADER_BUF_SIZE + 1);
        let fallback_body = r#"[
            {
                "id": "should-not-be-requested",
                "providerID": "fallback",
                "name": "Fallback"
            }
        ]"#;
        let (base_url, requests_rx) = spawn_response_sequence_server(vec![
            http_response("HTTP/1.1 200 OK", &malformed_body),
            http_response("HTTP/1.1 200 OK", fallback_body),
        ])
        .await;
        let mut backend = backend(&base_url);

        let error = backend.list_models().await.unwrap_err();
        let requests = requests_rx.await.unwrap();

        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("GET /config/providers HTTP/1.1"));
        assert!(matches!(error, BackendError::Parse { .. }));
    }

    #[tokio::test]
    async fn reply_permission_uses_request_id_route_and_json_body() {
        let (base_url, request_rx) = spawn_capture_server("").await;
        let mut backend = backend(&base_url);

        backend
            .reply_permission(&PermissionReply {
                session_id: "session-1".into(),
                request_id: "permission-1".into(),
                reply: "once".into(),
            })
            .await
            .unwrap();

        let request = request_rx.await.unwrap();
        assert!(request.contains("POST /permission/permission-1/reply HTTP/1.1"));
        assert_eq!(
            request.split("\r\n\r\n").nth(1).unwrap_or(""),
            "{\"reply\":\"once\"}"
        );
    }

    #[tokio::test]
    async fn reply_question_uses_request_id_route_and_nested_answers_body() {
        let (base_url, request_rx) = spawn_capture_server("").await;
        let mut backend = backend(&base_url);

        backend
            .reply_question(&QuestionReply {
                session_id: "session-1".into(),
                request_id: "question-1".into(),
                answers: vec![vec!["A".into()], vec!["custom".into(), "extra".into()]],
            })
            .await
            .unwrap();

        let request = request_rx.await.unwrap();
        assert!(request.contains("POST /question/question-1/reply HTTP/1.1"));
        assert_eq!(
            request.split("\r\n\r\n").nth(1).unwrap_or(""),
            "{\"answers\":[[\"A\"],[\"custom\",\"extra\"]]}"
        );
    }

    #[tokio::test]
    async fn reject_question_uses_request_id_route_without_body() {
        let (base_url, request_rx) = spawn_capture_server("").await;
        let mut backend = backend(&base_url);

        backend.reject_question("question-1").await.unwrap();

        let request = request_rx.await.unwrap();
        assert!(request.contains("POST /question/question-1/reject HTTP/1.1"));
        assert_eq!(request.split("\r\n\r\n").nth(1).unwrap_or(""), "");
    }

    #[tokio::test]
    async fn submit_prompt_uses_async_route_and_returns_accepted_receipt() {
        let (base_url, request_rx) = spawn_capture_server("").await;
        let mut backend = backend(&base_url);

        let receipt = backend
            .submit_prompt(&"ses_1".into(), "hello world", Some("builder"))
            .await
            .unwrap();

        assert_eq!(receipt.info.id, "pending");
        assert_eq!(receipt.info.session_id, "ses_1");
        assert_eq!(receipt.info.agent, "builder");
        assert!(receipt.parts.is_empty());

        let request = request_rx.await.unwrap();
        assert!(request.contains("POST /session/ses_1/prompt_async HTTP/1.1"));
        assert_eq!(
            request.split("\r\n\r\n").nth(1).unwrap_or(""),
            "{\"parts\":[{\"type\":\"text\",\"text\":\"hello world\"}],\"agent\":\"builder\"}"
        );
    }

    #[tokio::test]
    async fn submit_command_returns_created_message_receipt() {
        let response_body = r#"{"info":{"id":"msg_2","sessionID":"ses_2","role":"assistant","time":{"created":2},"parentID":"msg_1","modelID":"model-2","providerID":"provider-2","mode":"command","agent":"runner","cost":0.0},"parts":[]}"#;
        let (base_url, request_rx) = spawn_capture_server(response_body).await;
        let mut backend = backend(&base_url);

        let receipt = backend
            .submit_command(&"ses_2".into(), "build", Some("runner"))
            .await
            .unwrap();

        assert_eq!(receipt.info.id, "msg_2");
        assert_eq!(receipt.info.session_id, "ses_2");
        assert_eq!(receipt.info.agent, "runner");
        assert!(receipt.parts.is_empty());

        let request = request_rx.await.unwrap();
        assert!(request.contains("POST /session/ses_2/command HTTP/1.1"));
        assert_eq!(
            request.split("\r\n\r\n").nth(1).unwrap_or(""),
            "{\"command\":\"build\",\"arguments\":\"\",\"agent\":\"runner\"}"
        );
    }

    #[tokio::test]
    async fn subscribe_live_uses_global_event_endpoint() {
        let (base_url, request_rx, release_tx) = spawn_capture_streaming_sse_server(
            "data: {\"directory\":\"/tmp/project\",\"workspace\":\"wrk_1\",\"payload\":{\"type\":\"server.connected\",\"properties\":{}}}\n\n",
        )
        .await;
        let mut backend = backend(&base_url);

        let mut stream = backend.subscribe_live().await.unwrap();

        let envelope = timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("stream should yield before the SSE connection closes")
            .expect("stream should produce an event")
            .expect("event should parse successfully");

        assert!(matches!(envelope.event, BackendEvent::ServerConnected));
        assert_eq!(envelope.scope.directory.as_deref(), Some("/tmp/project"));
        assert_eq!(envelope.scope.workspace.as_deref(), Some("wrk_1"));

        let request = request_rx.await.unwrap();
        assert!(request.contains("GET /global/event HTTP/1.1"));

        let _ = release_tx.send(());
    }

    #[tokio::test]
    async fn subscribe_yields_event_before_connection_closes() {
        let (base_url, release_tx) = spawn_streaming_sse_server(
            "data: {\"type\":\"server.connected\",\"properties\":{}}\n\n",
        )
        .await;
        let mut backend = backend(&base_url);

        let mut stream = backend.subscribe_live().await.unwrap();
        let envelope = timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("stream should yield before the SSE connection closes")
            .expect("stream should produce an event")
            .expect("event should parse successfully");

        assert!(matches!(envelope.event, BackendEvent::ServerConnected));

        let _ = release_tx.send(());
    }

    #[tokio::test]
    async fn sync_history_parses_sync_records() {
        let response_body = "[{\"type\":\"message.part.updated.1\",\"id\":\"evt_part\",\"seq\":1,\"aggregateID\":\"ses1\",\"data\":{\"sessionID\":\"ses1\",\"part\":{\"id\":\"prt1\",\"sessionID\":\"ses1\",\"messageID\":\"msg1\",\"type\":\"text\",\"text\":\"Hello\"},\"time\":0}},{\"type\":\"message.updated.1\",\"id\":\"evt_done\",\"seq\":2,\"aggregateID\":\"ses1\",\"data\":{\"sessionID\":\"ses1\",\"info\":{\"id\":\"msg1\",\"sessionID\":\"ses1\",\"role\":\"assistant\",\"time\":{\"created\":0},\"parentID\":null,\"modelID\":\"mock/model\",\"providerID\":\"mock\",\"mode\":\"default\",\"agent\":\"build\",\"cost\":0.0}}}]";
        let (base_url, _) = spawn_capture_server(response_body).await;
        let mut backend = backend(&base_url);

        let batch = backend
            .sync_history(&SyncHistoryRequest::default())
            .await
            .unwrap();

        assert_eq!(batch.envelopes.len(), 2);
        assert!(matches!(
            batch.envelopes[0].event,
            BackendEvent::MessagePartUpdated { .. }
        ));
        assert!(matches!(
            batch.envelopes[1].event,
            BackendEvent::MessageUpdated { .. }
        ));
        assert_eq!(batch.known_sequences.get("ses1"), Some(&2));
    }

    #[tokio::test]
    async fn sync_history_uses_instance_scope_query() {
        let (base_url, request_rx) = spawn_capture_server("[]").await;
        let mut backend = backend(&base_url);
        let mut request = SyncHistoryRequest::default();
        request.scope = EventScope::instance(Some("/tmp/project".into()), Some("wrk_1".into()));

        let _ = backend.sync_history(&request).await.unwrap();

        let request = request_rx.await.unwrap();
        assert!(request
            .contains("POST /sync/history?directory=%2Ftmp%2Fproject&workspace=wrk_1 HTTP/1.1"));
        assert!(request.contains("\r\n\r\n{}"));
    }
}
