//! Executes a keymap Action against the herdr socket API.

use crate::client::{Client, Error};
use crate::keymap::{Action, BreakTo, Dir};
use crate::resume::Resume;
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

/// One row of herdr's space sidebar, in `workspace.list` order — the same
/// order the sidebar draws and `number` counts.
struct Space {
    id: String,
    label: String,
    /// The space's rolled-up agent status.
    status: String,
    focused: bool,
}

impl Space {
    /// A space wants you when its agents do: `blocked` is waiting on an answer,
    /// `done` is finished work you have not looked at.
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
    /// The space focus came from, for `last_space`.
    prev_workspace_id: Option<String>,
    /// Where the mode opened, for `cancel`.
    origin_workspace_id: String,
    origin_pane_id: String,
}

impl Session {
    pub fn new(client: Client, workspace_id: String, tab_id: String, pane_id: String) -> Self {
        Session {
            client,
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
        Resume {
            mode: mode.to_string(),
            written_unix_ms: 0,
            prev_tab_id: self.prev_tab_id.clone(),
            prev_agent_id: self.prev_agent_id.clone(),
            prev_workspace_id: self.prev_workspace_id.clone(),
            origin_workspace_id: self.origin_workspace_id.clone(),
            origin_pane_id: self.origin_pane_id.clone(),
            feedback: feedback.to_string(),
        }
    }

    /// Arrange for a fresh popup on whatever tab is on screen once this one
    /// is gone: leave the note, then have herdr run the mode's `open` action.
    /// The caller exits afterwards; the action waits for that.
    pub fn arm_hop(&mut self, mode: &str, feedback: &str) -> Result<(), Error> {
        self.resume(mode, feedback).stamp().write()?;
        let plugin_id =
            std::env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| "herdr-modes".to_string());
        let r = self.client.call(
            "plugin.action.invoke",
            json!({ "plugin_id": plugin_id, "action_id": mode }),
        );
        if r.is_err() {
            Resume::clear();
        }
        r.map(|_| ())
    }

    /// Put the viewing client on the tab the server (and so this popup) is on.
    pub fn sync_view(&mut self) -> Result<(), Error> {
        if self.tab_id.is_empty() {
            return Ok(());
        }
        let tab = self.tab_id.clone();
        self.client.call("tab.focus", json!({ "tab_id": tab }))?;
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
        let r = self
            .client
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
            .client
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
            return Ok(format!(
                "{} — {}",
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
        self.client.call("pane.focus", json!({ "pane_id": next }))?;
        self.pane_id = next;
        Ok("cycle".into())
    }

    fn close_pane(&mut self) -> Result<String, Error> {
        self.client
            .call("pane.close", json!({ "pane_id": self.pane_id }))?;
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
        self.client.call(
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
        self.client.call(
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
        self.client.call(
            "tab.create",
            json!({ "workspace_id": self.workspace_id, "focus": true }),
        )?;
        self.refresh()?;
        Ok("new tab".into())
    }

    fn close_tab(&mut self) -> Result<String, Error> {
        self.client
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
        self.client.call(
            "tab.rename",
            json!({ "tab_id": self.tab_id, "label": label }),
        )?;
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
        let (insert, landed) = if delta > 0 {
            (cur + 2, cur + 1)
        } else {
            (cur - 1, cur - 1)
        };
        if insert < 0 || insert > n {
            return Ok("at end".into());
        }
        self.client.call(
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
                let cur = tabs.iter().position(|t| *t == self.tab_id).unwrap_or(0) as i64;
                let n = tabs.len() as i64;
                let delta = if matches!(target, BreakTo::Prev) {
                    -1
                } else {
                    1
                };
                let idx = ((cur + delta) % n + n) % n;
                json!({ "type": "tab", "tab_id": tabs[idx as usize], "split": "right" })
            }
        };
        self.client.call(
            "pane.move",
            json!({ "pane_id": self.pane_id, "destination": destination, "focus": true }),
        )?;
        // Same as `agent.focus`: the move's `focus` reaches the server only,
        // so the client has to be brought along explicitly.
        let pane = self.pane_id.clone();
        self.client.call("pane.focus", json!({ "pane_id": pane }))?;
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
        Ok(if forward {
            "swap forward".into()
        } else {
            "swap back".into()
        })
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
        self.client
            .call("agent.focus", json!({ "target": &target.pane_id }))?;
        if self.agent_at_focus(agents).is_some() {
            self.prev_agent_id = Some(self.pane_id.clone());
        }
        // `agent.focus` moves the server's focus but, on herdr 0.9.0, not the
        // client's view. `pane.focus` is one of the calls the client follows.
        self.client
            .call("pane.focus", json!({ "pane_id": &target.pane_id }))?;
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

    fn spaces(&mut self) -> Result<Vec<Space>, Error> {
        let r = self.client.call("workspace.list", json!({}))?;
        Ok(r["workspaces"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|w| {
                        Some(Space {
                            id: w["workspace_id"].as_str()?.to_string(),
                            label: w["label"].as_str().unwrap_or("space").to_string(),
                            status: w["agent_status"].as_str().unwrap_or("unknown").to_string(),
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
        self.client
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
        let n = spaces.len() as i64;
        let cur = self.space_at_focus(&spaces).unwrap_or(0) as i64;
        let next = ((cur + delta) % n + n) % n;
        self.focus_space(&spaces, next as usize)
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
        let n = spaces.len() as i64;
        // Starting off the sidebar puts the search just outside the near end,
        // so the first step lands on that end row instead of skipping it.
        let off_sidebar = if delta > 0 { -1 } else { n };
        let from = self
            .space_at_focus(&spaces)
            .map_or(off_sidebar, |i| i as i64);
        let hit = (1..=n)
            .map(|step| (((from + delta * step) % n + n) % n) as usize)
            .find(|i| spaces[*i].wants_attention());
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
        self.client
            .call("workspace.focus", json!({ "workspace_id": workspace }))?;
        // The pane may be gone — closed from inside the mode — and then the
        // space it lived in is as close to where you were as there is.
        let _ = self.client.call("pane.focus", json!({ "pane_id": pane }));
        self.refresh()?;
        Ok(String::new())
    }
}
