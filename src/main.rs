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

use client::Client;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use crossterm::{cursor, execute};
use keymap::Action;
use serde_json::{Value, json};
use std::io::{Stdout, stdout};
use std::process::ExitCode;

const POPUP_WIDTH: &str = "90%";
/// herdr's minimum popup height; less the border, exactly two interior rows.
const POPUP_HEIGHT: u64 = 4;

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
        Some(p) => println!("config: {} (not present, using defaults)", p.display()),
        None => println!("config: <unresolved>"),
    }
    println!();

    let mut names: Vec<&String> = modes.keys().collect();
    names.sort();
    for name in names {
        let mode = &modes[name];
        println!("[{}]  label={}  {} bindings", mode.name, mode.label, mode.keys.len());
        println!("  {}", mode.hint_text());
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
    let res = c.call(
        "plugin.pane.open",
        json!({
            "plugin_id": plugin_id,
            "entrypoint": entrypoint,
            "placement": "popup",
            "focus": true,
            "width": POPUP_WIDTH,
            "height": POPUP_HEIGHT,
        }),
    );
    match res {
        // Only one popup may be open at a time. If one already is, the user is
        // already in a mode; treat it as a no-op rather than an error.
        Err(e) if e.is_popup_already_open() => Ok(()),
        Err(e) => Err(e),
        Ok(_) => Ok(()),
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

    enable_raw_mode()?;
    let _guard = RawGuard;
    let mut out = stdout();
    let hint_text = mode.hint_text();

    // Surface config problems where they will actually be seen, then let the
    // mode carry on with whatever did parse.
    let mut feedback = match warnings.len() {
        0 => String::new(),
        1 => format!("config: {}", warnings[0]),
        n => format!("config: {} ({} more)", warnings[0], n - 1),
    };

    loop {
        hint::render(&mut out, &mode.label, &hint_text, &feedback)?;

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

        feedback = match session.execute(binding.action, |label| prompt(&mut out, label)) {
            Ok(msg) => msg,
            Err(e) => format!("error: {e}"),
        };

        if !binding.sticky {
            break;
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
