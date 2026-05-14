use alloc::vec::Vec;
use core::net::IpAddr;
use embedded_io_async::{ErrorType, Read, Write};
use embedded_nal_async::AddrType;
use embedded_nal_async::{Dns, TcpConnect};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

/// A TCP transport over tokio `TcpStream`.
pub struct StdTcp;

impl TcpConnect for StdTcp {
    type Error = std::io::Error;

    type Connection<'a> = StdTcpStream;

    async fn connect<'a>(
        &'a self,
        remote: core::net::SocketAddr,
    ) -> Result<Self::Connection<'a>, Self::Error> {
        let stream = tokio::net::TcpStream::connect(remote).await?;
        Ok(StdTcpStream(stream))
    }
}

/// A DNS resolver over tokio.
pub struct StdDns;

impl Dns for StdDns {
    type Error = std::io::Error;

    async fn get_host_by_name(
        &self,
        host: &str,
        addr_type: AddrType,
    ) -> Result<IpAddr, Self::Error> {
        let addrs = tokio::net::lookup_host((host, 0)).await?;
        let addrs: Vec<std::net::SocketAddr> = addrs.collect();

        // When Either, prefer IPv4 over IPv6 (many servers don't listen on IPv6)
        let addr = match addr_type {
            AddrType::IPv4 => addrs.iter().find(|a| a.is_ipv4()),
            AddrType::IPv6 => addrs.iter().find(|a| a.is_ipv6()),
            AddrType::Either => addrs
                .iter()
                .find(|a| a.is_ipv4())
                .or_else(|| addrs.iter().find(|a| a.is_ipv6())),
        };
        match addr {
            Some(a) => Ok(a.ip()),
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
    ) -> Result<usize, Self::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "reverse DNS not supported",
        ))
    }
}

/// Wraps a tokio `TcpStream` to implement `embedded-io-async` traits.
pub struct StdTcpStream(pub tokio::net::TcpStream);

impl ErrorType for StdTcpStream {
    type Error = std::io::Error;
}

impl Read for StdTcpStream {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.read(buf).await
    }
}

impl Write for StdTcpStream {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.0.write(buf).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.0.flush().await
    }
}
