//! Minimal NDJSON client for the herdr socket API.
//!
//! The wire protocol is one JSON object per line: requests are
//! `{"id","method","params"}` and responses come back carrying the same `id`.
//!
//! The server closes the connection as soon as it has answered a request — it
//! is one request per connection, not a multiplexed channel. (Only
//! `events.subscribe` holds a connection open.) So each call dials a fresh
//! socket. That is still far cheaper than shelling out to the `herdr` binary,
//! which is what makes key-drumming feel instant, and it is the only route to
//! `tab.move`, which has no CLI subcommand at all.

use serde_json::{Value, json};
use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// The server understood the request and refused it.
    Api {
        code: String,
        message: String,
    },
    Protocol(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io: {e}"),
            Error::Api { code, message } => write!(f, "{code}: {message}"),
            Error::Protocol(m) => write!(f, "protocol: {m}"),
        }
    }
}

impl Error {
    /// Opening a popup while one is already up is an expected race, not a fault:
    /// the mode the user asked for is already on screen.
    pub fn is_popup_already_open(&self) -> bool {
        matches!(self, Error::Api { message, .. } if message.contains("already open"))
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub struct Client {
    path: String,
    next_id: u64,
}

impl Client {
    pub fn connect() -> Result<Self, Error> {
        let path = std::env::var("HERDR_SOCKET_PATH")
            .map_err(|_| Error::Protocol("HERDR_SOCKET_PATH is not set".into()))?;
        // Dial once up front so a bad socket path fails before raw mode is on.
        UnixStream::connect(&path)?;
        Ok(Client { path, next_id: 1 })
    }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        let id = format!("modes-{}", self.next_id);
        self.next_id += 1;

        let mut stream = UnixStream::connect(&self.path)?;
        let mut reader = BufReader::new(stream.try_clone()?);

        let req = json!({ "id": &id, "method": method, "params": params });
        writeln!(stream, "{req}")?;
        stream.flush()?;

        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                return Err(Error::Protocol("socket closed before a reply".into()));
            }
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            // Skip anything that isn't the reply we're waiting on.
            if v.get("id").and_then(Value::as_str) != Some(id.as_str()) {
                continue;
            }
            if let Some(err) = v.get("error") {
                return Err(Error::Api {
                    code: err
                        .get("code")
                        .and_then(Value::as_str)
                        .unwrap_or("error")
                        .to_string(),
                    message: err
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("request failed")
                        .to_string(),
                });
            }
            return Ok(v.get("result").cloned().unwrap_or(Value::Null));
        }
    }
}
