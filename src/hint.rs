//! The hint bar. The popup gives us two interior rows at `height = 4`:
//! row 0 is the persistent key legend, row 1 is transient feedback.

use crate::keymap::Mode;
use crossterm::style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};
use crossterm::{cursor, execute};
use std::io::{Stdout, Write};

/// Catppuccin mocha mauve, matching `ui.accent` in the herdr config.
const ACCENT: Color = Color::Rgb { r: 0xcb, g: 0xa6, b: 0xf7 };

pub fn render(out: &mut Stdout, mode: Mode, feedback: &str) -> std::io::Result<()> {
    execute!(
        out,
        cursor::Hide,
        cursor::MoveTo(0, 0),
        Clear(ClearType::CurrentLine),
        SetForegroundColor(ACCENT),
        SetAttribute(Attribute::Bold),
        Print(format!(" {} ", mode.label())),
        SetAttribute(Attribute::Reset),
        ResetColor,
        SetAttribute(Attribute::Dim),
        Print(format!(" {}", mode.hint())),
        SetAttribute(Attribute::Reset),
        cursor::MoveTo(0, 1),
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
        cursor::MoveTo(0, 1),
        Clear(ClearType::CurrentLine),
        Print(format!("  {label}{buf}")),
        cursor::Show,
    )?;
    out.flush()
}
