use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub pane: String,
    pub agent_type: String,
    pub directory: String,
    pub port: u16,
    pub session_id: String,
}
