use anyhow::{anyhow, Context};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use std::net::TcpListener;
use tokio::time::{sleep, Duration};

use crate::agents::AgentAdapter;
use crate::models::{AgentStatus, ContextInfo};
use crate::tmux;

pub struct OpenCodeAdapter {
    pub port: u16,
    pub session_id: String,
    pub client: Client,
}

impl OpenCodeAdapter {
    pub fn new(port: u16, session_id: String) -> Self {
        Self {
            port,
            session_id,
            client: Client::new(),
        }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Creates a new opencode agent: allocates port, opens tmux window, launches opencode,
    /// waits for health, creates session. Returns (adapter, window_index).
    pub async fn create(dir: &str, name: &str) -> anyhow::Result<(OpenCodeAdapter, usize)> {
        let port = find_free_port(14100);
        let window_index = tmux::new_window(dir, name)?;
        let pane = format!("stable:{}.0", window_index);
        tmux::send_keys(&pane, &format!("opencode --port {}\n", port))?;

        let client = Client::new();
        let health_url = format!("http://127.0.0.1:{}/global/health", port);

        let mut healthy = false;
        for _ in 0..25 {
            sleep(Duration::from_millis(200)).await;
            if let Ok(resp) = client.get(&health_url).send().await {
                if let Ok(body) = resp.json::<Value>().await {
                    if body.get("healthy")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        healthy = true;
                        break;
                    }
                }
            }
        }

        if !healthy {
            return Err(anyhow!("opencode did not become healthy within timeout"));
        }

        let session_url = format!("http://127.0.0.1:{}/session", port);
        let resp = client
            .post(&session_url)
            .json(&serde_json::json!({}))
            .send()
            .await
            .context("POST /session")?;
        let body: Value = resp.json().await.context("parse /session response")?;
        let session_id = body
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("missing id in /session response"))?
            .to_string();

        let adapter = OpenCodeAdapter {
            port,
            session_id,
            client,
        };
        Ok((adapter, window_index))
    }

    async fn fetch_messages(&self) -> Option<Vec<Value>> {
        let url = format!("{}/session/{}/message", self.base_url(), self.session_id);
        self.client
            .get(&url)
            .send()
            .await
            .ok()?
            .json::<Vec<Value>>()
            .await
            .ok()
    }
}

pub fn find_free_port(from: u16) -> u16 {
    let mut port = from;
    loop {
        if TcpListener::bind(format!("127.0.0.1:{}", port)).is_ok() {
            return port;
        }
        port += 1;
    }
}

fn first_text_part(msg: &Value) -> Option<String> {
    let parts = msg.get("parts")?.as_array()?;
    parts.iter().find_map(|p: &Value| {
        if p.get("type").and_then(Value::as_str) == Some("text") {
            p.get("text").and_then(Value::as_str).map(|s| s.to_string())
        } else {
            None
        }
    })
}

#[async_trait]
impl AgentAdapter for OpenCodeAdapter {
    async fn get_status(&self) -> AgentStatus {
        let url = format!("{}/session/status", self.base_url());
        let resp = match self.client.get(&url).send().await {
            Ok(r) => r,
            Err(_) => return AgentStatus::Stopped,
        };

        let body: Value = match resp.json().await {
            Ok(v) => v,
            Err(_) => return AgentStatus::Unknown,
        };

        // Response is a map keyed by session_id
        if let Some(obj) = body.as_object() {
            if let Some(entry) = obj.get(&self.session_id) {
                let status = entry
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                return match status {
                    "busy" | "retry" => AgentStatus::Running,
                    "idle" => AgentStatus::WaitingForInput,
                    _ => AgentStatus::Unknown,
                };
            }
        }

        AgentStatus::Unknown
    }

    async fn get_context(&self) -> Option<ContextInfo> {
        let messages: Vec<Value> = self.fetch_messages().await?;

        // Find latest assistant message by time.created
        let latest_assistant = messages
            .iter()
            .filter(|m: &&Value| m.get("role").and_then(Value::as_str) == Some("assistant"))
            .max_by_key(|m: &&Value| {
                m.get("time")
                    .and_then(|t: &Value| t.get("created"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
            })?;

        let tokens = latest_assistant.get("tokens")?;
        let input = tokens.get("input").and_then(Value::as_u64).unwrap_or(0);
        let output = tokens.get("output").and_then(Value::as_u64).unwrap_or(0);
        let cache_read = tokens
            .get("cache")
            .and_then(|c: &Value| c.get("read"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let cache_write = tokens
            .get("cache")
            .and_then(|c: &Value| c.get("write"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let used = input + output + cache_read + cache_write;

        // Get model from config
        let config_url = format!("{}/config", self.base_url());
        let config: Value = self
            .client
            .get(&config_url)
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;

        let model_str = config.get("model").and_then(Value::as_str)?;
        // e.g. "anthropic/claude-sonnet-4-5"
        let mut parts = model_str.splitn(2, '/');
        let provider_id = parts.next()?;
        let model_id = parts.next()?;

        // Get providers
        let providers_url = format!("{}/provider", self.base_url());
        let providers: Vec<Value> = self
            .client
            .get(&providers_url)
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;

        let provider = providers
            .iter()
            .find(|p: &&Value| p.get("id").and_then(Value::as_str) == Some(provider_id))?;

        let models = provider.get("models").and_then(Value::as_array)?;
        let model = models.iter().find(|m: &&Value| {
            m.get("id").and_then(Value::as_str) == Some(model_id)
        })?;

        let total = model
            .get("limit")
            .and_then(|l: &Value| l.get("context"))
            .and_then(Value::as_u64)?;

        Some(ContextInfo { used, total })
    }

    async fn get_first_prompt(&self) -> Option<String> {
        let messages: Vec<Value> = self.fetch_messages().await?;
        messages
            .into_iter()
            .find(|m: &Value| m.get("role").and_then(Value::as_str) == Some("user"))
            .and_then(|m| first_text_part(&m))
    }

    async fn get_last_prompt(&self) -> Option<String> {
        let messages: Vec<Value> = self.fetch_messages().await?;
        messages
            .into_iter()
            .filter(|m: &Value| m.get("role").and_then(Value::as_str) == Some("user"))
            .last()
            .and_then(|m| first_text_part(&m))
    }
}
