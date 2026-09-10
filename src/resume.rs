//! Carrying a mode across a popup hop.
//!
//! Since herdr 0.9.0 a popup belongs to the tab it opened on: the client
//! draws it and routes keys to it only while that tab is the one on screen.
//! Any action that lands on another tab (or space) therefore strands the
//! popup: alive, invisible, deaf. The way through is to let this popup die
//! and have a fresh one open on the new tab.
//!
//! The popup cannot reopen itself: a popup is a singleton, and herdr kills
//! every process in the popup's session when it closes, so a detached child
//! would die with it. Instead the popup leaves a note in the plugin state
//! dir, asks herdr to run the mode's own `open` action (which herdr spawns
//! outside the popup's session), and exits. The action finds the note,
//! retries `plugin.pane.open` until the old popup has actually gone, and
//! hands the note to the new popup through its environment.
//!
//! The state dir is per plugin, not per herdr session, so a note also
//! records the socket it was written for. An `open` running under another
//! herdr leaves a note that is not its own alone.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Env var the reopened popup reads its state from.
pub const ENV: &str = "HERDR_MODES_RESUME";

/// A note older than this belongs to a hop that never completed; ignore it.
const FRESH_FOR: Duration = Duration::from_secs(5);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Resume {
    pub mode: String,
    /// Set by `new`, so a note is never written unstamped.
    written_unix_ms: u64,
    /// The herdr session (its socket path) this note belongs to.
    #[serde(default)]
    pub socket: String,
    pub prev_tab_id: Option<String>,
    pub prev_agent_id: Option<String>,
    pub prev_workspace_id: Option<String>,
    pub origin_workspace_id: String,
    pub origin_pane_id: String,
    /// Last feedback line, so the new popup does not come up blank.
    pub feedback: String,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Resume {
    /// A note stamped with the current time. `mode` is the entrypoint the
    /// `open` action will be asked for, `socket` the herdr session it is
    /// meant for; the rest is what `Session::restore` picks back up.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mode: &str,
        socket: &str,
        feedback: &str,
        prev_tab_id: Option<String>,
        prev_agent_id: Option<String>,
        prev_workspace_id: Option<String>,
        origin_workspace_id: String,
        origin_pane_id: String,
    ) -> Self {
        Resume {
            mode: mode.to_string(),
            written_unix_ms: now_ms(),
            socket: socket.to_string(),
            prev_tab_id,
            prev_agent_id,
            prev_workspace_id,
            origin_workspace_id,
            origin_pane_id,
            feedback: feedback.to_string(),
        }
    }

    pub fn fresh(&self) -> bool {
        now_ms().saturating_sub(self.written_unix_ms) < FRESH_FOR.as_millis() as u64
    }

    /// Read back what the `open` action handed to this popup.
    pub fn from_env() -> Option<Resume> {
        let text = std::env::var(ENV).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn to_env(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// Where the note lives and which herdr session it is read for. Built from
/// the environment in `main`, from a temp dir in tests.
#[derive(Clone, Debug)]
pub struct Store {
    path: PathBuf,
    socket: String,
}

impl Store {
    /// The note under `dir`, scoped to the herdr session at `socket`.
    pub fn new(dir: &Path, socket: &str) -> Self {
        Store {
            path: dir.join("resume.json"),
            socket: socket.to_string(),
        }
    }

    /// `$HERDR_PLUGIN_STATE_DIR/resume.json` for the herdr at
    /// `$HERDR_SOCKET_PATH`.
    pub fn from_env() -> Self {
        let dir = std::env::var("HERDR_PLUGIN_STATE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join("herdr-modes"));
        let socket = std::env::var("HERDR_SOCKET_PATH").unwrap_or_default();
        Store::new(&dir, &socket)
    }

    /// The socket path stamped on every note written through this store.
    pub fn socket(&self) -> &str {
        &self.socket
    }

    /// Leave the note for the `open` action.
    pub fn write(&self, note: &Resume) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.path, serde_json::to_vec(note)?)
    }

    /// The note waiting for `open`, if any. A stale or unreadable note is
    /// removed rather than honoured, so a hop that never finished cannot
    /// haunt the next plain keypress. A fresh note for another herdr session
    /// is left alone: it belongs to an `open` that has not run yet.
    pub fn pending(&self, mode: &str) -> Option<Resume> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        let parsed: Option<Resume> = serde_json::from_str(&text).ok();
        match parsed {
            Some(r) if !r.fresh() => {
                let _ = std::fs::remove_file(&self.path);
                None
            }
            Some(r) if r.socket != self.socket => None,
            Some(r) if r.mode == mode => Some(r),
            _ => {
                let _ = std::fs::remove_file(&self.path);
                None
            }
        }
    }

    /// Whatever note is on disk, whichever session and mode it is for.
    fn read(&self) -> Option<Resume> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Whether this session's note is still on disk: the popup deletes it to
    /// call a hop off after the action it was armed for failed. A note
    /// another session wrote in the meantime does not count.
    pub fn still_pending(&self) -> bool {
        self.read().is_some_and(|r| r.socket == self.socket)
    }

    /// Remove the note, unless it is another session's and still fresh: that
    /// one belongs to an `open` that has not run yet.
    pub fn clear(&self) {
        if self
            .read()
            .is_some_and(|r| r.socket != self.socket && r.fresh())
        {
            return;
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A directory under the system temp dir that goes away with this value.
#[cfg(test)]
pub struct TempDir(PathBuf);

#[cfg(test)]
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A store in a directory of its own under the system temp dir, empty at
/// the start of the test and removed when the returned guard drops. Nothing
/// here touches process-wide environment variables, so tests can run in
/// parallel.
#[cfg(test)]
pub fn temp_store(test: &str, socket: &str) -> (TempDir, Store) {
    let dir = std::env::temp_dir().join(format!("herdr-modes-test-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    (TempDir(dir.clone()), Store::new(&dir, socket))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(mode: &str, socket: &str) -> Resume {
        Resume::new(
            mode,
            socket,
            "hello",
            Some("t0".into()),
            None,
            None,
            "w0".into(),
            "p0".into(),
        )
    }

    #[test]
    fn pending_returns_a_fresh_note_for_this_session() {
        let (_dir, store) = temp_store("pending_fresh", "/run/herdr/a.sock");
        let n = note("tab", "/run/herdr/a.sock");
        store.write(&n).unwrap();
        assert_eq!(store.pending("tab"), Some(n));
        assert!(store.still_pending());
    }

    #[test]
    fn pending_ignores_but_keeps_a_note_for_another_session() {
        let (_dir, store) = temp_store("pending_other_socket", "/run/herdr/a.sock");
        let n = note("tab", "/run/herdr/b.sock");
        store.write(&n).unwrap();
        assert_eq!(store.pending("tab"), None);
        assert!(!store.still_pending(), "not this session's note");
        let theirs = Store::new(&_dir.0, "/run/herdr/b.sock");
        assert_eq!(theirs.pending("tab"), Some(n), "their note must survive");
    }

    #[test]
    fn pending_removes_a_stale_note_whatever_its_session() {
        let (_dir, store) = temp_store("pending_stale", "/run/herdr/a.sock");
        let mut n = note("tab", "/run/herdr/b.sock");
        n.written_unix_ms = now_ms() - FRESH_FOR.as_millis() as u64 - 1;
        store.write(&n).unwrap();
        assert_eq!(store.pending("tab"), None);
        assert!(!store.still_pending());
    }

    #[test]
    fn pending_removes_a_note_for_another_mode() {
        let (_dir, store) = temp_store("pending_other_mode", "/run/herdr/a.sock");
        store.write(&note("pane", "/run/herdr/a.sock")).unwrap();
        assert_eq!(store.pending("tab"), None);
        assert!(!store.still_pending());
    }

    #[test]
    fn pending_removes_an_unreadable_note() {
        let (_dir, store) = temp_store("pending_garbage", "/run/herdr/a.sock");
        std::fs::write(&store.path, b"{not json").unwrap();
        assert_eq!(store.pending("tab"), None);
        assert!(!store.still_pending());
    }

    #[test]
    fn clear_removes_the_note() {
        let (_dir, store) = temp_store("clear", "/run/herdr/a.sock");
        store.write(&note("tab", "/run/herdr/a.sock")).unwrap();
        store.clear();
        assert!(!store.still_pending());
        assert_eq!(store.pending("tab"), None);
    }

    #[test]
    fn clear_leaves_a_fresh_note_for_another_session() {
        let (_dir, store) = temp_store("clear_other_socket", "/run/herdr/a.sock");
        let n = note("tab", "/run/herdr/b.sock");
        store.write(&n).unwrap();
        store.clear();
        assert!(!store.still_pending(), "not this session's note");
        let theirs = Store::new(&_dir.0, "/run/herdr/b.sock");
        assert!(theirs.still_pending());
        assert_eq!(theirs.pending("tab"), Some(n));
    }

    #[test]
    fn clear_removes_a_stale_note_for_another_session() {
        let (_dir, store) = temp_store("clear_other_stale", "/run/herdr/a.sock");
        let mut n = note("tab", "/run/herdr/b.sock");
        n.written_unix_ms = now_ms() - FRESH_FOR.as_millis() as u64 - 1;
        store.write(&n).unwrap();
        store.clear();
        assert!(!store.path.exists());
    }

    #[test]
    fn clear_removes_an_unreadable_note() {
        let (_dir, store) = temp_store("clear_garbage", "/run/herdr/a.sock");
        std::fs::write(&store.path, b"{not json").unwrap();
        store.clear();
        assert!(!store.path.exists());
    }

    /// What `open` does on a plain keypress in session A while session B's
    /// hop is in flight: find no note of its own, open its popup, clear.
    /// B's note has to survive that, or B's `open` finds nothing to reopen.
    #[test]
    fn a_plain_open_in_another_session_keeps_the_note() {
        let (_dir, mine) = temp_store("open_other_session", "/run/herdr/a.sock");
        let theirs = Store::new(&_dir.0, "/run/herdr/b.sock");
        let n = note("tab", "/run/herdr/b.sock");
        theirs.write(&n).unwrap();

        assert_eq!(mine.pending("tab"), None);
        mine.clear();

        assert!(theirs.still_pending());
        assert_eq!(theirs.pending("tab"), Some(n));
    }

    #[test]
    fn env_round_trip_keeps_every_field() {
        let n = note("space", "/run/herdr/a.sock");
        let back: Resume = serde_json::from_str(&n.to_env()).unwrap();
        assert_eq!(back, n);
    }
}
