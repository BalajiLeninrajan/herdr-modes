# herdr-modes

Zellij-style **sticky modes** for [herdr](https://herdr.dev). Press `prefix+p` and
then drum `hjkl` to move between panes — no re-pressing the prefix, and no
terminal keys consumed, so readline's `ctrl+l` / `ctrl+k` stay yours.

herdr's key model is tmux-shaped: one prefix plus a flat namespace of named
actions. There is no user-definable mode. But three modes *are* already sticky —
`resize_mode`, `copy_mode` (which subsumes zellij's scroll + search +
entersearch), and `goto` — so this plugin supplies only the three that are
missing: **pane**, **tab**, and **move**.

## How it works

A keybind invokes a plugin action, which opens a session-modal popup pane
running the key loop. The popup keeps receiving keystrokes even as pane focus
moves underneath it, which is what makes stickiness possible. Actions run
detached without a TTY, so the action → pane hop is required; it costs one round
trip on mode entry only.

Each keystroke is dispatched over the herdr socket API rather than by shelling
out to the `herdr` binary. That keeps drumming instant, and it is the only way
to reach `tab.move`, which has no CLI subcommand at all.

## Install

Requires herdr `>= 0.7.0` and a Rust toolchain.

```bash
git clone <this repo> ~/Documents/code/herdr-modes
cd ~/Documents/code/herdr-modes && cargo build --release
herdr plugin link ~/Documents/code/herdr-modes
```

Then add to `~/.config/herdr/config.toml`:

```toml
[keys]
# prefix+p is previous_tab by default; tab mode's h/k covers it.
previous_tab = ""

[[keys.command]]
key = "prefix+p"
type = "plugin_action"
command = "herdr-modes.pane"
description = "pane mode"

[[keys.command]]
key = "prefix+t"
type = "plugin_action"
command = "herdr-modes.tab"
description = "tab mode"

[[keys.command]]
key = "prefix+m"
type = "plugin_action"
command = "herdr-modes.move"
description = "move mode"
```

Validate and reload:

```bash
herdr config check
herdr server reload-config
```

## Keys

These are the **defaults** — every one is configurable, see
[Configuration](#configuration). They were transcribed from a zellij config,
**including per-key stickiness**: zellij is mixed about this, so movement keys
stay in the mode while creation keys fall back to normal.

### pane mode — `prefix+p`

| key | action | stays in mode |
|---|---|---|
| `h` `j` `k` `l` | focus left/down/up/right | yes |
| `p` | cycle focus | yes |
| `x` | close pane | yes |
| `d` / `r` | split down / right | no |
| `n` | new pane | no |
| `f` / `z` | zoom | no |
| `c` | rename pane | no |

### tab mode — `prefix+t`

| key | action | stays in mode |
|---|---|---|
| `h` `k` / `j` `l` | previous / next tab | yes |
| `tab` | last tab | yes |
| `H` / `L` | move tab left / right | yes |
| `x` | close tab | yes |
| `1`–`9` | go to tab | no |
| `n` | new tab | no |
| `r` | rename tab | no |
| `b` / `[` / `]` | break pane to new / prev / next tab | no |

### move mode — `prefix+m`

| key | action | stays in mode |
|---|---|---|
| `h` `j` `k` `l` | swap pane in direction | yes |
| `n` / `tab` | swap forward | yes |
| `p` | swap backward | yes |

`esc`, `enter`, and `ctrl+c` exit any mode.

Keys with no herdr equivalent are deliberately unbound: floating, pinned, and
stacked panes, pane-frame toggling, and tab input sync. `z` is a free alias for
zoom because zellij used it for pane frames.

## Configuration

Optional. Without a config file you get the defaults above.

Create `~/.config/herdr/plugins/config/herdr-modes/config.toml` — herdr makes
that directory per plugin and passes it as `$HERDR_PLUGIN_CONFIG_DIR`. See
[`config.example.toml`](config.example.toml) for the full reference.

```toml
[modes.pane]
label = "WINDOW"          # hint-bar label

[modes.pane.keys]
w = "focus_up"                                 # add or rebind
k = ""                                         # unbind
x = { action = "close_pane", sticky = false }  # override stickiness
```

Overrides **merge** into the defaults, so you write only what differs, and
`key = ""` unbinds — the same convention herdr's own config uses. To start a
mode from scratch instead, set `defaults = false` on it.

The hint bar is generated from whatever bindings are active, with keys sharing
an action collapsed together (`hjkl focus`), so it never drifts out of sync with
the keymap. Set `hint = "..."` on a mode to write it yourself.

Mode names are just strings. Adding a brand-new mode needs an `[[actions]]` and
a `[[panes]]` entry in `herdr-plugin.toml` and a `[[keys.command]]` in herdr's
config to open it, but nothing in the binary is hardcoded to the three built-ins.

Validate a config and print the resolved keymaps:

```bash
herdr-modes check
```

It reports unknown actions, unparseable keys, and malformed TOML, exits non-zero
if anything is wrong, and still applies every binding that did parse — a typo
costs you one binding, not the whole keymap. The same warnings appear in the
mode's feedback row at runtime.

## Notes on the herdr API

Two behaviours worth knowing if you build on this:

- **The socket is one request per connection.** The server closes the connection
  as soon as it answers. Only `events.subscribe` holds one open. A client that
  caches a connection will see `Broken pipe` on its second call.
- **`tab.move`'s `insert_index` counts the tab being moved.** The index is
  evaluated against the list *including* that tab, so moving one slot right
  needs `cur + 2`, not `cur + 1`. Moving left needs no adjustment.

## License

MIT
