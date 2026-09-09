//! herdr-modes — zellij-style sticky modes for herdr.
//!
//! `open <mode>` runs as a plugin action and opens the modal popup.
//! `run <mode>` runs inside that popup and owns the key loop.
//! `check` validates the config and prints the resolved keymaps.
//!
//! Actions run detached without a TTY, so the action -> pane hop is required;
//! it costs one round trip on mode entry only.

mod client;
mod config;
mod hint;
mod keymap;
mod modes;
mod resume;

use client::Client;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use crossterm::{cursor, execute};
use keymap::Action;
use resume::Resume;
use serde_json::{Value, json};
use std::io::{Stdout, stdout};
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
        Some("check") => return check(),
        Some("open") | Some("run") => {}
        _ => {
            eprintln!("usage: herdr-modes <open|run> <mode> | herdr-modes check");
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
/// without opening a popup and pressing keys.
fn check() -> ExitCode {
    let (modes, warnings) = config::load();
    match config::config_path() {
        Some(p) if p.exists() => println!("config: {}", p.display()),
        Some(p) => println!("config: {} (not present, only the exits are bound)", p.display()),
        None => println!("config: <unresolved>"),
    }
    println!();

    let mut names: Vec<&String> = modes.keys().collect();
    names.sort();
    for name in names {
        let mode = &modes[name];
        println!("[{}]  label={}  {} bindings", name, mode.label, mode.keys.len());
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

fn open(entrypoint: &str) -> Result<(), client::Error> {
    let plugin_id = std::env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| "herdr-modes".to_string());
    let mut c = Client::connect()?;

    // A note from a popup that just hopped: carry its state into the new one.
    let resume = Resume::pending(entrypoint);
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
            Ok(_) => {
                Resume::clear();
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
                if !Resume::still_pending() {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    Resume::clear();
                    return Err(e);
                }
                std::thread::sleep(HOP_POLL);
            }
            Err(e) => {
                Resume::clear();
                return Err(e);
            }
        }
    }
}

/// Restores cooked mode even if the loop exits early.
struct RawGuard;

impl Drop for RawGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), cursor::Show);
    }
}

fn run(mode_name: &str) -> Result<(), client::Error> {
    let (modes, warnings) = config::load();
    let Some(mode) = modes.get(mode_name) else {
        return Err(client::Error::Protocol(format!("no mode named `{mode_name}`")));
    };

    let ctx: Value = std::env::var("HERDR_PLUGIN_CONTEXT_JSON")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);

    let client = Client::connect()?;
    let mut session = modes::Session::new(
        client,
        ctx["workspace_id"].as_str().unwrap_or_default().to_string(),
        ctx["tab_id"].as_str().unwrap_or_default().to_string(),
        ctx["focused_pane_id"].as_str().unwrap_or_default().to_string(),
    );

    // Surface config problems where they will actually be seen, then let the
    // mode carry on with whatever did parse.
    let mut feedback = match warnings.len() {
        0 => String::new(),
        1 => format!("config: {}", warnings[0]),
        n => format!("config: {} ({} more)", warnings[0], n - 1),
    };

    if let Some(r) = Resume::from_env() {
        session.restore(&r);
        // The context was taken when the open was requested; focus has had
        // time to settle since, and it is what this popup is now tied to.
        session.refresh()?;
        // The popup is tied to the server's active tab, but the client may
        // still be looking elsewhere (herdr 0.9.0 only moves the client for
        // explicit focus calls, and a tab that just closed under it lands
        // wherever the client's fallback says). `tab.focus` is explicit, so
        // this puts the client on the popup's tab before the first key.
        session.sync_view()?;
        if feedback.is_empty() {
            feedback = r.feedback;
        }
    }
    // The tab this popup belongs to. herdr draws the popup and routes keys to
    // it only while this tab is the one on screen, so leaving it means a hop.
    let owner_tab_id = session.tab_id.clone();

    enable_raw_mode()?;
    let _guard = RawGuard;
    let mut out = stdout();
    let hint_text = mode.hint_text();

    loop {
        hint::render(&mut out, &mode.label, hint_text.as_deref(), &feedback)?;

        let Event::Key(key) = event::read()? else {
            continue;
        };
        // With the kitty keyboard protocol active, releases are reported too;
        // acting on them would fire every binding twice.
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            continue;
        }

        let Some(binding) = mode.lookup(&key) else {
            feedback = match keymap::KeySpec::from_event(&key) {
                Some(s) => format!("unbound: {s}"),
                None => "unbound".into(),
            };
            continue;
        };
        if matches!(binding.action, Action::Quit) {
            break;
        }

        // Taking the owner tab away closes the popup before the request even
        // returns, so the hop has to be armed beforehand. If the action then
        // fails, the note is withdrawn and the armed `open` stands down.
        let armed = match session.will_close_owner_tab(binding.action) {
            Ok(false) => false,
            Ok(true) => match session.arm_hop(mode_name, &feedback) {
                Ok(()) => true,
                Err(e) => {
                    feedback = format!("hop failed: {e}");
                    continue;
                }
            },
            Err(e) => {
                feedback = format!("error: {e}");
                continue;
            }
        };

        let result = session.execute(binding.action, |label| prompt(&mut out, label));
        if armed {
            match result {
                Ok(_) => break,
                Err(e) => {
                    Resume::clear();
                    feedback = format!("error: {e}");
                    continue;
                }
            }
        }

        feedback = match result {
            Ok(msg) => msg,
            Err(e) => format!("error: {e}"),
        };

        // `cancel` has already put focus back; staying open would only invite
        // another move from a place the user just said they were done with.
        if matches!(binding.action, Action::Cancel) || !binding.sticky {
            break;
        }

        // Landed on another tab: this popup is now invisible and deaf where
        // the user is looking. Hand over to a fresh one there.
        if binding.action.may_leave_tab()
            && session.refresh().is_ok()
            && session.tab_id != owner_tab_id
        {
            match session.arm_hop(mode_name, &feedback) {
                Ok(()) => break,
                Err(e) => feedback = format!("{feedback} \u{b7} hop failed: {e}"),
            }
        }
    }

    Ok(())
}

/// Read a line on the feedback row. Returns None if cancelled.
fn prompt(out: &mut Stdout, label: &str) -> Option<String> {
    let mut buf = String::new();
    loop {
        hint::draw_prompt(out, label, &buf).ok()?;
        let Ok(Event::Key(key)) = event::read() else {
            return None;
        };
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            continue;
        }
        match key.code {
            KeyCode::Enter => return Some(buf),
            KeyCode::Esc => return None,
            KeyCode::Backspace => {
                buf.pop();
            }
            KeyCode::Char(c) => buf.push(c),
            _ => {}
        }
    }
}
