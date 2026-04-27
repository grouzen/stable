use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub pane: String,
    pub agent_type: String,
    pub directory: String,
    pub port: u16,
    /// Last known session ID for this agent, persisted so the dashboard can
    /// show history immediately on startup without a global session lookup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Serialisable portion of the config (agents list only).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    agents: Vec<AgentConfig>,
}

/// Runtime config: the agents list plus the session name that determines
/// which file on disk this config is bound to.  The `session_name` field is
/// never written to disk.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub agents: Vec<AgentConfig>,
    /// The tmux session name this config belongs to.  Set by `load()`.
    pub session_name: String,
}

/// Path to the per-session config file:
///   `~/.config/stable/sessions/<session>.toml`
pub fn config_path(session: &str) -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("stable")
        .join("sessions")
        .join(format!("{}.toml", session))
}

impl Config {
    pub fn load(session: &str) -> anyhow::Result<Config> {
        let path = config_path(session);

        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("create config dir {:?}", parent))?;
            }
            return Ok(Config {
                agents: Vec::new(),
                session_name: session.to_string(),
            });
        }

        let content =
            std::fs::read_to_string(&path).with_context(|| format!("read config {:?}", path))?;

        let file: ConfigFile =
            toml::from_str(&content).with_context(|| format!("parse config {:?}", path))?;

        Ok(Config {
            agents: file.agents,
            session_name: session.to_string(),
        })
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = config_path(&self.session_name);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create config dir {:?}", parent))?;
        }

        let file = ConfigFile {
            agents: self.agents.clone(),
        };
        let content = toml::to_string_pretty(&file).context("serialize config to TOML")?;

        // Atomic write: write to a temp file then rename
        let tmp_path = path.with_extension("toml.tmp");
        std::fs::write(&tmp_path, content)
            .with_context(|| format!("write temp config {:?}", tmp_path))?;
        std::fs::rename(&tmp_path, &path)
            .with_context(|| format!("rename config {:?} -> {:?}", tmp_path, path))?;

        Ok(())
    }
}
