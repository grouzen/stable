# stable MVP — Implementation Todo

Module convention: use `<module>.rs` at the parent level instead of `<module>/mod.rs`.
So `agents.rs` declares the `AgentAdapter` trait, `ui.rs` declares submodules.
Submodule files live in `src/agents/` and `src/ui/` as normal.

---

## Step 1: Project scaffold
- [ ] `cargo new stable --name stable`
- [ ] Write `Cargo.toml` with all dependencies from the plan
- [ ] Verify `cargo build` compiles with empty main

## Step 2: Terminal setup (`src/tui.rs`)
- [ ] Implement `enter_terminal()`: enable raw mode, switch to alternate screen
- [ ] Implement `leave_terminal()`: disable raw mode, leave alternate screen
- [ ] Install panic hook that calls `leave_terminal()` before printing panic message
- [ ] Expose `run(app)` function that sets up terminal, runs event loop, tears down on exit

## Step 3: tmux layer (`src/tmux.rs`)
- [ ] `sanitize_name(s: &str) -> String` — replace non-`[a-zA-Z0-9_-]` with `-`, collapse consecutive dashes, trim leading/trailing dashes
- [ ] `ensure_session()` — `tmux has-session -t stable`; create if absent via `tmux_interface`
- [ ] `new_window(dir: &str, name: &str) -> anyhow::Result<usize>` — create window with `-c <dir> -n <name>`, return window index
- [ ] `send_keys(target: &str, keys: &str) -> anyhow::Result<()>` — `tmux_interface::SendKeys`
- [ ] `capture_pane(target: &str) -> anyhow::Result<String>` — raw `std::process::Command`, flags `-p -e -S -`
- [ ] `is_alive(target: &str) -> bool` — `display-message -p '#{pane_pid}'`, non-empty output = alive

## Step 4: Data models (`src/models.rs`)
- [ ] `AgentStatus` enum: `Running`, `WaitingForInput`, `Stopped`, `Unknown`
- [ ] `ContextInfo` struct: `used: u64`, `total: u64`
- [ ] `AgentMeta` struct: `status: AgentStatus`, `context: Option<ContextInfo>`, `first_prompt: Option<String>`, `last_prompt: Option<String>`
- [ ] `AgentEntry` struct: `config: AgentConfig`, `meta: AgentMeta`

## Step 5: Config layer (`src/config.rs`)
- [ ] `AgentConfig` struct: `name`, `pane`, `agent_type`, `directory`, `port: u16`, `session_id`; derive `Serialize`/`Deserialize`
- [ ] `Config` struct: `agents: Vec<AgentConfig>`; derive `Serialize`/`Deserialize`
- [ ] `config_path() -> PathBuf` — `~/.config/stable/agents.toml` via `dirs`
- [ ] `Config::load() -> anyhow::Result<Config>` — create dir+file if absent, parse TOML; return empty config if file missing
- [ ] `Config::save(&self) -> anyhow::Result<()>` — serialize to TOML, write atomically

## Step 6: OpenCode adapter (`src/agents.rs` + `src/agents/opencode.rs`)
- [ ] `src/agents.rs`: declare `AgentAdapter` trait — `get_status`, `get_context`, `get_first_prompt`, `get_last_prompt` (all async); declare `pub mod opencode`
- [ ] `src/agents/opencode.rs`: `OpenCodeAdapter` struct with `port: u16`, `session_id: String`, `client: reqwest::Client`
- [ ] `find_free_port(from: u16) -> u16` — probe `TcpListener::bind("127.0.0.1:<port>")` upward from `from`
- [ ] `OpenCodeAdapter::create(dir: &str, name: &str) -> anyhow::Result<(OpenCodeAdapter, usize)>`:
  - find free port from 14100
  - `tmux::new_window(dir, name)` → window index → pane `stable:<idx>.0`
  - `tmux::send_keys(pane, "opencode --port N\n")`
  - poll `GET /global/health` every 200ms up to 25× until `{ healthy: true }`
  - `POST /session {}` → `{ id: session_id }`
  - return adapter + window index (on timeout return `Err`)
- [ ] `impl AgentAdapter for OpenCodeAdapter`:
  - `get_status`: `GET /session/status`, find `session_id` in map → `busy`/`retry` → `Running`, `idle` → `WaitingForInput`, connection refused → `Stopped`
  - `get_context`: fetch messages, config, provider; compute `used` from latest assistant message tokens; `total` from model context limit
  - `get_first_prompt`: `GET /session/{id}/message`, first user message → first `TextPart.text`
  - `get_last_prompt`: same endpoint, last user message → first `TextPart.text`

## Step 7: App state machine (`src/app.rs`)
- [ ] `AppState` enum: `Dashboard`, `CreateAgentDialog`, `AgentView(usize)`, `RemoveAgentDialog(usize)`
- [ ] `Event` enum: `Key(KeyEvent)`, `Resize(u16, u16)`, `DashboardTick`, `AgentViewTick`
- [ ] `App` struct: `agents: Vec<AgentEntry>`, `adapters: Vec<Box<dyn AgentAdapter>>`, `state: AppState`, `selected: usize`, `config: Config`
- [ ] Spawn crossterm event reader task → sends `Event::Key` / `Event::Resize` on channel
- [ ] Spawn dashboard ticker task → sends `Event::DashboardTick` every 500ms
- [ ] Spawn agent view ticker task → sends `Event::AgentViewTick` every 50ms
- [ ] Main event loop: `recv()` from channel, dispatch to handler based on current `AppState`
- [ ] Dashboard handlers:
  - `n` → `CreateAgentDialog`
  - `d` → `RemoveAgentDialog(selected)` (no-op if no agents)
  - `Enter` → `AgentView(selected)` (no-op if no agents)
  - `q` → exit loop
  - `←`/`→` → decrement/increment `selected`, wrap around
- [ ] `DashboardTick` handler: for each agent call `get_status`, `get_context`, `get_first_prompt`, `get_last_prompt`; update `agents[i].meta`
- [ ] `AgentViewTick` handler: call `tmux::capture_pane` for current agent, update `AgentViewState`; on first tick where status transitions to `Stopped`, set stopped overlay flag
- [ ] Agent view key handler:
  - stopped overlay visible: `d` → remove + save + Dashboard; `Ctrl-g` → Dashboard; all other keys suppressed
  - no overlay: `Ctrl-g` → Dashboard; `PgUp`/`PgDn` → adjust scroll offset; all other keys → `tmux::send_keys`
- [ ] CreateAgentDialog key handler: `Esc` → Dashboard; `Enter` → run creation flow; `↑`/`↓` → move field focus; `Tab` → autocomplete directory; chars/backspace → edit focused field
- [ ] RemoveAgentDialog key handler: `y`/`Enter` → remove agent + save config + Dashboard; `n`/`Esc` → Dashboard

## Step 8: Dashboard UI (`src/ui.rs` + `src/ui/dashboard.rs`)
- [ ] `src/ui.rs`: declare `pub mod dashboard`, `pub mod agent_view`, `pub mod create_agent`, `pub mod remove_agent`
- [ ] `src/ui/dashboard.rs`: `render_dashboard(f, area, agents, selected)` function
- [ ] Empty state: when `agents.is_empty()`, render centered "No agents. Press [n] to create one."
- [ ] `grid_dim(n: usize) -> usize`: `if n <= 4 { 2 } else if n <= 9 { 3 } else { 4 }`
- [ ] Compute equal column constraints (count = `grid_dim`) and equal row constraints
- [ ] Render each agent card as a `Block` with title = agent name, border highlighted if selected
- [ ] Card body lines: status symbol + status label, ctx used/total (formatted as `42k`), first prompt truncated, last prompt truncated, pane target
- [ ] Status symbols: `●` Running, `⏸` WaitingForInput, `■` Stopped, `?` Unknown
- [ ] Render empty grid slots as plain bordered blank `Block`
- [ ] Render keybindings bar at bottom: `[n] New  [d] Delete  [Enter] Open  [q] Quit`

## Step 9: Agent view UI (`src/ui/agent_view.rs`)
- [ ] `AgentViewState` struct: `lines: Vec<String>`, `scroll_offset: usize`, `last_refresh: Option<std::time::Instant>`, `show_stopped_overlay: bool`
- [ ] `update_lines(raw: &str)`: split `capture_pane` output on `\n`, store in `lines`; if `scroll_offset == 0` stay at bottom; else hold offset (clamped to new line count)
- [ ] `page_up(viewport_height: usize)`: increment `scroll_offset` by `viewport_height`, clamp to `lines.len().saturating_sub(viewport_height)`
- [ ] `page_down(viewport_height: usize)`: decrement `scroll_offset` by `viewport_height`, clamp to 0
- [ ] `render_agent_view(f, area, state, agent_entry)`:
  - split area into content area (all but last row) and status bar (last row)
  - compute visible line slice based on `scroll_offset` and content height
  - convert visible lines to `ratatui::Text` via `ansi_to_tui`
  - render in `Paragraph` widget (no scroll — slice is pre-computed)
  - render status bar: `pane: <id> | opencode | last refresh <HH:MM:SS>` + ` [scrolled]` when offset > 0
  - if `show_stopped_overlay`: render centered overlay block on top with "Agent stopped." and `[d] Remove agent   [Ctrl-g] Dashboard`

## Step 10: CreateAgentDialog UI (`src/ui/create_agent.rs`)
- [ ] `Field` enum: `Name`, `Directory`
- [ ] `CreateAgentState` struct: `name: String`, `directory: String`, `focus: Field`, `error: Option<String>`, `tab_matches: Vec<String>`, `tab_idx: usize`
- [ ] `render_create_agent(f, area, state)`: centered modal block with name input, directory input, agent label "● opencode", error line if `state.error.is_some()`, `[Enter] Launch  [Esc] Cancel` hint
- [ ] Highlight focused field's input box
- [ ] `handle_tab(state)`: derive parent dir from current `directory` value; `read_dir` parent; filter entries by current prefix; cycle `tab_idx` on repeated Tab; set `state.directory` to current match
- [ ] `is_valid(state) -> bool`: both `name` and `directory` non-empty
- [ ] Creation flow called from `app.rs` on Enter: `sanitize_name`, `OpenCodeAdapter::create(dir, sanitized_name)` → on success push entry + save config + transition to `AgentView`; on error set `state.error`

## Step 11: RemoveAgentDialog UI (`src/ui/remove_agent.rs`)
- [ ] `render_remove_agent(f, area, agent_name)`: centered single-line block: `Remove "<name>"? [y/Enter] confirm  [n/Esc] cancel`

## Step 12: Entry point (`src/main.rs`)
- [ ] `clap` `Cli` struct — no subcommands for MVP; running `stable` launches the TUI
- [ ] `#[tokio::main]` async main
- [ ] Call `tmux::ensure_session()`
- [ ] `Config::load()` → build `App` (reconstruct `OpenCodeAdapter` for each stored agent from `port` + `session_id`)
- [ ] Call `tui::run(app)`

## Step 13: Integration smoke test
- [ ] `cargo build` — zero errors, zero warnings
- [ ] Launch `stable` outside any tmux session; verify `stable` tmux session is created
- [ ] Press `[n]`, fill in name + directory, confirm; verify tmux window appears with opencode running
- [ ] Verify dashboard card renders correct name, status, pane
- [ ] Open AgentView with `[Enter]`; type a prompt; verify it reaches opencode in the tmux window
- [ ] Verify `PgUp` / `PgDn` scrolls captured output; verify `[scrolled]` indicator appears/disappears
- [ ] Verify `Ctrl-g` returns to dashboard
- [ ] Quit `stable` with `[q]`; verify tmux session and opencode window persist
- [ ] Relaunch `stable`; verify agent card is restored from `agents.toml`
- [ ] Remove agent with `[d]`; verify card disappears and `agents.toml` is updated
