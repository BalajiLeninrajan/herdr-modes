//! The popup's terminal: raw mode for as long as the mode is up, the hint
//! bar, the keys the user presses, and the one-line rename prompt. Nothing
//! else in the crate touches stdin or stdout while a mode is open.

use crate::hint;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::style::Color;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use crossterm::{cursor, execute};
use std::io::{Stdout, stdout};

/// Restores cooked mode however the loop ends, a panic included.
struct RawGuard;

impl Drop for RawGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), cursor::Show);
    }
}

pub struct Popup {
    out: Stdout,
    accent: Color,
    label: String,
    /// The legend on row 0, or `None` when the config hid it.
    hint: Option<String>,
    _raw: RawGuard,
}

impl Popup {
    /// Take the terminal into raw mode. Cooked mode comes back when the
    /// `Popup` drops.
    pub fn open(accent: Color, label: &str, hint: Option<String>) -> std::io::Result<Popup> {
        enable_raw_mode()?;
        Ok(Popup {
            out: stdout(),
            accent,
            label: label.to_string(),
            hint,
            _raw: RawGuard,
        })
    }

    /// Draw the bar with `feedback` on its second row.
    pub fn render(&mut self, feedback: &str) -> std::io::Result<()> {
        hint::render(
            &mut self.out,
            self.accent,
            &self.label,
            self.hint.as_deref(),
            feedback,
        )
    }

    /// Block for the next key press. Errors once the terminal can no longer
    /// be read, which is the popup being torn down under us; the caller
    /// decides whether that is worth reporting.
    pub fn next_key(&mut self) -> std::io::Result<KeyEvent> {
        loop {
            let key = match event::read()? {
                Event::Key(key) => key,
                _ => continue,
            };
            // With the kitty keyboard protocol active, releases are reported
            // too; acting on them would fire every binding twice.
            if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                return Ok(key);
            }
        }
    }

    /// Read a line on the feedback row. `None` if cancelled with Esc, or if
    /// the terminal went away.
    pub fn prompt(&mut self, label: &str) -> Option<String> {
        let mut buf = String::new();
        loop {
            hint::draw_prompt(&mut self.out, label, &buf).ok()?;
            let key = self.next_key().ok()?;
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
}
