//! Executes a keymap Action against the herdr socket API.

use crate::client::{Client, Error};
use crate::keymap::{Action, BreakTarget, Dir};
use serde_json::{Value, json};

/// One row of herdr's agent panel, in the order `agent.list` reports them:
/// grouped by space, which is the panel's own `agent_panel_sort = "spaces"`
/// ordering.
struct Agent {
    pane_id: String,
    status: String,
    label: String,
}

impl Agent {
    /// `blocked` is an approval or question waiting on you; `done` is finished
    /// background work you have not seen yet. Those two are the panel's
    /// attention queue — the rest are either busy or already read.
    fn wants_attention(&self) -> bool {
        self.status == "blocked" || self.status == "done"
    }
}

pub struct Session {
    pub client: Client,
    pub workspace_id: String,
    pub tab_id: String,
    pub pane_id: String,
    /// For zellij's `tab` binding (ToggleTab).
    prev_tab_id: Option<String>,
    /// The agent pane focus came from, for `last_agent`.
    prev_agent_id: Option<String>,
}

impl Session {
    pub fn new(client: Client, workspace_id: String, tab_id: String, pane_id: String) -> Self {
        Session {
            client,
            workspace_id,
            tab_id,
            pane_id,
            prev_tab_id: None,
            prev_agent_id: None,
        }
    }

    /// Re-read focus from the server. Popups have no pane id of their own, so
    /// the snapshot always reports the real underlying pane even while a mode
    /// is on screen.
    fn refresh(&mut self) -> Result<(), Error> {
        let r = self.client.call("session.snapshot", json!({}))?;
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
        let r = self.client.call("tab.list", json!({ "workspace_id": self.workspace_id }))?;
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
        let r = self.client.call("pane.list", json!({ "workspace_id": self.workspace_id }))?;
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
        self.client.call("tab.focus", json!({ "tab_id": tab_id }))?;
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
        let cur = tabs.iter().position(|t| *t == self.tab_id).unwrap_or(0) as i64;
        let n = tabs.len() as i64;
        let next = ((cur + delta) % n + n) % n;
        let target = tabs[next as usize].clone();
        self.focus_tab(&target)?;
        Ok(format!("tab {}/{}", next + 1, n))
    }

    pub fn execute(&mut self, action: Action, prompt: impl FnOnce(&str) -> Option<String>) -> Result<String, Error> {
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

            Action::Quit => Ok(String::new()),
        }
    }

    fn focus(&mut self, dir: Dir) -> Result<String, Error> {
        let r = self.client.call(
            "pane.focus_direction",
            json!({ "direction": dir.as_str(), "pane_id": self.pane_id }),
        )?;
        let f = &r["focus"];
        if let Some(id) = f["focused_pane_id"].as_str() {
            self.pane_id = id.to_string();
        }
        if f["changed"].as_bool() == Some(false) {
            // `no_neighbor` at an edge — report it rather than silently doing nothing.
            return Ok(format!("{} — {}", dir.as_str(), f["reason"].as_str().unwrap_or("no change")));
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
        self.client.call("pane.focus", json!({ "pane_id": next }))?;
        self.pane_id = next;
        Ok("cycle".into())
    }

    fn close_pane(&mut self) -> Result<String, Error> {
        self.client.call("pane.close", json!({ "pane_id": self.pane_id }))?;
        self.refresh()?;
        Ok("closed pane".into())
    }

    fn split(&mut self, dir: &str) -> Result<String, Error> {
        let r = self.client.call(
            "pane.split",
            json!({ "direction": dir, "target_pane_id": self.pane_id, "focus": true }),
        )?;
        if let Some(id) = r["pane"]["pane_id"].as_str() {
            self.pane_id = id.to_string();
        }
        Ok(format!("split {dir}"))
    }

    fn zoom(&mut self) -> Result<String, Error> {
        self.client
            .call("pane.zoom", json!({ "pane_id": self.pane_id, "mode": "toggle" }))?;
        Ok("zoom".into())
    }

    fn rename_pane(&mut self, prompt: impl FnOnce(&str) -> Option<String>) -> Result<String, Error> {
        let Some(label) = prompt("rename pane: ") else {
            return Ok("cancelled".into());
        };
        let label: Value = if label.is_empty() { Value::Null } else { Value::String(label) };
        self.client
            .call("pane.rename", json!({ "pane_id": self.pane_id, "label": label }))?;
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
        self.client
            .call("tab.create", json!({ "workspace_id": self.workspace_id, "focus": true }))?;
        self.refresh()?;
        Ok("new tab".into())
    }

    fn close_tab(&mut self) -> Result<String, Error> {
        self.client.call("tab.close", json!({ "tab_id": self.tab_id }))?;
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
        self.client
            .call("tab.rename", json!({ "tab_id": self.tab_id, "label": label }))?;
        Ok("renamed tab".into())
    }

    /// `tab.move` has no CLI subcommand; it is reachable only over the socket.
    ///
    /// `insert_index` is evaluated against the tab list *including* this tab,
    /// so moving right has to clear its own slot: cur + 2, not cur + 1.
    /// Moving left needs no such adjustment.
    fn move_tab(&mut self, delta: i64) -> Result<String, Error> {
        let tabs = self.tabs()?;
        let n = tabs.len() as i64;
        if n < 2 {
            return Ok("only one tab".into());
        }
        let cur = tabs.iter().position(|t| *t == self.tab_id).unwrap_or(0) as i64;
        let (insert, landed) = if delta > 0 { (cur + 2, cur + 1) } else { (cur - 1, cur - 1) };
        if insert < 0 || insert > n {
            return Ok("at end".into());
        }
        self.client.call(
            "tab.move",
            json!({ "tab_id": self.tab_id, "insert_index": insert as u64 }),
        )?;
        Ok(format!("moved tab -> {}", landed + 1))
    }

    fn break_pane(&mut self, target: BreakTarget) -> Result<String, Error> {
        let destination = match target {
            BreakTarget::NewTab => json!({ "type": "new_tab" }),
            BreakTarget::PrevTab | BreakTarget::NextTab => {
                let tabs = self.tabs()?;
                if tabs.len() < 2 {
                    return Ok("no other tab".into());
                }
                let cur = tabs.iter().position(|t| *t == self.tab_id).unwrap_or(0) as i64;
                let n = tabs.len() as i64;
                let delta = if matches!(target, BreakTarget::PrevTab) { -1 } else { 1 };
                let idx = ((cur + delta) % n + n) % n;
                json!({ "type": "tab", "tab_id": tabs[idx as usize], "split": "right" })
            }
        };
        self.client.call(
            "pane.move",
            json!({ "pane_id": self.pane_id, "destination": destination, "focus": true }),
        )?;
        self.refresh()?;
        Ok("broke pane out".into())
    }

    fn swap(&mut self, dir: Dir) -> Result<String, Error> {
        self.client.call(
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
        let cur = panes.iter().position(|p| *p == self.pane_id).unwrap_or(0) as i64;
        let n = panes.len() as i64;
        let delta = if forward { 1 } else { -1 };
        let idx = ((cur + delta) % n + n) % n;
        self.client.call(
            "pane.swap",
            json!({ "source_pane_id": self.pane_id, "target_pane_id": panes[idx as usize] }),
        )?;
        Ok(if forward { "swap forward".into() } else { "swap back".into() })
    }

    fn agents(&mut self) -> Result<Vec<Agent>, Error> {
        let r = self.client.call("agent.list", json!({}))?;
        Ok(r["agents"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|a| {
                        Some(Agent {
                            pane_id: a["pane_id"].as_str()?.to_string(),
                            status: a["agent_status"].as_str().unwrap_or("unknown").to_string(),
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
            return Ok(format!("{}/{n} {} \u{b7} already here", index + 1, target.label));
        }
        let line = format!("{}/{n} {} \u{b7} {}", index + 1, target.label, target.status);
        self.client
            .call("agent.focus", json!({ "target": &target.pane_id }))?;
        if self.agent_at_focus(agents).is_some() {
            self.prev_agent_id = Some(self.pane_id.clone());
        }
        self.refresh()?;
        Ok(line)
    }

    fn step_agent(&mut self, delta: i64) -> Result<String, Error> {
        let agents = self.agents()?;
        if agents.is_empty() {
            return Ok("no agents".into());
        }
        let n = agents.len() as i64;
        // From a pane with no agent in it, stepping enters the panel at
        // whichever end the direction implies rather than doing nothing.
        let next = match self.agent_at_focus(&agents) {
            Some(cur) => ((cur as i64 + delta) % n + n) % n,
            None if delta > 0 => 0,
            None => n - 1,
        };
        self.focus_agent(&agents, next as usize)
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
        let n = agents.len() as i64;
        // Starting off the panel puts the search just outside the near end, so
        // the first step lands on that end row instead of skipping it.
        let off_panel = if delta > 0 { -1 } else { n };
        let from = self.agent_at_focus(&agents).map_or(off_panel, |i| i as i64);
        let hit = (1..=n)
            .map(|step| (((from + delta * step) % n + n) % n) as usize)
            .find(|i| agents[*i].wants_attention());
        match hit {
            Some(i) => self.focus_agent(&agents, i),
            None => Ok("nothing waiting".into()),
        }
    }
}
