#![cfg_attr(not(test), no_std)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![allow(async_fn_in_trait)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use futures_core::Stream;
pub use opencode_types::*;

#[cfg(feature = "mock")]
pub mod mock;

// --- Backend event (yielded by streams) ---

#[derive(Debug, Clone)]
pub enum BackendEvent {
    Part { part: Part, delta: Option<String> },
    Error { message: String },
    Done,
}

// --- Backend error ---

#[derive(Debug, Clone)]
pub enum BackendError {
    Connection { message: String },
    Api { status: u16, message: String },
    Timeout,
    Parse { message: String },
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection { message } => write!(f, "connection error: {message}"),
            Self::Api { status, message } => {
                write!(f, "api error ({status}): {message}")
            }
            Self::Timeout => write!(f, "request timed out"),
            Self::Parse { message } => write!(f, "parse error: {message}"),
        }
    }
}

impl fmt::Display for BackendEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Part { part, .. } => write!(f, "part: {part:?}"),
            Self::Error { message } => write!(f, "error: {message}"),
            Self::Done => write!(f, "done"),
        }
    }
}

pub type Result<T> = core::result::Result<T, BackendError>;

// --- The Backend trait ---

pub trait Backend {
    type PromptStream: Stream<Item = Result<BackendEvent>>;
    type EventStream: Stream<Item = Result<BackendEvent>>;

    async fn health(&mut self) -> Result<Health>;

    async fn list_agents(&mut self) -> Result<Vec<Agent>>;

    async fn list_sessions(&mut self) -> Result<Vec<Session>>;
    async fn get_session(&mut self, id: &SessionId) -> Result<Session>;
    async fn create_session(&mut self, title: &str, cwd: &str) -> Result<Session>;
    async fn delete_session(&mut self, id: &SessionId) -> Result<()>;
    async fn update_session(&mut self, id: &SessionId, title: &str) -> Result<Session>;
    async fn children_sessions(&mut self, id: &SessionId) -> Result<Vec<Session>>;
    async fn abort_session(&mut self, id: &SessionId) -> Result<()>;

    async fn list_messages(&mut self, id: &SessionId) -> Result<Vec<MessageSummary>>;
    async fn get_message(
        &mut self,
        session_id: &SessionId,
        message_id: &MessageId,
    ) -> Result<MessageDetail>;

    async fn prompt(&mut self, id: &SessionId, text: &str, agent: Option<&str>) -> Result<Self::PromptStream>;
    async fn command(&mut self, id: &SessionId, text: &str, agent: Option<&str>) -> Result<Self::PromptStream>;

    async fn find_text(&mut self, pattern: &str) -> Result<Vec<TextMatch>>;

    async fn subscribe(&mut self) -> Result<Self::EventStream>;

    async fn get_config(&mut self) -> Result<Config>;

    async fn set_auth(&mut self, provider: &str, api_key: &str) -> Result<()>;
}
