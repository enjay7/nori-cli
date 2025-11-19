//! ACP Model Client implementation
//!
//! Provides AcpModelClient for communicating with ACP-compliant agent subprocesses.

use crate::protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use crate::AgentProcess;
use anyhow::{Context, Result};
use futures::Stream;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use tokio::sync::mpsc;
use tracing::{debug, error};

/// Events emitted by AcpModelClient during streaming
#[derive(Debug, Clone)]
pub enum AcpEvent {
    /// Text delta from agent message
    TextDelta(String),
    /// Reasoning/thought delta
    ReasoningDelta(String),
    /// Stream completed
    Completed {
        stop_reason: String,
    },
    /// Error during streaming
    Error(String),
}

/// Stream of ACP events
pub struct AcpStream {
    rx: mpsc::Receiver<Result<AcpEvent>>,
}

impl Stream for AcpStream {
    type Item = Result<AcpEvent>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

/// Client for communicating with ACP-compliant agents
pub struct AcpModelClient {
    command: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    cwd: PathBuf,
}

impl AcpModelClient {
    /// Create a new ACP model client
    pub fn new(command: String, args: Vec<String>, cwd: PathBuf) -> Self {
        Self {
            command,
            args,
            env: vec![],
            cwd,
        }
    }

    /// Stream responses from the agent for a given prompt
    pub async fn stream(&self, prompt: &str) -> Result<AcpStream> {
        debug!("Starting ACP stream for prompt");

        // Spawn agent
        let mut agent = AgentProcess::spawn(&self.command, &self.args, &self.env)
            .await
            .context("Failed to spawn ACP agent")?;

        // Initialize
        let client_caps = json!({
            "fs": { "readTextFile": true, "writeTextFile": true },
            "terminal": true
        });
        agent
            .initialize(client_caps)
            .await
            .context("Failed to initialize agent")?;

        // Create channel for events
        let (tx, rx) = mpsc::channel(16);

        // Clone values for the spawned task
        let prompt = prompt.to_string();
        let cwd = self.cwd.clone();

        // Spawn task to handle session
        tokio::spawn(async move {
            if let Err(e) = run_session(&mut agent, &prompt, &cwd, tx.clone()).await {
                error!("Session error: {}", e);
                let _ = tx.send(Err(e)).await;
            }
            // Kill agent when done
            agent.kill().await.ok();
        });

        Ok(AcpStream { rx })
    }
}

/// Run a single session: create, prompt, stream events
async fn run_session(
    agent: &mut AgentProcess,
    prompt: &str,
    cwd: &Path,
    tx: mpsc::Sender<Result<AcpEvent>>,
) -> Result<()> {
    // Create new session
    let session_request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "session/new".to_string(),
        params: Some(json!({
            "cwd": cwd.to_string_lossy(),
            "mcpServers": []
        })),
        id: json!(2),
    };

    let session_response = agent
        .send_request(&session_request)
        .await
        .context("Failed to create session")?;

    if let Some(error) = session_response.error {
        anyhow::bail!("Session creation failed: {}", error.message);
    }

    let session_id = session_response
        .result
        .as_ref()
        .and_then(|r| r.get("sessionId"))
        .and_then(|s| s.as_str())
        .context("No session ID in response")?
        .to_string();

    debug!("Created session: {}", session_id);

    // Send prompt
    let prompt_request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "session/prompt".to_string(),
        params: Some(json!({
            "sessionId": session_id,
            "prompt": [{
                "type": "text",
                "text": prompt
            }]
        })),
        id: json!(3),
    };

    // Send request and process streaming notifications
    let response = stream_prompt(agent, &prompt_request, tx.clone()).await?;

    // Extract stop reason
    let stop_reason = response
        .result
        .as_ref()
        .and_then(|r| r.get("stopReason"))
        .and_then(|s| s.as_str())
        .unwrap_or("end_turn")
        .to_string();

    // Send completed event
    tx.send(Ok(AcpEvent::Completed { stop_reason }))
        .await
        .ok();

    Ok(())
}

/// Send prompt and stream notifications until response received
#[allow(clippy::collapsible_if)]
async fn stream_prompt(
    agent: &mut AgentProcess,
    request: &JsonRpcRequest,
    tx: mpsc::Sender<Result<AcpEvent>>,
) -> Result<JsonRpcResponse> {
    // Send the request
    let json = serde_json::to_string(request)?;
    agent
        .transport_mut()
        .write_raw(&json)
        .await
        .context("Failed to send prompt request")?;

    // Read messages until we get the response
    loop {
        let line = agent
            .transport_mut()
            .read_line()
            .await
            .context("Failed to read from agent")?;

        if line.is_empty() {
            anyhow::bail!("Agent closed connection unexpectedly");
        }

        // Try to parse as response first (has id)
        let value: Value = serde_json::from_str(&line)?;

        if value.get("id").is_some() && value.get("method").is_none() {
            // This is a response
            let response: JsonRpcResponse = serde_json::from_value(value)?;
            return Ok(response);
        }

        // Must be a notification - process session/update notifications
        if let Ok(notification) = serde_json::from_value::<JsonRpcNotification>(value.clone()) {
            if notification.method == "session/update" {
                if let Some(params) = notification.params {
                    process_session_update(params, &tx).await;
                }
            }
        }
    }
}

/// Extract text from content if it's a text content block
fn extract_text_content(content: &Value) -> Option<&str> {
    if content.get("type").and_then(|t| t.as_str()) == Some("text") {
        content.get("text").and_then(|t| t.as_str())
    } else {
        None
    }
}

/// Process a session/update notification and emit appropriate events
async fn process_session_update(params: Value, tx: &mpsc::Sender<Result<AcpEvent>>) {
    // Extract the update type and content
    // Format: { "sessionId": "...", "update": { "sessionUpdate": "agent_message_chunk", "content": {...} } }
    let Some(update) = params.get("update") else {
        return;
    };

    let update_type = update.get("sessionUpdate").and_then(|t| t.as_str());

    match update_type {
        Some("agent_message_chunk") => {
            if let Some(text) = update.get("content").and_then(extract_text_content) {
                let _ = tx.send(Ok(AcpEvent::TextDelta(text.to_string()))).await;
            }
        }
        Some("agent_thought_chunk") => {
            if let Some(text) = update.get("content").and_then(extract_text_content) {
                let _ = tx.send(Ok(AcpEvent::ReasoningDelta(text.to_string()))).await;
            }
        }
        _ => {
            // Ignore other update types for now
        }
    }
}
