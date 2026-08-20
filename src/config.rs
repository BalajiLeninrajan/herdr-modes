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

use crate::keymap::{Action, Binding, KeySpec, MODE_NAMES, Mode, SHARED_EXITS};
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

#[derive(Deserialize, Default)]
struct File {
    #[serde(default)]
    modes: HashMap<String, ModeConfig>,
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
    Full { action: String, sticky: Option<bool> },
}

pub fn config_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("HERDR_PLUGIN_CONFIG_DIR") {
        return Some(PathBuf::from(dir).join("config.toml"));
    }
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".config/herdr/plugins/config/herdr-modes/config.toml"))
}

/// Build every mode: defaults, then user overrides. Returns the modes plus any
/// non-fatal problems, so a typo degrades to "that one binding is ignored"
/// rather than losing the whole keymap.
pub fn load() -> (HashMap<String, Mode>, Vec<String>) {
    let mut warnings = Vec::new();

    let file = match config_path().filter(|p| p.exists()) {
        Some(p) => read(&p).unwrap_or_else(|e| {
            warnings.push(format!("{}: {e}", p.display()));
            File::default()
        }),
        None => File::default(),
    };

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
            label: cfg.and_then(|c| c.label.clone()).unwrap_or_else(|| name.to_uppercase()),
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
                let Some(spec) = parse_key(key, name, &mut warnings) else { continue };
                // `key = ""` unbinds.
                match action.is_empty() {
                    true => mode.unbind(spec),
                    false => insert(&mut mode, spec, action, sticky, &mut warnings, name),
                }
            }
        }

        modes.insert(name.to_string(), mode);
    }

    (modes, warnings)
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
    mode.bind(spec, Binding { action, sticky: sticky.unwrap_or(false) });
}
