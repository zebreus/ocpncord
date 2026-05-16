use alloc::string::String;
use alloc::vec::Vec;
use core::pin::Pin;
use core::task::{Context, Poll};
use futures_core::Stream;
use opencode_backend::{BackendEvent, Result};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};
use tokio::io::BufReader;

/// Real-time SSE stream over a raw TCP connection.
///
/// Sends a raw HTTP GET request, reads/discards response headers,
/// then parses SSE events as they arrive.
pub struct TcpSseStream {
    reader: BufReader<tokio::net::TcpStream>,
    partial: Vec<u8>,
}

impl TcpSseStream {
    pub async fn connect(url: &str) -> Result<crate::BufferedStream> {
        let (host, port, path) = parse_url(url);
        let addr = format!("{host}:{port}");
        let mut stream = tokio::net::TcpStream::connect(&addr)
            .await
            .map_err(|e| opencode_backend::BackendError::Connection {
                message: alloc::format!("{e}"),
            })?;

        let request = alloc::format!(
            "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nAccept: text/event-stream\r\nConnection: keep-alive\r\n\r\n"
        );

        use tokio::io::AsyncWriteExt;
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|e| opencode_backend::BackendError::Connection {
                message: alloc::format!("{e}"),
            })?;

        let mut reader = BufReader::new(stream);
        let mut header_line = String::new();
        loop {
            header_line.clear();
            let n = reader
                .read_line(&mut header_line)
                .await
                .map_err(|e| opencode_backend::BackendError::Connection {
                    message: alloc::format!("{e}"),
                })?;
            if n == 0 {
                return Err(opencode_backend::BackendError::Connection {
                    message: alloc::format!("server closed connection before headers complete"),
                });
            }
            if header_line == "\r\n" || header_line == "\n" {
                break;
            }
        }

        Ok(crate::BufferedStream::from_tcp_stream(Self {
            reader,
            partial: Vec::new(),
        }))
    }
}

impl Stream for TcpSseStream {
    type Item = Result<BackendEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = unsafe { self.get_unchecked_mut() };

        loop {
            if let Some((end, sep_len)) = crate::stream::find_event_boundary(&this.partial) {
                let block = &this.partial[..end];
                let mut events = crate::BufferedStream::parse_sse(block);
                this.partial.drain(..end + sep_len);
                if let Some(event) = events.drain(..).next() {
                    return Poll::Ready(Some(event));
                }
                continue;
            }

            let buf = this.reader.buffer();
            if !buf.is_empty() {
                let avail = buf.len();
                this.partial.extend_from_slice(buf);
                this.reader.consume(avail);
                continue;
            }

            let pinned = Pin::new(&mut this.reader);
            match pinned.poll_fill_buf(cx) {
                Poll::Ready(Ok(buf)) if buf.is_empty() => return Poll::Ready(None),
                Poll::Ready(Ok(buf)) => {
                    let avail = buf.len();
                    this.partial.extend_from_slice(buf);
                    this.reader.consume(avail);
                    continue;
                }
                Poll::Ready(Err(_)) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

fn parse_url(url: &str) -> (String, u16, String) {
    let without_scheme = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);

    let slash_pos = without_scheme.find('/');
    let (host_part, path) = match slash_pos {
        Some(pos) => (&without_scheme[..pos], &without_scheme[pos..]),
        None => (without_scheme, "/"),
    };

    let (host, port) = if let Some((h, p)) = host_part.split_once(':') {
        (h.to_owned(), p.parse::<u16>().unwrap_or(80))
    } else {
        (host_part.to_owned(), 80)
    };

    (host, port, path.to_owned())
}
