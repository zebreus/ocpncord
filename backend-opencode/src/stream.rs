use alloc::string::String;
use alloc::vec::Vec;
use core::pin::Pin;
use core::task::{Context, Poll};
use futures_core::Stream;
use opencode_backend::{BackendError, BackendEvent, Part, Result};

#[cfg(feature = "std")]
use crate::sse_stream::TcpSseStream;

enum SseSource {
    PreParsed {
        events: Vec<Result<BackendEvent>>,
        pos: usize,
    },
    #[cfg(feature = "std")]
    Direct(TcpSseStream),
}

/// A stream that yields [`BackendEvent`]s.
///
/// Can be either pre-parsed from a completed HTTP response body (used by `prompt`
/// and `command`), or a real-time SSE stream (used by `subscribe` on `std`).
pub struct BufferedStream {
    source: SseSource,
}

impl BufferedStream {
    pub fn new(events: Vec<Result<BackendEvent>>) -> Self {
        Self {
            source: SseSource::PreParsed { events, pos: 0 },
        }
    }

    #[cfg(feature = "std")]
    pub fn from_tcp_stream(stream: TcpSseStream) -> Self {
        Self {
            source: SseSource::Direct(stream),
        }
    }

    /// Parse all SSE events from a response body bytes.
    ///
    /// Each SSE event block is separated by `\n\n`:
    /// ```text
    /// event: message.part.updated
    /// data: {"part": {...}, "delta": "..."}
    ///
    /// event: session.status
    /// data: {"sessionID": "...", "status": {"type": "idle"}}
    /// ```
    pub fn parse_sse(body: &[u8]) -> Vec<Result<BackendEvent>> {
        let mut events = Vec::new();
        let mut pos = 0;

        while pos < body.len() {
            match find_event_boundary(&body[pos..]) {
                Some(end) => {
                    let block = &body[pos..pos + end];
                    if let Some(event) = parse_sse_block(block) {
                        events.push(event);
                    }
                    pos += end + 2;
                }
                None => {
                    let block = &body[pos..];
                    if let Some(event) = parse_sse_block(block) {
                        events.push(event);
                    }
                    break;
                }
            }
        }

        events
    }
}

impl Stream for BufferedStream {
    type Item = Result<BackendEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = unsafe { self.get_unchecked_mut() };
        match &mut this.source {
            SseSource::PreParsed { events, pos } => {
                if *pos < events.len() {
                    let item = match &events[*pos] {
                        Ok(e) => Ok(e.clone()),
                        Err(e) => Err(e.clone()),
                    };
                    *pos += 1;
                    Poll::Ready(Some(item))
                } else {
                    Poll::Ready(None)
                }
            }
            #[cfg(feature = "std")]
            SseSource::Direct(stream) => Pin::new(stream).poll_next(cx),
        }
    }
}

/// Find the end of the first SSE event block (position of `\n\n`).
pub(crate) fn find_event_boundary(data: &[u8]) -> Option<usize> {
    data.windows(2).position(|w| w == b"\n\n")
}

/// Parse a single SSE block into a `BackendEvent`.
///
/// Block format:
/// ```text
/// event: <type>
/// data: <json>
/// ```
/// (The leading `event:`+`data:` lines may be in any order,
/// and `event:` may be omitted.)
fn parse_sse_block(block: &[u8]) -> Option<Result<BackendEvent>> {
    let text = core::str::from_utf8(block).ok()?;

    let mut event_type = "";
    let mut data = "";

    for line in text.lines() {
        if let Some(value) = line.strip_prefix("event: ") {
            event_type = value;
        } else if let Some(value) = line.strip_prefix("data: ") {
            data = value;
        }
    }

    if data.is_empty() {
        return None;
    }

    match event_type {
        "message.part.updated" => match parse_part_updated(data) {
            Ok(event) => Some(Ok(event)),
            Err(e) => Some(Err(e)),
        },
        "message.updated" | "session.idle" => Some(Ok(BackendEvent::Done)),
        "session.error" => Some(Ok(BackendEvent::Error {
            message: data.to_owned(),
        })),
        "session.status" => {
            // Check for idle status
            if data.contains(r#""idle""#) {
                Some(Ok(BackendEvent::Done))
            } else {
                None
            }
        }
        "server.connected" => {
            // Skip handshake event
            None
        }
        _ => {
            // Unknown event type — skip
            None
        }
    }
}

/// Parse a `message.part.updated` event data JSON into a `BackendEvent::Part`.
fn parse_part_updated(data: &str) -> core::result::Result<BackendEvent, BackendError> {
    // The data JSON looks like: {"part": {...}, "delta": "..."}
    // We parse it manually to extract the nested "part" object and optional "delta"
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct PartUpdated {
        part: Part,
        #[allow(dead_code)]
        delta: Option<String>,
    }

    let parsed: PartUpdated =
        serde_json::from_slice(data.as_bytes()).map_err(|e| BackendError::Parse {
            message: alloc::format!("failed to parse part.updated: {e}"),
        })?;

    let delta = parsed.delta;
    Ok(BackendEvent::Part {
        part: parsed.part,
        delta,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_text_part() {
        let sse = b"event: message.part.updated\ndata: {\"part\":{\"type\":\"text\",\"text\":\"Hello\"},\"delta\":\"Hello\"}\n\n";
        let events = BufferedStream::parse_sse(sse);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Ok(BackendEvent::Part { part, delta }) => {
                assert!(matches!(part, Part::Text(_)));
                assert_eq!(delta.as_deref(), Some("Hello"));
            }
            _ => panic!("expected text part"),
        }
    }

    #[test]
    fn parse_tool_part() {
        let sse = b"event: message.part.updated\ndata: {\"part\":{\"type\":\"tool\",\"tool\":\"bash\",\"state\":\"running\"}}\n\n";
        let events = BufferedStream::parse_sse(sse);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Ok(BackendEvent::Part { part, .. }) => {
                assert!(matches!(part, Part::Tool(_)));
            }
            _ => panic!("expected tool part"),
        }
    }

    #[test]
    fn parse_done_event() {
        let sse = b"event: message.updated\ndata: {}\n\n";
        let events = BufferedStream::parse_sse(sse);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], Ok(BackendEvent::Done)));
    }

    #[test]
    fn parse_server_connected_skipped() {
        let sse = b"event: server.connected\ndata: {}\n\n";
        let events = BufferedStream::parse_sse(sse);
        assert!(events.is_empty());
    }

    #[test]
    fn parse_multiple_events() {
        let sse = b"\
event: message.part.updated
data: {\"part\":{\"type\":\"text\",\"text\":\"One\"},\"delta\":\"One\"}

event: message.part.updated
data: {\"part\":{\"type\":\"text\",\"text\":\"Two\"},\"delta\":\"Two\"}

event: message.updated
data: {}

";
        let events = BufferedStream::parse_sse(sse);
        assert_eq!(events.len(), 3);
        assert!(matches!(events[2], Ok(BackendEvent::Done)));
    }

    #[test]
    fn parse_no_events_for_empty_body() {
        let events = BufferedStream::parse_sse(b"");
        assert!(events.is_empty());
    }
}
