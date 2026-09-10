//! The hint bar. The popup gives us two interior rows at `height = 4` (which is
//! also herdr's minimum popup height): row 0 is the key legend, row 1 is
//! transient feedback. `hint = ""` drops the legend, leaving row 0 blank —
//! the popup keeps its height either way, since 4 is herdr's floor.
//!
//! Both rows are cut to the popup width before drawing. A row that wrapped
//! would push the other one off the two lines we have.

use crossterm::style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};
use crossterm::{cursor, execute, queue};
use std::io::{Stdout, Write};

const FEEDBACK_ROW: u16 = 1;

const ELLIPSIS: char = '\u{2026}';

/// `accent` colours the label; it comes from `[ui] accent` in the config.
pub fn render(
    out: &mut Stdout,
    accent: Color,
    label: &str,
    hint: Option<&str>,
    feedback: &str,
) -> std::io::Result<()> {
    let width = width();
    queue!(out, cursor::Hide)?;
    if let Some(hint) = hint {
        let label = fit(format!(" {label} "), width);
        // The hint gets whatever the label leaves over.
        let hint = fit(
            format!(" {hint}"),
            width.map(|w| w.saturating_sub(columns(&label))),
        );
        queue!(
            out,
            cursor::MoveTo(0, 0),
            Clear(ClearType::CurrentLine),
            SetForegroundColor(accent),
            SetAttribute(Attribute::Bold),
            Print(label),
            SetAttribute(Attribute::Reset),
            ResetColor,
            SetAttribute(Attribute::Dim),
            Print(hint),
            SetAttribute(Attribute::Reset),
        )?;
    }
    queue!(
        out,
        cursor::MoveTo(0, FEEDBACK_ROW),
        Clear(ClearType::CurrentLine),
        SetAttribute(Attribute::Dim),
        Print(fit(format!("  {feedback}"), width)),
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
        Print(fit(format!("  {label}{buf}"), width())),
        cursor::Show,
    )?;
    out.flush()
}

/// Columns the popup gives us. The popup interior is the process's terminal,
/// so this is its width. None when the size cannot be read, and then nothing
/// gets clipped.
fn width() -> Option<usize> {
    crossterm::terminal::size()
        .ok()
        .map(|(cols, _)| usize::from(cols))
        // Some environments report zero columns rather than failing; treat
        // that as unknown too, or both rows would be clipped to nothing.
        .filter(|cols| *cols > 0)
}

/// `text` unchanged when it fits, or cut to `width` columns ending in "…".
/// `None` means the width is unknown, so the text passes through.
fn fit(text: String, width: Option<usize>) -> String {
    match width {
        Some(w) => clip(&text, w),
        None => text,
    }
}

/// Cut `text` to at most `width` columns. Anything dropped is replaced by a
/// single "…", which counts toward the width.
fn clip(text: &str, width: usize) -> String {
    if columns(text) <= width {
        return text.to_string();
    }
    let Some(keep) = width.checked_sub(1) else {
        return String::new();
    };
    let mut out: String = text.chars().take(keep).collect();
    out.push(ELLIPSIS);
    out
}

/// Display width. Everything the bar draws is ASCII, the middle-dot separator
/// or the ellipsis, all one column each, so a char count is the column count.
/// Byte length is not: the separator alone is two bytes.
fn columns(text: &str) -> usize {
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_counts_display_cells_not_bytes() {
        let legend = "hjkl focus \u{b7} x close";
        assert_eq!(columns(legend), 20);
        assert!(legend.len() > 20);
        assert_eq!(columns("\u{2026}"), 1);
    }

    #[test]
    fn text_that_fits_is_untouched() {
        assert_eq!(clip("abc", 3), "abc");
        assert_eq!(clip("abc", 10), "abc");
        assert_eq!(clip("", 0), "");
    }

    #[test]
    fn long_text_ends_in_one_ellipsis_within_width() {
        let clipped = clip("hjkl focus \u{b7} x close", 12);
        assert_eq!(clipped, "hjkl focus \u{2026}");
        assert_eq!(columns(&clipped), 12);
    }

    #[test]
    fn cut_lands_on_a_char_boundary_around_the_separator() {
        // Cutting at column 12 splits the two-byte dot if bytes were used.
        assert_eq!(clip("hjkl focus \u{b7} x", 13), "hjkl focus \u{b7}\u{2026}");
    }

    #[test]
    fn zero_width_yields_nothing_and_one_yields_the_ellipsis() {
        assert_eq!(clip("abc", 0), "");
        assert_eq!(clip("abc", 1), "\u{2026}");
    }

    #[test]
    fn unknown_width_passes_text_through() {
        let long = "x".repeat(500);
        assert_eq!(fit(long.clone(), None), long);
        assert_eq!(fit(long.clone(), Some(4)), "xxx\u{2026}");
    }
}
