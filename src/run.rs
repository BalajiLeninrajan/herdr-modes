//! One key through a mode: which binding it hit, what the action did, and
//! whether the popup stays, exits, or hops. The terminal is not touched here;
//! `main` renders what comes back and reads the next key.

use crate::client::Api;
use crate::keymap::{Action, KeySpec, Mode};
use crate::modes::Session;
use crate::resume::Resume;
use crossterm::event::KeyEvent;

/// What the popup does after a key.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Stay up and show this on the feedback row.
    Continue(String),
    /// Close.
    Exit,
    /// Close; a hop is armed and the mode's `open` action is waiting for
    /// this popup to be gone.
    Hop,
}

/// What the popup shows before the first key. A config problem wins over
/// whatever the popup this one replaced was showing, since it is the only
/// place the problem will actually be seen.
pub fn opening_feedback(warnings: &[String], resumed: Option<&Resume>) -> String {
    match warnings {
        [] => resumed.map(|r| r.feedback.clone()).unwrap_or_default(),
        [one] => format!("config: {one}"),
        [first, rest @ ..] => format!("config: {first} ({} more)", rest.len()),
    }
}

/// A session driven by keys from one mode's keymap.
pub struct Driver<'m, A: Api> {
    pub session: Session<A>,
    mode_name: &'m str,
    mode: &'m Mode,
    /// The tab this popup belongs to. herdr draws the popup and routes keys
    /// to it only while this tab is the one on screen, so leaving it means
    /// a hop.
    owner_tab_id: String,
}

impl<'m, A: Api> Driver<'m, A> {
    /// The session must already know the server's focus (see
    /// `Session::refresh`): the tab it reports now is the popup's owner.
    pub fn new(session: Session<A>, mode_name: &'m str, mode: &'m Mode) -> Self {
        Driver {
            owner_tab_id: session.tab_id.clone(),
            session,
            mode_name,
            mode,
        }
    }

    /// Run whatever `key` is bound to. `shown` is the feedback line on screen
    /// as the key arrives; an action that closes the popup mid-request has
    /// no chance to report, so that line is what the hop carries over.
    /// `prompt` is asked for a line when the action needs one; `None` from
    /// it cancels the action.
    pub fn step(
        &mut self,
        key: &KeyEvent,
        shown: &str,
        prompt: impl FnOnce(&str) -> Option<String>,
    ) -> Outcome {
        let Some(binding) = self.mode.lookup(key) else {
            return Outcome::Continue(match KeySpec::from_event(key) {
                Some(s) => format!("unbound: {s}"),
                None => "unbound".into(),
            });
        };
        if matches!(binding.action, Action::Quit) {
            return Outcome::Exit;
        }

        // Taking the owner tab away closes the popup before the request even
        // returns, so the hop has to be armed beforehand. If the action then
        // fails, the note is withdrawn and the armed `open` stands down.
        let armed = match self.session.will_close_owner_tab(binding.action) {
            Ok(false) => false,
            Ok(true) => match self.session.arm_hop(self.mode_name, shown) {
                Ok(()) => true,
                Err(e) => return Outcome::Continue(format!("hop failed: {e}")),
            },
            Err(e) => return Outcome::Continue(format!("error: {e}")),
        };

        let result = self.session.execute(binding.action, prompt);
        if armed {
            return match result {
                Ok(_) => Outcome::Hop,
                Err(e) => {
                    self.session.disarm_hop();
                    Outcome::Continue(format!("error: {e}"))
                }
            };
        }

        let feedback = match result {
            Ok(msg) => msg,
            Err(e) => format!("error: {e}"),
        };

        // `cancel` has already put focus back; staying open would only invite
        // another move from a place the user just said they were done with.
        if matches!(binding.action, Action::Cancel) || !binding.sticky {
            return Outcome::Exit;
        }

        // Landed on another tab: this popup is now invisible and deaf where
        // the user is looking. Hand over to a fresh one there.
        if binding.action.may_leave_tab()
            && self.session.refresh().is_ok()
            && self.session.tab_id != self.owner_tab_id
        {
            return match self.session.arm_hop(self.mode_name, &feedback) {
                Ok(()) => Outcome::Hop,
                Err(e) => Outcome::Continue(format!("{feedback} \u{b7} hop failed: {e}")),
            };
        }

        Outcome::Continue(feedback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::fake::FakeApi;
    use crate::keymap::{Binding, Dir, KeySpec};
    use crate::resume::{Store, TempDir, temp_store};
    use crossterm::event::{KeyCode, KeyModifiers};
    use serde_json::{Value, json};

    const SOCKET: &str = "/run/herdr/test.sock";
    const MODE: &str = "tab";

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    /// A mode with one binding per `(key, action, sticky)`.
    fn mode(bindings: &[(char, Action, bool)]) -> Mode {
        Mode {
            label: MODE.into(),
            hint: None,
            keys: bindings
                .iter()
                .map(|(c, action, sticky)| {
                    let spec = KeySpec::parse(&c.to_string()).unwrap();
                    let binding = Binding {
                        action: *action,
                        sticky: *sticky,
                    };
                    (spec, binding)
                })
                .collect(),
        }
    }

    /// A driver on pane `p1` of tab `t1` in space `w1`, plus a handle on its
    /// hop note. The temp dir lives as long as the guard, so keep it in
    /// scope next to the driver.
    fn driver<'m>(
        test: &str,
        api: FakeApi,
        mode: &'m Mode,
    ) -> (TempDir, Store, Driver<'m, FakeApi>) {
        let (dir, store) = temp_store(test, SOCKET);
        let session = Session::new(
            api,
            store.clone(),
            "herdr-modes".into(),
            "w1".into(),
            "t1".into(),
            "p1".into(),
        );
        (dir, store, Driver::new(session, MODE, mode))
    }

    fn tabs(ids: &[&str]) -> Value {
        let tabs: Vec<Value> = ids.iter().map(|id| json!({ "tab_id": id })).collect();
        json!({ "tabs": tabs })
    }

    fn focused_tab(id: &str) -> Value {
        json!({ "snapshot": { "focused_tab_id": id } })
    }

    fn no_prompt(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn a_sticky_tab_switch_that_lands_elsewhere_hops() {
        let api = FakeApi::new()
            .reply("tab.list", tabs(&["t1", "t2"]))
            .reply("session.snapshot", focused_tab("t2"))
            .reply("session.snapshot", focused_tab("t2"));
        let mode = mode(&[('n', Action::NextTab, true)]);
        let (_dir, store, mut d) = driver("hop_after_next_tab", api, &mode);

        let outcome = d.step(&key('n'), "", no_prompt);

        assert_eq!(outcome, Outcome::Hop);
        assert_eq!(
            d.session.api.methods(),
            [
                "tab.list",
                "tab.focus",
                "session.snapshot",
                "session.snapshot",
                "plugin.action.invoke",
            ]
        );
        let note = store.pending(MODE).expect("note left for open");
        assert_eq!(note.feedback, "tab 2/2");
        assert_eq!(note.prev_tab_id.as_deref(), Some("t1"));
    }

    #[test]
    fn a_sticky_tab_switch_that_stays_put_continues() {
        // The server reports the same tab after the switch, so as far as the
        // popup is concerned nothing moved.
        let api = FakeApi::new()
            .reply("tab.list", tabs(&["t1", "t2"]))
            .reply("session.snapshot", focused_tab("t1"))
            .reply("session.snapshot", focused_tab("t1"));
        let mode = mode(&[('n', Action::NextTab, true)]);
        let (_dir, store, mut d) = driver("stay_after_next_tab", api, &mode);

        let outcome = d.step(&key('n'), "", no_prompt);

        assert_eq!(outcome, Outcome::Continue("tab 2/2".into()));
        assert!(!d.session.api.methods().contains(&"plugin.action.invoke"));
        assert!(!store.still_pending());
    }

    #[test]
    fn a_non_sticky_action_exits_without_hopping() {
        let api = FakeApi::new()
            .reply("tab.list", tabs(&["t1", "t2"]))
            .reply("session.snapshot", focused_tab("t2"));
        let mode = mode(&[('n', Action::NextTab, false)]);
        let (_dir, store, mut d) = driver("exit_non_sticky", api, &mode);

        let outcome = d.step(&key('n'), "", no_prompt);

        assert_eq!(outcome, Outcome::Exit);
        assert_eq!(
            d.session.api.methods(),
            ["tab.list", "tab.focus", "session.snapshot"]
        );
        assert!(!store.still_pending());
    }

    #[test]
    fn a_non_sticky_pane_action_exits() {
        let mode = mode(&[('h', Action::Focus(Dir::Left), false)]);
        let (_dir, _store, mut d) = driver("exit_focus", FakeApi::new(), &mode);

        assert_eq!(d.step(&key('h'), "", no_prompt), Outcome::Exit);
        assert_eq!(d.session.api.methods(), ["pane.focus_direction"]);
    }

    #[test]
    fn cancel_exits_even_when_sticky() {
        let mode = mode(&[('c', Action::Cancel, true)]);
        let (_dir, _store, mut d) = driver("exit_cancel", FakeApi::new(), &mode);

        assert_eq!(d.step(&key('c'), "", no_prompt), Outcome::Exit);
    }

    #[test]
    fn exit_is_decided_without_touching_the_server() {
        let mode = mode(&[('q', Action::Quit, true)]);
        let (_dir, _store, mut d) = driver("exit_quit", FakeApi::new(), &mode);

        assert_eq!(d.step(&key('q'), "", no_prompt), Outcome::Exit);
        assert!(d.session.api.calls().is_empty());
    }

    #[test]
    fn close_tab_arms_the_hop_before_the_call() {
        let mode = mode(&[('x', Action::CloseTab, true)]);
        let (_dir, store, mut d) = driver("close_tab_pre_arms", FakeApi::new(), &mode);

        let outcome = d.step(&key('x'), "tab 1/2", no_prompt);

        assert_eq!(outcome, Outcome::Hop);
        assert_eq!(
            d.session.api.calls(),
            &[
                (
                    "plugin.action.invoke".to_string(),
                    json!({ "plugin_id": "herdr-modes", "action_id": MODE })
                ),
                ("tab.close".to_string(), json!({ "tab_id": "t1" })),
                ("session.snapshot".to_string(), json!({})),
            ]
        );
        // The action never gets to report, so the line on screen carries over.
        let note = store.pending(MODE).expect("note left for open");
        assert_eq!(note.feedback, "tab 1/2");
    }

    #[test]
    fn a_failed_close_tab_withdraws_the_note_and_stays() {
        let api = FakeApi::new().fail("tab.close", "not_found", "no such tab");
        let mode = mode(&[('x', Action::CloseTab, true)]);
        let (_dir, store, mut d) = driver("close_tab_fails", api, &mode);

        let outcome = d.step(&key('x'), "", no_prompt);

        assert_eq!(
            outcome,
            Outcome::Continue("error: not_found: no such tab".into())
        );
        assert_eq!(
            d.session.api.methods(),
            ["plugin.action.invoke", "tab.close"]
        );
        assert!(!store.still_pending());
    }

    #[test]
    fn an_unbound_key_names_itself() {
        let mode = mode(&[('n', Action::NextTab, true)]);
        let (_dir, _store, mut d) = driver("unbound", FakeApi::new(), &mode);

        assert_eq!(
            d.step(&key('z'), "", no_prompt),
            Outcome::Continue("unbound: z".into())
        );
        assert!(d.session.api.calls().is_empty());
    }

    #[test]
    fn opening_feedback_prefers_config_problems() {
        let resumed = Resume::new(
            MODE,
            SOCKET,
            "tab 2/3",
            None,
            None,
            None,
            "w1".into(),
            "p1".into(),
        );
        assert_eq!(opening_feedback(&[], None), "");
        assert_eq!(opening_feedback(&[], Some(&resumed)), "tab 2/3");
        assert_eq!(
            opening_feedback(&["bad key".into()], Some(&resumed)),
            "config: bad key"
        );
        assert_eq!(
            opening_feedback(&["bad key".into(), "bad action".into(), "x".into()], None),
            "config: bad key (2 more)"
        );
    }
}
