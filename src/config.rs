//! User configuration, layered over the built-in defaults.
//!
//! Lives at `$HERDR_PLUGIN_CONFIG_DIR/config.toml`, which herdr creates per
//! plugin (`~/.config/herdr/plugins/config/herdr-modes/`).
//!
//! ```toml
//! [modes.pane]
//! label = "PANE"          # optional; defaults to the mode name uppercased
//! hint  = "custom text"   # optional; generated from the bindings otherwise
//!
//! [modes.pane.keys]
//! w = "focus_up"                                  # add or override
//! k = ""                                          # unbind
//! x = { action = "close_pane", sticky = false }   # override stickiness
//! ```
//!
//! Overrides merge into the defaults, matching herdr's own config style where
//! `previous_tab = ""` unbinds. Set `defaults = false` on a mode to start from
//! an empty table instead.

use crate::keymap::{Action, Binding, DEFAULTS, KeySpec, Mode, SHARED_EXITS};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Deserialize, Default)]
struct File {
    #[serde(default)]
    modes: HashMap<String, ModeConfig>,
}

#[derive(Deserialize, Default)]
struct ModeConfig {
    label: Option<String>,
    hint: Option<String>,
    /// Start from the built-in bindings for this mode. Default true.
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

    let file: File = match config_path() {
        Some(p) if p.exists() => match std::fs::read_to_string(&p) {
            Ok(text) => match toml::from_str(&text) {
                Ok(f) => f,
                Err(e) => {
                    warnings.push(format!("{}: {e}", p.display()));
                    File::default()
                }
            },
            Err(e) => {
                warnings.push(format!("{}: {e}", p.display()));
                File::default()
            }
        },
        _ => File::default(),
    };

    // Every mode named in the defaults or in the config gets built.
    let mut names: Vec<String> = Vec::new();
    for (mode, _, _) in DEFAULTS {
        if !names.iter().any(|n| n == mode) {
            names.push((*mode).to_string());
        }
    }
    for name in file.modes.keys() {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }

    let mut modes = HashMap::new();
    for name in names {
        let cfg = file.modes.get(&name);
        let mut mode = Mode {
            label: cfg
                .and_then(|c| c.label.clone())
                .unwrap_or_else(|| name.to_uppercase()),
            hint: cfg.and_then(|c| c.hint.clone()),
            name: name.clone(),
            keys: HashMap::new(),
            order: Vec::new(),
        };

        let use_defaults = cfg.map(|c| c.defaults).unwrap_or(true);
        if use_defaults {
            for (m, key, action) in DEFAULTS.iter().filter(|(m, _, _)| *m == name) {
                insert(&mut mode, key, action, None, &mut warnings, "default");
                let _ = m;
            }
        }
        // Shared exits are defaults too, so they can be rebound or unbound.
        if use_defaults {
            for (key, action) in SHARED_EXITS {
                insert(&mut mode, key, action, None, &mut warnings, "default");
            }
        }

        if let Some(cfg) = cfg {
            for (key, binding) in &cfg.keys {
                let (action, sticky) = match binding {
                    BindingConfig::Action(a) => (a.clone(), None),
                    BindingConfig::Full { action, sticky } => (action.clone(), *sticky),
                };
                if action.is_empty() {
                    // `key = ""` unbinds.
                    if let Ok(spec) = KeySpec::parse(key) {
                        mode.keys.remove(&spec);
                        mode.order.retain(|s| *s != spec);
                    }
                    continue;
                }
                insert(&mut mode, key, &action, sticky, &mut warnings, &name);
            }
        }

        modes.insert(name.clone(), mode);
    }

    (modes, warnings)
}

fn insert(
    mode: &mut Mode,
    key: &str,
    action: &str,
    sticky: Option<bool>,
    warnings: &mut Vec<String>,
    origin: &str,
) {
    let spec = match KeySpec::parse(key) {
        Ok(s) => s,
        Err(e) => {
            warnings.push(format!("[{origin}] {e}"));
            return;
        }
    };
    let action = match Action::parse(action, &spec) {
        Ok(a) => a,
        Err(e) => {
            warnings.push(format!("[{origin}] key `{key}`: {e}"));
            return;
        }
    };
    if !mode.keys.contains_key(&spec) {
        mode.order.push(spec);
    }
    mode.keys.insert(
        spec,
        Binding { action, sticky: sticky.unwrap_or_else(|| action.default_sticky()) },
    );
}
