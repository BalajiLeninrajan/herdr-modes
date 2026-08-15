//! Actions, key specs, and the built-in default keymaps.
//!
//! Everything here is data. The defaults below are one particular
//! transcription (from a zellij config); `config.rs` layers user overrides on
//! top, so nothing in this file is privileged beyond being the fallback.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl Dir {
    pub fn as_str(self) -> &'static str {
        match self {
            Dir::Left => "left",
            Dir::Right => "right",
            Dir::Up => "up",
            Dir::Down => "down",
        }
    }
}

#[derive(Clone, Copy)]
pub enum BreakTarget {
    NewTab,
    PrevTab,
    NextTab,
}

#[derive(Clone, Copy)]
pub enum Action {
    Focus(Dir),
    CycleFocus,
    ClosePane,
    Split(&'static str),
    Zoom,
    RenamePane,

    PrevTab,
    NextTab,
    LastTab,
    GotoTab(usize),
    NewTab,
    CloseTab,
    RenameTab,
    MoveTab(i64),
    BreakPane(BreakTarget),

    Swap(Dir),
    SwapCycle(bool),

    Quit,
}

impl Action {
    /// Parse a config action name. `key` is needed because `goto_tab` takes its
    /// tab number from the key it is bound to.
    pub fn parse(name: &str, key: &KeySpec) -> Result<Action, String> {
        let a = match name {
            "focus_left" => Action::Focus(Dir::Left),
            "focus_right" => Action::Focus(Dir::Right),
            "focus_up" => Action::Focus(Dir::Up),
            "focus_down" => Action::Focus(Dir::Down),
            "cycle_focus" => Action::CycleFocus,
            "close_pane" => Action::ClosePane,
            "split_right" => Action::Split("right"),
            "split_down" => Action::Split("down"),
            "zoom" => Action::Zoom,
            "rename_pane" => Action::RenamePane,

            "prev_tab" => Action::PrevTab,
            "next_tab" => Action::NextTab,
            "last_tab" => Action::LastTab,
            "new_tab" => Action::NewTab,
            "close_tab" => Action::CloseTab,
            "rename_tab" => Action::RenameTab,
            "move_tab_left" => Action::MoveTab(-1),
            "move_tab_right" => Action::MoveTab(1),
            "break_pane_new" => Action::BreakPane(BreakTarget::NewTab),
            "break_pane_prev" => Action::BreakPane(BreakTarget::PrevTab),
            "break_pane_next" => Action::BreakPane(BreakTarget::NextTab),
            "goto_tab" => {
                let Key::Char(c @ '1'..='9') = key.key else {
                    return Err("goto_tab must be bound to a digit 1-9".into());
                };
                Action::GotoTab(c.to_digit(10).unwrap() as usize)
            }

            "swap_left" => Action::Swap(Dir::Left),
            "swap_right" => Action::Swap(Dir::Right),
            "swap_up" => Action::Swap(Dir::Up),
            "swap_down" => Action::Swap(Dir::Down),
            "swap_forward" => Action::SwapCycle(true),
            "swap_backward" => Action::SwapCycle(false),

            "exit" => Action::Quit,
            other => return Err(format!("unknown action `{other}`")),
        };
        Ok(a)
    }

    /// Whether the mode stays open after this action, unless config overrides it.
    pub fn default_sticky(self) -> bool {
        match self {
            Action::Focus(_)
            | Action::CycleFocus
            | Action::ClosePane
            | Action::PrevTab
            | Action::NextTab
            | Action::LastTab
            | Action::CloseTab
            | Action::MoveTab(_)
            | Action::Swap(_)
            | Action::SwapCycle(_) => true,

            Action::Split(_)
            | Action::Zoom
            | Action::RenamePane
            | Action::GotoTab(_)
            | Action::NewTab
            | Action::RenameTab
            | Action::BreakPane(_)
            | Action::Quit => false,
        }
    }

    /// Short label for the auto-generated hint bar. Directional variants share
    /// a label so their keys collapse into one group ("hjkl focus").
    pub fn hint_label(self) -> &'static str {
        match self {
            Action::Focus(_) => "focus",
            Action::CycleFocus => "cycle",
            Action::ClosePane => "close",
            Action::Split(_) => "split",
            Action::Zoom => "zoom",
            Action::RenamePane => "rename",
            Action::PrevTab | Action::NextTab => "switch",
            Action::LastTab => "last",
            Action::GotoTab(_) => "goto",
            Action::NewTab => "new",
            Action::CloseTab => "close",
            Action::RenameTab => "rename",
            Action::MoveTab(_) => "move",
            Action::BreakPane(_) => "break",
            Action::Swap(_) | Action::SwapCycle(_) => "swap",
            Action::Quit => "exit",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Tab,
    Enter,
    Esc,
    Backspace,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeySpec {
    pub ctrl: bool,
    pub key: Key,
}

impl KeySpec {
    /// Parse a config key string: `h`, `H`, `1`, `tab`, `esc`, `enter`, `ctrl+c`.
    pub fn parse(s: &str) -> Result<KeySpec, String> {
        let (ctrl, rest) = match s.strip_prefix("ctrl+") {
            Some(r) => (true, r),
            None => (false, s),
        };
        let key = match rest {
            "tab" => Key::Tab,
            "enter" | "return" => Key::Enter,
            "esc" | "escape" => Key::Esc,
            "backspace" => Key::Backspace,
            "space" => Key::Char(' '),
            other => {
                let mut chars = other.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Key::Char(c),
                    _ => return Err(format!("unrecognised key `{s}`")),
                }
            }
        };
        Ok(KeySpec { ctrl, key })
    }

    /// Build a spec from a live key event so it can be looked up in the table.
    pub fn from_event(ev: &KeyEvent) -> Option<KeySpec> {
        let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
        let key = match ev.code {
            // Ctrl chords arrive uppercase on some terminals; normalise so
            // `ctrl+c` in config matches regardless.
            KeyCode::Char(c) if ctrl => Key::Char(c.to_ascii_lowercase()),
            KeyCode::Char(c) => Key::Char(c),
            KeyCode::Tab | KeyCode::BackTab => Key::Tab,
            KeyCode::Enter => Key::Enter,
            KeyCode::Esc => Key::Esc,
            KeyCode::Backspace => Key::Backspace,
            _ => return None,
        };
        Some(KeySpec { ctrl, key })
    }
}

impl fmt::Display for KeySpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            write!(f, "ctrl+")?;
        }
        match self.key {
            Key::Char(' ') => write!(f, "space"),
            Key::Char(c) => write!(f, "{c}"),
            Key::Tab => write!(f, "tab"),
            Key::Enter => write!(f, "enter"),
            Key::Esc => write!(f, "esc"),
            Key::Backspace => write!(f, "backspace"),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Binding {
    pub action: Action,
    pub sticky: bool,
}

pub struct Mode {
    pub name: String,
    pub label: String,
    pub hint: Option<String>,
    pub keys: HashMap<KeySpec, Binding>,
    /// Insertion order, so the generated hint reads in a stable, sensible order.
    pub order: Vec<KeySpec>,
}

impl Mode {
    pub fn lookup(&self, ev: &KeyEvent) -> Option<Binding> {
        self.keys.get(&KeySpec::from_event(ev)?).copied()
    }

    /// Hint bar text: the configured string, or one generated from the
    /// bindings with keys sharing a label collapsed together.
    pub fn hint_text(&self) -> String {
        if let Some(h) = &self.hint {
            return h.clone();
        }
        let mut groups: Vec<(&'static str, String)> = Vec::new();
        for spec in &self.order {
            let Some(b) = self.keys.get(spec) else { continue };
            let label = b.action.hint_label();
            match groups.iter_mut().find(|(l, _)| *l == label) {
                Some((_, keys)) => {
                    let k = spec.to_string();
                    // Single alphanumeric keys run together (hjkl); punctuation
                    // and multi-character names stay separated.
                    if k.chars().count() == 1
                        && k.chars().all(|c| c.is_alphanumeric())
                        && keys.chars().all(|c| c.is_alphanumeric())
                    {
                        keys.push_str(&k);
                    } else {
                        keys.push('/');
                        keys.push_str(&k);
                    }
                }
                None => groups.push((label, spec.to_string())),
            }
        }
        groups
            .into_iter()
            .map(|(label, keys)| format!("{keys} {label}"))
            .collect::<Vec<_>>()
            .join("  ")
    }
}

/// (mode, key, action) triples. Stickiness comes from `Action::default_sticky`
/// unless a user overrides it.
///
/// Transcribed from a zellij config, including its mixed stickiness: movement
/// keys stay in the mode, creation keys fall back to normal.
pub const DEFAULTS: &[(&str, &str, &str)] = &[
    // pane
    ("pane", "h", "focus_left"),
    ("pane", "j", "focus_down"),
    ("pane", "k", "focus_up"),
    ("pane", "l", "focus_right"),
    ("pane", "p", "cycle_focus"),
    ("pane", "x", "close_pane"),
    ("pane", "d", "split_down"),
    ("pane", "r", "split_right"),
    ("pane", "n", "split_right"),
    // `f` is zellij's fullscreen; `z` was pane frames, which herdr has no
    // runtime equivalent for, so it is free to alias zoom.
    ("pane", "f", "zoom"),
    ("pane", "z", "zoom"),
    ("pane", "c", "rename_pane"),
    // tab — zellij binds both axes: h/k back, j/l forward.
    ("tab", "h", "prev_tab"),
    ("tab", "k", "prev_tab"),
    ("tab", "j", "next_tab"),
    ("tab", "l", "next_tab"),
    ("tab", "tab", "last_tab"),
    ("tab", "H", "move_tab_left"),
    ("tab", "L", "move_tab_right"),
    ("tab", "x", "close_tab"),
    ("tab", "n", "new_tab"),
    ("tab", "r", "rename_tab"),
    ("tab", "b", "break_pane_new"),
    ("tab", "[", "break_pane_prev"),
    ("tab", "]", "break_pane_next"),
    ("tab", "1", "goto_tab"),
    ("tab", "2", "goto_tab"),
    ("tab", "3", "goto_tab"),
    ("tab", "4", "goto_tab"),
    ("tab", "5", "goto_tab"),
    ("tab", "6", "goto_tab"),
    ("tab", "7", "goto_tab"),
    ("tab", "8", "goto_tab"),
    ("tab", "9", "goto_tab"),
    // move
    ("move", "h", "swap_left"),
    ("move", "j", "swap_down"),
    ("move", "k", "swap_up"),
    ("move", "l", "swap_right"),
    ("move", "n", "swap_forward"),
    ("move", "tab", "swap_forward"),
    ("move", "p", "swap_backward"),
];

/// Bound in every mode, matching zellij's `shared_except` blocks. Users can
/// rebind these per mode like any other key.
pub const SHARED_EXITS: &[(&str, &str)] = &[("esc", "exit"), ("enter", "exit"), ("ctrl+c", "exit")];
