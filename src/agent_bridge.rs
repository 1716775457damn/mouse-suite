use crate::common::data_dir;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize)]
pub struct AgentCommand {
    pub id: String,
    pub action: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Serialize)]
pub struct AgentResponse {
    pub id: String,
    pub ok: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

pub struct AgentBridge {
    command_path: PathBuf,
    response_path: PathBuf,
    last_seen_id: Option<String>,
    last_poll_at: Instant,
}

impl AgentBridge {
    pub fn new() -> Self {
        let base = data_dir();
        let _ = fs::create_dir_all(base);
        Self {
            command_path: base.join("agent_command.json"),
            response_path: base.join("agent_response.json"),
            last_seen_id: None,
            last_poll_at: Instant::now() - Duration::from_secs(1),
        }
    }

    pub fn command_path(&self) -> &PathBuf {
        &self.command_path
    }

    pub fn poll(&mut self) -> Option<AgentCommand> {
        if self.last_poll_at.elapsed() < Duration::from_millis(350) {
            return None;
        }
        self.last_poll_at = Instant::now();

        let raw = fs::read_to_string(&self.command_path).ok()?;
        let cmd: AgentCommand = serde_json::from_str(&raw).ok()?;
        if self.last_seen_id.as_deref() == Some(cmd.id.as_str()) {
            return None;
        }
        self.last_seen_id = Some(cmd.id.clone());
        Some(cmd)
    }

    pub fn write_response(&self, resp: &AgentResponse) -> Result<(), String> {
        let txt = serde_json::to_string_pretty(resp).map_err(|e| e.to_string())?;
        fs::write(&self.response_path, txt).map_err(|e| e.to_string())
    }
}
