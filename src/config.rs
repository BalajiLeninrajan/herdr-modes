//! The user's keymap. The binary contributes only the exits, so everything
//! else in a mode comes from here.
//!
//! Lives at `$HERDR_PLUGIN_CONFIG_DIR/config.toml`, which herdr creates per
//! plugin (`~/.config/herdr/plugins/config/herdr-modes/`).
//!
//! ```toml
//! [modes.pane]
//! label = "PANE"          # optional; defaults to the mode name uppercased
//! hint  = "custom text"   # optional; generated from the bindings otherwise
//!                         # `hint = ""` hides the hint bar
//!
//! [modes.pane.keys]
//! w = "focus_up"                                  # add or override
//! k = ""                                          # unbind
//! x = { action = "close_pane", sticky = false }   # override stickiness
//! ```
//!
//! Bindings merge over the exits, matching herdr's own config style where
//! `previous_tab = ""` unbinds. Set `defaults = false` on a mode to drop the
//! exits too and start from an empty table.
//!
//! An optional `[ui]` table colours the hint bar:
//!
//! ```toml
//! [ui]
//! accent = "#cba6f7"      # hex, or a crossterm colour name like "blue"
//! ```

use crate::keymap::{Action, Binding, KeySpec, MODE_NAMES, Mode, SHARED_EXITS};
use crossterm::style::Color;
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

#[derive(Deserialize, Default)]
struct File {
    #[serde(default)]
    modes: HashMap<String, ModeConfig>,
    #[serde(default)]
    ui: UiConfig,
}

#[derive(Deserialize, Default)]
struct UiConfig {
    accent: Option<String>,
}

/// Everything a config file yields: the keymaps, the hint bar look, and any
/// non-fatal problems found on the way.
pub struct Loaded {
    pub modes: HashMap<String, Mode>,
    pub ui: Ui,
    pub warnings: Vec<String>,
}

/// Hint bar appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ui {
    /// Colour of the mode label. Defaults to the terminal's magenta so the
    /// popup follows whatever palette the user already runs.
    pub accent: Color,
}

impl Default for Ui {
    fn default() -> Self {
        Ui {
            accent: Color::Magenta,
        }
    }
}

#[derive(Deserialize, Default)]
struct ModeConfig {
    label: Option<String>,
    hint: Option<String>,
    /// Start from the built-in bindings, which are just the exits. Default true.
    #[serde(default = "yes")]
    defaults: bool,
    #[serde(default)]
    keys: HashMap<String, BindingConfig>,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BindingConfig {
    /// `h = "focus_left"`, or `h = ""` to unbind.
    Action(String),
    /// `h = { action = "focus_left", sticky = true }`
    Full {
        action: String,
        sticky: Option<bool>,
    },
}

pub fn config_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("HERDR_PLUGIN_CONFIG_DIR") {
        return Some(PathBuf::from(dir).join("config.toml"));
    }
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".config/herdr/plugins/config/herdr-modes/config.toml"))
}

/// Build every mode from the plugin config dir: defaults, then user
/// overrides. A missing file is not an error; only the exits are bound then.
pub fn load() -> Loaded {
    match config_path().filter(|p| p.exists()) {
        Some(p) => load_from(&p),
        None => build(File::default(), Vec::new()),
    }
}

/// Build every mode from one TOML file. Returns the modes plus any non-fatal
/// problems, so a typo degrades to "that one binding is ignored" rather than
/// losing the whole keymap. An unreadable file is reported the same way and
/// yields the exits only.
pub fn load_from(path: &Path) -> Loaded {
    let mut warnings = Vec::new();
    let file = read(path).unwrap_or_else(|e| {
        warnings.push(format!("{}: {e}", path.display()));
        File::default()
    });
    build(file, warnings)
}

fn build(file: File, mut warnings: Vec<String>) -> Loaded {
    let ui = build_ui(&file.ui, &mut warnings);

    // Every built-in mode, plus any the config names, gets built.
    let names: BTreeSet<&str> = MODE_NAMES
        .iter()
        .copied()
        .chain(file.modes.keys().map(String::as_str))
        .collect();

    let mut modes = HashMap::new();
    for name in names {
        let cfg = file.modes.get(name);
        let mut mode = Mode {
            label: cfg
                .and_then(|c| c.label.clone())
                .unwrap_or_else(|| name.to_uppercase()),
            hint: cfg.and_then(|c| c.hint.clone()),
            keys: Vec::new(),
        };

        // The shared exits are the only built-ins, and they can still be
        // rebound or unbound per mode.
        if cfg.map(|c| c.defaults).unwrap_or(true) {
            for (key, action) in SHARED_EXITS {
                if let Some(spec) = parse_key(key, "default", &mut warnings) {
                    insert(&mut mode, spec, action, None, &mut warnings, "default");
                }
            }
        }

        if let Some(cfg) = cfg {
            // TOML tables deserialize unordered; sort so the generated hint bar
            // and any warnings come out the same on every run.
            let mut keys: Vec<_> = cfg.keys.iter().collect();
            keys.sort_by(|a, b| a.0.cmp(b.0));
            for (key, binding) in keys {
                let (action, sticky) = match binding {
                    BindingConfig::Action(a) => (a.as_str(), None),
                    BindingConfig::Full { action, sticky } => (action.as_str(), *sticky),
                };
                let Some(spec) = parse_key(key, name, &mut warnings) else {
                    continue;
                };
                // `key = ""` unbinds.
                match action.is_empty() {
                    true => mode.unbind(spec),
                    false => insert(&mut mode, spec, action, sticky, &mut warnings, name),
                }
            }
        }

        modes.insert(name.to_string(), mode);
    }

    Loaded {
        modes,
        ui,
        warnings,
    }
}

/// A bad colour is a warning, not an error: the popup opens in the default
/// accent and the feedback row says why.
fn build_ui(cfg: &UiConfig, warnings: &mut Vec<String>) -> Ui {
    let mut ui = Ui::default();
    if let Some(raw) = &cfg.accent {
        match parse_color(raw) {
            Ok(c) => ui.accent = c,
            Err(e) => warnings.push(format!("[ui] accent: {e}")),
        }
    }
    ui
}

/// `#rrggbb` (the `#` is optional) or one of crossterm's colour names:
/// black, red, green, yellow, blue, magenta, cyan, white, grey, and the
/// `dark_` variants of all but black and white.
fn parse_color(raw: &str) -> Result<Color, String> {
    let raw = raw.trim();
    let hex = raw.strip_prefix('#').unwrap_or(raw);
    if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0);
        return Ok(Color::Rgb {
            r: channel(0),
            g: channel(2),
            b: channel(4),
        });
    }
    // crossterm's FromStr swallows unknown names as white; TryFrom reports them.
    Color::try_from(raw)
        .map_err(|()| format!("`{raw}` is not #rrggbb or a colour name such as `magenta`"))
}

fn read(path: &Path) -> Result<File, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    toml::from_str(&text).map_err(|e| e.to_string())
}

fn parse_key(key: &str, origin: &str, warnings: &mut Vec<String>) -> Option<KeySpec> {
    match KeySpec::parse(key) {
        Ok(s) => Some(s),
        Err(e) => {
            warnings.push(format!("[{origin}] {e}"));
            None
        }
    }
}

fn insert(
    mode: &mut Mode,
    spec: KeySpec,
    action: &str,
    sticky: Option<bool>,
    warnings: &mut Vec<String>,
    origin: &str,
) {
    let action = match Action::parse(action, &spec) {
        Ok(a) => a,
        Err(e) => {
            warnings.push(format!("[{origin}] key `{spec}`: {e}"));
            return;
        }
    };
    // Nothing is sticky unless the binding asks for it: one keystroke, then
    // the mode closes.
    mode.bind(
        spec,
        Binding {
            action,
            sticky: sticky.unwrap_or(false),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_str(text: &str) -> Loaded {
        build(toml::from_str(text).unwrap(), Vec::new())
    }

    #[test]
    fn accent_defaults_to_terminal_magenta() {
        let loaded = load_str("");
        assert_eq!(loaded.ui.accent, Color::Magenta);
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn accent_accepts_hex_with_or_without_hash() {
        let mauve = Color::Rgb {
            r: 0xcb,
            g: 0xa6,
            b: 0xf7,
        };
        assert_eq!(parse_color("#cba6f7"), Ok(mauve));
        assert_eq!(parse_color("cba6f7"), Ok(mauve));
        assert_eq!(parse_color("CBA6F7"), Ok(mauve));
    }

    #[test]
    fn accent_accepts_crossterm_names() {
        assert_eq!(parse_color("blue"), Ok(Color::Blue));
        assert_eq!(parse_color("dark_red"), Ok(Color::DarkRed));
        assert_eq!(parse_color("Cyan"), Ok(Color::Cyan));
    }

    #[test]
    fn bad_accent_warns_and_keeps_the_default() {
        let loaded = load_str("[ui]\naccent = \"mauve\"\n");
        assert_eq!(loaded.ui.accent, Color::Magenta);
        assert_eq!(loaded.warnings.len(), 1);
        assert!(
            loaded.warnings[0].starts_with("[ui] accent:"),
            "{:?}",
            loaded.warnings
        );
        assert!(parse_color("#cba6").is_err());
        assert!(parse_color("#gggggg").is_err());
    }

    #[test]
    fn accent_reaches_the_ui_alongside_the_modes() {
        let loaded =
            load_str("[ui]\naccent = \"#cba6f7\"\n\n[modes.pane.keys]\nh = \"focus_left\"\n");
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert!(matches!(loaded.ui.accent, Color::Rgb { .. }));
        assert!(loaded.modes["pane"].keys.len() > SHARED_EXITS.len());
    }
}
