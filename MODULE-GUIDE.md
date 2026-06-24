# Writing a bohay module

A **module** is the way you extend bohay. It's a plain directory with one manifest file
(`bohay-module.toml`) that declares **commands** — argv arrays bohay runs as subprocesses.
There's no SDK, no scripting runtime, and no language requirement: if it can be executed and
read environment variables, it can be a bohay module. A command does its work by reading the
injected `BOHAY_*` context and, when it needs to change the workspace, by calling the same
`bohay` CLI you use by hand.

> **What works today:** **actions** (on-demand commands), **panes** (long-lived UI in a real
> bohay pane), **event hooks** (run a command when an agent blocks/finishes, a pane opens, etc.),
> and install from either a local dir (`link`) or GitHub (`install`). The design spec is
> [docs/13](docs/13-modules.md).

---

## 1. A module in five minutes

Create a directory with a manifest and a script:

```
my-module/
├── bohay-module.toml
└── refresh.sh
```

`bohay-module.toml`:

```toml
id = "you.hello"                 # required, globally unique-ish
name = "Hello"                   # required
version = "0.1.0"                # required
min_bohay_version = "0.1.0"      # required

[[actions]]
id = "refresh"                   # local id (no dots)
title = "Say hello"
command = ["sh", "refresh.sh"]   # argv — run without a shell
```

`refresh.sh`:

```sh
#!/bin/sh
echo "hello from $BOHAY_MODULE_ID"
echo "context: $BOHAY_MODULE_CONTEXT_JSON"
```

Register and run it:

```sh
bohay module link ./my-module        # → { "id": "you.hello" }
bohay module run you.hello refresh   # → { "log_id": 1 }
bohay module log                     # see status + captured output
```

`module run` is fire-and-forget: it returns a `log_id` immediately and the command runs in the
background. Read the captured stdout/stderr (and exit status) back with `module log`.

---

## 2. The manifest (`bohay-module.toml`)

Lives at the module root and is parsed as TOML.

### Top level

| Key | Required | Notes |
|---|---|---|
| `id` | ✅ | `[a-z0-9:._-]`, ≤120 chars. Dots **allowed** (e.g. `you.git-status`). This is the module's global id. |
| `name` | ✅ | Human-readable, non-empty. |
| `version` | ✅ | Your module's version, non-empty. |
| `min_bohay_version` | ✅ | Minimum bohay version. Install is refused if it's newer than the host. |
| `description` | — | One-line summary. |
| `platforms` | — | e.g. `["macos", "linux"]`. Omit for all platforms. **`[]` is an error.** A module not allowed on the current OS stays listed but is not runnable. |

### `[[actions]]` — on-demand commands *(live)*

```toml
[[actions]]
id = "commit"                    # local id: [a-z0-9:_-], ≤120, NO dots
title = "Commit staged changes"
contexts = ["node", "pane"]      # optional, informational for now
command = ["bun", "run", "commit.ts"]
```

Invoke with `bohay module run <module-id> <action-id>`. The qualified id is
`{module-id}.{action-id}` (e.g. `you.git-status.commit`). `contexts` (`global` / `node` / `tab`
/ `pane` / `selection`) is recorded for a future "where to offer this action" UI — it isn't
enforced yet.

### `[[build]]` — install-time setup *(parsed; runs at git-install, MOD-4)*

```toml
[[build]]
command = ["bun", "install"]
```

For local `link` you run your own setup. Build steps run with a **scrubbed** environment (no
`BOHAY_*` / socket access) when git-install lands.

### `[[events]]` — run a command on a lifecycle event *(live)*

```toml
[[events]]
on = "pane.agent_status_changed" # see the event list below
command = ["sh", "notify.sh"]
```

When the event fires, your command runs with `BOHAY_MODULE_EVENT` (the event name) and
`BOHAY_MODULE_EVENT_JSON` (the payload) in its environment. Events you can hook:
`node.created`, `node.closed`, `tab.created`, `tab.closed`, `pane.created`, `pane.closed`, and
`pane.agent_status_changed` (payload `{pane, status, agent}` — the highest-value hook, e.g.
notify when `status` becomes `blocked` or `done`).

### `[[panes]]` — a long-lived process in a real pane *(live)*

```toml
[[panes]]
id = "board"                     # local id, no dots
title = "Git board"
placement = "split"              # overlay | split | tab  (default: split)
command = ["bun", "run", "board.ts"]
```

Open one with `bohay module pane open <module-id> board [--placement …]`. It becomes a real
bohay pane (a TUI, a log tail, anything), runs in the module root with the full `BOHAY_*`
environment, and is auto-untracked when you close it. `overlay` opens a split and zooms it to
fill the screen; `tab` opens it in a new tab.

### Validation rules

A manifest is rejected (and the module won't load) if:

- any `command` is empty or contains an empty string,
- `min_bohay_version` is newer than the host,
- an `id` uses characters outside its allowed set, or a local id contains a dot,
- two actions (or two panes) share an id,
- `platforms = []`.

Commands are **run without a shell** — `["git", "status"]`, not `"git status"`. If you want
shell features (pipes, `$VAR`, `&&`), invoke a shell explicitly: `["sh", "-c", "git status | head"]`.

---

## 3. How a command runs

When you invoke an action, bohay:

1. Builds the environment (below) and a context snapshot of the focused node/tab/pane.
2. Spawns your `command` as a subprocess with the **working directory set to the module root**.
3. Captures stdout and stderr, each **capped at 64 KiB**, and waits for exit.
4. Records a log entry: `running` → `succeeded` (exit 0) or `failed`.

Caps: at most **32** module commands run at once; the **200** most recent logs are kept.
Commands are isolated processes — a crash or hang in one never takes down bohay.

---

## 4. Environment variables

Every command receives:

| Variable | Meaning |
|---|---|
| `BOHAY_ENV` | Always `1` — you're running under bohay. |
| `BOHAY_MODULE_ID` | Your module's id. |
| `BOHAY_MODULE_ROOT` | The module directory (also the command's cwd). **Read-only by convention** — for git-installed modules it's a managed checkout. |
| `BOHAY_MODULE_CONFIG_DIR` | Durable, user-owned config/secrets dir. Created for you. |
| `BOHAY_MODULE_STATE_DIR` | Durable state/cache dir. Created for you. |
| `BOHAY_MODULE_CONTEXT_JSON` | JSON snapshot of the focused node/tab/pane (next section). |
| `BOHAY_SOCKET_PATH` | The control socket (informational; the CLI finds it automatically). |
| `BOHAY_BIN_PATH` | Absolute path to the `bohay` binary — **use this to call back**. |
| `BOHAY_MODULE_ACTION_ID` | The action id, for action commands. |

**Persist data in `BOHAY_MODULE_STATE_DIR` / `BOHAY_MODULE_CONFIG_DIR`, never in
`BOHAY_MODULE_ROOT`.** Print `bohay module config-dir <id>` to find (and create) the config dir.

---

## 5. The context blob

`BOHAY_MODULE_CONTEXT_JSON` is a JSON object describing what was focused when your command ran:

```json
{
  "node": { "id": "0", "name": "sudos", "cwd": "/Users/you/skyrizz/sudos" },
  "tab":  { "index": "1" },
  "pane": { "id": "4", "cwd": "/Users/you/skyrizz/sudos", "agent": "claude", "status": "working" },
  "invocation_source": "cli",
  "correlation_id": "c7"
}
```

Parse it with whatever JSON tooling your language has (`jq`, `JSON.parse`, `serde_json`, …).
`status` is one of `idle` / `working` / `blocked` / `done` / `unknown`. `invocation_source` is
`cli` or `api`. `correlation_id` is unique per invocation, handy for log correlation.

Quick `jq` example:

```sh
node_cwd=$(printf '%s' "$BOHAY_MODULE_CONTEXT_JSON" | jq -r .node.cwd)
cd "$node_cwd" && git status --short
```

---

## 6. Calling back into bohay

To *do* something — open a pane, run a command, split — call the `bohay` CLI via
`$BOHAY_BIN_PATH`. It connects to the same session automatically:

```sh
"$BOHAY_BIN_PATH" pane run "git pull"     # run a command in the focused pane
"$BOHAY_BIN_PATH" node list               # inspect nodes (JSON to stdout)
"$BOHAY_BIN_PATH" pane split --down       # split the layout
```

Anything in `bohay help` is available: `node`, `tab`, `pane`, `agent`, `ui`, `events`. This is
the whole point of the model — **the bohay CLI is the module API**. Your command reads context
from the environment and effects change through the CLI.

---

## 7. Managing modules

```sh
bohay module search [<query>]      # discover modules tagged `bohay-module` on GitHub
bohay module list                  # installed modules + runnable state
bohay module info <id>             # a module's actions / panes / events / source
bohay module link <path>           # register a local dir (--disabled to skip enabling)
bohay module install <owner>/<repo>[/sub] [--ref REF] [--yes]   # install from GitHub
bohay module unlink <id>           # remove a linked module from the registry
bohay module uninstall <id>        # unlink + delete a git-installed module's checkout
bohay module enable <id>           # / disable <id>
bohay module actions               # every action across all modules
bohay module run <id> <action>     # invoke an action  (or: run <action> if unambiguous)
bohay module pane open <id> <entrypoint> [--placement split|overlay|tab]
bohay module pane focus <pane> | close <pane>
bohay module log [<id>]            # tail command logs  (--limit N)
bohay module config-dir <id>       # print + create a module's config dir
```

The registry lives at `~/.bohay/modules.json`; per-module data lives under
`~/.bohay/modules/{config,state}/`. On startup bohay re-reads every manifest from disk — if a
module's manifest goes missing or breaks, the entry stays **listed but not runnable** with a
warning, so a bad edit never silently disappears.

You can also manage modules from the UI: **Settings → Modules** (the ⚙ gear, or `Ctrl+Space`
then `,`) lists installed modules with enable/disable toggles. And a module **pane** that's
open when you detach is re-opened on the next launch — if the module is still installed and
enabled, bohay re-runs its entrypoint; otherwise that pane falls back to a shell.

---

## 8. Distribution & discovery

Share a module as a public Git repo (one repo can hold several modules in subdirectories), and
**tag it with the `bohay-module` GitHub topic** so others can find it:

```sh
bohay module search            # most-starred modules in the topic
bohay module search git        # narrow by a keyword
```

`search` is a read-only GitHub lookup (it shells out to `curl`/`wget`) — it needs no server and
never auto-installs anything; you copy an `owner/repo` and run `module install`. Install it with:

```sh
bohay module install owner/repo            # whole repo is the module
bohay module install owner/repo/path/to    # a module in a subdirectory
bohay module install owner/repo --ref v1   # pin a branch / tag / commit
```

Install does a shallow clone, **shows you every command the module declares and asks to
proceed** (`--yes` skips the prompt for CI), runs any `[[build]]` steps in a scrubbed
environment, verifies the manifest didn't change during the build, and moves it into a managed
directory pinned to the installed commit. There is intentionally **no central registry or
publishing step** — "publishing" is just pushing a public repo. `module uninstall <id>` deletes
the managed checkout; `module unlink <id>` is for locally-linked dirs (it never deletes files).

---

## 9. Troubleshooting

- **`module link` fails with "invalid manifest"** — check TOML syntax and that every `command`
  is a non-empty array of non-empty strings. Run `bohay module link` and read the error.
- **Module is listed but `runnable: false`** — it's disabled (`module enable <id>`), gated out
  by `platforms`, or its manifest has a load `warning` (shown in `module list`).
- **Action "is ambiguous"** — two modules expose the same action id; pass the module:
  `bohay module run <module-id> <action>`.
- **No output in the log** — confirm the command exits (it's `running` until it does), and
  remember stdout/stderr are capped at 64 KiB. Errors from a failed spawn show up in the log's
  `err` field.
- **`$BOHAY_BIN_PATH` callbacks do nothing** — they only work while a bohay server is running
  for the session your command was launched from.

---

## Roadmap

Everything ships today: actions, panes, event hooks, `link`/`install`/`uninstall`, and
`search`. The only thing left is an *optional* hosted marketplace (a cron crawler producing a
browse page) — install never needs it, so it's a nicety, not a blocker. See
[docs/13](docs/13-modules.md) for the full design.
