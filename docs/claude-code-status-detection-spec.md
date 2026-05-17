# Claude Code Agent Status Detection — Improvement Spec

## Background

Claude Code exposes a rich set of lifecycle hook events that fire during agent execution. The current
`stable` integration only registers 4 of them (`SessionStart`, `UserPromptSubmit`, `Stop`,
`SessionEnd`), and conflates "turn finished — idle, ready for next prompt" with "blocked on a
permission request" under a single `WaitingForInput` status.

This spec describes the changes required to:
1. Add the missing hook events for finer-grained status transitions.
2. Introduce a new `Idle` status distinct from `WaitingForInput`.
3. Align the OpenCode integration to the same semantics.
4. Surface a 3rd counter ("idle") in the dashboard and agent-view status bars.

---

## Hook-Event → Status Mapping

The mapping is modelled after the [herdr project](https://github.com/ogulcancelik/herdr):

| Claude Code hook event | herdr status | `AgentStatus` variant |
|---|---|---|
| `SessionStart` | — | `Running` _(existing)_ |
| `UserPromptSubmit` | working | `Running` |
| `PreToolUse` | working | `Running` _(new event)_ |
| `PostToolUse` | working | `Running` _(new event)_ |
| `SubagentStop` | working | `Running` _(new event)_ |
| `PermissionRequest` | blocked | `WaitingForInput` _(new event)_ |
| `Stop` | idle | `Idle` _(changed from `WaitingForInput`)_ |
| `SessionEnd` | release | `Stopped` _(existing)_ |

### Status Semantics

| `AgentStatus` variant | Meaning |
|---|---|
| `Running` | Agent is actively processing — model is working or a tool is executing. |
| `WaitingForInput` | Agent is **blocked** — waiting for the user to grant/deny a permission request. |
| `Idle` | Agent has finished its current turn and is ready for the next user prompt. |
| `Stopped` | Session has ended — process has exited or `SessionEnd` was received. |
| `Unknown` | Status has not yet been determined (startup / restore in progress). |

---

## Changes Required

### 1. `src/models.rs`

Add the `Idle` variant to `AgentStatus`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum AgentStatus {
    Running,
    WaitingForInput,
    Idle,       // turn finished, ready for next user prompt
    Stopped,
    Unknown,
}
```

---

### 2. `src/agents/claude/claude_hook_server.rs`

#### 2a. Change `Stop` arm

```rust
// before
entry.status = AgentStatus::WaitingForInput;

// after
entry.status = AgentStatus::Idle;
```

#### 2b. Add new match arms (inside the existing `match event_name.as_str()` block)

```rust
"PreToolUse" | "PostToolUse" | "SubagentStop" => {
    let mut map = state.hook_state.lock().unwrap();
    if let Some(entry) = map.get_mut(&agent_id) {
        entry.status = AgentStatus::Running;
    }
}

"PermissionRequest" => {
    let mut map = state.hook_state.lock().unwrap();
    if let Some(entry) = map.get_mut(&agent_id) {
        entry.status = AgentStatus::WaitingForInput;
    }
}
```

---

### 3. `src/agents/claude.rs`

#### 3a. `build_hooks_block()` — register 4 new hook events

```rust
let hooks_map: serde_json::Map<String, Value> = [
    make_hook("SessionStart"),
    make_hook("UserPromptSubmit"),
    make_hook("PreToolUse"),
    make_hook("PostToolUse"),
    make_hook("SubagentStop"),
    make_hook("PermissionRequest"),
    make_hook("Stop"),
    make_hook("SessionEnd"),
]
.into_iter()
.collect();
```

#### 3b. `restore()` — set `Idle` instead of `WaitingForInput`

Both locations in `restore()` that currently set `WaitingForInput` must be changed to `Idle`,
because a restored session has already completed its last turn:

```rust
// line ~209: transcript found on disk
entry.status = AgentStatus::Idle;   // was WaitingForInput

// line ~221: session_id known but no transcript yet
entry.status = AgentStatus::Idle;   // was WaitingForInput
```

---

### 4. `src/agents/opencode.rs`

OpenCode has no `PermissionRequest` equivalent. Its natural "not-busy" state (API reports `"idle"`,
`"error"`, or anything that is not `"busy"` / `"retry"`) should map to `Idle`, not
`WaitingForInput`. `WaitingForInput` will be plumbed in for OpenCode in a future iteration when
a permission-request concept is available.

Three locations to update:

| Location | Change |
|---|---|
| line ~298: `let mut best = AgentStatus::WaitingForInput;` | → `AgentStatus::Idle` |
| line ~323: empty session object fallback | → `AgentStatus::Idle` |
| line ~370: `session.status` SSE catch-all | → `AgentStatus::Idle` |

---

### 5. `src/ui/theme.rs`

Add a new icon constant for the `Idle` status and a `CYAN` colour:

```rust
/// Cyan — idle state (turn complete, awaiting next prompt).
pub const CYAN: Color = Color::Rgb(104, 157, 106);  // Gruvbox aqua

/// U+25CB WHITE CIRCLE — idle status.
pub const ICON_IDLE: &str = "○";
```

> Note: Gruvbox "aqua" (`#689d6a`) is the soft blue-green that fits the palette without clashing
> with the existing `BLUE` teal (`#458588`).

---

### 6. `src/ui/dashboard.rs`

#### 6a. Add `count_idle` helper

```rust
fn count_idle(agents: &[AgentEntry]) -> usize {
    agents
        .iter()
        .filter(|a| matches!(a.meta.status, AgentStatus::Idle))
        .count()
}
```

#### 6b. `status_symbol` — add `Idle` arm

```rust
AgentStatus::Idle => ICON_IDLE,
```

#### 6c. `status_label` — add `Idle` arm

```rust
AgentStatus::Idle => "Idle",
```

#### 6d. `status_color` — add `Idle` arm

```rust
AgentStatus::Idle => CYAN,
```

#### 6e. Status counts bar — add 3rd counter

```rust
spans.push(Span::styled(
    format!(" {} {} idle", ICON_IDLE, idle),
    ds(dimmed).fg(CYAN),
));
```

The full trio will render as:
```
● 2 running  ⏸ 1 waiting  ○ 3 idle
```

---

### 7. `src/ui/agent_view.rs`

#### 7a. Add idle count alongside running / waiting

```rust
let idle = agents
    .iter()
    .filter(|a| matches!(a.meta.status, AgentStatus::Idle))
    .count();
```

#### 7b. Append idle span to status bar

```rust
status_spans.push(Span::styled(
    format!(" {} {} idle", ICON_IDLE, idle),
    Style::default().fg(CYAN),
));
```

---

## Testing Checklist

- [ ] Send a `PermissionRequest` hook event → agent transitions to `WaitingForInput`
- [ ] Send a `Stop` hook event → agent transitions to `Idle` (not `WaitingForInput`)
- [ ] Send `PreToolUse` / `PostToolUse` / `SubagentStop` → agent transitions to `Running`
- [ ] `SessionEnd` → agent transitions to `Stopped`
- [ ] Restart `stable` with a known transcript → restored agent is `Idle` (not `WaitingForInput`)
- [ ] OpenCode agent with a non-busy session → shows `Idle`
- [ ] Dashboard status bar displays all three counters: running, waiting, idle
- [ ] Agent-view status bar displays all three counters: running, waiting, idle
- [ ] `has_stable_hooks` test: all 8 hook events are registered after `build_hooks_block()`
- [ ] Existing install/uninstall/idempotency hook tests still pass

---

## Step-by-Step Implementation Todo

### Step 1 — `src/models.rs`: Add `Idle` variant
- [ ] Add `Idle` variant to `AgentStatus` enum between `WaitingForInput` and `Stopped`

### Step 2 — `src/ui/theme.rs`: Add `CYAN` colour and `ICON_IDLE` constant
- [ ] Add `pub const CYAN: Color = Color::Rgb(104, 157, 106);`
- [ ] Add `pub const ICON_IDLE: &str = "○";` with doc comment

### Step 3 — `src/agents/claude/claude_hook_server.rs`: Update hook handler
- [ ] Change `Stop` arm: `WaitingForInput` → `Idle`
- [ ] Add `"PreToolUse" | "PostToolUse" | "SubagentStop"` arm → `Running`
- [ ] Add `"PermissionRequest"` arm → `WaitingForInput`

### Step 4 — `src/agents/claude.rs`: Register new hooks + fix restore
- [ ] Add `PreToolUse`, `PostToolUse`, `SubagentStop`, `PermissionRequest` to `build_hooks_block()`
- [ ] Update `restore()` line ~209: `WaitingForInput` → `Idle`
- [ ] Update `restore()` line ~221: `WaitingForInput` → `Idle`

### Step 5 — `src/agents/opencode.rs`: Align to new semantics
- [ ] Line ~298 `best` initializer: `WaitingForInput` → `Idle`
- [ ] Line ~323 empty-session fallback: `WaitingForInput` → `Idle`
- [ ] Line ~370 `session.status` SSE catch-all: `WaitingForInput` → `Idle`

### Step 6 — `src/ui/dashboard.rs`: Add `Idle` to display + 3rd counter
- [ ] Add `count_idle()` helper function
- [ ] Add `AgentStatus::Idle` arm to `status_symbol()`
- [ ] Add `AgentStatus::Idle` arm to `status_label()`
- [ ] Add `AgentStatus::Idle` arm to `status_color()`
- [ ] Pass `idle` count into the keybindings-bar render function
- [ ] Append idle counter span to the status counts bar

### Step 7 — `src/ui/agent_view.rs`: Add idle counter to status bar
- [ ] Compute `idle` count from agents list
- [ ] Append idle counter span after waiting span

### Step 8 — Compile and fix exhaustiveness errors
- [ ] Run `cargo build` and fix any `non-exhaustive patterns` errors in match expressions
  that cover `AgentStatus` elsewhere in the codebase (`app.rs`, etc.)

### Step 9 — Verify existing tests
- [ ] Run `cargo test` and fix any test failures related to hook count assertions
  (the `has_stable_hooks` / install tests now expect 8 hook events instead of 4)
