//! Executes a keymap Action against the herdr socket API.

use crate::client::{Api, Error};
use crate::keymap::{Action, BreakTo, Dir};
use crate::nav;
use crate::resume::{Resume, Store};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::fmt;
use std::str::FromStr;

/// An agent's status as herdr reports it, in `agent_status` on both
/// `agent.list` rows and `workspace.list` rows (where it is the roll-up of the
/// space's agents). Unknown values are kept verbatim so feedback lines print
/// what the server said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentStatus {
    Blocked,
    Done,
    Working,
    Idle,
    Other(String),
}

impl AgentStatus {
    /// `blocked` is an approval or question waiting on you; `done` is finished
    /// background work you have not seen yet. Those two are the panel's
    /// attention queue; the rest are either busy or already read.
    pub fn wants_attention(&self) -> bool {
        matches!(self, AgentStatus::Blocked | AgentStatus::Done)
    }
}

impl FromStr for AgentStatus {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Infallible> {
        Ok(match s {
            "blocked" => AgentStatus::Blocked,
            "done" => AgentStatus::Done,
            "working" => AgentStatus::Working,
            "idle" => AgentStatus::Idle,
            other => AgentStatus::Other(other.to_string()),
        })
    }
}

impl fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AgentStatus::Blocked => "blocked",
            AgentStatus::Done => "done",
            AgentStatus::Working => "working",
            AgentStatus::Idle => "idle",
            AgentStatus::Other(s) => s,
        })
    }
}

/// The `agent_status` field of a list row; "unknown" when it is missing.
fn status_of(row: &Value) -> AgentStatus {
    let Ok(status) = row["agent_status"].as_str().unwrap_or("unknown").parse();
    status
}

/// One row of herdr's agent panel, in the order `agent.list` reports them:
/// grouped by space, which is the panel's own `agent_panel_sort = "spaces"`
/// ordering.
struct Agent {
    pane_id: String,
    status: AgentStatus,
    label: String,
}

/// One row of herdr's space sidebar, in `workspace.list` order, the same
/// order the sidebar draws and `number` counts.
struct Space {
    id: String,
    label: String,
    /// The space's rolled-up agent status.
    status: AgentStatus,
    focused: bool,
}

pub struct Session<A: Api> {
    pub api: A,
    /// Where a hop note is left, and for which herdr session.
    store: Store,
    /// This plugin's id, the one `plugin.action.invoke` is addressed to.
    plugin_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    pub pane_id: String,
    /// For zellij's `tab` binding (ToggleTab).
    prev_tab_id: Option<String>,
    /// The agent pane focus came from, for `last_agent`.
    prev_agent_id: Option<String>,
    /// The space focus came from, for `last_space`.
    prev_workspace_id: Option<String>,
    /// Where the mode opened, for `cancel`.
    origin_workspace_id: String,
    origin_pane_id: String,
}

impl<A: Api> Session<A> {
    pub fn new(
        api: A,
        store: Store,
        plugin_id: String,
        workspace_id: String,
        tab_id: String,
        pane_id: String,
    ) -> Self {
        Session {
            api,
            store,
            plugin_id,
            origin_workspace_id: workspace_id.clone(),
            origin_pane_id: pane_id.clone(),
            workspace_id,
            tab_id,
            pane_id,
            prev_tab_id: None,
            prev_agent_id: None,
            prev_workspace_id: None,
        }
    }

    /// Pick up where the popup this one replaced left off.
    pub fn restore(&mut self, r: &Resume) {
        self.prev_tab_id = r.prev_tab_id.clone();
        self.prev_agent_id = r.prev_agent_id.clone();
        self.prev_workspace_id = r.prev_workspace_id.clone();
        self.origin_workspace_id = r.origin_workspace_id.clone();
        self.origin_pane_id = r.origin_pane_id.clone();
    }

    fn resume(&self, mode: &str, feedback: &str) -> Resume {
        Resume::new(
            mode,
            self.store.socket(),
            feedback,
            self.prev_tab_id.clone(),
            self.prev_agent_id.clone(),
            self.prev_workspace_id.clone(),
            self.origin_workspace_id.clone(),
            self.origin_pane_id.clone(),
        )
    }

    /// Arrange for a fresh popup on whatever tab is on screen once this one
    /// is gone: leave the note, then have herdr run the mode's `open` action.
    /// The caller exits afterwards; the action waits for that.
    pub fn arm_hop(&mut self, mode: &str, feedback: &str) -> Result<(), Error> {
        self.store.write(&self.resume(mode, feedback))?;
        let r = self.api.call(
            "plugin.action.invoke",
            json!({ "plugin_id": &self.plugin_id, "action_id": mode }),
        );
        if r.is_err() {
            self.store.clear();
        }
        r.map(|_| ())
    }

    /// Call an armed hop off: the action it was armed for failed, so the
    /// popup is staying and the waiting `open` should stand down.
    pub fn disarm_hop(&self) {
        self.store.clear();
    }

    /// Put the viewing client on the tab the server (and so this popup) is on.
    pub fn sync_view(&mut self) -> Result<(), Error> {
        if self.tab_id.is_empty() {
            return Ok(());
        }
        let tab = self.tab_id.clone();
        self.api.call("tab.focus", json!({ "tab_id": tab }))?;
        Ok(())
    }

    /// Whether this action is about to take the popup's own tab away, which
    /// herdr answers by closing the popup before the request even returns.
    /// Such actions arm the hop first, since there is no "after".
    pub fn will_close_owner_tab(&mut self, action: Action) -> Result<bool, Error> {
        Ok(match action {
            Action::CloseTab => true,
            Action::ClosePane => self.tab_panes()?.len() <= 1,
            _ => false,
        })
    }

    /// Re-read focus from the server. Popups have no pane id of their own, so
    /// the snapshot always reports the real underlying pane even while a mode
    /// is on screen.
    pub fn refresh(&mut self) -> Result<(), Error> {
        let r = self.api.call("session.snapshot", json!({}))?;
        let s = &r["snapshot"];
        if let Some(v) = s["focused_workspace_id"].as_str() {
            self.workspace_id = v.to_string();
        }
        if let Some(v) = s["focused_tab_id"].as_str() {
            self.tab_id = v.to_string();
        }
        if let Some(v) = s["focused_pane_id"].as_str() {
            self.pane_id = v.to_string();
        }
        Ok(())
    }

    fn tabs(&mut self) -> Result<Vec<String>, Error> {
        let r = self
            .api
            .call("tab.list", json!({ "workspace_id": self.workspace_id }))?;
        Ok(r["tabs"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|t| t["tab_id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Panes of the current tab, in list order.
    fn tab_panes(&mut self) -> Result<Vec<String>, Error> {
        let r = self
            .api
            .call("pane.list", json!({ "workspace_id": self.workspace_id }))?;
        Ok(r["panes"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|p| p["tab_id"].as_str() == Some(self.tab_id.as_str()))
                    .filter_map(|p| p["pane_id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }

    fn focus_tab(&mut self, tab_id: &str) -> Result<(), Error> {
        self.api.call("tab.focus", json!({ "tab_id": tab_id }))?;
        if tab_id != self.tab_id {
            self.prev_tab_id = Some(std::mem::replace(&mut self.tab_id, tab_id.to_string()));
        }
        self.refresh()
    }

    fn step_tab(&mut self, delta: i64) -> Result<String, Error> {
        let tabs = self.tabs()?;
        if tabs.len() < 2 {
            return Ok("only one tab".into());
        }
        let cur = tabs.iter().position(|t| *t == self.tab_id).unwrap_or(0);
        let n = tabs.len();
        let next = nav::step(Some(cur), delta, n);
        let target = tabs[next].clone();
        self.focus_tab(&target)?;
        Ok(format!("tab {}/{}", next + 1, n))
    }

    pub fn execute(
        &mut self,
        action: Action,
        prompt: impl FnOnce(&str) -> Option<String>,
    ) -> Result<String, Error> {
        match action {
            Action::Focus(dir) => self.focus(dir),
            Action::CycleFocus => self.cycle_focus(),
            Action::ClosePane => self.close_pane(),
            Action::Split(dir) => self.split(dir),
            Action::Zoom => self.zoom(),
            Action::RenamePane => self.rename_pane(prompt),

            Action::PrevTab => self.step_tab(-1),
            Action::NextTab => self.step_tab(1),
            Action::LastTab => self.last_tab(),
            Action::GotoTab(n) => self.goto_tab(n),
            Action::NewTab => self.new_tab(),
            Action::CloseTab => self.close_tab(),
            Action::RenameTab => self.rename_tab(prompt),
            Action::MoveTab(delta) => self.move_tab(delta),
            Action::BreakPane(target) => self.break_pane(target),

            Action::Swap(dir) => self.swap(dir),
            Action::SwapCycle(forward) => self.swap_cycle(forward),

            Action::PrevAgent => self.step_agent(-1),
            Action::NextAgent => self.step_agent(1),
            Action::LastAgent => self.last_agent(),
            Action::GotoAgent(n) => self.goto_agent(n),
            Action::NextAttention => self.step_attention(1),
            Action::PrevAttention => self.step_attention(-1),

            Action::PrevSpace => self.step_space(-1),
            Action::NextSpace => self.step_space(1),
            Action::LastSpace => self.last_space(),
            Action::GotoSpace(n) => self.goto_space(n),
            Action::NextSpaceAttention => self.step_space_attention(1),
            Action::PrevSpaceAttention => self.step_space_attention(-1),

            Action::Cancel => self.cancel(),
            // `exit` closes the popup and touches nothing, so the key loop
            // answers it before asking the session to execute anything.
            Action::Quit => unreachable!("exit is decided before execute"),
        }
    }

    fn focus(&mut self, dir: Dir) -> Result<String, Error> {
        let r = self.api.call(
            "pane.focus_direction",
            json!({ "direction": dir.as_str(), "pane_id": self.pane_id }),
        )?;
        let f = &r["focus"];
        if let Some(id) = f["focused_pane_id"].as_str() {
            self.pane_id = id.to_string();
        }
        if f["changed"].as_bool() == Some(false) {
            // `no_neighbor` at an edge. Report it rather than silently doing nothing.
            return Ok(format!(
                "{} \u{b7} {}",
                dir.as_str(),
                f["reason"].as_str().unwrap_or("no change")
            ));
        }
        Ok(format!("focus {}", dir.as_str()))
    }

    fn cycle_focus(&mut self) -> Result<String, Error> {
        let panes = self.tab_panes()?;
        if panes.len() < 2 {
            return Ok("only one pane".into());
        }
        let cur = panes.iter().position(|p| *p == self.pane_id).unwrap_or(0);
        let next = panes[(cur + 1) % panes.len()].clone();
        self.api.call("pane.focus", json!({ "pane_id": next }))?;
        self.pane_id = next;
        Ok("cycle".into())
    }

    fn close_pane(&mut self) -> Result<String, Error> {
        self.api
            .call("pane.close", json!({ "pane_id": self.pane_id }))?;
        self.refresh()?;
        Ok("closed pane".into())
    }

    fn split(&mut self, dir: &str) -> Result<String, Error> {
        let r = self.api.call(
            "pane.split",
            json!({ "direction": dir, "target_pane_id": self.pane_id, "focus": true }),
        )?;
        if let Some(id) = r["pane"]["pane_id"].as_str() {
            self.pane_id = id.to_string();
        }
        Ok(format!("split {dir}"))
    }

    fn zoom(&mut self) -> Result<String, Error> {
        self.api.call(
            "pane.zoom",
            json!({ "pane_id": self.pane_id, "mode": "toggle" }),
        )?;
        Ok("zoom".into())
    }

    fn rename_pane(
        &mut self,
        prompt: impl FnOnce(&str) -> Option<String>,
    ) -> Result<String, Error> {
        let Some(label) = prompt("rename pane: ") else {
            return Ok("cancelled".into());
        };
        let label: Value = if label.is_empty() {
            Value::Null
        } else {
            Value::String(label)
        };
        self.api.call(
            "pane.rename",
            json!({ "pane_id": self.pane_id, "label": label }),
        )?;
        Ok("renamed pane".into())
    }

    fn last_tab(&mut self) -> Result<String, Error> {
        let Some(prev) = self.prev_tab_id.clone() else {
            return Ok("no previous tab".into());
        };
        self.focus_tab(&prev)?;
        Ok("last tab".into())
    }

    fn goto_tab(&mut self, n: usize) -> Result<String, Error> {
        let tabs = self.tabs()?;
        let Some(target) = tabs.get(n - 1).cloned() else {
            return Ok(format!("no tab {n}"));
        };
        self.focus_tab(&target)?;
        Ok(format!("tab {n}"))
    }

    fn new_tab(&mut self) -> Result<String, Error> {
        self.api.call(
            "tab.create",
            json!({ "workspace_id": self.workspace_id, "focus": true }),
        )?;
        self.refresh()?;
        Ok("new tab".into())
    }

    fn close_tab(&mut self) -> Result<String, Error> {
        self.api
            .call("tab.close", json!({ "tab_id": self.tab_id }))?;
        self.prev_tab_id = None;
        self.refresh()?;
        Ok("closed tab".into())
    }

    fn rename_tab(&mut self, prompt: impl FnOnce(&str) -> Option<String>) -> Result<String, Error> {
        let Some(label) = prompt("rename tab: ") else {
            return Ok("cancelled".into());
        };
        if label.is_empty() {
            return Ok("cancelled".into());
        }
        self.api.call(
            "tab.rename",
            json!({ "tab_id": self.tab_id, "label": label }),
        )?;
        Ok("renamed tab".into())
    }

    /// `tab.move` has no CLI subcommand; it is reachable only over the socket.
    /// `nav::tab_move` explains the off-by-one in `insert_index`.
    fn move_tab(&mut self, delta: i64) -> Result<String, Error> {
        let tabs = self.tabs()?;
        if tabs.len() < 2 {
            return Ok("only one tab".into());
        }
        let cur = tabs.iter().position(|t| *t == self.tab_id).unwrap_or(0);
        let Some((insert, landed)) = nav::tab_move(cur, delta, tabs.len()) else {
            return Ok("at end".into());
        };
        self.api.call(
            "tab.move",
            json!({ "tab_id": self.tab_id, "insert_index": insert as u64 }),
        )?;
        Ok(format!("moved tab -> {}", landed + 1))
    }

    fn break_pane(&mut self, target: BreakTo) -> Result<String, Error> {
        let destination = match target {
            BreakTo::New => json!({ "type": "new_tab" }),
            BreakTo::Prev | BreakTo::Next => {
                let tabs = self.tabs()?;
                if tabs.len() < 2 {
                    return Ok("no other tab".into());
                }
                let cur = tabs.iter().position(|t| *t == self.tab_id).unwrap_or(0);
                let delta = if matches!(target, BreakTo::Prev) {
                    -1
                } else {
                    1
                };
                let idx = nav::wrap(cur as i64 + delta, tabs.len());
                json!({ "type": "tab", "tab_id": tabs[idx], "split": "right" })
            }
        };
        self.api.call(
            "pane.move",
            json!({ "pane_id": self.pane_id, "destination": destination, "focus": true }),
        )?;
        // Same as `agent.focus`: the move's `focus` reaches the server only,
        // so the client has to be brought along explicitly.
        let pane = self.pane_id.clone();
        self.api.call("pane.focus", json!({ "pane_id": pane }))?;
        self.refresh()?;
        Ok("broke pane out".into())
    }

    fn swap(&mut self, dir: Dir) -> Result<String, Error> {
        self.api.call(
            "pane.swap",
            json!({ "direction": dir.as_str(), "pane_id": self.pane_id }),
        )?;
        Ok(format!("swap {}", dir.as_str()))
    }

    fn swap_cycle(&mut self, forward: bool) -> Result<String, Error> {
        let panes = self.tab_panes()?;
        if panes.len() < 2 {
            return Ok("only one pane".into());
        }
        let cur = panes.iter().position(|p| *p == self.pane_id).unwrap_or(0);
        let delta = if forward { 1 } else { -1 };
        let idx = nav::wrap(cur as i64 + delta, panes.len());
        self.api.call(
            "pane.swap",
            json!({ "source_pane_id": self.pane_id, "target_pane_id": panes[idx] }),
        )?;
        Ok(if forward {
            "swap forward".into()
        } else {
            "swap back".into()
        })
    }

    fn agents(&mut self) -> Result<Vec<Agent>, Error> {
        let r = self.api.call("agent.list", json!({}))?;
        Ok(r["agents"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|a| {
                        Some(Agent {
                            pane_id: a["pane_id"].as_str()?.to_string(),
                            status: status_of(a),
                            // Most identifying first: several rows are usually
                            // the same kind of agent, so "claude" names nothing.
                            label: ["name", "terminal_title_stripped", "display_agent", "agent"]
                                .iter()
                                .find_map(|k| a[k].as_str())
                                .unwrap_or("agent")
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Where the panel's cursor sits: the agent holding the focused pane, or
    /// `None` when focus is on a pane that has no agent in it at all.
    fn agent_at_focus(&self, agents: &[Agent]) -> Option<usize> {
        agents.iter().position(|a| a.pane_id == self.pane_id)
    }

    /// `agent.focus` crosses spaces and tabs on its own, so the whole snapshot
    /// has to be re-read afterwards, not just the pane.
    fn focus_agent(&mut self, agents: &[Agent], index: usize) -> Result<String, Error> {
        let target = &agents[index];
        let n = agents.len();
        if target.pane_id == self.pane_id {
            return Ok(format!(
                "{}/{n} {} \u{b7} already here",
                index + 1,
                target.label
            ));
        }
        let line = format!(
            "{}/{n} {} \u{b7} {}",
            index + 1,
            target.label,
            target.status
        );
        self.api
            .call("agent.focus", json!({ "target": &target.pane_id }))?;
        if self.agent_at_focus(agents).is_some() {
            self.prev_agent_id = Some(self.pane_id.clone());
        }
        // `agent.focus` moves the server's focus but, on herdr 0.9.0, not the
        // client's view. `pane.focus` is one of the calls the client follows.
        self.api
            .call("pane.focus", json!({ "pane_id": &target.pane_id }))?;
        self.refresh()?;
        Ok(line)
    }

    fn step_agent(&mut self, delta: i64) -> Result<String, Error> {
        let agents = self.agents()?;
        if agents.is_empty() {
            return Ok("no agents".into());
        }
        // From a pane with no agent in it, stepping enters the panel at
        // whichever end the direction implies rather than doing nothing.
        let next = nav::step(self.agent_at_focus(&agents), delta, agents.len());
        self.focus_agent(&agents, next)
    }

    fn goto_agent(&mut self, n: usize) -> Result<String, Error> {
        let agents = self.agents()?;
        if n > agents.len() {
            return Ok(format!("no agent {n}"));
        }
        self.focus_agent(&agents, n - 1)
    }

    fn last_agent(&mut self) -> Result<String, Error> {
        let agents = self.agents()?;
        let prev = self.prev_agent_id.clone();
        // The agent may have exited while we were away.
        let Some(index) = prev.and_then(|p| agents.iter().position(|a| a.pane_id == p)) else {
            return Ok("no previous agent".into());
        };
        self.focus_agent(&agents, index)
    }

    /// Walk the panel from the cursor until an agent that wants you turns up,
    /// wrapping once around. Skipping the busy ones is the whole point, so this
    /// searches the list rather than cycling a filtered copy of it.
    fn step_attention(&mut self, delta: i64) -> Result<String, Error> {
        let agents = self.agents()?;
        if agents.is_empty() {
            return Ok("no agents".into());
        }
        let cur = self.agent_at_focus(&agents);
        let hit = nav::find_attention(&agents, cur, delta, |a| a.status.wants_attention());
        match hit {
            Some(i) => self.focus_agent(&agents, i),
            None => Ok("nothing waiting".into()),
        }
    }

    fn spaces(&mut self) -> Result<Vec<Space>, Error> {
        let r = self.api.call("workspace.list", json!({}))?;
        Ok(r["workspaces"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|w| {
                        Some(Space {
                            id: w["workspace_id"].as_str()?.to_string(),
                            label: w["label"].as_str().unwrap_or("space").to_string(),
                            status: status_of(w),
                            focused: w["focused"].as_bool().unwrap_or(false),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Where the sidebar's cursor sits. `focused` comes from the server, so it
    /// stays right even if a space was created or closed since the mode opened.
    fn space_at_focus(&self, spaces: &[Space]) -> Option<usize> {
        spaces
            .iter()
            .position(|s| s.focused)
            .or_else(|| spaces.iter().position(|s| s.id == self.workspace_id))
    }

    /// Focusing *is* the preview: the space really switches underneath, and the
    /// popup keeps the keystrokes because it is session-modal. `cancel` is what
    /// makes that safe to do while only browsing.
    fn focus_space(&mut self, spaces: &[Space], index: usize) -> Result<String, Error> {
        let target = &spaces[index];
        let n = spaces.len();
        let line = format!(
            "{}/{n} {} \u{b7} {}",
            index + 1,
            target.label,
            target.status
        );
        if target.id == self.workspace_id {
            return Ok(format!("{line} \u{b7} already here"));
        }
        let id = target.id.clone();
        self.api
            .call("workspace.focus", json!({ "workspace_id": id.clone() }))?;
        self.prev_workspace_id = Some(std::mem::replace(&mut self.workspace_id, id));
        // The new space brings its own active tab and pane with it.
        self.refresh()?;
        Ok(line)
    }

    fn step_space(&mut self, delta: i64) -> Result<String, Error> {
        let spaces = self.spaces()?;
        if spaces.len() < 2 {
            return Ok("only one space".into());
        }
        let cur = self.space_at_focus(&spaces).unwrap_or(0);
        let next = nav::step(Some(cur), delta, spaces.len());
        self.focus_space(&spaces, next)
    }

    fn goto_space(&mut self, n: usize) -> Result<String, Error> {
        let spaces = self.spaces()?;
        if n > spaces.len() {
            return Ok(format!("no space {n}"));
        }
        self.focus_space(&spaces, n - 1)
    }

    fn last_space(&mut self) -> Result<String, Error> {
        let spaces = self.spaces()?;
        let prev = self.prev_workspace_id.clone();
        // The space may have been closed while we were away.
        let Some(index) = prev.and_then(|p| spaces.iter().position(|s| s.id == p)) else {
            return Ok("no previous space".into());
        };
        self.focus_space(&spaces, index)
    }

    /// `step_attention` one level up: skip the spaces whose agents are all busy
    /// or already read, and land on the next one that is waiting on you.
    fn step_space_attention(&mut self, delta: i64) -> Result<String, Error> {
        let spaces = self.spaces()?;
        if spaces.is_empty() {
            return Ok("no spaces".into());
        }
        let cur = self.space_at_focus(&spaces);
        let hit = nav::find_attention(&spaces, cur, delta, |s| s.status.wants_attention());
        match hit {
            Some(i) => self.focus_space(&spaces, i),
            None => Ok("nothing waiting".into()),
        }
    }

    /// Leaving has two meanings once navigation is live: `exit` keeps wherever
    /// you landed, `cancel` puts you back where the mode opened.
    fn cancel(&mut self) -> Result<String, Error> {
        if self.pane_id == self.origin_pane_id {
            return Ok(String::new());
        }
        let workspace = self.origin_workspace_id.clone();
        let pane = self.origin_pane_id.clone();
        self.api
            .call("workspace.focus", json!({ "workspace_id": workspace }))?;
        // The pane may be gone, closed from inside the mode, and then the
        // space it lived in is as close to where you were as there is.
        let _ = self.api.call("pane.focus", json!({ "pane_id": pane }));
        self.refresh()?;
        Ok(String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::fake::FakeApi;
    use crate::resume::{TempDir, temp_store};

    const SOCKET: &str = "/run/herdr/test.sock";

    /// A session opened on pane `p1` of tab `t1` in space `w1`, with its
    /// hop note in a temp dir named after the test. The dir lives as long
    /// as the guard, so keep it in scope next to the session.
    fn session(test: &str, api: FakeApi) -> (TempDir, Session<FakeApi>) {
        let (dir, store) = temp_store(test, SOCKET);
        let session = Session::new(
            api,
            store,
            "herdr-modes".into(),
            "w1".into(),
            "t1".into(),
            "p1".into(),
        );
        (dir, session)
    }

    fn agents(rows: &[(&str, &str)]) -> Value {
        let agents: Vec<Value> = rows
            .iter()
            .map(|(pane, status)| json!({ "pane_id": pane, "name": pane, "agent_status": status }))
            .collect();
        json!({ "agents": agents })
    }

    fn tabs(ids: &[&str]) -> Value {
        let tabs: Vec<Value> = ids.iter().map(|id| json!({ "tab_id": id })).collect();
        json!({ "tabs": tabs })
    }

    fn panes(rows: &[(&str, &str)]) -> Value {
        let panes: Vec<Value> = rows
            .iter()
            .map(|(pane, tab)| json!({ "pane_id": pane, "tab_id": tab }))
            .collect();
        json!({ "panes": panes })
    }

    fn no_prompt(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn focus_agent_from_an_agent_row_records_where_it_came_from() {
        let api = FakeApi::new().reply("agent.list", agents(&[("p1", "idle"), ("p2", "done")]));
        let (_dir, mut s) = session("focus_agent_from_agent", api);

        let line = s.execute(Action::GotoAgent(2), no_prompt).unwrap();

        assert_eq!(line, "2/2 p2 \u{b7} done");
        assert_eq!(
            s.api.calls(),
            &[
                ("agent.list".to_string(), json!({})),
                ("agent.focus".to_string(), json!({ "target": "p2" })),
                ("pane.focus".to_string(), json!({ "pane_id": "p2" })),
                ("session.snapshot".to_string(), json!({})),
            ]
        );
        assert_eq!(s.prev_agent_id.as_deref(), Some("p1"));
    }

    #[test]
    fn focus_agent_from_a_plain_pane_records_no_previous_agent() {
        let api = FakeApi::new().reply("agent.list", agents(&[("p2", "blocked")]));
        let (_dir, mut s) = session("focus_agent_from_plain_pane", api);

        s.execute(Action::NextAgent, no_prompt).unwrap();

        assert_eq!(
            s.api.methods(),
            [
                "agent.list",
                "agent.focus",
                "pane.focus",
                "session.snapshot"
            ]
        );
        assert_eq!(s.api.calls()[2].1, json!({ "pane_id": "p2" }));
        assert_eq!(s.prev_agent_id, None);
    }

    #[test]
    fn break_pane_to_a_new_tab_moves_then_brings_the_client_along() {
        let (_dir, mut s) = session("break_pane_new", FakeApi::new());

        let line = s
            .execute(Action::BreakPane(BreakTo::New), no_prompt)
            .unwrap();

        assert_eq!(line, "broke pane out");
        assert_eq!(
            s.api.calls(),
            &[
                (
                    "pane.move".to_string(),
                    json!({
                        "pane_id": "p1",
                        "destination": { "type": "new_tab" },
                        "focus": true,
                    })
                ),
                ("pane.focus".to_string(), json!({ "pane_id": "p1" })),
                ("session.snapshot".to_string(), json!({})),
            ]
        );
    }

    #[test]
    fn closing_the_tab_always_takes_the_owner_tab_away() {
        let (_dir, mut s) = session("will_close_close_tab", FakeApi::new());
        assert!(s.will_close_owner_tab(Action::CloseTab).unwrap());
        assert!(s.api.calls().is_empty());
    }

    #[test]
    fn closing_the_last_pane_takes_the_owner_tab_away() {
        let api = FakeApi::new().reply("pane.list", panes(&[("p1", "t1"), ("p9", "t2")]));
        let (_dir, mut s) = session("will_close_last_pane", api);
        assert!(s.will_close_owner_tab(Action::ClosePane).unwrap());
        assert_eq!(
            s.api.calls(),
            &[("pane.list".to_string(), json!({ "workspace_id": "w1" }))]
        );
    }

    #[test]
    fn closing_one_of_two_panes_keeps_the_owner_tab() {
        let api = FakeApi::new().reply("pane.list", panes(&[("p1", "t1"), ("p2", "t1")]));
        let (_dir, mut s) = session("will_close_one_of_two", api);
        assert!(!s.will_close_owner_tab(Action::ClosePane).unwrap());
    }

    #[test]
    fn other_actions_never_take_the_owner_tab_away() {
        let (_dir, mut s) = session("will_close_other", FakeApi::new());
        assert!(!s.will_close_owner_tab(Action::NextTab).unwrap());
        assert!(!s.will_close_owner_tab(Action::Zoom).unwrap());
        assert!(s.api.calls().is_empty());
    }

    #[test]
    fn arm_hop_leaves_a_note_and_invokes_the_open_action() {
        let (_dir, mut s) = session("arm_hop_ok", FakeApi::new());
        s.prev_tab_id = Some("t0".into());
        s.prev_workspace_id = Some("w0".into());

        s.arm_hop("tab", "tab 2/3").unwrap();

        assert_eq!(
            s.api.calls(),
            &[(
                "plugin.action.invoke".to_string(),
                json!({ "plugin_id": "herdr-modes", "action_id": "tab" })
            )]
        );
        let note = s.store.pending("tab").expect("note written");
        assert_eq!(note.mode, "tab");
        assert_eq!(note.socket, SOCKET);
        assert_eq!(note.feedback, "tab 2/3");
        assert_eq!(note.prev_tab_id.as_deref(), Some("t0"));
        assert_eq!(note.prev_agent_id, None);
        assert_eq!(note.prev_workspace_id.as_deref(), Some("w0"));
        assert_eq!(note.origin_workspace_id, "w1");
        assert_eq!(note.origin_pane_id, "p1");
    }

    #[test]
    fn arm_hop_withdraws_the_note_when_the_invoke_fails() {
        let api = FakeApi::new().fail("plugin.action.invoke", "not_found", "no such action");
        let (_dir, mut s) = session("arm_hop_fail", api);

        let err = s.arm_hop("tab", "").unwrap_err();

        assert_eq!(err.to_string(), "not_found: no such action");
        assert_eq!(s.api.methods(), ["plugin.action.invoke"]);
        assert!(!s.store.still_pending());
    }

    #[test]
    fn disarm_hop_removes_the_note() {
        let (_dir, mut s) = session("disarm_hop", FakeApi::new());
        s.arm_hop("tab", "").unwrap();
        assert!(s.store.still_pending());
        s.disarm_hop();
        assert!(!s.store.still_pending());
    }

    #[test]
    fn cancel_at_the_origin_pane_does_nothing() {
        let (_dir, mut s) = session("cancel_at_origin", FakeApi::new());
        assert_eq!(s.execute(Action::Cancel, no_prompt).unwrap(), "");
        assert!(s.api.calls().is_empty());
    }

    #[test]
    fn cancel_elsewhere_goes_back_to_the_origin() {
        let (_dir, mut s) = session("cancel_elsewhere", FakeApi::new());
        s.workspace_id = "w2".into();
        s.tab_id = "t5".into();
        s.pane_id = "p7".into();

        assert_eq!(s.execute(Action::Cancel, no_prompt).unwrap(), "");

        assert_eq!(
            s.api.calls(),
            &[
                (
                    "workspace.focus".to_string(),
                    json!({ "workspace_id": "w1" })
                ),
                ("pane.focus".to_string(), json!({ "pane_id": "p1" })),
                ("session.snapshot".to_string(), json!({})),
            ]
        );
    }

    #[test]
    fn step_tab_wraps_from_the_last_tab_to_the_first() {
        let api = FakeApi::new().reply("tab.list", tabs(&["ta", "tb", "t1"]));
        let (_dir, mut s) = session("step_tab_wraps", api);

        let line = s.execute(Action::NextTab, no_prompt).unwrap();

        assert_eq!(line, "tab 1/3");
        assert_eq!(
            s.api.calls(),
            &[
                ("tab.list".to_string(), json!({ "workspace_id": "w1" })),
                ("tab.focus".to_string(), json!({ "tab_id": "ta" })),
                ("session.snapshot".to_string(), json!({})),
            ]
        );
        assert_eq!(s.tab_id, "ta");
        assert_eq!(s.prev_tab_id.as_deref(), Some("t1"));
    }

    #[test]
    fn restore_then_resume_round_trips_the_ids() {
        let note = Resume::new(
            "pane",
            SOCKET,
            "before the hop",
            Some("t0".into()),
            Some("p0".into()),
            Some("w0".into()),
            "w9".into(),
            "p9".into(),
        );
        let (_dir, mut s) = session("restore_resume", FakeApi::new());

        s.restore(&note);
        let again = s.resume("pane", "after the hop");

        assert_eq!(again.prev_tab_id.as_deref(), Some("t0"));
        assert_eq!(again.prev_agent_id.as_deref(), Some("p0"));
        assert_eq!(again.prev_workspace_id.as_deref(), Some("w0"));
        assert_eq!(again.origin_workspace_id, "w9");
        assert_eq!(again.origin_pane_id, "p9");
        assert_eq!(again.socket, SOCKET);
        assert_eq!(again.feedback, "after the hop");
        assert!(s.api.calls().is_empty());
    }

    #[test]
    fn status_round_trips_through_display() {
        for raw in ["blocked", "done", "working", "idle", "thinking"] {
            let Ok(status) = raw.parse::<AgentStatus>();
            assert_eq!(status.to_string(), raw);
        }
    }

    #[test]
    fn only_blocked_and_done_want_attention() {
        let wants = |raw: &str| {
            let Ok(status) = raw.parse::<AgentStatus>();
            status.wants_attention()
        };
        assert!(wants("blocked"));
        assert!(wants("done"));
        assert!(!wants("working"));
        assert!(!wants("idle"));
        assert!(!wants("unknown"));
    }

    #[test]
    fn missing_status_reads_as_unknown() {
        assert_eq!(status_of(&json!({})).to_string(), "unknown");
        assert_eq!(
            status_of(&json!({ "agent_status": "done" })),
            AgentStatus::Done
        );
    }
}
