//! A read-buffering adapter for any [`embedded_nal_async::TcpConnect`] transport.
//!
//! reqwless's chunked-transfer decoder parses each chunk-size line one byte at a
//! time, reading directly from the connection it is given. With no buffer beneath
//! it, every byte becomes a separate transport read — a separate socket round-trip
//! on a plain-HTTP connection, or extra work per TCP-fragmented record on TLS.
//! Wrapping the transport in [`BufferedTcpConnect`] serves those tiny reads from a
//! small in-memory buffer that a single underlying read fills.

use alloc::boxed::Box;
use core::net::SocketAddr;
use embedded_io_async::{ErrorType, Read, Write};
use embedded_nal_async::TcpConnect;

/// Size of the per-connection read buffer. A single underlying read fills up to
/// this many bytes, after which byte-at-a-time reads are served from memory.
const READ_BUFFER_SIZE: usize = 4 * 1024;

/// Wraps a [`TcpConnect`] transport so every connection it produces buffers reads.
///
/// Construct it once around the real transport and hand the wrapper to the
/// backend; all callers (and the badge, once it picks up this crate version)
/// then get buffering for free.
pub struct BufferedTcpConnect<T> {
    inner: T,
}

impl<T> BufferedTcpConnect<T> {
    /// Wrap `inner`. `const` so it can initialise a `static` transport.
    pub const fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: TcpConnect> TcpConnect for BufferedTcpConnect<T> {
    type Error = T::Error;
    type Connection<'a>
        = BufferedConnection<T::Connection<'a>>
    where
        Self: 'a;

    async fn connect<'a>(
        &'a self,
        remote: SocketAddr,
    ) -> Result<Self::Connection<'a>, Self::Error> {
        Ok(BufferedConnection::new(self.inner.connect(remote).await?))
    }
}

/// A connection that serves reads from a small internal buffer and writes through.
///
/// The buffer is heap-allocated: this connection is embedded in reqwless's request
/// future, which lives on the caller's task stack, and a 4 KiB inline array there
/// overflows the badge's stack-constrained network task. `Box` keeps the future small.
pub struct BufferedConnection<C> {
    inner: C,
    buf: Box<[u8; READ_BUFFER_SIZE]>,
    start: usize,
    end: usize,
}

impl<C> BufferedConnection<C> {
    fn new(inner: C) -> Self {
        Self {
            inner,
            buf: Box::new([0u8; READ_BUFFER_SIZE]),
            start: 0,
            end: 0,
        }
    }
}

impl<C: ErrorType> ErrorType for BufferedConnection<C> {
    type Error = C::Error;
}

impl<C: Read> Read for BufferedConnection<C> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        // Preserve the zero-length-read contract callers rely on: reqwless's
        // chunked body reader issues a final empty read after the terminating
        // chunk, and the underlying transport short-circuits it to `Ok(0)`.
        if buf.is_empty() {
            return Ok(0);
        }

        if self.start == self.end {
            // Buffer empty. A request at least as large as the buffer can read
            // straight into the caller's slice — buffering would only add a copy
            // without saving a read.
            if buf.len() >= self.buf.len() {
                return self.inner.read(buf).await;
            }
            // Otherwise refill with a single underlying read. This never
            // over-blocks: one read returns whatever is already available.
            let n = self.inner.read(&mut self.buf[..]).await?;
            if n == 0 {
                return Ok(0);
            }
            self.start = 0;
            self.end = n;
        }

        let available = self.end - self.start;
        let n = available.min(buf.len());
        buf[..n].copy_from_slice(&self.buf[self.start..self.start + n]);
        self.start += n;
        Ok(n)
    }
}

impl<C: Write> Write for BufferedConnection<C> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.inner.write(buf).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::convert::Infallible;
    use core::future::Future;
    use core::task::{Context, Poll, Waker};

    /// Minimal executor: the connection futures here are always ready, so a
    /// busy-poll with a no-op waker resolves them without pulling in a runtime.
    fn block_on<F: Future>(fut: F) -> F::Output {
        let mut fut = core::pin::pin!(fut);
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    /// A fake connection that hands out a fixed payload, capping each read at
    /// `chunk` bytes and counting how many times `read` is called.
    struct CountingConn {
        data: Vec<u8>,
        pos: usize,
        reads: usize,
        chunk: usize,
    }

    impl CountingConn {
        fn new(data: Vec<u8>, chunk: usize) -> Self {
            Self {
                data,
                pos: 0,
                reads: 0,
                chunk,
            }
        }
    }

    impl ErrorType for CountingConn {
        type Error = Infallible;
    }

    impl Read for CountingConn {
        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Infallible> {
            self.reads += 1;
            if buf.is_empty() {
                return Ok(0);
            }
            let remaining = self.data.len() - self.pos;
            let n = remaining.min(buf.len()).min(self.chunk);
            buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }

    impl Write for CountingConn {
        async fn write(&mut self, buf: &[u8]) -> Result<usize, Infallible> {
            Ok(buf.len())
        }
        async fn flush(&mut self) -> Result<(), Infallible> {
            Ok(())
        }
    }

    /// Drain `conn` one byte at a time, returning (bytes, underlying read count).
    fn drain_byte_by_byte(conn: &mut BufferedConnection<CountingConn>) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut b = [0u8; 1];
            let n = block_on(conn.read(&mut b)).unwrap();
            if n == 0 {
                break;
            }
            out.push(b[0]);
        }
        out
    }

    #[test]
    fn coalesces_byte_reads() {
        let data: Vec<u8> = (0..200u8).collect();
        // Inner could serve everything in one read; the buffer should absorb it.
        let mut conn = BufferedConnection::new(CountingConn::new(data.clone(), 4096));
        let out = drain_byte_by_byte(&mut conn);
        assert_eq!(out, data);
        // One read to fill, one to observe EOF — far fewer than 200.
        assert_eq!(conn.inner.reads, 2);
    }

    #[test]
    fn large_read_bypasses_buffer() {
        let data: Vec<u8> = (0..2000u32).map(|n| n as u8).collect();
        let mut conn = BufferedConnection::new(CountingConn::new(data.clone(), 4096));
        let mut buf = [0u8; 4096];
        let n = block_on(conn.read(&mut buf)).unwrap();
        assert_eq!(n, data.len());
        assert_eq!(&buf[..n], &data[..]);
        // Bypass path: a single inner read, internal buffer untouched.
        assert_eq!(conn.inner.reads, 1);
        assert_eq!(conn.start, 0);
        assert_eq!(conn.end, 0);
    }

    #[test]
    fn empty_caller_buf_returns_zero_without_inner_read() {
        let mut conn = BufferedConnection::new(CountingConn::new(vec![1, 2, 3], 4096));
        let n = block_on(conn.read(&mut [])).unwrap();
        assert_eq!(n, 0);
        assert_eq!(conn.inner.reads, 0);
    }

    #[test]
    fn serves_partial_then_refills() {
        // Inner yields only 8 bytes per read, so the buffer must refill several
        // times to deliver the full payload — content must stay correct.
        let data: Vec<u8> = (0..50u8).collect();
        let mut conn = BufferedConnection::new(CountingConn::new(data.clone(), 8));
        let out = drain_byte_by_byte(&mut conn);
        assert_eq!(out, data);
        // 50 bytes / 8 per read = 7 fills + 1 EOF read; nowhere near 50.
        assert!(conn.inner.reads <= 8, "reads = {}", conn.inner.reads);
    }
}
