use crate::config::AgentConfig;

#[derive(Debug, Clone, PartialEq)]
pub enum AgentStatus {
    Running,
    WaitingForInput,
    Stopped,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ContextInfo {
    pub used: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct AgentMeta {
    pub status: AgentStatus,
    pub context: Option<ContextInfo>,
    pub first_prompt: Option<String>,
    pub last_prompt: Option<String>,
}

impl Default for AgentMeta {
    fn default() -> Self {
        Self {
            status: AgentStatus::Unknown,
            context: None,
            first_prompt: None,
            last_prompt: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentEntry {
    pub config: AgentConfig,
    pub meta: AgentMeta,
}
