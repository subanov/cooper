//! `cooper-server healthcheck`: probes `GET /health/live` on the local listener, for the
//! container `HEALTHCHECK` (the distroless image has no shell or curl; ADR-0023).

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// Whether `GET /health/live` on `addr` answers `200`. A wildcard bind address
/// (`0.0.0.0`, `::`) is probed on loopback.
#[must_use]
pub fn live(addr: SocketAddr) -> bool {
    let mut target = addr;
    if target.ip().is_unspecified() {
        target.set_ip(if addr.is_ipv4() {
            std::net::Ipv4Addr::LOCALHOST.into()
        } else {
            std::net::Ipv6Addr::LOCALHOST.into()
        });
    }
    probe(target).unwrap_or(false)
}

fn probe(addr: SocketAddr) -> std::io::Result<bool> {
    let timeout = Duration::from_secs(3);
    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream
        .write_all(b"GET /health/live HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
    let mut head = [0_u8; 12];
    stream.read_exact(&mut head)?;
    // "HTTP/1.1 200"
    Ok(head.ends_with(b" 200"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use std::net::TcpListener;

    fn answer_once(status_line: &'static str) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0_u8; 256];
            let _ = socket.read(&mut request).unwrap();
            socket
                .write_all(format!("{status_line}\r\ncontent-length: 0\r\n\r\n").as_bytes())
                .unwrap();
        });
        addr
    }

    #[test]
    fn healthy_only_on_200() {
        assert!(live(answer_once("HTTP/1.1 200 OK")));
        assert!(!live(answer_once("HTTP/1.1 503 Service Unavailable")));
    }

    #[test]
    fn unreachable_is_unhealthy_and_wildcards_use_loopback() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(!live(SocketAddr::from(([0, 0, 0, 0], port))));
    }
}
