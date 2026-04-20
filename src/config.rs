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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub agents: Vec<AgentConfig>,
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("stable")
        .join("agents.toml")
}

impl Config {
    pub fn load() -> anyhow::Result<Config> {
        let path = config_path();

        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("create config dir {:?}", parent))?;
            }
            return Ok(Config::default());
        }

        let content =
            std::fs::read_to_string(&path).with_context(|| format!("read config {:?}", path))?;

        let config: Config =
            toml::from_str(&content).with_context(|| format!("parse config {:?}", path))?;

        Ok(config)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = config_path();

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create config dir {:?}", parent))?;
        }

        let content = toml::to_string_pretty(self).context("serialize config to TOML")?;

        // Atomic write: write to a temp file then rename
        let tmp_path = path.with_extension("toml.tmp");
        std::fs::write(&tmp_path, content)
            .with_context(|| format!("write temp config {:?}", tmp_path))?;
        std::fs::rename(&tmp_path, &path)
            .with_context(|| format!("rename config {:?} -> {:?}", tmp_path, path))?;

        Ok(())
    }
}
