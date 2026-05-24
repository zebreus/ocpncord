use crate::*;
use alloc::string::String;
use alloc::vec::Vec;
use core::pin::Pin;
use core::task::{Context, Poll};
use futures_core::Stream;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockSubmissionCall {
    pub session_id: String,
    pub text: String,
    pub agent: Option<String>,
}

/// A mock backend for testing TUI code without a real server.
pub struct MockBackend {
    pub sessions: Vec<Session>,
    pub messages: Vec<MessageSummary>,
    pub message_detail: Option<MessageDetail>,
    pub health_status: Option<Health>,
    pub server_url: String,
    pub set_server_url_calls: Vec<String>,
    pub config_info: Option<Config>,
    pub models: Option<Vec<ModelSummary>>,
    pub list_models_calls: usize,
    pub agents: Vec<Agent>,
    pub prompt_receipt: Option<SubmissionReceipt>,
    pub command_receipt: Option<SubmissionReceipt>,
    pub live_events: Vec<Result<EventEnvelope>>,
    pub live_event_stream_pending_polls: usize,
    pub sync_history_batch: SyncHistoryBatch,
    pub sync_history_error: Option<BackendError>,
    pub sync_history_requests: Vec<SyncHistoryRequest>,
    pub text_matches: Vec<TextMatch>,
    pub prompt_calls: Vec<MockSubmissionCall>,
    pub command_calls: Vec<MockSubmissionCall>,
    pub find_text_calls: Vec<String>,
    pub fail_create_session: Option<BackendError>,
    pub permission_replies: Vec<PermissionReply>,
    pub question_replies: Vec<QuestionReply>,
    pub rejected_questions: Vec<String>,
}

impl Default for MockBackend {
    fn default() -> Self {
        Self {
            sessions: Vec::new(),
            messages: Vec::new(),
            message_detail: None,
            health_status: Some(Health {
                healthy: true,
                version: "mock".into(),
            }),
            server_url: "http://localhost:4096".into(),
            set_server_url_calls: Vec::new(),
            config_info: Some(Config {
                model: Some("mock/model".into()),
                username: Some("mock-user".into()),
                provider: Default::default(),
                agent: Default::default(),
            }),
            models: None,
            list_models_calls: 0,
            agents: Vec::new(),
            prompt_receipt: None,
            command_receipt: None,
            live_events: Vec::new(),
            live_event_stream_pending_polls: 0,
            sync_history_batch: SyncHistoryBatch::default(),
            sync_history_error: None,
            sync_history_requests: Vec::new(),
            text_matches: Vec::new(),
            prompt_calls: Vec::new(),
            command_calls: Vec::new(),
            find_text_calls: Vec::new(),
            fail_create_session: None,
            permission_replies: Vec::new(),
            question_replies: Vec::new(),
            rejected_questions: Vec::new(),
        }
    }
}

fn default_prompt_submission_receipt(
    session_id: &SessionId,
    agent: Option<&str>,
) -> SubmissionReceipt {
    SubmissionReceipt::Accepted {
        session_id: session_id.clone(),
        agent: agent.map(|value| value.into()),
    }
}

fn default_command_submission_receipt(
    session_id: &SessionId,
    agent: Option<&str>,
) -> SubmissionReceipt {
    SubmissionReceipt::Created(CreatedSubmissionReceipt {
        info: AssistantMessage {
            id: "mock-message-id".into(),
            session_id: session_id.clone(),
            role: MessageRole::Assistant,
            time: MessageTime {
                created: 0,
                completed: None,
            },
            error: None,
            parent_id: None,
            model_id: "mock/model".into(),
            provider_id: "mock".into(),
            mode: "default".into(),
            agent: agent.unwrap_or("mock-agent").into(),
            path: None,
            summary: None,
            cost: 0.0,
            tokens: None,
            structured: None,
            variant: None,
            finish: None,
        },
        parts: Vec::new(),
    })
}

impl Backend for MockBackend {
    type EventStream = MockStream;

    fn server_url(&self) -> Option<&str> {
        Some(&self.server_url)
    }

    async fn test_server_url(&mut self, _url: &str) -> Result<Health> {
        self.health().await.map_err(|_| BackendError::Connection {
            message: "server test failed".into(),
        })
    }

    async fn set_server_url(&mut self, url: &str) -> Result<()> {
        self.health().await?;
        self.server_url = url.trim_end_matches('/').into();
        self.set_server_url_calls.push(self.server_url.clone());
        Ok(())
    }

    async fn health(&mut self) -> Result<Health> {
        self.health_status.clone().ok_or(BackendError::Connection {
            message: "no health stub".into(),
        })
    }

    async fn list_agents(&mut self) -> Result<Vec<Agent>> {
        Ok(self.agents.clone())
    }

    async fn list_sessions(&mut self) -> Result<Vec<Session>> {
        Ok(self.sessions.clone())
    }

    async fn get_session(&mut self, _id: &SessionId) -> Result<Session> {
        self.sessions.first().cloned().ok_or(BackendError::Api {
            status: 404,
            message: "not found".into(),
        })
    }

    async fn create_session(&mut self, title: &str, session_directory: &str) -> Result<Session> {
        if let Some(err) = self.fail_create_session.take() {
            return Err(err);
        }
        let session = Session {
            id: "mock-session-id".into(),
            title: title.into(),
            project_id: "mock-project".into(),
            directory: session_directory.into(),
            path: None,
            parent_id: None,
            time: SessionTime {
                created: 0,
                updated: 0,
                compacting: None,
                archived: None,
            },
            slug: String::new(),
            version: String::new(),
            workspace_id: None,
            summary: None,
            cost: None,
            tokens: None,
            share: None,
            agent: None,
            model: None,
            permission: None,
            revert: None,
        };
        self.sessions.push(session.clone());
        Ok(session)
    }

    async fn delete_session(&mut self, _id: &SessionId) -> Result<()> {
        Ok(())
    }

    async fn update_session(&mut self, id: &SessionId, title: &str) -> Result<Session> {
        Ok(Session {
            id: id.clone(),
            title: title.into(),
            project_id: "mock-project".into(),
            directory: "/mock".into(),
            path: None,
            parent_id: None,
            time: SessionTime {
                created: 0,
                updated: 0,
                compacting: None,
                archived: None,
            },
            slug: String::new(),
            version: String::new(),
            workspace_id: None,
            summary: None,
            cost: None,
            tokens: None,
            share: None,
            agent: None,
            model: None,
            permission: None,
            revert: None,
        })
    }

    async fn children_sessions(&mut self, _id: &SessionId) -> Result<Vec<Session>> {
        Ok(Vec::new())
    }

    async fn abort_session(&mut self, _id: &SessionId) -> Result<()> {
        Ok(())
    }

    async fn list_messages(&mut self, _id: &SessionId) -> Result<Vec<MessageSummary>> {
        Ok(self.messages.clone())
    }

    async fn get_message(
        &mut self,
        _session_id: &SessionId,
        _message_id: &MessageId,
    ) -> Result<MessageDetail> {
        self.message_detail.clone().ok_or(BackendError::Api {
            status: 404,
            message: "not found".into(),
        })
    }

    async fn submit_prompt(
        &mut self,
        id: &SessionId,
        text: &str,
        agent: Option<&str>,
    ) -> Result<SubmissionReceipt> {
        self.prompt_calls.push(MockSubmissionCall {
            session_id: id.clone(),
            text: text.into(),
            agent: agent.map(|value| value.into()),
        });
        Ok(self
            .prompt_receipt
            .clone()
            .unwrap_or_else(|| default_prompt_submission_receipt(id, agent)))
    }

    async fn submit_command(
        &mut self,
        id: &SessionId,
        text: &str,
        agent: Option<&str>,
    ) -> Result<SubmissionReceipt> {
        self.command_calls.push(MockSubmissionCall {
            session_id: id.clone(),
            text: text.into(),
            agent: agent.map(|value| value.into()),
        });
        Ok(self
            .command_receipt
            .clone()
            .unwrap_or_else(|| default_command_submission_receipt(id, agent)))
    }

    async fn reply_permission(&mut self, reply: &PermissionReply) -> Result<()> {
        self.permission_replies.push(reply.clone());
        Ok(())
    }

    async fn reply_question(&mut self, reply: &QuestionReply) -> Result<()> {
        self.question_replies.push(reply.clone());
        Ok(())
    }

    async fn reject_question(&mut self, request_id: &str) -> Result<()> {
        self.rejected_questions.push(request_id.into());
        Ok(())
    }

    async fn find_text(&mut self, pattern: &str) -> Result<Vec<TextMatch>> {
        self.find_text_calls.push(pattern.into());
        Ok(self.text_matches.clone())
    }

    async fn subscribe_live(&mut self) -> Result<Self::EventStream> {
        let events = core::mem::take(&mut self.live_events);
        Ok(MockStream {
            events,
            pos: 0,
            pending_polls: self.live_event_stream_pending_polls,
        })
    }

    async fn sync_history(&mut self, request: &SyncHistoryRequest) -> Result<SyncHistoryBatch> {
        self.sync_history_requests.push(request.clone());
        if let Some(error) = self.sync_history_error.take() {
            return Err(error);
        }
        Ok(self.sync_history_batch.clone())
    }

    async fn get_config(&mut self) -> Result<Config> {
        self.config_info.clone().ok_or(BackendError::Connection {
            message: "no config stub".into(),
        })
    }

    async fn list_models(&mut self) -> Result<Vec<ModelSummary>> {
        self.list_models_calls += 1;
        self.models.clone().ok_or(BackendError::Connection {
            message: "no model catalog stub".into(),
        })
    }

    async fn set_auth(&mut self, _provider: &str, _api_key: &str) -> Result<()> {
        Ok(())
    }

    async fn set_config(&mut self, config: &Config) -> Result<Config> {
        if self.config_info.is_none() {
            return Err(BackendError::Connection {
                message: "no config stub".into(),
            });
        }
        self.config_info = Some(config.clone());
        Ok(config.clone())
    }

    async fn dispose(&mut self) -> Result<()> {
        Ok(())
    }

    async fn upgrade(&mut self) -> Result<()> {
        Ok(())
    }

    async fn log(&mut self, _level: &str, _message: &str) -> Result<()> {
        Ok(())
    }

    async fn remove_auth(&mut self, _provider: &str) -> Result<()> {
        Ok(())
    }
}

pub struct MockStream {
    events: Vec<Result<EventEnvelope>>,
    pos: usize,
    pending_polls: usize,
}

impl Stream for MockStream {
    type Item = Result<EventEnvelope>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.pending_polls > 0 {
            self.pending_polls -= 1;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }

        if self.pos < self.events.len() {
            // Can't take from a Vec, clone instead
            let item = match &self.events[self.pos] {
                Ok(e) => Ok(e.clone()),
                Err(e) => Err(e.clone()),
            };
            self.pos += 1;
            Poll::Ready(Some(item))
        } else {
            Poll::Ready(None)
        }
    }
}
