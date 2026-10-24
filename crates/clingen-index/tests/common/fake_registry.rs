//! A scripted HTTP/1.1 server standing in for the ClinGen Allele Registry.
//!
//! `clingen-index registry` runs as a child process, so the stand-in has to be
//! a real socket: a `TcpListener` on `127.0.0.1:0` and a thread that answers
//! one request per connection with the next scripted reply, then closes. Every
//! request is recorded (request line, headers and body) before it is answered,
//! so once the child has exited every request it made is in the record.
//!
//! The whole request, POST body included, is read before replying: replying
//! to a large POST with its body unread makes macOS reset the connection
//! instead of delivering the response. A request beyond the script is still
//! recorded and answered with a 418, which the client does not retry, so a
//! test that counts requests sees the extra one instead of a refused
//! connection that leaves no trace.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;

/// One request as the server received it.
pub struct Request {
    /// The request line, e.g. `GET /alleles?refseq=... HTTP/1.1`.
    pub line: String,
    /// Header names in lower case, with their values, in arrival order.
    pub headers: Vec<(String, String)>,
    /// The body, empty for a GET.
    pub body: String,
}

impl Request {
    /// The value of the header `name` (lower case), if it was sent.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// A running fake Registry; it serves until the test process ends.
pub struct FakeRegistry {
    base_url: String,
    requests: mpsc::Receiver<Request>,
}

impl FakeRegistry {
    /// Start serving `replies`, one `(status, body)` per connection, in order.
    pub fn start(replies: Vec<(u16, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let (tx, requests) = mpsc::channel();
        thread::spawn(move || {
            let mut replies = replies.into_iter();
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let request = read_request(&stream);
                if tx.send(request).is_err() {
                    return;
                }
                let (status, body) = replies
                    .next()
                    .unwrap_or_else(|| (418, "unscripted request".to_owned()));
                let head = format!(
                    "HTTP/1.1 {status} Scripted\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                // The client may have hung up (a timeout, say); that is its
                // business, not a failure of the server.
                let _ = stream
                    .write_all(head.as_bytes())
                    .and_then(|()| stream.write_all(body.as_bytes()));
            }
        });
        FakeRegistry { base_url, requests }
    }

    /// `http://127.0.0.1:<port>`, for `--base-url`.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Every request received so far, in order. Call it after the client
    /// process has exited.
    pub fn requests(&self) -> Vec<Request> {
        self.requests.try_iter().collect()
    }
}

/// Read the request line, the headers and the whole `Content-Length` body.
fn read_request(stream: &TcpStream) -> Request {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).unwrap();
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(':').unwrap();
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .map_or(0, |(_, value)| value.parse().unwrap());
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    Request {
        line: line.trim_end().to_owned(),
        headers,
        body: String::from_utf8(body).unwrap(),
    }
}
