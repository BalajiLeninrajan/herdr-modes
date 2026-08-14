//! Key tables, transcribed from ~/.config/zellij/config.kdl.
//!
//! Stickiness is per-key, not per-mode, because the zellij config is mixed:
//! movement keys stay in the mode while creation keys fall back to normal.
//! `sticky: false` means the action runs and the mode exits.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Pane,
    Tab,
    Move,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "pane" => Some(Mode::Pane),
            "tab" => Some(Mode::Tab),
            "move" => Some(Mode::Move),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Pane => "PANE",
            Mode::Tab => "TAB",
            Mode::Move => "MOVE",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Mode::Pane => {
                "hjkl focus  p cycle  x close  d/r split  n new  f/z zoom  c rename  esc exit"
            }
            Mode::Tab => {
                "hjkl/1-9 switch  tab last  H/L move  n new  x close  r rename  b/[/] break  esc exit"
            }
            Mode::Move => "hjkl swap  n/tab forward  p backward  esc exit",
        }
    }
}

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
    /// SplitDirection is only right|down, matching zellij's `d` and `r`.
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

pub struct Binding {
    pub action: Action,
    pub sticky: bool,
}

const fn stay(action: Action) -> Option<Binding> {
    Some(Binding { action, sticky: true })
}

const fn exit(action: Action) -> Option<Binding> {
    Some(Binding { action, sticky: false })
}

pub fn lookup(mode: Mode, ev: &KeyEvent) -> Option<Binding> {
    // Shared exits, matching zellij's `shared_except` blocks. Checked before
    // anything else so ctrl+c never collides with pane mode's `c`.
    if ev.modifiers.contains(KeyModifiers::CONTROL) {
        return match ev.code {
            KeyCode::Char('c') => exit(Action::Quit),
            _ => None,
        };
    }
    if matches!(ev.code, KeyCode::Esc | KeyCode::Enter) {
        return exit(Action::Quit);
    }

    match mode {
        Mode::Pane => pane(ev),
        Mode::Tab => tab(ev),
        Mode::Move => move_(ev),
    }
}

fn pane(ev: &KeyEvent) -> Option<Binding> {
    match ev.code {
        KeyCode::Char('h') => stay(Action::Focus(Dir::Left)),
        KeyCode::Char('j') => stay(Action::Focus(Dir::Down)),
        KeyCode::Char('k') => stay(Action::Focus(Dir::Up)),
        KeyCode::Char('l') => stay(Action::Focus(Dir::Right)),
        KeyCode::Char('p') => stay(Action::CycleFocus),
        KeyCode::Char('x') => stay(Action::ClosePane),
        KeyCode::Char('d') => exit(Action::Split("down")),
        KeyCode::Char('r') => exit(Action::Split("right")),
        KeyCode::Char('n') => exit(Action::Split("right")),
        // `f` is zellij's ToggleFocusFullscreen. `z` was TogglePaneFrames,
        // which herdr has no runtime equivalent for, so it is a free alias.
        KeyCode::Char('f') | KeyCode::Char('z') => exit(Action::Zoom),
        KeyCode::Char('c') => exit(Action::RenamePane),
        _ => None,
    }
}

fn tab(ev: &KeyEvent) -> Option<Binding> {
    match ev.code {
        // zellij binds both axes: h/k go back, j/l go forward.
        KeyCode::Char('h') | KeyCode::Char('k') => stay(Action::PrevTab),
        KeyCode::Char('j') | KeyCode::Char('l') => stay(Action::NextTab),
        KeyCode::Tab => stay(Action::LastTab),
        KeyCode::Char('H') => stay(Action::MoveTab(-1)),
        KeyCode::Char('L') => stay(Action::MoveTab(1)),
        KeyCode::Char('x') => stay(Action::CloseTab),
        KeyCode::Char('n') => exit(Action::NewTab),
        KeyCode::Char('r') => exit(Action::RenameTab),
        KeyCode::Char('b') => exit(Action::BreakPane(BreakTarget::NewTab)),
        KeyCode::Char('[') => exit(Action::BreakPane(BreakTarget::PrevTab)),
        KeyCode::Char(']') => exit(Action::BreakPane(BreakTarget::NextTab)),
        KeyCode::Char(c @ '1'..='9') => {
            exit(Action::GotoTab(c.to_digit(10).unwrap() as usize))
        }
        _ => None,
    }
}

fn move_(ev: &KeyEvent) -> Option<Binding> {
    match ev.code {
        KeyCode::Char('h') => stay(Action::Swap(Dir::Left)),
        KeyCode::Char('j') => stay(Action::Swap(Dir::Down)),
        KeyCode::Char('k') => stay(Action::Swap(Dir::Up)),
        KeyCode::Char('l') => stay(Action::Swap(Dir::Right)),
        KeyCode::Char('n') | KeyCode::Tab => stay(Action::SwapCycle(true)),
        KeyCode::Char('p') => stay(Action::SwapCycle(false)),
        _ => None,
    }
}
