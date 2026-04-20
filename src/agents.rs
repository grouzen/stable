use async_trait::async_trait;
use crate::models::{AgentStatus, ContextInfo};

#[async_trait]
pub trait AgentAdapter: Send + Sync {
    async fn get_status(&self) -> AgentStatus;
    async fn get_context(&self) -> Option<ContextInfo>;
    async fn get_first_prompt(&self) -> Option<String>;
    async fn get_last_prompt(&self) -> Option<String>;
}

pub mod opencode;
