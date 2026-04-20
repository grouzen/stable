# stable — MVP Plan (OpenCode only)

## Scope

OpenCode-only MVP. ClaudeAdapter, hook subcommands, and claude-code integration are excluded.

---

## Locked Decisions

| Topic | Decision |
|---|---|
| Scope | OpenCode only |
| Dashboard grid | 2×2 → 3×3 → 4×4 based on agent count; max 16 agents |
| Empty dashboard | Centered message "No agents. Press [n] to create one." |
| AgentView scroll | PgUp/PgDn; offset=0 means live/auto-scroll |
| Launch overlay | No loader — brief freeze while opencode starts |
| Name sanitization | Non-`[a-zA-Z0-9_-]` → `-`, collapse runs, trim edges |
| cwd | Via `tmux new-window -c <dir>` |
| Port allocation | Probe via `TcpListener::bind` from 14100 |

---

## File Structure

```
src/
  main.rs           # clap entry, tokio::main, calls tui::run()
  app.rs            # App, AppState, event loop, mpsc channels, polling tasks
  tui.rs            # enter/leave alternate screen, raw mode, panic hook
  config.rs         # AgentConfig (with port/session_id), load/save agents.toml
  models.rs         # AgentStatus, AgentMeta, ContextInfo, AgentEntry
  tmux.rs           # ensure_session, new_window, capture_pane, send_keys, liveness, sanitize_name
  agents/
    mod.rs          # AgentAdapter trait (async_trait)
    opencode.rs     # OpenCodeAdapter + creation flow
  ui/
    mod.rs
    dashboard.rs    # card grid, empty message, keybindings bar
    agent_view.rs   # ansi-to-tui render, PgUp/PgDn scroll, Ctrl-g, status bar
    create_agent.rs # name/dir/agent fields, Tab-completion, confirm flow
    remove_agent.rs # y/n confirm dialog
```

---

## Dependencies (Cargo.toml)

```toml
[dependencies]
ratatui          = "0.29"
crossterm        = "0.28"
ansi-to-tui      = "4"
tokio            = { version = "1", features = ["rt-multi-thread", "macros", "time"] }
tmux_interface   = { version = "0.4", default-features = false, features = ["tmux_3_3a"] }
regex            = "1"
serde            = { version = "1", features = ["derive"] }
toml             = "0.8"
anyhow           = "1"
clap             = { version = "4", features = ["derive"] }
dirs             = "5"
reqwest          = { version = "0.12", features = ["json"] }
async-trait      = "0.1"
```

---

## Config Schema (`~/.config/stable/agents.toml`)

```toml
# This file is written and managed by stable. Do not edit manually.

[[agents]]
name = "my-agent"
pane = "stable:1.0"
agent_type = "opencode"
directory = "/home/user/projects/foo"
port = 14100
session_id = "sess_abc123"
```

Loaded on startup, saved on every create/remove.

---

## Event Model

```rust
enum Event {
    Key(crossterm::event::KeyEvent),
    Resize(u16, u16),
    DashboardTick,   // 500ms — refresh AgentMeta for all agents
    AgentViewTick,   // 50ms  — capture_pane for current agent
}
```

Single `mpsc::unbounded_channel`. Three producers:
- `crossterm::EventStream` task
- Dashboard interval task (always running)
- AgentView interval task (always running; ignored when not in AgentView)

---

## App State Machine

```
AppState
  ├── Dashboard
  │     ├── [n]     → CreateAgentDialog
  │     ├── [d]     → RemoveAgentDialog(selected)
  │     └── [Enter] → AgentView(selected)
  │
  ├── CreateAgentDialog
  │     ├── [Enter] → new_window() + send-keys(opencode --port N) + health check + POST /session → AgentView(new)
  │     └── [Esc]   → Dashboard (cancelled)
  │
  ├── AgentView(idx)
  │     └── [Ctrl-g] → Dashboard
  │
  └── RemoveAgentDialog(idx)
        ├── [y/Enter] → Dashboard (agent removed + config saved)
        └── [n/Esc]   → Dashboard (cancelled)
```

---

## Dashboard View

### Empty State

When there are no agents, render a centered message:

```
No agents. Press [n] to create one.
```

### Grid Layout

Grid dimension is determined by agent count:

| Agent count | Grid |
|---|---|
| 1–4   | 2×2 |
| 5–9   | 3×3 |
| 10–16 | 4×4 |

```rust
fn grid_dim(n: usize) -> usize {
    if n <= 4 { 2 } else if n <= 9 { 3 } else { 4 }
}
```

Cards are placed left-to-right, top-to-bottom. Empty slots render as blank bordered cells. Selected card is highlighted with a bold/colored border.

### Card Layout

```
┌─ <name> ──────────┐
│ ● Running          │
│ ctx: 42k/200k      │
│ first: "Refac…"    │
│ last:  "Now w…"    │
│ pane: stable:1.0   │
└────────────────────┘
```

### Keybindings Bar

```
[n] New  [d] Delete  [Enter] Open  [q] Quit
```

---

## Agent View

- Full-screen render of `capture_pane` output via `ansi_to_tui::bytes_to_text` → `ratatui::Text`
- Rendered in a `Paragraph` widget

### Scroll

- `scroll_offset: usize` — lines from the bottom (0 = live/auto-scroll)
- `PgUp` increments offset (toward history), `PgDn` decrements (toward live), clamped to line count
- When `scroll_offset == 0`: auto-scroll — always show latest output on each tick
- When `scroll_offset > 0`: lines update each tick but offset is held (user is reading history)

### Status Bar

```
pane: stable:1.0 | opencode | last refresh 12:34:56  [scrolled]
```

`[scrolled]` indicator shown only when `scroll_offset > 0`.

### Agent Stopped Overlay

When poller detects `AgentStatus::Stopped` (edge, not every tick):

```
┌──────────────────────────────────────────┐
│                                          │
│   Agent stopped.                         │
│                                          │
│   [d] Remove agent   [Ctrl-g] Dashboard  │
│                                          │
└──────────────────────────────────────────┘
```

- Keypresses not forwarded to tmux while overlay is visible
- `[d]` removes agent from registry, saves config, returns to Dashboard
- `[Ctrl-g]` dismisses overlay, returns to Dashboard (card shown as `■ Stopped`)

---

## CreateAgentDialog

```
┌─── New Agent ──────────────────────────────┐
│                                            │
│  Name:       [my-agent                 ]   │
│                                            │
│  Directory:  [/home/user/projects/foo  ]   │
│              Tab: path autocomplete        │
│                                            │
│  Agent:      ● opencode                    │
│                                            │
│  [Enter] Launch        [Esc] Cancel        │
└────────────────────────────────────────────┘
```

- `↑/↓` moves between fields
- `Tab` on Directory: `std::fs::read_dir` on current prefix parent, cycle through matches
- All fields must be non-empty to enable Launch
- On error (timeout): show inline error message, allow retry or cancel

### Name Sanitization

```rust
fn sanitize_name(s: &str) -> String {
    let s = s.trim();
    let sanitized: String = s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    Regex::new(r"-{2,}").unwrap()
        .replace_all(&sanitized, "-")
        .trim_matches('-')
        .to_string()
}
```

### OpenCode Creation Flow

```
1. sanitize name
2. find free port: probe TcpListener::bind("127.0.0.1:<port>") from 14100 upward
3. tmux::new_window(dir, sanitized_name) → window_index
4. pane = format!("stable:{}.0", window_index)
5. tmux::send_keys(pane, format!("opencode --port {}\n", port))
6. loop up to 25× with 200ms sleep:
     GET http://127.0.0.1:<port>/global/health → { healthy: true } → break
7. POST http://127.0.0.1:<port>/session {} → { id: session_id }
8. push AgentEntry to app.agents, save agents.toml
9. transition to AppState::AgentView(new_index)
   on timeout → show error in dialog, stay in CreateAgentDialog
```

---

## RemoveAgentDialog

```
Remove "my-agent"? [y/Enter] confirm  [n/Esc] cancel
```

- Does **not** kill the tmux window (session keeps running)
- On confirm: remove from `app.agents`, save `agents.toml`, return to Dashboard

---

## OpenCodeAdapter

```rust
pub struct OpenCodeAdapter {
    port: u16,
    session_id: String,
    client: reqwest::Client,
}
```

### Methods

| Method | API | Logic |
|---|---|---|
| `get_status` | `GET /session/status` | Find `session_id` in map → `busy`/`retry` → `Running`; `idle` → `WaitingForInput`; connection refused → `Stopped` |
| `get_context` | messages + config + provider | Latest AssistantMessage tokens summed → `used`; model → provider lookup → `limit.context` → `total` |
| `get_first_prompt` | `GET /session/{id}/message` | First user message → first TextPart.text |
| `get_last_prompt` | `GET /session/{id}/message` | Last user message → first TextPart.text |

### `get_context` Detail

```
GET /session/{id}/message
  → filter role="assistant"
  → take latest by time.created
  → used = tokens.input + tokens.output + tokens.cache.read + tokens.cache.write

GET /config → config.model e.g. "anthropic/claude-sonnet-4-5"
  → providerID="anthropic", modelID="claude-sonnet-4-5"

GET /provider
  → find provider where id == providerID
  → find model where key == modelID
  → total = model.limit.context

→ Some(ContextInfo { used, total }) or None if any step fails
```

---

## tmux Operations

| Operation | Method | Command |
|---|---|---|
| Ensure session | `tmux_interface` | `tmux new-session -d -s stable` |
| Create window | `tmux_interface` | `tmux new-window -t stable -c <dir> -n <name>` |
| Send keys | `tmux_interface` | `tmux send-keys -t <pane> <keys>` |
| Capture pane | `std::process::Command` | `tmux capture-pane -t <pane> -p -e -S -` |
| Liveness | `std::process::Command` | `tmux display-message -t <pane> -p '#{pane_pid}'` |

---

## Implementation Phases

1. `cargo new stable` + `Cargo.toml`
2. `tui.rs` — alternate screen, raw mode, panic hook
3. `tmux.rs` — all tmux operations + `sanitize_name`
4. `config.rs` + `models.rs` — TOML schema, structs
5. `agents/opencode.rs` — OpenCodeAdapter (HTTP polling)
6. `app.rs` — App struct, state machine, event loop, polling tasks
7. `ui/dashboard.rs` — grid layout, card rendering, empty state
8. `ui/agent_view.rs` — ansi-to-tui render, scroll, stopped overlay
9. `ui/create_agent.rs` — dialog, Tab-completion, creation flow
10. `ui/remove_agent.rs` — confirm dialog
11. `main.rs` — clap CLI wiring
