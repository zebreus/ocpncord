#![cfg_attr(not(test), no_std)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![allow(async_fn_in_trait)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use embedded_io_async::Read;
use ocpncord_backend::*;
use reqwless::client::{HttpClient, TlsConfig, TlsVerify};
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
const DEFAULT_SERVER_USERNAME: &str = "opencode";

/// TLS read buffer: must hold a full incoming TLS record (16 KiB plaintext +
/// overhead). The server chooses its record size, so this cannot be shrunk
/// without risking a handshake/read failure on large records.
const TLS_READ_BUF_SIZE: usize = 16 * 1024 + 256;
/// TLS write buffer: holds one outgoing TLS record before it is encrypted and
/// flushed. embedded-tls chunks outgoing application data through this buffer
/// (a large request body is split across several records), so the only hard
/// floor is fitting the ClientHello (a few hundred bytes) plus record overhead.
/// 4 KiB leaves generous headroom while saving 12 KiB per connection versus a
/// full-size record buffer.
const TLS_WRITE_BUF_SIZE: usize = 4 * 1024;

/// Whether a server URL uses `https://` and therefore needs a TLS-configured
/// client. Plain `http://` connections skip the TLS config entirely so they
/// allocate none of the (large) TLS record buffers.
fn url_is_https(url: &str) -> bool {
    url.starts_with("https://")
}

/// An HTTP client for the opencode server REST API.
///
/// Generic over the TCP transport and DNS resolver, allowing it to work
/// with any platform that implements `embedded-nal-async` traits.
pub struct OpenCodeBackend<
    T: embedded_nal_async::TcpConnect + 'static,
    D: embedded_nal_async::Dns + 'static,
> {
    server_connection: ServerConnection,
    header_buf: Vec<u8>,
    transport: &'static T,
    dns: &'static D,
    /// Unpredictable per-process base seed for the TLS RNG, supplied by the
    /// host (`backend-opencode` is `no_std` and has no entropy source).
    tls_seed: u64,
    /// Incremented per connection so each TLS handshake gets a distinct seed
    /// (reusing one seed would reuse the client's ephemeral key material).
    tls_seed_counter: u64,
}

impl<T: embedded_nal_async::TcpConnect + 'static, D: embedded_nal_async::Dns + 'static>
    OpenCodeBackend<T, D>
{
    /// `seed` is an unpredictable random value used to seed the TLS RNG. It
    /// must come from the host's entropy source (e.g. `/dev/urandom`).
    pub fn new(base_url: &str, transport: &'static T, dns: &'static D, seed: u64) -> Self {
        Self {
            server_connection: normalize_server_connection(ServerConnection::unauthenticated(
                base_url,
            )),
            header_buf: vec![0; HTTP_HEADER_BUF_SIZE],
            transport,
            dns,
            tls_seed: seed,
            tls_seed_counter: 0,
        }
    }

    /// `seed` is an unpredictable random value used to seed the TLS RNG. It
    /// must come from the host's entropy source (e.g. `/dev/urandom`).
    pub fn new_with_connection(
        connection: ServerConnection,
        transport: &'static T,
        dns: &'static D,
        seed: u64,
    ) -> Self {
        Self {
            server_connection: normalize_server_connection(connection),
            header_buf: vec![0; HTTP_HEADER_BUF_SIZE],
            transport,
            dns,
            tls_seed: seed,
            tls_seed_counter: 0,
        }
    }

    /// Returns a fresh TLS RNG seed for the next connection, advancing the
    /// counter so consecutive connections never share ephemeral key material.
    fn next_tls_seed(&mut self) -> u64 {
        let seed = self
            .tls_seed
            .wrapping_add(self.tls_seed_counter.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        self.tls_seed_counter = self.tls_seed_counter.wrapping_add(1);
        seed
    }

    async fn health_for_base_url(
        &mut self,
        base_url: &str,
        connection: Option<&ServerConnection>,
    ) -> Result<Health> {
        let url = alloc::format!("{}/global/health", base_url.trim_end_matches('/'));
        let body = self
            .send_get_body(Method::GET, &url, None, connection.cloned())
            .await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }
}

fn normalize_server_connection(mut connection: ServerConnection) -> ServerConnection {
    connection.url = connection.url.trim_end_matches('/').into();
    connection.username = connection.username.filter(|username| !username.is_empty());
    connection.password = connection.password.filter(|password| !password.is_empty());
    if connection.password.is_none() {
        connection.username = None;
    }
    connection
}

fn server_basic_auth(connection: &ServerConnection) -> Option<(String, String)> {
    let password = connection.password.as_ref()?;
    let username = connection
        .username
        .clone()
        .unwrap_or_else(|| DEFAULT_SERVER_USERNAME.into());
    Some((username, password.clone()))
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
    let receipt: CreatedSubmissionReceipt = serde_json::from_slice(body).map_err(parse_err)?;
    Ok(SubmissionReceipt::Created(receipt))
}

fn accepted_submission_receipt(session_id: &SessionId, agent: Option<&str>) -> SubmissionReceipt {
    SubmissionReceipt::Accepted {
        session_id: session_id.clone(),
        agent: agent.map(|value| value.into()),
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
        connection: Option<ServerConnection>,
    ) -> Result<Vec<u8>> {
        // TLS buffers are scope-local and must outlive `client` (declared first
        // for drop order). `http://` URLs skip the TLS config entirely, so the
        // buffers stay unallocated for plain-HTTP connections.
        let mut tls_read;
        let mut tls_write;
        let mut client = if url_is_https(url) {
            let seed = self.next_tls_seed();
            tls_read = alloc::vec![0u8; TLS_READ_BUF_SIZE];
            tls_write = alloc::vec![0u8; TLS_WRITE_BUF_SIZE];
            // We intentionally do not verify the server certificate: this client
            // talks to a user-configured opencode server (often behind a
            // reverse proxy) where we are not defending against MITM, and
            // dropping verification avoids the RSA/cert-chain crypto entirely
            // (smaller binary, less handshake stack/heap).
            let tls = TlsConfig::new(seed, &mut tls_read, &mut tls_write, TlsVerify::None);
            HttpClient::new_with_tls(self.transport, self.dns, tls)
        } else {
            HttpClient::new(self.transport, self.dns)
        };
        let basic_auth = connection.as_ref().and_then(server_basic_auth);
        if let Some(bytes) = body_bytes {
            let handle = client.request(method, url).await.map_err(conn_err)?;
            let handle = if let Some((username, password)) = basic_auth.as_ref() {
                handle.basic_auth(username, password)
            } else {
                handle
            };
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
            let handle = client.request(method, url).await.map_err(conn_err)?;
            let mut handle = if let Some((username, password)) = basic_auth.as_ref() {
                handle.basic_auth(username, password)
            } else {
                handle
            };
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
    connection: Option<ServerConnection>,
    seed: u64,
) -> BufferedStream {
    BufferedStream::live(move |sink| async move {
        let mut header_buf = alloc::vec![0u8; HTTP_HEADER_BUF_SIZE];
        let mut read_buf = alloc::vec![0u8; SSE_READ_BUF_SIZE];
        let mut parser = SseParser::new();
        // TLS buffers stay unallocated for plain-HTTP (`http://`) connections;
        // they are declared before `client` so they outlive it (drop order).
        let mut tls_read_buf;
        let mut tls_write_buf;
        let mut client = if url_is_https(&url) {
            tls_read_buf = alloc::vec![0u8; TLS_READ_BUF_SIZE];
            tls_write_buf = alloc::vec![0u8; TLS_WRITE_BUF_SIZE];
            // Server certificate is intentionally not verified; see the note in
            // `send_get_body`.
            let tls = TlsConfig::new(seed, &mut tls_read_buf, &mut tls_write_buf, TlsVerify::None);
            HttpClient::new_with_tls(transport, dns, tls)
        } else {
            HttpClient::new(transport, dns)
        };
        let basic_auth = connection.as_ref().and_then(server_basic_auth);

        let mut handle = match client.request(Method::GET, &url).await {
            Ok(handle) => {
                let handle = if let Some((username, password)) = basic_auth.as_ref() {
                    handle.basic_auth(username, password)
                } else {
                    handle
                };
                handle.headers(&SSE_HEADERS)
            }
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

    fn server_connection(&self) -> Option<ServerConnection> {
        Some(self.server_connection.clone())
    }

    async fn test_server_connection(&mut self, connection: &ServerConnection) -> Result<Health> {
        let connection = normalize_server_connection(connection.clone());
        self.health_for_base_url(&connection.url, Some(&connection))
            .await
    }

    async fn set_server_connection(&mut self, connection: ServerConnection) -> Result<()> {
        self.server_connection = normalize_server_connection(connection);
        Ok(())
    }

    async fn health(&mut self) -> Result<Health> {
        let base_url = self.server_connection.url.clone();
        let connection = self.server_connection.clone();
        self.health_for_base_url(&base_url, Some(&connection)).await
    }

    async fn list_sessions(&mut self) -> Result<Vec<Session>> {
        let url = alloc::format!("{}/session", self.server_connection.url);
        let body = self
            .send_get_body(
                Method::GET,
                &url,
                None,
                Some(self.server_connection.clone()),
            )
            .await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn get_session(&mut self, id: &SessionId) -> Result<Session> {
        let url = alloc::format!("{}/session/{id}", self.server_connection.url);
        let body = self
            .send_get_body(
                Method::GET,
                &url,
                None,
                Some(self.server_connection.clone()),
            )
            .await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn create_session(&mut self, title: &str, session_directory: &str) -> Result<Session> {
        let url = alloc::format!("{}/session", self.server_connection.url);
        let body = ocpncord_backend::CreateSessionBody {
            title,
            directory: session_directory,
        };
        let json = serde_json::to_string(&body).map_err(parse_err)?;
        let body = self
            .send_get_body(
                Method::POST,
                &url,
                Some(json.as_bytes()),
                Some(self.server_connection.clone()),
            )
            .await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn delete_session(&mut self, id: &SessionId) -> Result<()> {
        let url = alloc::format!("{}/session/{id}", self.server_connection.url);
        self.send_get_body(
            Method::DELETE,
            &url,
            None,
            Some(self.server_connection.clone()),
        )
        .await?;
        Ok(())
    }

    async fn update_session(&mut self, id: &SessionId, title: &str) -> Result<Session> {
        let url = alloc::format!("{}/session/{id}", self.server_connection.url);
        let body = ocpncord_backend::UpdateSessionBody { title };
        let json = serde_json::to_string(&body).map_err(parse_err)?;
        let body = self
            .send_get_body(
                Method::PATCH,
                &url,
                Some(json.as_bytes()),
                Some(self.server_connection.clone()),
            )
            .await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn children_sessions(&mut self, id: &SessionId) -> Result<Vec<Session>> {
        let url = alloc::format!("{}/session/{id}/children", self.server_connection.url);
        let body = self
            .send_get_body(
                Method::GET,
                &url,
                None,
                Some(self.server_connection.clone()),
            )
            .await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn abort_session(&mut self, id: &SessionId) -> Result<()> {
        let url = alloc::format!("{}/session/{id}/abort", self.server_connection.url);
        self.send_get_body(
            Method::POST,
            &url,
            None,
            Some(self.server_connection.clone()),
        )
        .await?;
        Ok(())
    }

    async fn list_messages(&mut self, id: &SessionId) -> Result<Vec<MessageSummary>> {
        let url = alloc::format!("{}/session/{id}/message", self.server_connection.url);
        let body = self
            .send_get_body(
                Method::GET,
                &url,
                None,
                Some(self.server_connection.clone()),
            )
            .await?;
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
            self.server_connection.url,
            session_id,
            message_id
        );
        let body = self
            .send_get_body(
                Method::GET,
                &url,
                None,
                Some(self.server_connection.clone()),
            )
            .await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn submit_prompt(
        &mut self,
        id: &SessionId,
        text: &str,
        agent: Option<&str>,
    ) -> Result<SubmissionReceipt> {
        let url = alloc::format!("{}/session/{id}/prompt_async", self.server_connection.url);
        let prompt_body = ocpncord_backend::PromptBody {
            parts: &[ocpncord_backend::TextPartBody {
                type_: "text",
                text,
            }],
            agent,
        };
        let json = serde_json::to_string(&prompt_body).map_err(parse_err)?;
        self.send_get_body(
            Method::POST,
            &url,
            Some(json.as_bytes()),
            Some(self.server_connection.clone()),
        )
        .await?;
        Ok(accepted_submission_receipt(id, agent))
    }

    async fn submit_command(
        &mut self,
        id: &SessionId,
        text: &str,
        agent: Option<&str>,
    ) -> Result<SubmissionReceipt> {
        let url = alloc::format!("{}/session/{id}/command", self.server_connection.url);
        let cmd_body = ocpncord_backend::CommandBody {
            command: text,
            arguments: "",
            agent,
        };
        let json = serde_json::to_string(&cmd_body).map_err(parse_err)?;
        let body = self
            .send_get_body(
                Method::POST,
                &url,
                Some(json.as_bytes()),
                Some(self.server_connection.clone()),
            )
            .await?;
        parse_submission_receipt(&body)
    }

    async fn reply_permission(&mut self, reply: &PermissionReply) -> Result<()> {
        let url = alloc::format!(
            "{}/permission/{}/reply",
            self.server_connection.url,
            reply.request_id
        );
        let body = ocpncord_backend::PermissionReplyBody {
            reply: reply.reply.as_str(),
        };
        let json = serde_json::to_string(&body).map_err(parse_err)?;
        self.send_get_body(
            Method::POST,
            &url,
            Some(json.as_bytes()),
            Some(self.server_connection.clone()),
        )
        .await?;
        Ok(())
    }

    async fn reply_question(&mut self, reply: &QuestionReply) -> Result<()> {
        let url = alloc::format!(
            "{}/question/{}/reply",
            self.server_connection.url,
            reply.request_id
        );
        let body = ocpncord_backend::QuestionReplyBody {
            answers: reply.answers.as_slice(),
        };
        let json = serde_json::to_string(&body).map_err(parse_err)?;
        self.send_get_body(
            Method::POST,
            &url,
            Some(json.as_bytes()),
            Some(self.server_connection.clone()),
        )
        .await?;
        Ok(())
    }

    async fn reject_question(&mut self, request_id: &str) -> Result<()> {
        let url = alloc::format!(
            "{}/question/{request_id}/reject",
            self.server_connection.url
        );
        self.send_get_body(
            Method::POST,
            &url,
            None,
            Some(self.server_connection.clone()),
        )
        .await?;
        Ok(())
    }

    async fn list_agents(&mut self) -> Result<Vec<Agent>> {
        let url = alloc::format!("{}/agent", self.server_connection.url);
        let body = self
            .send_get_body(
                Method::GET,
                &url,
                None,
                Some(self.server_connection.clone()),
            )
            .await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn find_text(&mut self, pattern: &str) -> Result<Vec<TextMatch>> {
        let encoded = encode_query_component(pattern);
        let url = alloc::format!("{}/find?pattern={}", self.server_connection.url, encoded);
        let body = self
            .send_get_body(
                Method::GET,
                &url,
                None,
                Some(self.server_connection.clone()),
            )
            .await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn subscribe_live(&mut self) -> Result<Self::EventStream> {
        let url = global_event_url(&self.server_connection.url);
        let seed = self.next_tls_seed();
        Ok(incremental_sse_stream(
            self.transport,
            self.dns,
            url,
            Some(self.server_connection.clone()),
            seed,
        ))
    }

    async fn sync_history(&mut self, request: &SyncHistoryRequest) -> Result<SyncHistoryBatch> {
        let url = sync_history_url(&self.server_connection.url, &request.scope);
        let json = serde_json::to_string(&request.known_sequences).map_err(parse_err)?;
        let body = self
            .send_get_body(
                Method::POST,
                &url,
                Some(json.as_bytes()),
                Some(self.server_connection.clone()),
            )
            .await?;
        parse_sync_history(&body, &request.scope)
    }

    async fn get_config(&mut self) -> Result<Config> {
        let url = alloc::format!("{}/global/config", self.server_connection.url);
        let body = self
            .send_get_body(
                Method::GET,
                &url,
                None,
                Some(self.server_connection.clone()),
            )
            .await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn list_models(&mut self) -> Result<Vec<ModelSummary>> {
        let url = alloc::format!("{}/config/providers", self.server_connection.url);
        match self
            .send_get_body(
                Method::GET,
                &url,
                None,
                Some(self.server_connection.clone()),
            )
            .await
        {
            Ok(body) => parse_config_provider_models(&body),
            Err(error) if should_fallback_to_api_models(&error) => {
                let url = alloc::format!("{}/api/model", self.server_connection.url);
                let body = self
                    .send_get_body(
                        Method::GET,
                        &url,
                        None,
                        Some(self.server_connection.clone()),
                    )
                    .await?;
                parse_api_models(&body)
            }
            Err(error) => Err(error),
        }
    }

    async fn set_auth(&mut self, provider: &str, api_key: &str) -> Result<()> {
        let url = alloc::format!("{}/auth/{provider}", self.server_connection.url);
        let body = ocpncord_backend::AuthBody {
            type_: "api",
            key: api_key,
        };
        let json = serde_json::to_string(&body).map_err(parse_err)?;
        self.send_get_body(
            Method::PUT,
            &url,
            Some(json.as_bytes()),
            Some(self.server_connection.clone()),
        )
        .await?;
        Ok(())
    }

    async fn set_config(&mut self, config: &Config) -> Result<Config> {
        let url = alloc::format!("{}/global/config", self.server_connection.url);
        let body = ocpncord_backend::ConfigBody {
            model: config.model.as_deref(),
            username: config.username.as_deref(),
        };
        let json = serde_json::to_string(&body).map_err(parse_err)?;
        let body = self
            .send_get_body(
                Method::PATCH,
                &url,
                Some(json.as_bytes()),
                Some(self.server_connection.clone()),
            )
            .await?;
        serde_json::from_slice(&body).map_err(parse_err)
    }

    async fn dispose(&mut self) -> Result<()> {
        let url = alloc::format!("{}/global/dispose", self.server_connection.url);
        self.send_get_body(
            Method::POST,
            &url,
            None,
            Some(self.server_connection.clone()),
        )
        .await?;
        Ok(())
    }

    async fn upgrade(&mut self) -> Result<()> {
        let url = alloc::format!("{}/global/upgrade", self.server_connection.url);
        self.send_get_body(
            Method::POST,
            &url,
            None,
            Some(self.server_connection.clone()),
        )
        .await?;
        Ok(())
    }

    async fn log(&mut self, level: &str, message: &str) -> Result<()> {
        let url = alloc::format!("{}/log", self.server_connection.url);
        let body = ocpncord_backend::LogBody { level, message };
        let json = serde_json::to_string(&body).map_err(parse_err)?;
        self.send_get_body(
            Method::POST,
            &url,
            Some(json.as_bytes()),
            Some(self.server_connection.clone()),
        )
        .await?;
        Ok(())
    }

    async fn remove_auth(&mut self, provider: &str) -> Result<()> {
        let url = alloc::format!("{}/auth/{provider}", self.server_connection.url);
        self.send_get_body(
            Method::DELETE,
            &url,
            None,
            Some(self.server_connection.clone()),
        )
        .await?;
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
            // Mirror the production transport: a zero-length read returns
            // immediately instead of awaiting socket readability, which would
            // otherwise hang on a kept-alive connection. See the matching guard
            // (and explanation) in `native/src/main.rs`.
            if buf.is_empty() {
                return Ok(0);
            }
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
        OpenCodeBackend::new(base_url, &TCP, &DNS, 0)
    }

    fn backend_with_connection(connection: ServerConnection) -> OpenCodeBackend<StdTcp, StdDns> {
        static TCP: StdTcp = StdTcp;
        static DNS: StdDns = StdDns;
        OpenCodeBackend::new_with_connection(connection, &TCP, &DNS, 0)
    }

    fn health_body() -> &'static str {
        r#"{"healthy":true,"version":"mock"}"#
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

    /// Serves a single `Transfer-Encoding: chunked` response (no
    /// `Content-Length`) and then deliberately keeps the connection open
    /// without sending anything further — exactly how a Caddy/nginx reverse
    /// proxy fronting the opencode server behaves with HTTP keep-alive. This is
    /// the scenario that used to hang the client: after consuming the
    /// terminating `0\r\n\r\n` chunk, the body reader issues a final
    /// zero-length read that must not block.
    async fn spawn_chunked_keepalive_server(body: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();

            // Drain the request headers.
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

            // Emit the body as two chunks followed by the terminating chunk.
            let (first, second) = body.split_at(body.len() / 2);
            let response = alloc::format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{first}\r\n{:x}\r\n{second}\r\n0\r\n\r\n",
                first.len(),
                second.len(),
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.flush().await.unwrap();

            // Hold the connection open (keep-alive) so any trailing read on the
            // client would block forever if it were not zero-length.
            tokio::time::sleep(Duration::from_secs(30)).await;
        });

        alloc::format!("http://127.0.0.1:{}", addr.port())
    }

    #[tokio::test]
    async fn reads_chunked_response_on_keepalive_connection() {
        let base_url = spawn_chunked_keepalive_server(
            r#"[{"name":"build","mode":"primary"},{"name":"plan","mode":"primary"}]"#,
        )
        .await;
        let mut backend = backend(&base_url);

        let agents = timeout(Duration::from_secs(5), backend.list_agents())
            .await
            .expect("list_agents must not hang on a chunked keep-alive response")
            .expect("list_agents should parse the chunked body");

        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].name, "build");
        assert_eq!(agents[1].name, "plan");
    }

    #[tokio::test]
    async fn server_auth_is_omitted_when_not_configured() {
        let (base_url, request_rx) = spawn_capture_server(health_body()).await;
        let mut backend = backend(&base_url);

        backend.health().await.unwrap();
        let request = request_rx.await.unwrap();

        assert!(request.contains("GET /global/health HTTP/1.1"));
        assert!(!request.contains("Authorization:"));
    }

    #[tokio::test]
    async fn server_auth_uses_default_username_for_health() {
        let (base_url, request_rx) = spawn_capture_server(health_body()).await;
        let mut backend = backend_with_connection(ServerConnection::new(
            base_url,
            None::<String>,
            Some("secret"),
        ));

        backend.health().await.unwrap();
        let request = request_rx.await.unwrap();

        assert!(request.contains("GET /global/health HTTP/1.1"));
        assert!(request.contains("Authorization: Basic b3BlbmNvZGU6c2VjcmV0\r\n"));
    }

    #[tokio::test]
    async fn server_auth_uses_custom_username_for_health() {
        let (base_url, request_rx) = spawn_capture_server(health_body()).await;
        let mut backend = backend_with_connection(ServerConnection::new(
            base_url,
            Some("user"),
            Some("secret"),
        ));

        backend.health().await.unwrap();
        let request = request_rx.await.unwrap();

        assert!(request.contains("Authorization: Basic dXNlcjpzZWNyZXQ=\r\n"));
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

        match receipt {
            SubmissionReceipt::Accepted { session_id, agent } => {
                assert_eq!(session_id, "ses_1");
                assert_eq!(agent.as_deref(), Some("builder"));
            }
            other => panic!("expected accepted receipt, got {other:?}"),
        }

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

        match receipt {
            SubmissionReceipt::Created(receipt) => {
                assert_eq!(receipt.info.id, "msg_2");
                assert_eq!(receipt.info.session_id, "ses_2");
                assert_eq!(receipt.info.agent, "runner");
                assert!(receipt.parts.is_empty());
            }
            other => panic!("expected created receipt, got {other:?}"),
        }

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
    async fn subscribe_live_sends_server_auth_header() {
        let (base_url, request_rx, release_tx) = spawn_capture_streaming_sse_server(
            "data: {\"type\":\"server.connected\",\"properties\":{}}\n\n",
        )
        .await;
        let mut backend = backend_with_connection(ServerConnection::new(
            base_url,
            Some("user"),
            Some("secret"),
        ));

        let mut stream = backend.subscribe_live().await.unwrap();
        let _ = timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("stream should yield before the SSE connection closes")
            .expect("stream should produce an event")
            .expect("event should parse successfully");

        let request = request_rx.await.unwrap();
        assert!(request.contains("GET /global/event HTTP/1.1"));
        assert!(request.contains("Authorization: Basic dXNlcjpzZWNyZXQ=\r\n"));

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
