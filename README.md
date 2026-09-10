# herdr-modes

Sticky, user-defined modes for [herdr](https://herdr.dev).

A mode is a keymap that owns the keyboard until you leave it. Bind any key to
any herdr action and say whether it acts once or keeps the mode open, so
`j` `j` `j` walks three panes, tabs, agents or spaces on one prefix press,
and none of it reaches the shell.

There are five modes: **pane**, **tab**, **move**, **agent** and **space**.
Agent mode can jump straight to the next agent waiting on you. Space mode
switches spaces as you browse and can cancel back to where you started.
Nothing is bound until you write a config;
[`config.example.toml`](config.example.toml) is a complete zellij-shaped
keymap to start from.

## Install

Requires herdr `>= 0.9.0` and a Rust toolchain (`cargo` on your `PATH`), on
Linux or macOS. The plugin talks to herdr over a Unix socket, so there is no
Windows build.

```bash
herdr plugin install BalajiLeninrajan/herdr-modes
```

herdr fetches the repository and runs the `cargo build --release` step from
the plugin manifest, so the first install takes as long as a release build.
There is no `plugin update`; to move to a newer version, run the install
again.

Then add to `~/.config/herdr/config.toml`:

```toml
[keys]
# prefix+p is previous_tab and prefix+w is workspace_picker by default;
# free them up for pane mode and space mode.
previous_tab = ""
workspace_picker = ""

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

[[keys.command]]
key = "prefix+a"
type = "plugin_action"
command = "herdr-modes.agent"
description = "agent mode"

[[keys.command]]
key = "prefix+w"
type = "plugin_action"
command = "herdr-modes.space"
description = "space mode"
```

Validate and reload:

```bash
herdr config check
herdr server reload-config
```

At this point every mode opens but only `esc`, `enter` and `ctrl+c` do
anything. Copy [`config.example.toml`](config.example.toml) to
`~/.config/herdr/plugins/config/herdr-modes/config.toml` to get the zellij
keymap; see [Configuration](#configuration).

## How it works

A keybind invokes a plugin action, which opens a session-modal popup pane
running the key loop. The popup keeps receiving keystrokes even as pane focus
moves underneath it, which is what makes stickiness possible. Actions run
detached without a TTY, so the hop from action to pane is required; it costs
one round trip on mode entry only.

Each keystroke is dispatched over the herdr socket API rather than by shelling
out to the `herdr` binary. That keeps drumming instant, and it is the only way
to reach `tab.move`, which has no CLI subcommand at all.

## Keys

Only the **exits** are bound by default: `esc`, `enter` and `ctrl+c` leave any
mode. Everything else you bind yourself, and every binding decides whether it
stays in the mode (`sticky = true`) or acts once and closes it (the default).

[`config.example.toml`](config.example.toml) is a complete zellij-transcribed
keymap: `hjkl` focus and swap, `x` close, `n`, `d` and `r` splits, `1`-`9` tab
jumps, with the movement keys sticky. Copy it and edit from there.

The actions each mode can bind:

| mode | actions |
|---|---|
| pane | `focus_left` `focus_right` `focus_up` `focus_down` `cycle_focus` `close_pane` `split_right` `split_down` `zoom` `rename_pane` |
| tab | `prev_tab` `next_tab` `last_tab` `goto_tab` (bind to `1`-`9`) `new_tab` `close_tab` `rename_tab` `move_tab_left` `move_tab_right` `break_pane_new` `break_pane_prev` `break_pane_next` |
| move | `swap_left` `swap_right` `swap_up` `swap_down` `swap_forward` `swap_backward` |
| agent | `prev_agent` `next_agent` `last_agent` `goto_agent` (bind to `1`-`9`) `next_attention` `prev_attention` |
| space | `prev_space` `next_space` `last_space` `goto_space` (bind to `1`-`9`) `next_space_attention` `prev_space_attention` |
| any | `exit` `cancel` |

Modes are only namespaces for a keymap, so any action can be bound in any mode.
Keys with no herdr equivalent have no action at all: floating, pinned and
stacked panes, pane-frame toggling, and tab input sync.

### Agent mode

herdr's own `previous_agent`, `next_agent` and `focus_agent` are flat prefix
bindings, so stepping three agents down the panel costs three prefixes. Agent
mode makes the same walk sticky, and adds the jump the flat namespace has no
room for. `next_attention` skips every agent that is still working and lands on
the next one that is **blocked** on you or **done** with work you have not
seen. That is herdr's attention queue, walked one keystroke at a time.

Navigation follows the order `agent.list` reports, which is the panel's own
`agent_panel_sort = "spaces"` grouping; `goto_agent` counts rows in that same
order. Focusing an agent crosses spaces and tabs on its own, and (as anywhere
in herdr) marks it seen, so a `done` agent becomes `idle` once you land on it.

### Space mode

herdr's own space navigation is a picker: the sidebar cursor moves, the view
does not, and the space only changes when you commit. Space mode inverts that.
Every keystroke calls `workspace.focus`, so the space switches underneath the
popup as you drum `jk`. The preview *is* the switch, and there is no selection
that can disagree with what you are looking at.

That only works because the popup is session-modal: it keeps the keyboard while
focus crosses spaces beneath it, exactly as agent mode already crosses them.

Which makes leaving mean two things, so there are two ways out:

- `exit` (`enter`, `ctrl+c`) keeps wherever you landed.
- `cancel` puts focus back where the mode opened, both the space and the pane
  inside it. Bind it to `esc` and browsing costs nothing.

`next_space_attention` is `next_attention` one level up: it skips the spaces
whose agents are all busy or already read, and lands on the next one holding an
agent that is **blocked** on you or **done** with work you have not seen.
`goto_space` counts sidebar rows, the same order `workspace.list` reports.

## Configuration

Optional, but without it only the exit keys are bound.

Create `~/.config/herdr/plugins/config/herdr-modes/config.toml`. herdr makes
that directory per plugin and passes it as `$HERDR_PLUGIN_CONFIG_DIR`. See
[`config.example.toml`](config.example.toml) for the full reference.

```toml
[modes.pane]
label = "WINDOW"          # hint-bar label

[modes.pane.keys]
w = "focus_up"                                # act once, then leave the mode
k = { action = "focus_up", sticky = true }    # stay in the mode
esc = ""                                      # unbind
```

Overrides **merge**, so you write only what differs, and `key = ""` unbinds,
the same convention herdr's own config uses. The exits are ordinary bindings
and can be rebound or unbound too; `defaults = false` on a mode drops them as
well, leaving it completely empty.

The hint bar is generated from whatever bindings are active, with keys sharing
an action collapsed together (`hjkl focus`), so it never drifts out of sync with
the keymap. Set `hint = "..."` on a mode to write it yourself, or `hint = ""` to
hide the bar entirely; the mode then shows nothing but its feedback line.

Mode names are just strings. Adding a brand-new mode needs an `[[actions]]` and
a `[[panes]]` entry in `herdr-plugin.toml` and a `[[keys.command]]` in herdr's
config to open it, but nothing in the binary is hardcoded to the five built-in
modes.

Validate a config and print the resolved keymaps:

```bash
herdr-modes check
herdr-modes check path/to/config.toml
```

Without a path it reads the file in the plugin config dir; with one it checks
that file instead. It reports unknown actions, unparseable keys and malformed TOML, exits non-zero
if anything is wrong, and still applies every binding that did parse, so a typo
costs you one binding, not the whole keymap. The same warnings appear in the
mode's feedback row at runtime.

## Troubleshooting

A mode stops reacting after the first key, and the prefix key for that mode
then does nothing. The popup is stranded on another tab. Since herdr 0.9.0 a
popup only gets keys while the client views the tab it opened on (see
[Internals](#internals)), and while one is open herdr answers every
`plugin.pane.open` with `ui_busy`, so the prefix key is a no-op until it goes
away. Switch back to the tab the mode opened on and press `esc`, or close the
popup over the socket. Either of these works, depending on what is installed:

```bash
printf '{"id":"1","method":"popup.close","params":{}}\n' | socat - UNIX-CONNECT:"$HERDR_SOCKET_PATH"
printf '{"id":"1","method":"popup.close","params":{}}\n' | nc -U "$HERDR_SOCKET_PATH"
```

`HERDR_SOCKET_PATH` is set inside any herdr pane.

If a mode does not open at all, the `open` action's stderr is in
`herdr plugin log list`. If a key does nothing and the feedback row starts with
`config:`, run `herdr-modes check`; it prints every binding that failed to
parse.

## Internals

Since herdr 0.9.0 a popup belongs to the tab it opened on: the client draws it
and sends it keys only while that tab is on screen. Pane moves within a tab are
unaffected, but any action that lands on another tab or space would leave the
popup behind, alive and invisible. So after such an action the popup *hops*: it
writes its state to `$HERDR_PLUGIN_STATE_DIR/resume.json`, asks herdr to run
the mode's `open` action again, and exits. That action retries
`plugin.pane.open` until the old popup is gone, opens a fresh one on the new
tab, and passes the state in through `HERDR_MODES_RESUME`, so `last_tab`,
`last_space`, `cancel` and the feedback line all survive. Closing the popup's
own tab (or its last pane) kills the popup mid-request, so those arm the hop
first. A hop costs one extra process spawn, about a tenth of a second.

Only `workspace.focus`, `tab.focus` and `pane.focus` move the viewing client
along with the server's focus. `agent.focus` and `pane.move` change what the
server calls focused while the client keeps looking at the old tab, which would
strand the popup again. So both are followed by a `pane.focus`, and a popup
that has just hopped calls `tab.focus` before reading its first key.

## Notes on the herdr API

A few behaviours worth knowing if you build on this:

- **The socket is one request per connection.** The server closes the connection
  as soon as it answers. Only `events.subscribe` holds one open. A client that
  caches a connection will see `Broken pipe` on its second call.
- **Focusing a space carries its own tab and pane.** `workspace.focus` restores
  whatever was active in that space, so leaving one and coming back needs no
  bookkeeping beyond the workspace id, which is what makes `cancel` a
  one-call undo.
- **`agent.focus` takes any agent target, including a pane id.** `agent.list`
  hands back `pane_id` for every row, so panel navigation needs no separate
  name lookup, and the focus crosses workspace and tab boundaries itself.
- **`tab.move`'s `insert_index` counts the tab being moved.** The index is
  evaluated against the list *including* that tab, so moving one slot right
  needs `cur + 2`, not `cur + 1`. Moving left needs no adjustment.

## Development

Link a checkout instead of installing from GitHub, so herdr runs the binary
you just built:

```bash
cargo build --release
herdr plugin link /path/to/herdr-modes
./target/release/herdr-modes check
```

`herdr plugin link` points herdr at the checkout; rebuild and reopen a mode to
pick up a change. Action stderr goes to `herdr plugin log list`.

## License

MIT
