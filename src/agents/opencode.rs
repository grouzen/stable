use anyhow::anyhow;
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
    pub client: Client,
}

impl OpenCodeAdapter {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            client: Client::new(),
        }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Creates a new opencode agent: allocates port, opens tmux window, launches opencode,
    /// waits for health. Returns (adapter, window_index).
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

        let adapter = OpenCodeAdapter { port, client };
        Ok((adapter, window_index))
    }

    /// Resolves the session ID that opencode is currently using.
    ///
    /// Strategy:
    /// 1. Check `/session/status` — if any session is actively tracked there, use the
    ///    one with the highest `time.updated` among those sessions.
    /// 2. Fall back to the most recently updated session from `GET /session`.
    async fn resolve_session_id(&self) -> Option<String> {
        // Step 1: check active sessions in /session/status
        let status_url = format!("{}/session/status", self.base_url());
        if let Ok(resp) = self.client.get(&status_url).send().await {
            if let Ok(body) = resp.json::<Value>().await {
                if let Some(obj) = body.as_object() {
                    if !obj.is_empty() {
                        // Return the only / most recently active session ID
                        // (usually just one entry)
                        let ids: Vec<&str> = obj.keys().map(String::as_str).collect();
                        if ids.len() == 1 {
                            return Some(ids[0].to_string());
                        }
                        // Multiple active sessions — pick the most recently updated
                        if let Some(sid) = self.most_recently_updated_among(ids).await {
                            return Some(sid);
                        }
                    }
                }
            }
        }

        // Step 2: fall back to most recently updated session overall
        self.most_recently_updated_session().await
    }

    /// Among the given session IDs, returns the one with the highest `time.updated`.
    async fn most_recently_updated_among(&self, ids: Vec<&str>) -> Option<String> {
        let sessions = self.fetch_all_sessions().await?;
        sessions
            .into_iter()
            .filter(|s| {
                if let Some(id) = s.get("id").and_then(Value::as_str) {
                    ids.contains(&id)
                } else {
                    false
                }
            })
            .max_by_key(|s| {
                s.get("time")
                    .and_then(|t| t.get("updated"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
            })
            .and_then(|s| s.get("id").and_then(Value::as_str).map(str::to_string))
    }

    async fn most_recently_updated_session(&self) -> Option<String> {
        self.fetch_all_sessions()
            .await?
            .into_iter()
            .max_by_key(|s| {
                s.get("time")
                    .and_then(|t| t.get("updated"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
            })
            .and_then(|s| s.get("id").and_then(Value::as_str).map(str::to_string))
    }

    async fn fetch_all_sessions(&self) -> Option<Vec<Value>> {
        let url = format!("{}/session", self.base_url());
        self.client
            .get(&url)
            .send()
            .await
            .ok()?
            .json::<Vec<Value>>()
            .await
            .ok()
    }

    async fn fetch_messages(&self) -> Option<Vec<Value>> {
        let session_id = self.resolve_session_id().await?;
        let url = format!("{}/session/{}/message", self.base_url(), session_id);
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

fn msg_role(msg: &Value) -> Option<&str> {
    msg.get("info")?.get("role")?.as_str()
}

fn msg_time_created(msg: &Value) -> u64 {
    msg.get("info")
        .and_then(|i| i.get("time"))
        .and_then(|t| t.get("created"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

fn msg_tokens(msg: &Value) -> Option<&Value> {
    msg.get("info")?.get("tokens")
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

        // Response is a map keyed by session_id. Pick the "most active" status
        // across all tracked sessions (busy > idle > unknown).
        if let Some(obj) = body.as_object() {
            if obj.is_empty() {
                return AgentStatus::WaitingForInput;
            }
            let mut best = AgentStatus::Unknown;
            for entry in obj.values() {
                let s = entry.get("status").and_then(Value::as_str).unwrap_or("");
                let candidate = match s {
                    "busy" | "retry" => AgentStatus::Running,
                    "idle" => AgentStatus::WaitingForInput,
                    _ => AgentStatus::Unknown,
                };
                if candidate == AgentStatus::Running {
                    return AgentStatus::Running; // can't do better
                }
                if candidate == AgentStatus::WaitingForInput {
                    best = AgentStatus::WaitingForInput;
                }
            }
            return best;
        }

        AgentStatus::Unknown
    }

    async fn get_context(&self) -> Option<ContextInfo> {
        let messages: Vec<Value> = self.fetch_messages().await?;

        // Find latest assistant message by info.time.created
        let latest_assistant = messages
            .iter()
            .filter(|m: &&Value| msg_role(m) == Some("assistant"))
            .max_by_key(|m: &&Value| msg_time_created(m))?;

        let tokens = msg_tokens(latest_assistant)?;
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
            .find(|m: &Value| msg_role(m) == Some("user"))
            .and_then(|m| first_text_part(&m))
    }

    async fn get_last_prompt(&self) -> Option<String> {
        let messages: Vec<Value> = self.fetch_messages().await?;
        messages
            .into_iter()
            .filter(|m: &Value| msg_role(m) == Some("user"))
            .last()
            .and_then(|m| first_text_part(&m))
    }
}
