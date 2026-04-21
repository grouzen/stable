use anyhow::anyhow;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use std::net::TcpListener;
use std::sync::Mutex;
use tokio::time::{sleep, Duration};

use crate::agents::AgentAdapter;
use crate::models::{AgentStatus, ContextInfo};
use crate::tmux;

pub struct OpenCodeAdapter {
    pub port: u16,
    pub client: Client,
    /// Last session ID seen via `/session/status` for this instance.
    /// Used as a fallback when the agent is idle and `/session/status` returns empty.
    cached_session_id: Mutex<Option<String>>,
}

impl OpenCodeAdapter {
    pub fn new(port: u16, session_id: Option<String>) -> Self {
        Self {
            port,
            client: Client::new(),
            cached_session_id: Mutex::new(session_id),
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

        let adapter = OpenCodeAdapter { port, client, cached_session_id: Mutex::new(None) };
        Ok((adapter, window_index))
    }

    /// Resolves the session ID that opencode is currently using.
    ///
    /// 1. `/session/status` — port-scoped, used when the agent is active. Result is
    ///    cached so it persists while the agent is idle between turns.
    /// 2. Cache — populated from persisted config on startup, then kept up to date
    ///    from step 1. Ensures history is visible immediately after launch and
    ///    between turns, with no global session lookup that could leak another
    ///    agent's data.
    async fn resolve_session_id(&self) -> Option<String> {
        let status_url = format!("{}/session/status", self.base_url());
        if let Ok(resp) = self.client.get(&status_url).send().await {
            if let Ok(body) = resp.json::<Value>().await {
                if let Some(obj) = body.as_object() {
                    if !obj.is_empty() {
                        // Pick the session with the highest `time.updated`.
                        let id = obj
                            .keys()
                            .max_by_key(|id| {
                                obj[*id]
                                    .get("time")
                                    .and_then(|t| t.get("updated"))
                                    .and_then(Value::as_u64)
                                    .unwrap_or(0)
                            })
                            .map(|id| id.to_string());

                        if let Some(ref sid) = id {
                            if let Ok(mut cache) = self.cached_session_id.lock() {
                                *cache = Some(sid.clone());
                            }
                        }
                        return id;
                    }
                }
            }
        }

        // Agent is idle or not yet active — use the persisted/cached session.
        self.cached_session_id.lock().ok()?.clone()
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

impl OpenCodeAdapter {
    async fn resolve_context_total(&self, provider_id: &str, model_id: &str) -> Option<u64> {
        if provider_id.is_empty() || model_id.is_empty() {
            return None;
        }

        let providers_url = format!("{}/provider", self.base_url());
        let body: Value = self
            .client
            .get(&providers_url)
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;

        // Response is { "all": [ { "id": "...", "models": { "<model_id>": { ... } } } ] }
        let providers = body.get("all").and_then(Value::as_array)?;
        let provider = providers
            .iter()
            .find(|p: &&Value| p.get("id").and_then(Value::as_str) == Some(provider_id))?;

        // models is a map keyed by model id
        provider
            .get("models")
            .and_then(|m| m.get(model_id))
            .and_then(|m| m.get("limit"))
            .and_then(|l| l.get("context"))
            .and_then(Value::as_u64)
    }
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

        // Find the latest assistant message that has non-zero token usage.
        // In-flight messages exist but have zeroed token counts while streaming.
        let latest_assistant = messages
            .iter()
            .filter(|m: &&Value| msg_role(m) == Some("assistant"))
            .filter(|m: &&Value| {
                msg_tokens(m)
                    .map(|t| {
                        let input = t.get("input").and_then(Value::as_u64).unwrap_or(0);
                        let output = t.get("output").and_then(Value::as_u64).unwrap_or(0);
                        input > 0 || output > 0
                    })
                    .unwrap_or(false)
            })
            .max_by_key(|m: &&Value| msg_time_created(m))?;

        let tokens = msg_tokens(latest_assistant)?;
        let used = tokens.get("total").and_then(Value::as_u64).unwrap_or_else(|| {
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
            input + output + cache_read + cache_write
        });

        // modelID and providerID are on the message itself
        let provider_id = latest_assistant
            .get("info")
            .and_then(|i| i.get("providerID"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let model_id = latest_assistant
            .get("info")
            .and_then(|i| i.get("modelID"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        let total = self.resolve_context_total(&provider_id, &model_id).await;

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

    async fn get_last_model_response(&self) -> Option<String> {
        let messages: Vec<Value> = self.fetch_messages().await?;
        messages
            .into_iter()
            .filter(|m: &Value| msg_role(m) == Some("assistant"))
            .last()
            .and_then(|m| first_text_part(&m))
    }

    fn get_cached_session_id(&self) -> Option<String> {
        self.cached_session_id.lock().ok()?.clone()
    }
}
