//! Optional webhook fired when the human submits a review.
//!
//! RedLine is pull-only for agents: `get_feedback` and `wait_for_feedback` ask for work, nothing
//! pushes it. That is fine for an agent that is already looping, but silence is what a human sees
//! when no agent is watching. `REDLINE_WEBHOOK_URL` gives the host a way to wake one up.
//!
//! Deliberately tiny: a plain HTTP POST, no TLS, no retries, no queue. Point it at a local relay
//! (or an `http://` endpoint on the LAN) and do the clever part there. A failed webhook never
//! blocks or fails the submission.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Fire and forget: spawn this on a thread from the request path.
pub fn post_json(url: &str, payload: &serde_json::Value) {
    let Some(rest) = url.strip_prefix("http://") else {
        eprintln!("redline: webhook only supports http:// ({url}); ignored");
        return;
    };
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => match port.parse::<u16>() {
            Ok(port) => (host.to_string(), port),
            Err(_) => {
                eprintln!("redline: webhook has an invalid port ({url}); ignored");
                return;
            }
        },
        None => (authority.to_string(), 80),
    };
    let Ok(body) = serde_json::to_vec(payload) else {
        return;
    };
    let Ok(mut addresses) = (host.as_str(), port).to_socket_addrs() else {
        eprintln!("redline: webhook host does not resolve ({url})");
        return;
    };
    let Some(address) = addresses.next() else {
        return;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_secs(5)) else {
        eprintln!("redline: webhook unreachable ({url})");
        return;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
    // Read the status line so a misconfigured endpoint shows up in the logs, then hang up.
    let mut sink = [0u8; 128];
    let _ = stream.read(&mut sink);
}
