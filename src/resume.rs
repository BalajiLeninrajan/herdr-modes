//! Carrying a mode across a popup hop.
//!
//! Since herdr 0.9.0 a popup belongs to the tab it opened on: the client
//! draws it and routes keys to it only while that tab is the one on screen.
//! Any action that lands on another tab (or space) therefore strands the
//! popup — alive, invisible, deaf. The way through is to let this popup die
//! and have a fresh one open on the new tab.
//!
//! The popup cannot reopen itself: a popup is a singleton, and herdr kills
//! every process in the popup's session when it closes, so a detached child
//! would die with it. Instead the popup leaves a note in the plugin state
//! dir, asks herdr to run the mode's own `open` action (which herdr spawns
//! outside the popup's session), and exits. The action finds the note,
//! retries `plugin.pane.open` until the old popup has actually gone, and
//! hands the note to the new popup through its environment.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Env var the reopened popup reads its state from.
pub const ENV: &str = "HERDR_MODES_RESUME";

/// A note older than this belongs to a hop that never completed; ignore it.
const FRESH_FOR: Duration = Duration::from_secs(5);

#[derive(Serialize, Deserialize, Clone)]
pub struct Resume {
    pub mode: String,
    pub written_unix_ms: u64,
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

fn path() -> PathBuf {
    let dir = std::env::var("HERDR_PLUGIN_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("herdr-modes"));
    dir.join("resume.json")
}

impl Resume {
    pub fn fresh(&self) -> bool {
        now_ms().saturating_sub(self.written_unix_ms) < FRESH_FOR.as_millis() as u64
    }

    pub fn stamp(mut self) -> Self {
        self.written_unix_ms = now_ms();
        self
    }

    /// Leave the note for the `open` action.
    pub fn write(&self) -> std::io::Result<()> {
        let p = path();
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(p, serde_json::to_vec(self)?)
    }

    /// The note waiting for `open`, if any. A stale or unreadable note is
    /// removed rather than honoured, so a hop that never finished cannot
    /// haunt the next plain keypress.
    pub fn pending(mode: &str) -> Option<Resume> {
        let p = path();
        let text = std::fs::read_to_string(&p).ok()?;
        let parsed: Option<Resume> = serde_json::from_str(&text).ok();
        match parsed {
            Some(r) if r.fresh() && r.mode == mode => Some(r),
            _ => {
                let _ = std::fs::remove_file(&p);
                None
            }
        }
    }

    /// Whether the note is still on disk: the popup deletes it to call a hop
    /// off after the action it was armed for failed.
    pub fn still_pending() -> bool {
        path().exists()
    }

    pub fn clear() {
        let _ = std::fs::remove_file(path());
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
