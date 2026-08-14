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

Transcribed from a zellij config, **including per-key stickiness** — zellij is
mixed about this, so movement keys stay in the mode while creation keys fall
back to normal.

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
