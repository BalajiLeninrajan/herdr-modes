//! The hint bar. The popup gives us two interior rows at `height = 4` (which is
//! also herdr's minimum popup height): row 0 is the key legend, row 1 is
//! transient feedback. `hint = ""` drops the legend, leaving row 0 blank —
//! the popup keeps its height either way, since 4 is herdr's floor.

use crossterm::style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};
use crossterm::{cursor, execute, queue};
use std::io::{Stdout, Write};

/// Catppuccin mocha mauve, matching `ui.accent` in the herdr config.
const ACCENT: Color = Color::Rgb { r: 0xcb, g: 0xa6, b: 0xf7 };

const FEEDBACK_ROW: u16 = 1;

pub fn render(
    out: &mut Stdout,
    label: &str,
    hint: Option<&str>,
    feedback: &str,
) -> std::io::Result<()> {
    queue!(out, cursor::Hide)?;
    if let Some(hint) = hint {
        queue!(
            out,
            cursor::MoveTo(0, 0),
            Clear(ClearType::CurrentLine),
            SetForegroundColor(ACCENT),
            SetAttribute(Attribute::Bold),
            Print(format!(" {label} ")),
            SetAttribute(Attribute::Reset),
            ResetColor,
            SetAttribute(Attribute::Dim),
            Print(format!(" {hint}")),
            SetAttribute(Attribute::Reset),
        )?;
    }
    queue!(
        out,
        cursor::MoveTo(0, FEEDBACK_ROW),
        Clear(ClearType::CurrentLine),
        SetAttribute(Attribute::Dim),
        Print(format!("  {feedback}")),
        SetAttribute(Attribute::Reset),
    )?;
    out.flush()
}

/// Draw a prompt on the feedback row and leave the cursor after it.
pub fn draw_prompt(out: &mut Stdout, label: &str, buf: &str) -> std::io::Result<()> {
    execute!(
        out,
        cursor::MoveTo(0, FEEDBACK_ROW),
        Clear(ClearType::CurrentLine),
        Print(format!("  {label}{buf}")),
        cursor::Show,
    )?;
    out.flush()
}
