# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- `[ui] accent` in the plugin config sets the hint bar colour, as a hex value or a crossterm colour name.
- `herdr-modes check <path>` validates any keymap file, not only the installed one.
- `herdr-modes check --actions` prints every action with its hint label and whether it can leave the tab.
- CI runs fmt, clippy, tests, a release build and `check config.example.toml` on Linux and macOS.

### Changed

- The hint bar defaults to the terminal's magenta instead of a fixed hex colour.
- Both hint bar rows are clipped to the popup width so a long legend never wraps into the feedback row.
- Mode entry reads focus from the server before choosing the tab the popup belongs to.
- `open` treats herdr's `ui_busy` error code, as well as its message, as "a popup is already open".

### Fixed

- The hop note records the herdr socket it was written for, so an `open` under another herdr session no longer picks it up or deletes it while it is fresh.

## 0.2.0 - 2026-09-09

### Changed

- The popup hops to the new tab after any action that leaves its own tab, since herdr 0.9.0 only sends keys to a popup while the client views the tab it opened on.
- Closing the popup's own tab or its last pane arms the hop before the call, because herdr kills the popup mid-request.
- agent.focus and pane.move are followed by pane.focus so the viewing client moves with the server's focus.
- A resumed popup calls tab.focus before reading its first key, which also covers a tab that closed under it.
- Requires herdr 0.9.0 or newer.

## 0.1.0 - 2026-08-24

### Added

- pane, tab and move modes, opened from a plugin action as a session-modal popup that keeps the key loop running while focus moves underneath it.
- agent mode, which walks herdr's agent panel and can skip to the next agent waiting on you.
- space mode, which previews each space as you switch to it.
- A configurable keymap in config.toml, with keys, labels, hint text and per-key stickiness; only the exits (esc, enter, ctrl+c) are bound by default.
- Bindings are non-sticky unless a key sets sticky = true.
- A hint bar generated from the active bindings, hidden with hint = "".
- herdr-modes check, which validates the config and prints the resolved keymaps.
- Keys are dispatched over the herdr socket API, which is the only way to reach tab.move.
