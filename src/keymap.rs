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

#[derive(Clone, Copy, PartialEq, Eq)]
/// Where `break_pane_*` sends the pane.
pub enum BreakTo {
    New,
    Prev,
    Next,
}

#[derive(Clone, Copy, PartialEq, Eq)]
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

/// The mode an action is documented under. Modes are only namespaces for a
/// keymap, so this says where the action is listed, not where it may be bound.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Pane,
    Tab,
    Move,
    Agent,
    Space,
    Any,
}

impl Group {
    /// In the order the docs list them.
    pub const ALL: [Group; 6] = [
        Group::Pane,
        Group::Tab,
        Group::Move,
        Group::Agent,
        Group::Space,
        Group::Any,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Group::Pane => "pane",
            Group::Tab => "tab",
            Group::Move => "move",
            Group::Agent => "agent",
            Group::Space => "space",
            Group::Any => "any",
        }
    }
}

/// How a name becomes an `Action`. Most names are one fixed variant; the
/// `goto_*` names take their index from the digit key they are bound to.
#[derive(Clone, Copy)]
pub enum Build {
    Plain(Action),
    Digit(fn(usize) -> Action),
}

/// One row of the action table: everything the config parser, the hint bar,
/// the hop check and the docs need to know about a name.
pub struct ActionSpec {
    pub name: &'static str,
    pub group: Group,
    /// Short label for the auto-generated hint bar. Directional variants
    /// share a label so their keys collapse into one group ("hjkl focus").
    pub label: &'static str,
    /// Whether the action can land the view on a different tab. Since herdr
    /// 0.9.0 a popup is tied to the tab it opened on, so after one of these
    /// the popup has to hop (see `resume`). Pane-level moves within a tab
    /// never need it, which keeps `hjkl` drumming at one round trip.
    pub leaves_tab: bool,
    pub build: Build,
}

impl ActionSpec {
    /// Build the action for a binding. `key` matters only for the `goto_*`
    /// names, which read their index from it.
    pub fn action(&self, key: &KeySpec) -> Result<Action, String> {
        match self.build {
            Build::Plain(a) => Ok(a),
            Build::Digit(f) => Ok(f(digit(self.name, key)?)),
        }
    }

    /// Whether the name must be bound to a digit key.
    pub fn takes_digit(&self) -> bool {
        matches!(self.build, Build::Digit(_))
    }

    /// Whether `action` came from this row. Plain rows compare the whole
    /// value, so `focus_left` and `focus_right` stay apart; digit rows compare
    /// the variant, since the digit is the only payload.
    fn matches(&self, action: Action) -> bool {
        match self.build {
            Build::Plain(a) => a == action,
            Build::Digit(f) => std::mem::discriminant(&f(1)) == std::mem::discriminant(&action),
        }
    }
}

const fn act(
    name: &'static str,
    group: Group,
    label: &'static str,
    leaves_tab: bool,
    action: Action,
) -> ActionSpec {
    ActionSpec {
        name,
        group,
        label,
        leaves_tab,
        build: Build::Plain(action),
    }
}

const fn goto(
    name: &'static str,
    group: Group,
    label: &'static str,
    leaves_tab: bool,
    action: fn(usize) -> Action,
) -> ActionSpec {
    ActionSpec {
        name,
        group,
        label,
        leaves_tab,
        build: Build::Digit(action),
    }
}

/// Every action a config may name. `Action::parse`, the hint bar, the hop
/// check and the tests against README.md and config.example.toml all read
/// this table, so a new action is one row here plus a mention in those two
/// files.
#[rustfmt::skip]
pub const ACTIONS: &[ActionSpec] = &[
    act("focus_left", Group::Pane, "focus", false, Action::Focus(Dir::Left)),
    act("focus_right", Group::Pane, "focus", false, Action::Focus(Dir::Right)),
    act("focus_up", Group::Pane, "focus", false, Action::Focus(Dir::Up)),
    act("focus_down", Group::Pane, "focus", false, Action::Focus(Dir::Down)),
    act("cycle_focus", Group::Pane, "cycle", false, Action::CycleFocus),
    act("close_pane", Group::Pane, "close", false, Action::ClosePane),
    act("split_right", Group::Pane, "split", false, Action::Split("right")),
    act("split_down", Group::Pane, "split", false, Action::Split("down")),
    act("zoom", Group::Pane, "zoom", false, Action::Zoom),
    act("rename_pane", Group::Pane, "rename", false, Action::RenamePane),

    act("prev_tab", Group::Tab, "switch", true, Action::PrevTab),
    act("next_tab", Group::Tab, "switch", true, Action::NextTab),
    act("last_tab", Group::Tab, "last", true, Action::LastTab),
    goto("goto_tab", Group::Tab, "goto", true, Action::GotoTab),
    act("new_tab", Group::Tab, "new", true, Action::NewTab),
    act("close_tab", Group::Tab, "close", false, Action::CloseTab),
    act("rename_tab", Group::Tab, "rename", false, Action::RenameTab),
    act("move_tab_left", Group::Tab, "move", false, Action::MoveTab(-1)),
    act("move_tab_right", Group::Tab, "move", false, Action::MoveTab(1)),
    act("break_pane_new", Group::Tab, "break", true, Action::BreakPane(BreakTo::New)),
    act("break_pane_prev", Group::Tab, "break", true, Action::BreakPane(BreakTo::Prev)),
    act("break_pane_next", Group::Tab, "break", true, Action::BreakPane(BreakTo::Next)),

    act("swap_left", Group::Move, "swap", false, Action::Swap(Dir::Left)),
    act("swap_right", Group::Move, "swap", false, Action::Swap(Dir::Right)),
    act("swap_up", Group::Move, "swap", false, Action::Swap(Dir::Up)),
    act("swap_down", Group::Move, "swap", false, Action::Swap(Dir::Down)),
    act("swap_forward", Group::Move, "swap", false, Action::SwapCycle(true)),
    act("swap_backward", Group::Move, "swap", false, Action::SwapCycle(false)),

    act("prev_agent", Group::Agent, "agent", true, Action::PrevAgent),
    act("next_agent", Group::Agent, "agent", true, Action::NextAgent),
    act("last_agent", Group::Agent, "last", true, Action::LastAgent),
    goto("goto_agent", Group::Agent, "goto", true, Action::GotoAgent),
    act("next_attention", Group::Agent, "attention", true, Action::NextAttention),
    act("prev_attention", Group::Agent, "attention", true, Action::PrevAttention),

    act("prev_space", Group::Space, "space", true, Action::PrevSpace),
    act("next_space", Group::Space, "space", true, Action::NextSpace),
    act("last_space", Group::Space, "last", true, Action::LastSpace),
    goto("goto_space", Group::Space, "goto", true, Action::GotoSpace),
    act("next_space_attention", Group::Space, "attention", true, Action::NextSpaceAttention),
    act("prev_space_attention", Group::Space, "attention", true, Action::PrevSpaceAttention),

    act("exit", Group::Any, "exit", false, Action::Quit),
    act("cancel", Group::Any, "cancel", false, Action::Cancel),
];

impl Action {
    /// Parse a config action name. `key` is needed because the `goto_*` actions
    /// take their index from the key they are bound to.
    pub fn parse(name: &str, key: &KeySpec) -> Result<Action, String> {
        ACTIONS
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| format!("unknown action `{name}`"))?
            .action(key)
    }

    /// The table row this action came from. A scan over a few dozen rows,
    /// once per keystroke.
    pub fn spec(self) -> &'static ActionSpec {
        ACTIONS
            .iter()
            .find(|s| s.matches(self))
            .expect("every Action variant has a row in ACTIONS")
    }

    /// See `ActionSpec::leaves_tab`.
    pub fn may_leave_tab(self) -> bool {
        self.spec().leaves_tab
    }

    /// See `ActionSpec::label`.
    pub fn hint_label(self) -> &'static str {
        self.spec().label
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn key(c: char) -> KeySpec {
        KeySpec {
            ctrl: false,
            key: Key::Char(c),
        }
    }

    fn doc(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {name}: {e}"))
    }

    fn table_names() -> BTreeMap<&'static str, BTreeSet<&'static str>> {
        let mut by_group = BTreeMap::new();
        for spec in ACTIONS {
            by_group
                .entry(spec.group.as_str())
                .or_insert_with(BTreeSet::new)
                .insert(spec.name);
        }
        by_group
    }

    fn is_name(token: &str) -> bool {
        !token.is_empty() && token.chars().all(|c| c.is_ascii_lowercase() || c == '_')
    }

    #[test]
    fn names_are_unique() {
        let names: BTreeSet<_> = ACTIONS.iter().map(|s| s.name).collect();
        assert_eq!(names.len(), ACTIONS.len());
    }

    #[test]
    fn every_name_parses_and_finds_its_own_row() {
        for spec in ACTIONS {
            let k = if spec.takes_digit() {
                key('3')
            } else {
                key('h')
            };
            let action = Action::parse(spec.name, &k)
                .unwrap_or_else(|e| panic!("{} did not parse: {e}", spec.name));
            assert_eq!(action.spec().name, spec.name);
            assert_eq!(action.hint_label(), spec.label);
            assert_eq!(action.may_leave_tab(), spec.leaves_tab);
        }
    }

    #[test]
    fn goto_reads_the_digit_and_refuses_other_keys() {
        assert!(Action::parse("goto_tab", &key('7')).unwrap() == Action::GotoTab(7));
        assert!(Action::parse("goto_agent", &key('1')).unwrap() == Action::GotoAgent(1));
        assert!(Action::parse("goto_space", &key('9')).unwrap() == Action::GotoSpace(9));
        assert!(Action::parse("goto_tab", &key('0')).is_err());
        assert!(Action::parse("goto_tab", &key('h')).is_err());
        assert!(Action::parse("goto_nowhere", &key('1')).is_err());
    }

    #[test]
    fn the_hop_set_is_unchanged() {
        let leaving: BTreeSet<_> = ACTIONS
            .iter()
            .filter(|s| s.leaves_tab)
            .map(|s| s.name)
            .collect();
        let expected: BTreeSet<_> = [
            "prev_tab",
            "next_tab",
            "last_tab",
            "goto_tab",
            "new_tab",
            "break_pane_new",
            "break_pane_prev",
            "break_pane_next",
            "prev_agent",
            "next_agent",
            "last_agent",
            "goto_agent",
            "next_attention",
            "prev_attention",
            "prev_space",
            "next_space",
            "last_space",
            "goto_space",
            "next_space_attention",
            "prev_space_attention",
        ]
        .into_iter()
        .collect();
        assert_eq!(leaving, expected);
    }

    /// The Keys table in README.md: one row per group, names in backticks.
    /// Backticked digits in "(bind to `1`-`9`)" are not names and are skipped.
    #[test]
    fn readme_keys_table_lists_the_actions() {
        let readme = doc("README.md");
        let mut seen: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for line in readme.lines() {
            let Some(row) = line.strip_prefix("| ") else {
                continue;
            };
            let Some((group, rest)) = row.split_once(" | ") else {
                continue;
            };
            if !Group::ALL.iter().any(|g| g.as_str() == group) {
                continue;
            }
            let names = rest
                .split('`')
                .skip(1)
                .step_by(2)
                .filter(|t| is_name(t))
                .map(str::to_string)
                .collect();
            seen.insert(group.to_string(), names);
        }
        let expected: BTreeMap<String, BTreeSet<String>> = table_names()
            .into_iter()
            .map(|(g, names)| {
                (
                    g.to_string(),
                    names.into_iter().map(str::to_string).collect(),
                )
            })
            .collect();
        assert_eq!(
            seen, expected,
            "README.md Keys table is out of sync with ACTIONS"
        );
    }

    /// The trailing "actions" comment block in config.example.toml:
    /// `# group: name name ...` with continuation lines, up to a bare `#`.
    #[test]
    fn example_config_comment_lists_the_actions() {
        let example = doc("config.example.toml");
        let block = example
            .lines()
            .skip_while(|l| !(l.starts_with("# ") && l.contains(" actions ")))
            .skip(1)
            .take_while(|l| l.trim() != "#");
        let mut seen: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut group = String::new();
        for line in block {
            let text = line.trim_start_matches('#');
            // "(bind to 1-9)" is a hint about the key, not a name.
            let text = match (text.find('('), text.find(')')) {
                (Some(a), Some(b)) if a < b => format!("{}{}", &text[..a], &text[b + 1..]),
                _ => text.to_string(),
            };
            for token in text.split_whitespace() {
                if let Some(g) = token.strip_suffix(':') {
                    group = g.to_string();
                } else if is_name(token) {
                    seen.entry(group.clone())
                        .or_default()
                        .insert(token.to_string());
                } else {
                    panic!("unexpected token `{token}` in the actions block");
                }
            }
        }
        let expected: BTreeMap<String, BTreeSet<String>> = table_names()
            .into_iter()
            .map(|(g, names)| {
                (
                    g.to_string(),
                    names.into_iter().map(str::to_string).collect(),
                )
            })
            .collect();
        assert_eq!(
            seen, expected,
            "config.example.toml actions block is out of sync with ACTIONS"
        );
    }
}
