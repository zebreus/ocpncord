use crate::*;
use alloc::vec::Vec;
use core::pin::Pin;
use core::task::{Context, Poll};
use futures_core::Stream;

/// A mock backend for testing TUI code without a real server.
pub struct MockBackend {
    pub sessions: Vec<Session>,
    pub messages: Vec<MessageSummary>,
    pub message_detail: Option<MessageDetail>,
    pub health_status: Option<Health>,
    pub config_info: Option<Config>,
    pub models: Option<Vec<ModelSummary>>,
    pub list_models_calls: usize,
    pub agents: Vec<Agent>,
    pub prompt_events: Vec<Result<BackendEvent>>,
    pub event_events: Vec<Result<BackendEvent>>,
    pub text_matches: Vec<TextMatch>,
    pub fail_create_session: Option<BackendError>,
    pub last_prompt_agent: Option<alloc::string::String>,
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
            config_info: Some(Config {
                model: Some("mock/model".into()),
                username: Some("mock-user".into()),
                provider: Default::default(),
                agent: Default::default(),
            }),
            models: None,
            list_models_calls: 0,
            agents: Vec::new(),
            prompt_events: Vec::new(),
            event_events: Vec::new(),
            text_matches: Vec::new(),
            fail_create_session: None,
            last_prompt_agent: None,
        }
    }
}

impl Backend for MockBackend {
    type PromptStream = MockStream;
    type EventStream = MockStream;

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

    async fn create_session(&mut self, title: &str, cwd: &str) -> Result<Session> {
        if let Some(err) = self.fail_create_session.take() {
            return Err(err);
        }
        let session = Session {
            id: "mock-session-id".into(),
            title: title.into(),
            project_id: "mock-project".into(),
            directory: cwd.into(),
            parent_id: None,
            time: SessionTime {
                created: 0,
                updated: 0,
            },
            slug: String::new(),
            version: String::new(),
            workspace_id: None,
            summary: None,
            share: None,
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
            parent_id: None,
            time: SessionTime {
                created: 0,
                updated: 0,
            },
            slug: String::new(),
            version: String::new(),
            workspace_id: None,
            summary: None,
            share: None,
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

    async fn prompt(
        &mut self,
        _id: &SessionId,
        _text: &str,
        agent: Option<&str>,
    ) -> Result<Self::PromptStream> {
        self.last_prompt_agent = agent.map(|a| a.into());
        let events = core::mem::take(&mut self.prompt_events);
        Ok(MockStream { events, pos: 0 })
    }

    async fn command(
        &mut self,
        _id: &SessionId,
        _text: &str,
        _agent: Option<&str>,
    ) -> Result<Self::PromptStream> {
        let events = core::mem::take(&mut self.prompt_events);
        Ok(MockStream { events, pos: 0 })
    }

    async fn find_text(&mut self, _pattern: &str) -> Result<Vec<TextMatch>> {
        Ok(self.text_matches.clone())
    }

    async fn subscribe(&mut self) -> Result<Self::EventStream> {
        let events = core::mem::take(&mut self.event_events);
        Ok(MockStream { events, pos: 0 })
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

    async fn sync_events(&mut self) -> Result<Self::EventStream> {
        let events = core::mem::take(&mut self.event_events);
        Ok(MockStream { events, pos: 0 })
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
    events: Vec<Result<BackendEvent>>,
    pos: usize,
}

impl Stream for MockStream {
    type Item = Result<BackendEvent>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
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
