mod agents;
mod app;
mod config;
mod models;
mod tmux;
mod tui;
mod ui;

use anyhow::Result;
use clap::Parser;

use agents::opencode::OpenCodeAdapter;
use agents::AgentAdapter;
use app::App;
use config::Config;
use models::{AgentEntry, AgentMeta, AgentStatus};

/// stable — multi-agent TUI dashboard
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI (no subcommands for MVP — just launch the TUI)
    let _cli = Cli::parse();

    // Ensure the stable tmux session exists (starts the server if needed)
    tmux::ensure_session()?;

    // Load persisted config
    let config = Config::load()?;

    // Reconstruct agents and adapters from stored config
    let mut agents: Vec<AgentEntry> = Vec::new();
    let mut adapters: Vec<Box<dyn AgentAdapter>> = Vec::new();

    for agent_config in &config.agents {
        let adapter = OpenCodeAdapter::new(agent_config.port, agent_config.session_id.clone());
        agents.push(AgentEntry {
            config: agent_config.clone(),
            meta: AgentMeta {
                status: AgentStatus::Unknown,
                context: None,
                first_prompt: None,
                last_prompt: None,
            },
        });
        adapters.push(Box::new(adapter));
    }

    // Build App and spawn background tasks
    let mut app = App::new(config, agents, adapters);
    app.spawn_tasks();

    tui::run(|mut terminal| async move {
        loop {
            // Draw only when state has changed since the last frame.
            if app.dirty {
                app.dirty = false;
                let state = app.state.clone();
                terminal.draw(|f| {
                    let area = f.area();
                    match &state {
                        app::AppState::Dashboard => {
                            ui::dashboard::render_dashboard(f, area, &app.agents, app.selected);
                        }
                        app::AppState::AgentView(idx) => {
                            if let Some(entry) = app.agents.get(*idx) {
                                // Content area is full area minus 1 row for the status bar.
                                // Resize the tmux window to match so capture_pane fills the viewport.
                                let content_height = area.height.saturating_sub(1);
                                let _ = tmux::resize_window(&entry.config.pane, area.width, content_height);
                                ui::agent_view::render_agent_view(
                                    f,
                                    area,
                                    &app.agent_view_state,
                                    entry,
                                );
                            }
                        }
                        app::AppState::CreateAgentDialog => {
                            ui::dashboard::render_dashboard(f, area, &app.agents, app.selected);
                            ui::create_agent::render_create_agent(f, area, &app.create_state);
                        }
                        app::AppState::RemoveAgentDialog(idx) => {
                            ui::dashboard::render_dashboard(f, area, &app.agents, app.selected);
                            let name = app
                                .agents
                                .get(*idx)
                                .map(|e| e.config.name.as_str())
                                .unwrap_or("");
                            ui::remove_agent::render_remove_agent(f, area, name);
                        }
                    }
                })?;
            }

            // Wait for next event and dispatch
            let should_continue = if let Some(event) = app.rx.recv().await {
                app.handle_event(event).await
            } else {
                false
            };

            if !should_continue {
                break;
            }
        }
        Ok(())
    })
    .await
}
