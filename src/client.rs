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
    ///
    /// herdr 0.9.0 reports this as code `ui_busy` with a message containing
    /// "already open". Either one is enough, since the wording may change
    /// without the code doing so, and vice versa.
    pub fn is_popup_already_open(&self) -> bool {
        matches!(
            self,
            Error::Api { code, message } if code == "ui_busy" || message.contains("already open")
        )
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// One request to herdr, answered or refused. `Client` speaks to the socket;
/// tests stand in a `FakeApi` so a `Session` can be driven without a server.
pub trait Api {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, Error>;
}

pub struct Client {
    path: String,
    next_id: u64,
}

impl Api for Client {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        Client::call(self, method, params)
    }
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

/// A scripted `Api` for tests: canned replies per method, handed out in
/// order, and a record of every call as it came in. A method with nothing
/// queued answers `null`, which is what the session treats as "no data".
#[cfg(test)]
pub mod fake {
    use super::{Api, Error};
    use serde_json::Value;
    use std::collections::{HashMap, VecDeque};

    #[derive(Default)]
    pub struct FakeApi {
        replies: HashMap<String, VecDeque<Result<Value, Error>>>,
        calls: Vec<(String, Value)>,
    }

    impl FakeApi {
        pub fn new() -> Self {
            Self::default()
        }

        /// Queue a successful reply for `method`.
        pub fn reply(mut self, method: &str, value: Value) -> Self {
            self.replies
                .entry(method.to_string())
                .or_default()
                .push_back(Ok(value));
            self
        }

        /// Queue a refusal for `method`.
        pub fn fail(mut self, method: &str, code: &str, message: &str) -> Self {
            self.replies
                .entry(method.to_string())
                .or_default()
                .push_back(Err(Error::Api {
                    code: code.to_string(),
                    message: message.to_string(),
                }));
            self
        }

        /// Every call so far, in order.
        pub fn calls(&self) -> &[(String, Value)] {
            &self.calls
        }

        /// The method names called so far, in order.
        pub fn methods(&self) -> Vec<&str> {
            self.calls.iter().map(|(m, _)| m.as_str()).collect()
        }
    }

    impl Api for FakeApi {
        fn call(&mut self, method: &str, params: Value) -> Result<Value, Error> {
            self.calls.push((method.to_string(), params));
            self.replies
                .get_mut(method)
                .and_then(VecDeque::pop_front)
                .unwrap_or(Ok(Value::Null))
        }
    }
}
