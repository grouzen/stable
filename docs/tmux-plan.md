# stable - Agent Operations TUI

## Project Overview

`stable` — a single binary Rust TUI for managing a swarm of heterogeneous coding agents running in tmux panes.

A dashboard for terminal junkies who run multiple CLI coding agents (Claude Code, OpenCode, Pi, etc.) in tmux and want a unified overview with snappy switching between agents.

---

## Confirmed Decisions

| Topic | Decision |
|---|---|
| Project name | `stable` |
| Session backend | tmux panes |
| Agent config | `~/.config/stable/agents.toml` (persisted) |
| Dashboard refresh | Per-agent adapters + regex parsing, 500ms interval |
| Agent view refresh | 50ms live capture for near-real-time feel |
| Agent view input | Full keyboard passthrough via `tmux send-keys` |
| Escape chord | `Ctrl-g` → back to dashboard |
| Pane capture | Full scrollback (`tmux capture-pane -S -`) to emulate native terminal |
| tmux library | `tmux_interface` for `list_panes` + `send_keys`; raw `Command` for `capture_pane` |
| TUI library | `ratatui` + `crossterm` |
| ANSI rendering | `ansi-to-tui` for color-faithful rendering |

---

## Architecture

### Concept

A single Rust binary that sits on top of **tmux**. Your coding agents run in tmux panes as normal. `stable` attaches to those panes, polls their output, extracts status metadata via per-agent adapters, and presents a unified dashboard. You can jump from the dashboard into any agent's full view with keyboard passthrough.

### tmux Integration Strategy

```rust
// tmux_interface: structured data where parsing saves effort
use tmux_interface::{ListPanes, SendKeys, Tmux};

// list_panes → typed Pane structs for the add-agent dialog
// send_keys  → key encoding edge cases handled (arrows, ctrl-*, etc.)

// std::process::Command: raw text output, no value in wrapping
fn capture_pane(target: &str) -> anyhow::Result<String> {
    // tmux capture-pane -t <id> -p -e -S -
    // -p  print to stdout
    // -e  include escape sequences (color)
    // -S - from start of scrollback history
}
```

### tmux Operations

| Operation | Implementation | Purpose |
|---|---|---|
| List panes | `tmux list-panes -a -F '#{...}'` | Enumerate panes for add-agent dialog |
| Capture pane | `tmux capture-pane -t <id> -p -e -S -` | Get full scrollback with ANSI codes |
| Send keys | `tmux send-keys -t <id> <key>` | Forward keyboard input to agent |
| Check liveness | `tmux display-message -t <id> -p '#{pane_pid}'` | Verify pane/process still exists |

### Polling Architecture

Two independent refresh cycles using `tokio`:

1. **Dashboard poller** (500ms interval): For each registered agent, runs `tmux capture-pane` + adapter parsing. Updates in-memory `AgentMeta` (status, context, prompts). Dashboard re-renders on each tick.

2. **Agent view poller** (50ms interval): When in `AgentView`, continuously captures pane output and re-renders the content widget. This gives a near-live feel matching a real terminal. Keypresses are forwarded immediately on each `crossterm` event via `tmux send-keys`.

---

## Configuration

### Config File Schema

```toml
# ~/.config/stable/agents.toml

[[agents]]
name = "refactor-api"
pane = "main:1.0"          # tmux target (session:window.pane)
agent_type = "claude"      # claude | opencode | generic

[[agents]]
name = "add-feature"
pane = "work:0.1"
agent_type = "opencode"

[[agents]]
name = "pi-bot"
pane = "main:2.0"
agent_type = "generic"
```

Loaded on startup, written on every add/remove action.

---

## Agent Adapters

### AgentAdapter Trait

```rust
trait AgentAdapter {
    fn agent_type(&self) -> &str;
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

**OpenCodeAdapter** (`opencode` binary)

Similar structure — will be calibrated once we can observe its actual output format. Fallback to `GenericAdapter` if pattern doesn't match.

**GenericAdapter**

- Status: running if `tmux display-message -p '#{pane_pid}'` process is alive, stopped otherwise
- Context/prompts: `None`

---

## UI Design

### Dashboard View

```
┌─ stable ───────────────────────────────────────────────────────┐
│  [a] Add  [d] Delete  [Enter] Open  [q] Quit                   │
├────────────────┬────────────────┬────────────────┬─────────────┤
│ claude-code    │ opencode       │ pi-agent       │             │
│ ● Running      │ ⏸ Waiting      │ ■ Stopped      │             │
│ ctx: 42k/200k  │ ctx: 18k/128k  │ ctx: n/a       │             │
│ first: "Refac… │ first: "Add f… │ first: "Setup… │             │
│ last:  "Now w… │ last:  "What'… │ last:  n/a     │             │
│ pane: main:1.0 │ pane: main:1.1 │ pane: work:0.0 │             │
└────────────────┴────────────────┴────────────────┴─────────────┘
```

### Agent View

- Full-screen render of captured pane content (color-faithful via `ansi-to-tui`)
- All keypresses forwarded via `tmux send-keys`
- `Ctrl-g` returns to dashboard
- Status bar at bottom: pane id, agent type, last refresh time

### Add Agent Dialog

- Shows a list of all current tmux panes (from `tmux list-panes -a`)
- User picks a pane, assigns a name, picks agent type
- Added to in-memory registry and saved to TOML

### Remove Agent Dialog

- Confirm prompt before removing agent from registry
- `y/Enter` confirms, `n/Esc` cancels

---

## App State Machine

```
AppState
  ├── Dashboard          # default view
  │     ├── [a]          → AddAgentDialog
  │     ├── [d]          → RemoveAgentDialog
  │     └── [Enter]      → AgentView(selected)
  │
  ├── AgentView(id)      # full pane render + passthrough
  │     └── [Ctrl-g]     → Dashboard
  │
  ├── AddAgentDialog     # pane picker + name + type
  │     ├── [Enter]      → Dashboard (agent added + saved)
  │     └── [Esc]        → Dashboard (cancelled)
  │
  └── RemoveAgentDialog  # confirm prompt
        ├── [y/Enter]    → Dashboard (agent removed + saved)
        └── [n/Esc]      → Dashboard (cancelled)
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
    tmux.rs             # capture_pane() via Command; list_panes/send_keys via tmux_interface
    models.rs           # AgentEntry, AgentStatus, AgentMeta, ContextInfo
    agents/
      mod.rs            # AgentAdapter trait
      claude.rs         # ClaudeAdapter (regex patterns)
      opencode.rs       # OpenCodeAdapter (regex patterns)
      generic.rs        # GenericAdapter (process liveness only)
    ui/
      mod.rs
      dashboard.rs      # card grid, keybindings bar
      agent_view.rs     # ansi-to-tui render + scrollback state
      add_agent.rs      # pane list picker, name input, type selector
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

2. **tmux layer** — `capture_pane()`, `list_panes()`, `send_keys()` wrappers; pane liveness check using `display-message`

3. **Config layer** — TOML load/save, `~/.config/stable/` directory creation, `AgentConfig` struct

4. **Models + GenericAdapter** — `AgentStatus`, `AgentMeta`, `ContextInfo` structs; liveness-only adapter

5. **Dashboard view** — card grid with placeholder data, keybindings bar at top

6. **Agent view** — `ansi-to-tui` render of full scrollback, `Ctrl-g` escape back to dashboard

7. **Keyboard passthrough** — `send_keys` forwarding in agent view, special-key mapping (arrows, Enter, Ctrl-*)

8. **Add/Remove agent dialogs** — pane picker list widget, name input field, agent type selector, confirmation dialog

9. **ClaudeAdapter + OpenCodeAdapter** — real regex patterns, calibrated against live output from actual agents

10. **Polish** — error states (pane gone, tmux not running), refresh timestamp in status bar, `?` help overlay with keybindings

---

## Open Questions / Tradeoffs

- **Persistence**: Agent registry saved to `~/.config/stable/agents.toml` — survives restarts
- **Pane content scrollback**: `capture-pane -S -` captures the visible pane + full history; agent view will render it all
- **Error handling**: If a pane is killed externally, adapter marks it as `Stopped` and shows error in card; user can remove it manually

---

## Why This Architecture?

- **Attach to existing sessions**: No need to launch agents through `stable` — just point at running tmux panes
- **Per-agent adapters**: Different agents have different output formats; plugins make it extensible
- **Live in TUI**: Add/remove agents interactively without editing TOML manually
- **Full keyboard passthrough**: When focused on an agent, you interact with it exactly as if you'd switched to the tmux pane directly
- **Minimal dependencies on tmux internals**: Only uses documented CLI commands, no tmux server socket/control mode complexity
