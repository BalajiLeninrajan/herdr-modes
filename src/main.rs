//! herdr-modes — zellij-style sticky modes for herdr.
//!
//! `open <mode>` runs as a plugin action and opens the modal popup.
//! `run <mode>` runs inside that popup and owns the key loop.
//! `check [path]` validates the config and prints the resolved keymaps.
//! `check --actions` prints the action table.
//!
//! Actions run detached without a TTY, so the action -> pane hop is required;
//! it costs one round trip on mode entry only.

mod client;
mod config;
mod hint;
mod keymap;
mod modes;
mod nav;
mod popup;
mod resume;
mod run;

use client::Client;
use keymap::{ACTIONS, Group};
use modes::Session;
use popup::Popup;
use resume::{Resume, Store};
use run::Outcome;
use serde_json::{Value, json};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

const POPUP_WIDTH: &str = "90%";
/// herdr's minimum popup height; less the border, exactly two interior rows.
const POPUP_HEIGHT: u64 = 4;
/// How long a hop waits for the old popup to be gone before giving up.
const HOP_WAIT: Duration = Duration::from_secs(3);
const HOP_POLL: Duration = Duration::from_millis(20);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let sub = args.get(1).map(String::as_str);

    match sub {
        Some("check") => {
            return match args.get(2).map(String::as_str) {
                Some("--actions") => list_actions(),
                path => check(path.map(Path::new)),
            };
        }
        Some("open") | Some("run") => {}
        _ => {
            eprintln!(
                "usage: herdr-modes <open|run> <mode> | herdr-modes check [path | --actions]"
            );
            return ExitCode::from(2);
        }
    }

    let Some(mode_name) = args.get(2) else {
        eprintln!("usage: herdr-modes <open|run> <mode>");
        return ExitCode::from(2);
    };

    let result = match sub {
        Some("open") => open(mode_name),
        _ => run(mode_name),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("herdr-modes: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Validate config and print the resolved keymaps, so a typo is findable
/// without opening a popup and pressing keys. With a path, that file is
/// checked instead of the one in the plugin config dir.
fn check(path: Option<&Path>) -> ExitCode {
    let config::Loaded {
        modes, warnings, ..
    } = match path {
        Some(p) => {
            println!("config: {}", p.display());
            config::load_from(p)
        }
        None => {
            match config::config_path() {
                Some(p) if p.exists() => println!("config: {}", p.display()),
                Some(p) => println!(
                    "config: {} (not present, only the exits are bound)",
                    p.display()
                ),
                None => println!("config: <unresolved>"),
            }
            config::load()
        }
    };
    println!();

    let mut names: Vec<&String> = modes.keys().collect();
    names.sort();
    for name in names {
        let mode = &modes[name];
        println!(
            "[{}]  label={}  {} bindings",
            name,
            mode.label,
            mode.keys.len()
        );
        match mode.hint_text() {
            Some(h) => println!("  {h}"),
            None => println!("  (hint bar hidden)"),
        }
    }

    if warnings.is_empty() {
        println!("\nok");
        ExitCode::SUCCESS
    } else {
        println!("\n{} problem(s):", warnings.len());
        for w in &warnings {
            println!("  - {w}");
        }
        ExitCode::FAILURE
    }
}

/// Print the action table grouped by mode: each name with its hint label and
/// whether it can leave the tab, which is what makes a sticky binding hop.
fn list_actions() -> ExitCode {
    for group in Group::ALL {
        println!("[{}]", group.as_str());
        for spec in ACTIONS.iter().filter(|s| s.group == group) {
            let name = if spec.takes_digit() {
                format!("{} (bind to 1-9)", spec.name)
            } else {
                spec.name.to_string()
            };
            let tab = if spec.leaves_tab {
                "may leave the tab, so a sticky binding hops"
            } else {
                "stays on the tab"
            };
            println!("  {name:<28} {:<10} {tab}", spec.label);
        }
    }
    ExitCode::SUCCESS
}

fn plugin_id() -> String {
    std::env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| "herdr-modes".to_string())
}

fn open(entrypoint: &str) -> Result<(), client::Error> {
    let plugin_id = plugin_id();
    let mut c = Client::connect()?;
    let store = Store::from_env();

    // A note from a popup that just hopped: carry its state into the new one.
    let resume = store.pending(entrypoint);
    let mut params = json!({
        "plugin_id": plugin_id,
        "entrypoint": entrypoint,
        "placement": "popup",
        "focus": true,
        "width": POPUP_WIDTH,
        "height": POPUP_HEIGHT,
    });
    if let Some(r) = &resume {
        params["env"] = json!({ resume::ENV: r.to_env() });
    }

    let deadline = Instant::now() + HOP_WAIT;
    loop {
        match c.call("plugin.pane.open", params.clone()) {
            // `clear` leaves a fresh note of another session's alone, so a
            // plain keypress here cannot cancel a hop in flight elsewhere.
            Ok(_) => {
                store.clear();
                return Ok(());
            }
            // Only one popup may be open at a time. On a plain keypress that
            // means the user is already in a mode: a no-op, not an error. On
            // a hop it means the old popup has not finished dying yet.
            Err(e) if e.is_popup_already_open() => {
                if resume.is_none() {
                    return Ok(());
                }
                // The popup calls a hop off by removing the note.
                if !store.still_pending() {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    store.clear();
                    return Err(e);
                }
                std::thread::sleep(HOP_POLL);
            }
            Err(e) => {
                store.clear();
                return Err(e);
            }
        }
    }
}

fn run(mode_name: &str) -> Result<(), client::Error> {
    let config::Loaded {
        modes,
        ui,
        warnings,
    } = config::load();
    let Some(mode) = modes.get(mode_name) else {
        return Err(client::Error::Protocol(format!(
            "no mode named `{mode_name}`"
        )));
    };

    let mut session = session_from_env()?;
    // The context is a snapshot from when the open was requested, and it may
    // not name a tab at all. The popup is tied to whatever tab the server has
    // focused now, so read that back before deciding what counts as leaving.
    session.refresh()?;

    let resumed = Resume::from_env();
    if let Some(r) = &resumed {
        session.restore(r);
        // The popup is tied to the server's active tab, but the client may
        // still be looking elsewhere (herdr 0.9.0 only moves the client for
        // explicit focus calls, and a tab that just closed under it lands
        // wherever the client's fallback says). `tab.focus` is explicit, so
        // this puts the client on the popup's tab before the first key.
        session.sync_view()?;
    }
    let mut feedback = run::opening_feedback(&warnings, resumed.as_ref());

    let mut driver = run::Driver::new(session, mode_name, mode);
    let mut popup = Popup::open(ui.accent, &mode.label, mode.hint_text())?;
    loop {
        popup.render(&feedback)?;
        let key = popup.next_key()?;
        match driver.step(&key, &feedback, |label| popup.prompt(label)) {
            Outcome::Continue(line) => feedback = line,
            Outcome::Exit | Outcome::Hop => break,
        }
    }
    Ok(())
}

/// A session on the herdr socket, starting from the focus the plugin
/// context reports.
fn session_from_env() -> Result<Session<Client>, client::Error> {
    let ctx: Value = std::env::var("HERDR_PLUGIN_CONTEXT_JSON")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    let id = |field: &str| ctx[field].as_str().unwrap_or_default().to_string();
    Ok(Session::new(
        Client::connect()?,
        Store::from_env(),
        plugin_id(),
        id("workspace_id"),
        id("tab_id"),
        id("focused_pane_id"),
    ))
}
