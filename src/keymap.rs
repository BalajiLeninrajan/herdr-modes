//! Actions, key specs, and the resolved keymap a mode runs on.
//!
//! Everything here is data. The keymap itself comes from the user's config;
//! the only bindings this file contributes are the exits, so a mode is always
//! escapable before it is configured.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
/// Where `break_pane_*` sends the pane.
pub enum BreakTo {
    New,
    Prev,
    Next,
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
    BreakPane(BreakTo),

    Swap(Dir),
    SwapCycle(bool),

    PrevAgent,
    NextAgent,
    LastAgent,
    GotoAgent(usize),
    /// Step to the next agent that wants you: `blocked` or `done`.
    NextAttention,
    PrevAttention,

    PrevSpace,
    NextSpace,
    LastSpace,
    GotoSpace(usize),
    /// Step to the next space whose agents want you: `blocked` or `done`.
    NextSpaceAttention,
    PrevSpaceAttention,

    /// Leave, putting focus back where the mode opened. Every action here
    /// moves focus for real, so a browse needs a way to be taken back.
    Cancel,
    Quit,
}

impl Action {
    /// Parse a config action name. `key` is needed because the `goto_*` actions
    /// take their index from the key they are bound to.
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
            "break_pane_new" => Action::BreakPane(BreakTo::New),
            "break_pane_prev" => Action::BreakPane(BreakTo::Prev),
            "break_pane_next" => Action::BreakPane(BreakTo::Next),
            "goto_tab" => Action::GotoTab(digit(name, key)?),

            "swap_left" => Action::Swap(Dir::Left),
            "swap_right" => Action::Swap(Dir::Right),
            "swap_up" => Action::Swap(Dir::Up),
            "swap_down" => Action::Swap(Dir::Down),
            "swap_forward" => Action::SwapCycle(true),
            "swap_backward" => Action::SwapCycle(false),

            "prev_agent" => Action::PrevAgent,
            "next_agent" => Action::NextAgent,
            "last_agent" => Action::LastAgent,
            "goto_agent" => Action::GotoAgent(digit(name, key)?),
            "next_attention" => Action::NextAttention,
            "prev_attention" => Action::PrevAttention,

            "prev_space" => Action::PrevSpace,
            "next_space" => Action::NextSpace,
            "last_space" => Action::LastSpace,
            "goto_space" => Action::GotoSpace(digit(name, key)?),
            "next_space_attention" => Action::NextSpaceAttention,
            "prev_space_attention" => Action::PrevSpaceAttention,

            "cancel" => Action::Cancel,
            "exit" => Action::Quit,
            other => return Err(format!("unknown action `{other}`")),
        };
        Ok(a)
    }

    /// Whether the action can land the view on a different tab. Since herdr
    /// 0.9.0 a popup is tied to the tab it opened on, so after one of these
    /// the popup has to hop (see `resume`). Pane-level moves within a tab
    /// never need it, which keeps `hjkl` drumming at one round trip.
    pub fn may_leave_tab(self) -> bool {
        matches!(
            self,
            Action::PrevTab
                | Action::NextTab
                | Action::LastTab
                | Action::GotoTab(_)
                | Action::NewTab
                | Action::BreakPane(_)
                | Action::PrevAgent
                | Action::NextAgent
                | Action::LastAgent
                | Action::GotoAgent(_)
                | Action::NextAttention
                | Action::PrevAttention
                | Action::PrevSpace
                | Action::NextSpace
                | Action::LastSpace
                | Action::GotoSpace(_)
                | Action::NextSpaceAttention
                | Action::PrevSpaceAttention
        )
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
            Action::PrevAgent | Action::NextAgent => "agent",
            Action::LastAgent => "last",
            Action::GotoAgent(_) => "goto",
            Action::NextAttention | Action::PrevAttention => "attention",
            Action::PrevSpace | Action::NextSpace => "space",
            Action::LastSpace => "last",
            Action::GotoSpace(_) => "goto",
            Action::NextSpaceAttention | Action::PrevSpaceAttention => "attention",
            Action::Cancel => "cancel",
            Action::Quit => "exit",
        }
    }
}

/// The `goto_*` actions take their index from the key they are bound to, so
/// `1 = "goto_tab"` reads the way the keymap looks.
fn digit(name: &str, key: &KeySpec) -> Result<usize, String> {
    let Key::Char(c @ '1'..='9') = key.key else {
        return Err(format!("{name} must be bound to a digit 1-9"));
    };
    Ok(c.to_digit(10).unwrap() as usize)
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
    pub label: String,
    pub hint: Option<String>,
    /// Kept in insertion order, so the generated hint reads the same way twice.
    /// A handful of entries per mode, so a scan beats a map.
    pub keys: Vec<(KeySpec, Binding)>,
}

impl Mode {
    pub fn lookup(&self, ev: &KeyEvent) -> Option<Binding> {
        let spec = KeySpec::from_event(ev)?;
        self.keys.iter().find(|(s, _)| *s == spec).map(|(_, b)| *b)
    }

    /// Bind a key, replacing whatever it was bound to.
    pub fn bind(&mut self, spec: KeySpec, binding: Binding) {
        match self.keys.iter_mut().find(|(s, _)| *s == spec) {
            Some((_, b)) => *b = binding,
            None => self.keys.push((spec, binding)),
        }
    }

    pub fn unbind(&mut self, spec: KeySpec) {
        self.keys.retain(|(s, _)| *s != spec);
    }

    /// Hint bar text: the configured string, or one generated from the
    /// bindings with keys sharing a label collapsed together. `hint = ""`
    /// hides the bar entirely, so this returns `None`.
    pub fn hint_text(&self) -> Option<String> {
        if let Some(h) = &self.hint {
            return (!h.is_empty()).then(|| h.clone());
        }
        let mut groups: Vec<(&'static str, String)> = Vec::new();
        for (spec, b) in &self.keys {
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
        Some(
            groups
                .into_iter()
                .map(|(label, keys)| format!("{keys} {label}"))
                .collect::<Vec<_>>()
                .join("  "),
        )
    }
}

/// The modes the binary ships with. They exist so an unconfigured popup still
/// opens (with only the shared exits bound); every action key is the user's to
/// choose. Config may name modes beyond these.
pub const MODE_NAMES: &[&str] = &["pane", "tab", "move", "agent", "space"];

/// The only built-in bindings: bound in every mode, matching zellij's
/// `shared_except` blocks, so a mode is always escapable before it is
/// configured. Users can rebind or unbind them per mode like any other key.
pub const SHARED_EXITS: &[(&str, &str)] = &[("esc", "exit"), ("enter", "exit"), ("ctrl+c", "exit")];
