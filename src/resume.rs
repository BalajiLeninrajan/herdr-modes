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
//! The state dir is per plugin, not per herdr session, so each session
//! keeps its note in a file named from a hash of its socket path. Two herdr
//! sessions hopping at once then never touch each other's note.

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
    /// `open` action will be asked for; the rest is what `Session::restore`
    /// picks back up.
    pub fn new(
        mode: &str,
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

/// Where this herdr session's note lives. Built from the environment in
/// `main`, from a temp dir in tests.
#[derive(Clone, Debug)]
pub struct Store {
    path: PathBuf,
}

/// FNV-1a over the bytes, so a socket path maps to the same file name
/// whichever build of the plugin wrote or reads it.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

impl Store {
    /// The note under `dir` for the herdr session at `socket`.
    pub fn new(dir: &Path, socket: &str) -> Self {
        Store {
            path: dir.join(format!("resume-{:016x}.json", fnv1a(socket.as_bytes()))),
        }
    }

    /// `$HERDR_PLUGIN_STATE_DIR/resume-<hash>.json` for the herdr at
    /// `$HERDR_SOCKET_PATH`.
    pub fn from_env() -> Self {
        let dir = std::env::var("HERDR_PLUGIN_STATE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join("herdr-modes"));
        let socket = std::env::var("HERDR_SOCKET_PATH").unwrap_or_default();
        Store::new(&dir, &socket)
    }

    /// Leave the note for the `open` action.
    pub fn write(&self, note: &Resume) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.path, serde_json::to_vec(note)?)
    }

    /// The note waiting for `open`, if any. A stale, unreadable or
    /// other-mode note is removed rather than honoured, so a hop that never
    /// finished cannot haunt the next plain keypress.
    pub fn pending(&self, mode: &str) -> Option<Resume> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        match serde_json::from_str::<Resume>(&text) {
            Ok(r) if r.fresh() && r.mode == mode => Some(r),
            _ => {
                let _ = std::fs::remove_file(&self.path);
                None
            }
        }
    }

    /// Whether the note is still on disk: the popup deletes it to call a hop
    /// off after the action it was armed for failed.
    pub fn still_pending(&self) -> bool {
        self.path.exists()
    }

    /// Remove the note.
    pub fn clear(&self) {
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

    fn note(mode: &str) -> Resume {
        Resume::new(
            mode,
            "hello",
            Some("t0".into()),
            None,
            None,
            "w0".into(),
            "p0".into(),
        )
    }

    #[test]
    fn pending_returns_a_fresh_note_for_this_mode() {
        let (_dir, store) = temp_store("pending_fresh", "/run/herdr/a.sock");
        let n = note("tab");
        store.write(&n).unwrap();
        assert_eq!(store.pending("tab"), Some(n));
        assert!(store.still_pending());
    }

    #[test]
    fn pending_removes_a_stale_note() {
        let (_dir, store) = temp_store("pending_stale", "/run/herdr/a.sock");
        let mut n = note("tab");
        n.written_unix_ms = now_ms() - FRESH_FOR.as_millis() as u64 - 1;
        store.write(&n).unwrap();
        assert_eq!(store.pending("tab"), None);
        assert!(!store.still_pending());
    }

    #[test]
    fn pending_removes_a_note_for_another_mode() {
        let (_dir, store) = temp_store("pending_other_mode", "/run/herdr/a.sock");
        store.write(&note("pane")).unwrap();
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
        store.write(&note("tab")).unwrap();
        store.clear();
        assert!(!store.still_pending());
        assert_eq!(store.pending("tab"), None);
    }

    /// The name has to come out the same from every build, or a popup and
    /// the `open` it invokes could disagree about where the note is.
    #[test]
    fn the_file_name_is_a_fixed_hash_of_the_socket() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        let (_dir, store) = temp_store("file_name", "/run/herdr/a.sock");
        assert_eq!(
            store.path.file_name().unwrap(),
            "resume-1efef9257105457d.json"
        );
    }

    /// Two herdr sessions hopping close together each write a note before
    /// either `open` has run. Neither may overwrite the other's.
    #[test]
    fn two_sessions_keep_their_notes_apart() {
        let (_dir, a) = temp_store("two_sessions", "/run/herdr/a.sock");
        let b = Store::new(&_dir.0, "/run/herdr/b.sock");
        let na = note("tab");
        let nb = Resume {
            feedback: "from b".into(),
            ..note("space")
        };
        a.write(&na).unwrap();
        b.write(&nb).unwrap();

        assert!(a.still_pending());
        assert_eq!(a.pending("tab"), Some(na));
        a.clear();

        assert!(b.still_pending());
        assert_eq!(b.pending("space"), Some(nb));
    }

    /// What `open` does on a plain keypress in session A while session B's
    /// hop is in flight: find no note of its own, open its popup, clear.
    /// B's note has to survive that, or B's `open` finds nothing to reopen.
    #[test]
    fn a_plain_open_in_another_session_keeps_the_note() {
        let (_dir, mine) = temp_store("open_other_session", "/run/herdr/a.sock");
        let theirs = Store::new(&_dir.0, "/run/herdr/b.sock");
        let n = note("tab");
        theirs.write(&n).unwrap();

        assert_eq!(mine.pending("tab"), None);
        mine.clear();

        assert!(theirs.still_pending());
        assert_eq!(theirs.pending("tab"), Some(n));
    }

    #[test]
    fn env_round_trip_keeps_every_field() {
        let n = note("space");
        let back: Resume = serde_json::from_str(&n.to_env()).unwrap();
        assert_eq!(back, n);
    }
}
