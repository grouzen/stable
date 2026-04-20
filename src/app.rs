use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::time::{interval, Duration};

use crate::agents::opencode::OpenCodeAdapter;
use crate::agents::AgentAdapter;
use crate::config::{AgentConfig, Config};
use crate::models::{AgentEntry, AgentMeta, AgentStatus};
use crate::tmux;

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum AppState {
    Dashboard,
    CreateAgentDialog,
    AgentView(usize),
    RemoveAgentDialog(usize),
}

// ---------------------------------------------------------------------------
// Event
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum Event {
    Key(KeyEvent),
    DashboardTick,
    AgentViewTick,
}

// ---------------------------------------------------------------------------
// AgentViewState (owned by App, rendered by ui)
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct AgentViewState {
    pub lines: Vec<String>,
    pub last_refresh: Option<std::time::Instant>,
    pub show_stopped_overlay: bool,
    /// Cursor position within the pane's visible screen (col, row).
    pub cursor: Option<(u16, u16)>,
    /// Track previous status to detect edge transitions
    prev_status: Option<AgentStatus>,
}

impl AgentViewState {
    pub fn update_lines(&mut self, raw: &str) {
        let new_lines: Vec<String> = raw.trim_end_matches('\n').split('\n').map(|s| s.to_string()).collect();
        self.lines = new_lines;
        self.last_refresh = Some(std::time::Instant::now());
    }
}

// ---------------------------------------------------------------------------
// CreateAgentState
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum CreateField {
    Name,
    Directory,
}

#[derive(Debug)]
pub struct CreateAgentState {
    pub name: String,
    pub directory: String,
    pub focus: CreateField,
    pub error: Option<String>,
    pub tab_matches: Vec<String>,
    pub tab_idx: usize,
}

impl Default for CreateAgentState {
    fn default() -> Self {
        Self {
            name: String::new(),
            directory: String::new(),
            focus: CreateField::Name,
            error: None,
            tab_matches: Vec::new(),
            tab_idx: 0,
        }
    }
}

impl CreateAgentState {
    pub fn is_valid(&self) -> bool {
        !self.name.trim().is_empty() && !self.directory.trim().is_empty()
    }

    /// Perform Tab completion on the Directory field.
    pub fn handle_tab(&mut self) {
        let current = self.directory.clone();
        let path = std::path::Path::new(&current);

        let (parent, prefix) = if current.ends_with('/') || current.is_empty() {
            (
                if current.is_empty() {
                    std::path::PathBuf::from(".")
                } else {
                    std::path::PathBuf::from(&current)
                },
                String::new(),
            )
        } else {
            let p = path.parent().unwrap_or(std::path::Path::new("."));
            let fname = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            (p.to_path_buf(), fname)
        };

        // If no matches cached (or prefix changed), rebuild
        if self.tab_matches.is_empty() {
            if let Ok(rd) = std::fs::read_dir(&parent) {
                let mut matches: Vec<String> = rd
                    .flatten()
                    .filter_map(|e| {
                        let name = e.file_name().into_string().ok()?;
                        if name.starts_with(&prefix) && e.file_type().ok()?.is_dir() {
                            let mut full = parent.join(&name);
                            // Canonicalize for nicer display
                            if let Ok(c) = full.canonicalize() {
                                full = c;
                            }
                            Some(full.to_string_lossy().to_string())
                        } else {
                            None
                        }
                    })
                    .collect();
                matches.sort();
                self.tab_matches = matches;
                self.tab_idx = 0;
            }
        } else {
            self.tab_idx = (self.tab_idx + 1) % self.tab_matches.len().max(1);
        }

        if let Some(m) = self.tab_matches.get(self.tab_idx) {
            self.directory = m.clone();
        }
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

pub struct App {
    pub agents: Vec<AgentEntry>,
    pub adapters: Vec<Box<dyn AgentAdapter>>,
    pub state: AppState,
    pub selected: usize,
    pub config: Config,
    pub agent_view_state: AgentViewState,
    pub create_state: CreateAgentState,
    pub tx: UnboundedSender<Event>,
    pub rx: UnboundedReceiver<Event>,
}

impl App {
    pub fn new(config: Config, agents: Vec<AgentEntry>, adapters: Vec<Box<dyn AgentAdapter>>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            agents,
            adapters,
            state: AppState::Dashboard,
            selected: 0,
            config,
            agent_view_state: AgentViewState::default(),
            create_state: CreateAgentState::default(),
            tx,
            rx,
        }
    }

    /// Spawn background tasks (crossterm events, dashboard ticker, agent view ticker).
    pub fn spawn_tasks(&self) {
        // Crossterm event reader
        let tx = self.tx.clone();
        tokio::spawn(async move {
            use crossterm::event::{Event as CEvent, EventStream};
            use futures::StreamExt;
            let mut stream = EventStream::new();
            while let Some(Ok(event)) = stream.next().await {
                match event {
                    CEvent::Key(k) => {
                        let _ = tx.send(Event::Key(k));
                    }

                    _ => {}
                }
            }
        });

        // Dashboard ticker — 500 ms
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_millis(500));
            loop {
                ticker.tick().await;
                let _ = tx.send(Event::DashboardTick);
            }
        });

        // AgentView ticker — 50 ms
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_millis(50));
            loop {
                ticker.tick().await;
                let _ = tx.send(Event::AgentViewTick);
            }
        });
    }

    // -----------------------------------------------------------------------
    // Event dispatch
    // -----------------------------------------------------------------------

    /// Returns false when the app should quit.
    pub async fn handle_event(&mut self, event: Event) -> bool {
        match event {
            Event::Key(key) => self.handle_key(key).await,
            Event::DashboardTick => {
                self.handle_dashboard_tick().await;
                true
            }
            Event::AgentViewTick => {
                self.handle_agent_view_tick().await;
                true
            }
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) -> bool {
        match &self.state.clone() {
            AppState::Dashboard => self.handle_dashboard_key(key),
            AppState::AgentView(idx) => {
                let idx = *idx;
                self.handle_agent_view_key(key, idx).await
            }
            AppState::CreateAgentDialog => self.handle_create_key(key).await,
            AppState::RemoveAgentDialog(idx) => {
                let idx = *idx;
                self.handle_remove_key(key, idx)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Dashboard key handler
    // -----------------------------------------------------------------------

    fn handle_dashboard_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('q') => return false,
            KeyCode::Char('n') => {
                self.create_state = CreateAgentState::default();
                self.state = AppState::CreateAgentDialog;
            }
            KeyCode::Char('d') => {
                if !self.agents.is_empty() {
                    self.state = AppState::RemoveAgentDialog(self.selected);
                }
            }
            KeyCode::Enter => {
                if !self.agents.is_empty() {
                    self.agent_view_state = AgentViewState::default();
                    self.state = AppState::AgentView(self.selected);
                }
            }
            KeyCode::Left => {
                if !self.agents.is_empty() {
                    if self.selected == 0 {
                        self.selected = self.agents.len() - 1;
                    } else {
                        self.selected -= 1;
                    }
                }
            }
            KeyCode::Right => {
                if !self.agents.is_empty() {
                    self.selected = (self.selected + 1) % self.agents.len();
                }
            }
            _ => {}
        }
        true
    }

    // -----------------------------------------------------------------------
    // Dashboard tick — poll all agents
    // -----------------------------------------------------------------------

    async fn handle_dashboard_tick(&mut self) {
        let len = self.adapters.len();
        for i in 0..len {
            let status = self.adapters[i].get_status().await;
            let context = self.adapters[i].get_context().await;
            let first_prompt = self.adapters[i].get_first_prompt().await;
            let last_prompt = self.adapters[i].get_last_prompt().await;
            if let Some(entry) = self.agents.get_mut(i) {
                entry.meta.status = status;
                entry.meta.context = context;
                entry.meta.first_prompt = first_prompt;
                entry.meta.last_prompt = last_prompt;
            }
        }
    }

    // -----------------------------------------------------------------------
    // AgentView tick — capture pane, detect stopped
    // -----------------------------------------------------------------------

    async fn handle_agent_view_tick(&mut self) {
        let idx = match &self.state {
            AppState::AgentView(i) => *i,
            _ => return,
        };

        if let Some(entry) = self.agents.get(idx) {
            let pane = entry.config.pane.clone();
            if let Ok(raw) = tmux::capture_pane(&pane) {
                self.agent_view_state.update_lines(&raw);
            }
            self.agent_view_state.cursor = tmux::cursor_position(&pane);

            // If the pane is no longer alive, immediately mark as Stopped
            if !tmux::is_alive(&pane) {
                let prev = self.agent_view_state.prev_status.clone();
                if prev.as_ref() != Some(&AgentStatus::Stopped) {
                    self.agent_view_state.show_stopped_overlay = true;
                }
                self.agent_view_state.prev_status = Some(AgentStatus::Stopped.clone());
                if let Some(e) = self.agents.get_mut(idx) {
                    e.meta.status = AgentStatus::Stopped;
                }
                return;
            }

            // Update status via adapter
            if let Some(adapter) = self.adapters.get(idx) {
                let new_status = adapter.get_status().await;
                let prev = self.agent_view_state.prev_status.clone();
                // Detect edge transition to Stopped
                if new_status == AgentStatus::Stopped
                    && prev.as_ref() != Some(&AgentStatus::Stopped)
                {
                    self.agent_view_state.show_stopped_overlay = true;
                }
                self.agent_view_state.prev_status = Some(new_status.clone());
                if let Some(e) = self.agents.get_mut(idx) {
                    e.meta.status = new_status;
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // AgentView key handler
    // -----------------------------------------------------------------------

    async fn handle_agent_view_key(&mut self, key: KeyEvent, idx: usize) -> bool {
        if self.agent_view_state.show_stopped_overlay {
            match key.code {
                KeyCode::Char('d') => {
                    self.remove_agent(idx);
                    self.state = AppState::Dashboard;
                }
                KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.agent_view_state.show_stopped_overlay = false;
                    self.state = AppState::Dashboard;
                }
                _ => {}
            }
            return true;
        }

        match key.code {
            KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.state = AppState::Dashboard;
            }
            _ => {
                // Forward key to tmux pane
                if let Some(entry) = self.agents.get(idx) {
                    let pane = entry.config.pane.clone();
                    let keys = key_event_to_tmux(&key);
                    if !keys.is_empty() {
                        let _ = tmux::send_keys(&pane, &keys);
                    }
                }
            }
        }
        true
    }

    // -----------------------------------------------------------------------
    // CreateAgentDialog key handler
    // -----------------------------------------------------------------------

    async fn handle_create_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.state = AppState::Dashboard;
            }
            KeyCode::Tab => {
                if self.create_state.focus == CreateField::Directory {
                    self.create_state.handle_tab();
                }
            }
            KeyCode::Up => {
                self.create_state.focus = CreateField::Name;
            }
            KeyCode::Down => {
                self.create_state.focus = CreateField::Directory;
            }
            KeyCode::Enter => {
                if self.create_state.is_valid() {
                    let name = tmux::sanitize_name(&self.create_state.name.clone());
                    let dir = self.create_state.directory.clone();
                    match OpenCodeAdapter::create(&dir, &name).await {
                        Ok((adapter, window_index)) => {
                            let pane = format!("stable:{}.0", window_index);
                            let config = AgentConfig {
                                name: name.clone(),
                                pane: pane.clone(),
                                agent_type: "opencode".to_string(),
                                directory: dir,
                                port: adapter.port,
                                session_id: adapter.session_id.clone(),
                            };
                            self.config.agents.push(config.clone());
                            let _ = self.config.save();
                            let entry = AgentEntry {
                                config,
                                meta: AgentMeta::default(),
                            };
                            self.agents.push(entry);
                            self.adapters.push(Box::new(adapter));
                            let new_idx = self.agents.len() - 1;
                            self.selected = new_idx;
                            self.agent_view_state = AgentViewState::default();
                            self.state = AppState::AgentView(new_idx);
                        }
                        Err(e) => {
                            self.create_state.error = Some(e.to_string());
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                match self.create_state.focus {
                    CreateField::Name => {
                        self.create_state.name.pop();
                    }
                    CreateField::Directory => {
                        self.create_state.directory.pop();
                        // Invalidate tab matches when user edits
                        self.create_state.tab_matches.clear();
                        self.create_state.tab_idx = 0;
                    }
                }
            }
            KeyCode::Char(c) => {
                match self.create_state.focus {
                    CreateField::Name => {
                        self.create_state.name.push(c);
                    }
                    CreateField::Directory => {
                        self.create_state.directory.push(c);
                        // Invalidate tab matches when user edits
                        self.create_state.tab_matches.clear();
                        self.create_state.tab_idx = 0;
                    }
                }
            }
            _ => {}
        }
        true
    }

    // -----------------------------------------------------------------------
    // RemoveAgentDialog key handler
    // -----------------------------------------------------------------------

    fn handle_remove_key(&mut self, key: KeyEvent, idx: usize) -> bool {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                self.remove_agent(idx);
                self.state = AppState::Dashboard;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.state = AppState::Dashboard;
            }
            _ => {}
        }
        true
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn remove_agent(&mut self, idx: usize) {
        if idx < self.agents.len() {
            self.agents.remove(idx);
            self.adapters.remove(idx);
            self.config.agents.remove(idx);
            let _ = self.config.save();
            // Adjust selected if needed
            if self.selected >= self.agents.len() && !self.agents.is_empty() {
                self.selected = self.agents.len() - 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Key → tmux string conversion
// ---------------------------------------------------------------------------

fn key_event_to_tmux(key: &KeyEvent) -> String {
    // Ctrl combos
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(c) = key.code {
            return format!("C-{}", c);
        }
    }
    match key.code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Backspace => "BSpace".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::Esc => "Escape".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::PageUp => "PPage".to_string(),
        KeyCode::PageDown => "NPage".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::Delete => "DC".to_string(),
        _ => String::new(),
    }
}
