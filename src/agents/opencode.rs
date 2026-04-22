use anyhow::anyhow;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use std::net::TcpListener;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::time::sleep;

use crate::agents::AgentAdapter;
use crate::models::{AgentStatus, ContextInfo};
use crate::tmux;

/// How long a `TickCache` entry is considered fresh.
/// All `get_*` calls within a single 500 ms dashboard tick fire within a few
/// milliseconds of each other, so 400 ms is safely within one tick while
/// guaranteeing the cache is always stale before the next tick fires.
const TICK_CACHE_TTL: Duration = Duration::from_millis(400);

/// Data fetched in a single pass and shared across all `get_*` calls that
/// occur within the same dashboard tick.
struct TickCache {
    fetched_at: Instant,
    status: AgentStatus,
    messages: Option<Vec<Value>>,
}

pub struct OpenCodeAdapter {
    pub port: u16,
    pub client: Client,
    /// Long-lived session ID cache — persists across ticks so history is
    /// visible while the agent is idle between turns.
    cached_session_id: Mutex<Option<String>>,
    /// Short-lived per-tick cache — expires after `TICK_CACHE_TTL` so that
    /// repeated `get_*` calls within the same tick share a single HTTP
    /// round-trip without any external coordination.
    tick_cache: Mutex<Option<TickCache>>,
}

impl OpenCodeAdapter {
    pub fn new(port: u16, session_id: Option<String>) -> Self {
        Self {
            port,
            client: Client::new(),
            cached_session_id: Mutex::new(session_id),
            tick_cache: Mutex::new(None),
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
            sleep(tokio::time::Duration::from_millis(200)).await;
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

        let adapter = OpenCodeAdapter {
            port,
            client,
            cached_session_id: Mutex::new(None),
            tick_cache: Mutex::new(None),
        };
        Ok((adapter, window_index))
    }

    /// Ensures the per-tick cache is populated and fresh.
    ///
    /// If the cache was filled within the last `TICK_CACHE_TTL`, this is a
    /// no-op. Otherwise it issues exactly one `GET /session/status` and (when
    /// a session ID is known) one `GET /session/{id}/message`, storing the
    /// results for reuse by subsequent `get_*` calls in the same tick.
    async fn ensure_tick_cache(&self) {
        // Check whether the existing cache is still fresh — if so, nothing to do.
        {
            let guard = self.tick_cache.lock().unwrap();
            if let Some(ref c) = *guard {
                if c.fetched_at.elapsed() < TICK_CACHE_TTL {
                    return;
                }
            }
        }

        // --- fetch status + session ID ---
        let status_url = format!("{}/session/status", self.base_url());
        let status_body: Option<Value> = match self.client.get(&status_url).send().await {
            Ok(resp) => resp.json().await.ok(),
            Err(_) => None,
        };

        let (session_id, status) = match status_body.as_ref().and_then(Value::as_object) {
            None => (None, AgentStatus::Stopped),
            Some(obj) if obj.is_empty() => {
                // Agent is idle — fall back to the long-lived cached session ID.
                let sid = self.cached_session_id.lock().unwrap().clone();
                (sid, AgentStatus::WaitingForInput)
            }
            Some(obj) => {
                // Pick the session with the most recent `time.updated`.
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

                let mut best = AgentStatus::Unknown;
                for entry in obj.values() {
                    match entry.get("type").or_else(|| entry.get("status")).and_then(Value::as_str).unwrap_or("") {
                        "busy" | "retry" | "running" => { best = AgentStatus::Running; break; }
                        "idle" | "waiting" => { best = AgentStatus::WaitingForInput; }
                        _ => {}
                    }
                }
                (id, best)
            }
        };

        // Keep the long-lived session ID cache up to date.
        if let Some(ref sid) = session_id {
            *self.cached_session_id.lock().unwrap() = Some(sid.clone());
        }

        // --- fetch messages ---
        let messages: Option<Vec<Value>> = match &session_id {
            None => None,
            Some(sid) => {
                let url = format!("{}/session/{}/message", self.base_url(), sid);
                match self.client.get(&url).send().await {
                    Ok(resp) => resp.json().await.ok(),
                    Err(_) => None,
                }
            }
        };

        *self.tick_cache.lock().unwrap() = Some(TickCache {
            fetched_at: Instant::now(),
            status,
            messages,
        });
    }

    /// Returns the full message list for the current session, using the tick
    /// cache when fresh.
    async fn fetch_messages(&self) -> Option<Vec<Value>> {
        self.ensure_tick_cache().await;
        self.tick_cache.lock().unwrap()
            .as_ref()
            .and_then(|c| c.messages.clone())
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
        self.ensure_tick_cache().await;
        self.tick_cache.lock().unwrap()
            .as_ref()
            .map(|c| c.status.clone())
            .unwrap_or(AgentStatus::Stopped)
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
        self.cached_session_id.lock().unwrap().clone()
    }
}
