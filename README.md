# bohay

**A terminal workspace manager for AI coding agents.**

bohay is a client/server terminal multiplexer that runs inside your existing terminal as a
single static Rust binary. It gives you persistent panes, tabs, and workspaces that survive
detach; a live sidebar showing every agent's state (blocked / working / done / idle); a
mouse-native split/resize UI; and a local socket API that lets the agents themselves drive
the multiplexer.

```
┌ NODES ─────────────┐ │  1    ✕   +
│ ● bohay     main    │ ┏ ● ~/bohay ━━━━━━━━ × ┓ ┏ ● ~/bohay ━━━━━━━━┓
│   ~/skyrizz/bohay   │ ┃ $ claude              ┃ ┃ $ cargo test       ┃
│                     │ ┃ › working…            ┃ ┃ running 20 tests   ┃
├ AGENTS ─────────────┤ ┃                       ┃ ┃                    ┃
│ ● working  claude   │ ┗━━━━━━━━━━━━━━━━━━━━━━━┛ ┗━━━━━━━━━━━━━━━━━━━━┛
│   bohay · tab 1     │
└─────────────────────┘ ⌃Space prefix   v split → s split ↓ x close   NORMAL · 2 panes
```

## Why

- **`cargo build` is the entire build.** Pure Rust — no Zig, no FFI, no C toolchain. Clone
  and build in one command on any platform Rust supports.
- **Lean runtime.** Event-driven with zero idle redraws — small binary, low idle CPU, low
  memory per pane.
- **Agent-first.** The socket API and the "agents can orchestrate bohay" story are
  first-class. Real pub/sub for agent-status changes — no polling latency.
- **A codebase you can hold in your head.** Pure state separated from runtime, one event
  loop, modules that each do one thing.

## Install

Requires a recent stable Rust toolchain.

```bash
git clone <repo-url> bohay
cd bohay
cargo install --path .      # installs the `bohay` binary into ~/.cargo/bin
```

Or build without installing:

```bash
cargo build --release       # ./target/release/bohay
```

## Quick start

```bash
bohay                       # launch (or attach to) the session
```

The first run spawns a detached background **server** that owns your panes, then attaches a
thin **client** to it. Detach with `Ctrl+Space` then `q` — your panes keep running. Run
`bohay` again to re-attach. Stop everything with `bohay server stop`.

### Keybindings

All commands are prefixed with **`Ctrl+Space`** (press it, then the key):

| Key | Action | Key | Action |
|-----|--------|-----|--------|
| `v` | split right (vertical divider) | `c` | new tab |
| `s` / `-` | split down (horizontal divider) | `n` / `p` | next / previous tab |
| `x` / `X` | close the focused pane | `1`–`9` | jump to tab _n_ |
| `z` | zoom the focused pane | `N` | new node (workspace) |
| `h` `j` `k` `l` | move focus between panes | `D` | close the current node |
| `b` | toggle the sidebar | `w` | cycle to the next node |
| `q` / `d` | detach (leave the server running) | | |

Pressing `Ctrl+Space` twice sends a literal `Ctrl+Space` to the focused program. The UI is
also fully mouse-driven — click tabs, nodes, agents, panes, the `+`/`✕` buttons, and scroll.

## CLI

Every TUI action is also a scriptable command that talks to the running server over a Unix
socket. Anything an agent (or you) can do in the UI, it can do from a shell:

```bash
bohay ping                          # check the server
bohay pane list                     # panes in the current tab
bohay pane split --down             # split the focused pane downward
bohay pane run 7 cargo test         # run a command in pane 7
bohay pane read 7                   # print pane 7's recent output
bohay agent list                    # every agent across all nodes/tabs
bohay events                        # stream live agent-status changes
```

Full surface (`bohay help`):

```
nodes (spaces):   node list | node new | node focus <i> | node close [<i>]
tabs:             tab list  | tab new  | tab focus <n>   | tab close [<n>]
panes / agents:   pane list | pane split [<id>] [--down] | pane focus <id>
                  pane run/send/read/close [<id>] | agent list
events:           events                  # stream status changes
server:           server stop             # stop the server and all panes
```

When a command runs inside a bohay pane, the target pane defaults to that pane (via the
injected `$BOHAY_PANE_ID`), so `bohay pane split` "just works" without an explicit id.

## Agent integration

Agents that expose a session id can report it to bohay so their native session is **resumed**
automatically after a restart (e.g. `claude --resume <id>`). Install the hook once:

```bash
bohay integration install claude
```

This drops a `SessionStart` hook into the agent's config; it reports the session id over the
socket using the `BOHAY_*` environment injected into every pane.

## Configuration

State lives in **`~/.bohay/`** (debug builds use `~/.bohay-dev/`). Override the location with
`$BOHAY_HOME`.

| File | Purpose |
|------|---------|
| `~/.bohay/session.json` | Saved workspaces / tabs / pane tree (restored on launch) |
| `~/.bohay/bohay.sock` | JSON control-API socket (the CLI + agents) |
| `~/.bohay/bohay-client.sock` | Binary render-frame socket (client ↔ server) |

## Architecture

A headless **server** renders frames into an off-screen buffer and streams them to a thin
**client** that just blits to the real terminal; a `--local` mode runs both in one process
for development. State is pure and separated from the runtime — one event loop, one timer.

```
src/
  main.rs            entry point + arg dispatch (server / client / cli / local)
  app/               application state & behavior
    mod.rs             workspaces → tabs → BSP pane tree; construction & mutations
    input.rs           key/mouse events + the Ctrl+Space command map
    dispatch.rs        JSON control-API dispatch + agent-detection tick
  ui/                rendering (off-screen draw pass)
    mod.rs             render() orchestration + shared layout helpers
    borders.rs         manual cell-by-cell pane borders
    panes.rs           terminal blit + pane titles
    sidebar.rs         NODES + AGENTS lists
    tabbar.rs          tab bar
    status.rs          bottom status line
    theme.rs           color palette
  terminal/          PTY actor (pty) + pure-Rust VT engine (vt/)
  ipc/               Unix-socket layer: control api, frame protocol, client, server
  layout.rs          BSP tiling tree
  detect.rs          agent detection (screen + activity based)
  persist.rs         session snapshot / restore
  platform.rs        OS-specific bits (process cwd)
  integration.rs     agent integration hooks
```

The `docs/` directory contains the full design notes (architecture, data model, terminal
handling, the socket API, persistence, and the execution plan).

## Development

```bash
cargo build                                 # debug build
cargo test                                  # unit + render tests (off-screen, no tty)
cargo clippy && cargo fmt --check           # lints + formatting
cargo run -- --local                        # run client+server in one process
cargo test generate_preview -- --ignored    # regenerate preview.html / preview.ans
```

Tests render the full UI into a `TestBackend` buffer, so layout and draw paths are exercised
without a real terminal.

## License

MIT
