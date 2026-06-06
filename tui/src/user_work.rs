use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use crate::chat::{user_loaded_message, LoadedMessage};

pub(crate) type UserWorkId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SubmissionKind {
    Prompt,
    Command,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Submission {
    pub(crate) kind: SubmissionKind,
    pub(crate) session_id: String,
    pub(crate) text: String,
    pub(crate) execution_text: String,
    pub(crate) agent: String,
}

impl Submission {
    pub(crate) fn prompt(session_id: String, text: String, agent: String) -> Self {
        Self {
            kind: SubmissionKind::Prompt,
            session_id,
            execution_text: text.clone(),
            text,
            agent,
        }
    }

    pub(crate) fn command(session_id: String, text: String, agent: String) -> Self {
        let execution_text = text
            .trim_start()
            .strip_prefix('!')
            .unwrap_or(text.as_str())
            .trim_start()
            .to_string();
        Self {
            kind: SubmissionKind::Command,
            session_id,
            execution_text,
            text,
            agent,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UserWorkKind {
    Prompt,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UserWorkStatus {
    WaitingForSession,
    CreatingSession,
    Queued,
    Active,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UserWork {
    id: UserWorkId,
    kind: UserWorkKind,
    text: String,
    agent: String,
    session_id: Option<String>,
    status: UserWorkStatus,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkStart {
    pub(crate) id: UserWorkId,
    pub(crate) submission: Submission,
    pub(crate) message: LoadedMessage,
}

#[derive(Debug, Clone)]
pub(crate) enum UserWorkEffect {
    CreateSession { work_id: UserWorkId },
    Submit(WorkStart),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct UserWorkQueue {
    next_id: UserWorkId,
    active: Option<UserWork>,
    pending: Vec<UserWork>,
}

impl UserWorkQueue {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn has_active_work(&self) -> bool {
        self.active.is_some()
    }

    pub(crate) fn has_pending_work(&self) -> bool {
        !self.pending.is_empty()
    }

    pub(crate) fn enqueue(
        &mut self,
        kind: UserWorkKind,
        text: String,
        agent: String,
        session_id: Option<String>,
        response_active: bool,
    ) -> Option<UserWorkEffect> {
        let id = self.alloc_id();
        let work = UserWork {
            id,
            kind,
            text,
            agent,
            session_id,
            status: UserWorkStatus::Queued,
        };

        if work.session_id.is_none() {
            let status = if self.has_session_creation_in_flight() {
                UserWorkStatus::WaitingForSession
            } else {
                UserWorkStatus::CreatingSession
            };
            self.pending.push(UserWork { status, ..work });
            return if status == UserWorkStatus::CreatingSession {
                Some(UserWorkEffect::CreateSession { work_id: id })
            } else {
                None
            };
        }

        if response_active || self.active.is_some() {
            self.pending.push(work);
            return None;
        }

        Some(UserWorkEffect::Submit(self.start_work(work)))
    }

    pub(crate) fn request_next_session_creation(&mut self) -> Option<UserWorkId> {
        if self.has_session_creation_in_flight() {
            return None;
        }

        let work = self.pending.iter_mut().find(|work| {
            work.session_id.is_none() && work.status == UserWorkStatus::WaitingForSession
        })?;
        work.status = UserWorkStatus::CreatingSession;
        Some(work.id)
    }

    pub(crate) fn session_created(
        &mut self,
        work_id: UserWorkId,
        session_id: String,
        response_active: bool,
    ) -> Option<WorkStart> {
        for work in &mut self.pending {
            if work.session_id.is_none() {
                work.session_id = Some(session_id.clone());
                if work.status == UserWorkStatus::WaitingForSession
                    || work.status == UserWorkStatus::CreatingSession
                {
                    work.status = UserWorkStatus::Queued;
                }
            }
        }

        if response_active || self.active.is_some() {
            return None;
        }

        let index = self
            .pending
            .iter()
            .position(|work| work.id == work_id && work.session_id.is_some())
            .or_else(|| {
                self.pending.iter().position(|work| {
                    work.session_id.is_some() && work.status == UserWorkStatus::Queued
                })
            })?;
        let work = self.pending.remove(index);
        Some(self.start_work(work))
    }

    pub(crate) fn create_session_failed(&mut self, work_id: UserWorkId) {
        if let Some(work) = self.pending.iter_mut().find(|work| work.id == work_id) {
            work.status = UserWorkStatus::WaitingForSession;
        }
    }

    pub(crate) fn submit_failed(&mut self, work_id: UserWorkId) {
        if self.active.as_ref().map(|work| work.id) == Some(work_id) {
            let mut work = self.active.take().expect("active work just checked");
            work.status = UserWorkStatus::Failed;
            self.pending.insert(0, work);
        }
    }

    pub(crate) fn finish_active(&mut self, response_active: bool) -> Option<WorkStart> {
        self.active = None;
        self.dispatch_next(response_active)
    }

    pub(crate) fn dispatch_next(&mut self, response_active: bool) -> Option<WorkStart> {
        if response_active || self.active.is_some() {
            return None;
        }

        let index = self
            .pending
            .iter()
            .position(|work| work.session_id.is_some() && work.status == UserWorkStatus::Queued)?;
        let work = self.pending.remove(index);
        Some(self.start_work(work))
    }

    pub(crate) fn queued_messages(&self) -> Vec<LoadedMessage> {
        self.pending
            .iter()
            .map(|work| user_loaded_message(&work.text))
            .collect()
    }

    pub(crate) fn clear(&mut self) {
        self.active = None;
        self.pending.clear();
    }

    pub(crate) fn clear_active(&mut self) {
        self.active = None;
    }

    fn alloc_id(&mut self) -> UserWorkId {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    fn has_session_creation_in_flight(&self) -> bool {
        self.pending
            .iter()
            .any(|work| work.status == UserWorkStatus::CreatingSession)
    }

    fn start_work(&mut self, mut work: UserWork) -> WorkStart {
        work.status = UserWorkStatus::Active;
        let submission = work
            .submission()
            .expect("started user work must have a session id");
        let message = user_loaded_message(&work.text);
        let id = work.id;
        self.active = Some(work);
        WorkStart {
            id,
            submission,
            message,
        }
    }
}

impl UserWork {
    fn submission(&self) -> Option<Submission> {
        let session_id = self.session_id.clone()?;
        Some(match self.kind {
            UserWorkKind::Prompt => {
                Submission::prompt(session_id, self.text.clone(), self.agent.clone())
            }
            UserWorkKind::Command => {
                Submission::command(session_id, self.text.clone(), self.agent.clone())
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_without_session_requests_session_and_keeps_visible_message() {
        let mut queue = UserWorkQueue::new();

        let effect = queue.enqueue(
            UserWorkKind::Prompt,
            "hello".into(),
            "build".into(),
            None,
            false,
        );

        assert!(matches!(
            effect,
            Some(UserWorkEffect::CreateSession { work_id: 0 })
        ));
        assert_eq!(queue.queued_messages().len(), 1);
        assert!(!queue.has_active_work());
    }

    #[test]
    fn session_created_starts_waiting_work() {
        let mut queue = UserWorkQueue::new();
        queue.enqueue(
            UserWorkKind::Prompt,
            "hello".into(),
            "build".into(),
            None,
            false,
        );

        let start = queue
            .session_created(0, "session-1".into(), false)
            .expect("work should start");

        assert_eq!(start.submission.session_id, "session-1");
        assert_eq!(start.submission.text, "hello");
        assert_eq!(queue.queued_messages().len(), 0);
        assert!(queue.has_active_work());
    }

    #[test]
    fn active_work_blocks_later_work_and_preserves_fifo() {
        let mut queue = UserWorkQueue::new();
        let first = queue
            .enqueue(
                UserWorkKind::Prompt,
                "first".into(),
                "build".into(),
                Some("session-1".into()),
                false,
            )
            .expect("first should start");
        queue.enqueue(
            UserWorkKind::Prompt,
            "second".into(),
            "build".into(),
            Some("session-1".into()),
            true,
        );
        queue.enqueue(
            UserWorkKind::Prompt,
            "third".into(),
            "build".into(),
            Some("session-1".into()),
            true,
        );

        let UserWorkEffect::Submit(first) = first else {
            panic!("first should start");
        };
        assert_eq!(first.submission.text, "first");
        assert_eq!(queued_texts(&queue), vec!["second", "third"]);

        let next = queue
            .finish_active(false)
            .expect("second should dispatch next");
        assert_eq!(next.submission.text, "second");
        assert_eq!(queued_texts(&queue), vec!["third"]);
    }

    #[test]
    fn failed_create_can_be_requested_again() {
        let mut queue = UserWorkQueue::new();
        queue.enqueue(
            UserWorkKind::Prompt,
            "hello".into(),
            "build".into(),
            None,
            false,
        );

        queue.create_session_failed(0);
        assert_eq!(queue.request_next_session_creation(), Some(0));
        assert_eq!(queue.request_next_session_creation(), None);
    }

    #[test]
    fn multiple_no_session_enqueues_share_one_session_creation() {
        let mut queue = UserWorkQueue::new();

        let first = queue.enqueue(
            UserWorkKind::Prompt,
            "first".into(),
            "build".into(),
            None,
            false,
        );
        let second = queue.enqueue(
            UserWorkKind::Prompt,
            "second".into(),
            "build".into(),
            None,
            false,
        );

        assert!(matches!(
            first,
            Some(UserWorkEffect::CreateSession { work_id: 0 })
        ));
        assert!(second.is_none());
        assert_eq!(queue.request_next_session_creation(), None);

        let start = queue
            .session_created(0, "session-1".into(), false)
            .expect("first work should start");
        assert_eq!(start.submission.text, "first");
        assert_eq!(start.submission.session_id, "session-1");

        let next = queue
            .finish_active(false)
            .expect("second work should dispatch after first");
        assert_eq!(next.submission.text, "second");
        assert_eq!(next.submission.session_id, "session-1");
    }

    fn queued_texts(queue: &UserWorkQueue) -> Vec<String> {
        queue
            .queued_messages()
            .into_iter()
            .filter_map(|message| match message.parts.into_iter().next() {
                Some(ocpncord_backend::Part::Text(text)) => Some(text.text),
                _ => None,
            })
            .collect()
    }
}
