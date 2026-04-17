# stable - Agent Operations TUI

## Project Overview

`stable` — a single binary Rust TUI for managing a swarm of heterogeneous coding agents running in tmux panes.

A dashboard for terminal junkies who run multiple CLI coding agents (Claude Code, OpenCode, etc.) in tmux and want a unified overview with snappy switching between agents.

The only prerequisite for the user is having `tmux` installed. `stable` owns and manages the tmux session entirely — no manual tmux setup required.

---

## Confirmed Decisions

| Topic | Decision |
|---|---|
| Project name | `stable` |
| Session backend | tmux windows (one agent per window, full screen) |
| Session name | `stable` (fixed, created by stable on first launch) |
| Session lifetime | Persists after stable exits; reattached on next launch |
| Agent config | `~/.config/stable/agents.toml` (written by stable, not manually) |
| Dashboard refresh | Per-agent adapters + regex parsing, 500ms interval |
| Agent view refresh | 50ms live capture for near-real-time feel |
| Agent view input | Full keyboard passthrough via `tmux send-keys` |
| Escape chord | `Ctrl-g` → back to dashboard |
| Pane capture | Full scrollback (`tmux capture-pane -S -`) to emulate native terminal |
| tmux library | `tmux_interface` for `list_panes` + `send_keys`; raw `Command` for `capture_pane` |
| TUI library | `ratatui` + `crossterm` |
| ANSI rendering | `ansi-to-tui` for color-faithful rendering |
| Agent types | `claude` and `opencode` only (no generic) |
| Agent creation | Modal dialog in TUI: name + directory + agent type → creates tmux window + launches agent |
| Attach to existing pane | Not supported; all agents created through stable |

---

## Architecture

### Concept

A single Rust binary that owns a dedicated `stable` tmux session. On first launch, stable creates the session. On subsequent launches, it reattaches. Users create agents through a TUI dialog — stable opens a new tmux window with the chosen working directory, runs the agent CLI, and immediately switches to its AgentView.

### Session Lifecycle

```
stable launches
        ↓
tmux has-session -t stable?
    ├── NO  → tmux new-session -d -s stable   (background session, 1 empty window)
    └── YES → reattach (session survived previous quit)
        ↓
stable TUI renders in user's current terminal (outside the managed session)
        ↓
user quits stable
        ↓
tmux session stays alive; agents keep running
```

### tmux Integration Strategy

```rust
// tmux_interface: structured data where parsing saves effort
use tmux_interface::{NewSession, NewWindow, SendKeys, Tmux};

// ensure_session → create 'stable' session if absent
// new_window     → open agent window with correct cwd
// send_keys      → key encoding edge cases handled (arrows, ctrl-*, etc.)

// std::process::Command: raw text output, no value in wrapping
fn capture_pane(target: &str) -> anyhow::Result<String> {
    // tmux capture-pane -t <id> -p -e -S -
    // -p  print to stdout
    // -e  include escape sequences (color)
    // -S - from start of scrollback history
}
```

### tmux Operations

| Operation | Command | When |
|---|---|---|
| Ensure session | `tmux new-session -d -s stable` | Startup |
| Create window | `tmux new-window -t stable -c <dir>` | CreateAgentDialog confirm |
| Launch agent | `tmux send-keys -t <pane> "claude\n"` or `"opencode\n"` | Immediately after window creation |
| Capture pane | `tmux capture-pane -t <id> -p -e -S -` | Dashboard + AgentView polling |
| Send keys | `tmux send-keys -t <id> <key>` | AgentView passthrough |
| Check liveness | `tmux display-message -t <id> -p '#{pane_pid}'` | Dashboard poller |

### Polling Architecture

Two independent refresh cycles using `tokio`:

1. **Dashboard poller** (500ms interval): For each registered agent, runs `tmux capture-pane` + adapter parsing. Updates in-memory `AgentMeta` (status, context, prompts). Dashboard re-renders on each tick.

2. **Agent view poller** (50ms interval): When in `AgentView`, continuously captures pane output and re-renders the content widget. This gives a near-live feel matching a real terminal. Keypresses are forwarded immediately on each `crossterm` event via `tmux send-keys`.

---

## Configuration

### Config File Schema

```toml
# ~/.config/stable/agents.toml
# This file is written and managed by stable. Do not edit manually.

[[agents]]
name = "my-refactor"
pane = "stable:1.0"           # assigned by stable on creation
agent_type = "claude"         # claude | opencode
directory = "/home/user/projects/foo"

[[agents]]
name = "add-feature"
pane = "stable:2.0"
agent_type = "opencode"
directory = "/home/user/projects/bar"
```

Loaded on startup, written on every add/remove action.

---

## Agent Adapters

### AgentAdapter Trait

```rust
trait AgentAdapter {
    fn agent_type(&self) -> &str;
    fn launch_command(&self) -> &str;
    fn parse_status(&self, output: &str) -> AgentStatus;
    fn parse_context_window(&self, output: &str) -> Option<ContextInfo>;
    fn parse_first_prompt(&self, output: &str) -> Option<String>;
    fn parse_last_prompt(&self, output: &str) -> Option<String>;
}
```

### AgentStatus Enum

```rust
enum AgentStatus {
    Running,
    WaitingForInput,
    Stopped,
    Unknown
}
```

Inferred heuristically per adapter.

### Adapter Implementations

**ClaudeAdapter** (`claude` binary)

| Field | Pattern to match |
|---|---|
| Waiting for input | Prompt line ending with `>` or `❯` after output settles |
| Running | Spinner chars (`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`) or `Thinking…` lines |
| Stopped | Process no longer present / pane shows shell prompt |
| Context window | Lines like `Context window: 42,341 / 200,000 tokens` |
| First prompt | First line after banner/header that looks like user input |
| Last prompt | Last occurrence of user-turn line before agent response |
| Launch command | `claude` |

**OpenCodeAdapter** (`opencode` binary)

Similar structure — will be calibrated once we can observe its actual output format.

| Field | Pattern to match |
|---|---|
| Launch command | `opencode` |
| Status / context / prompts | TBD — calibrated against live output |

---

## UI Design

### Dashboard View

```
┌─ stable ───────────────────────────────────────────────────────┐
│  [n] New  [d] Delete  [Enter] Open  [q] Quit                   │
├────────────────┬────────────────┬────────────────┬─────────────┤
│ claude-code    │ opencode       │                │             │
│ ● Running      │ ⏸ Waiting      │                │             │
│ ctx: 42k/200k  │ ctx: 18k/128k  │                │             │
│ first: "Refac… │ first: "Add f… │                │             │
│ last:  "Now w… │ last:  "What'… │                │             │
│ pane: stable:1 │ pane: stable:2 │                │             │
└────────────────┴────────────────┴────────────────┴─────────────┘
```

### Agent View

- Full-screen render of captured pane content (color-faithful via `ansi-to-tui`)
- All keypresses forwarded via `tmux send-keys`
- `Ctrl-g` returns to dashboard
- Status bar at bottom: pane id, agent type, last refresh time

### CreateAgentDialog

Modal overlay on the dashboard:

```
┌─── New Agent ──────────────────────────────┐
│                                            │
│  Name:       [my-refactor              ]   │
│                                            │
│  Directory:  [/home/user/projects/foo  ]   │
│              Tab: path autocomplete        │
│                                            │
│  Agent:      ● claude                      │
│              ○ opencode                    │
│                                            │
│  [Enter] Launch        [Esc] Cancel        │
└────────────────────────────────────────────┘
```

- **Tab** on directory field: completes path via `std::fs::read_dir` (no subprocess)
- **↑/↓** moves between fields
- **Space** toggles agent type radio
- All fields must be non-empty to enable Launch
- On confirm: creates tmux window → sends agent command → transitions to `AgentView(new)`

### RemoveAgentDialog

- Confirm prompt before removing agent from registry
- `y/Enter` confirms, `n/Esc` cancels
- Does **not** kill the tmux window (session stays alive)

---

## App State Machine

```
AppState
  ├── Dashboard                  # default view
  │     ├── [n]     → CreateAgentDialog
  │     ├── [d]     → RemoveAgentDialog
  │     └── [Enter] → AgentView(selected)
  │
  ├── CreateAgentDialog          # name + dir + agent type modal
  │     ├── [Enter] → new_window() + send-keys(agent cmd) → AgentView(new)
  │     └── [Esc]   → Dashboard (cancelled)
  │
  ├── AgentView(id)              # full pane render + passthrough
  │     └── [Ctrl-g] → Dashboard
  │
  └── RemoveAgentDialog          # confirm prompt
        ├── [y/Enter] → Dashboard (agent removed + saved)
        └── [n/Esc]   → Dashboard (cancelled)
```

---

## Project Structure

```
stable/
  Cargo.toml
  src/
    main.rs             # clap CLI, tokio runtime, launch App
    app.rs              # App struct, state machine, event dispatch
    tui.rs              # ratatui + crossterm setup/teardown, panic hook
    config.rs           # TOML load/save, AgentConfig struct
    tmux.rs             # ensure_session, new_window, capture_pane, send_keys, liveness
    models.rs           # AgentEntry, AgentStatus, AgentMeta, ContextInfo
    agents/
      mod.rs            # AgentAdapter trait
      claude.rs         # ClaudeAdapter (regex patterns)
      opencode.rs       # OpenCodeAdapter (regex patterns)
    ui/
      mod.rs
      dashboard.rs      # card grid, keybindings bar
      agent_view.rs     # ansi-to-tui render + scrollback state
      create_agent.rs   # name input, dir input w/ Tab-completion, agent radio
      remove_agent.rs   # confirm dialog
```

---

## Dependencies

### Cargo.toml

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
dirs             = "5"     # for ~/.config resolution
```

| Crate | Purpose |
|---|---|
| `ratatui` | TUI framework |
| `crossterm` | Terminal backend (raw mode, events) |
| `ansi-to-tui` | ANSI escape sequences → ratatui Text conversion |
| `tokio` | Async runtime for polling timers |
| `tmux_interface` | Typed tmux command builders |
| `regex` | Adapter output parsing |
| `serde` + `toml` | Config serialization |
| `anyhow` | Error handling |
| `clap` | CLI args parser |
| `dirs` | Cross-platform config dir resolution |

---

## Implementation Phases

1. **Scaffold** — `cargo new stable`, deps, `tui.rs` boilerplate (enter/leave alternate screen, raw mode, panic hook to restore terminal on crash)

2. **tmux layer** — `ensure_session()`, `new_window()`, `capture_pane()`, `send_keys()`, liveness check via `display-message`

3. **Config layer** — TOML load/save, `~/.config/stable/` directory creation, `AgentConfig` struct with `directory` field

4. **Models** — `AgentStatus`, `AgentMeta`, `ContextInfo` structs

5. **Dashboard view** — card grid with placeholder data, keybindings bar (`[n] [d] [Enter] [q]`)

6. **Agent view** — `ansi-to-tui` render of full scrollback, `Ctrl-g` escape back to dashboard

7. **Keyboard passthrough** — `send_keys` forwarding in agent view, special-key mapping (arrows, Enter, Ctrl-*)

8. **CreateAgentDialog** — name input, directory input with Tab-completion, agent type radio, creates tmux window + sends agent command on confirm, transitions to AgentView

9. **RemoveAgentDialog** — confirm prompt, removes from registry and config

10. **ClaudeAdapter + OpenCodeAdapter** — real regex patterns calibrated against live output

11. **Polish** — dead pane handling (mark Stopped, show error in card), refresh timestamp in status bar, `?` help overlay

---

## Design Notes

- **No manual tmux setup**: stable creates and owns the `stable` tmux session. User only needs `tmux` in PATH.
- **Session survives stable**: Quitting stable leaves agents running. Re-launching stable reattaches to the existing session and re-reads config.
- **No pty handling in stable**: All terminal emulation is delegated to tmux. stable only reads `capture-pane` output and writes via `send-keys`.
- **One window per agent**: Each agent gets a dedicated tmux window (full screen), making `capture-pane` targeting unambiguous.
- **Config is stable-owned**: `agents.toml` is written by stable on every create/remove. Users should not edit it manually.
