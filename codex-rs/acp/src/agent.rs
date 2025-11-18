//! Agent subprocess management

use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::transport::StdioTransport;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Maximum number of stderr lines to buffer
const STDERR_BUFFER_CAPACITY: usize = 500;

/// Maximum length of a single stderr line in bytes (10KB)
const STDERR_LINE_MAX_LENGTH: usize = 10240;

/// ACP agent subprocess
pub struct AgentProcess {
    child: Child,
    transport: StdioTransport<ChildStdin, ChildStdout>,
    capabilities: Option<Value>,
    /// Buffer for captured stderr lines
    stderr_lines: Arc<Mutex<Vec<String>>>,
}

impl AgentProcess {
    /// Spawn a new ACP agent subprocess
    ///
    /// # Arguments
    /// * `command` - Command to execute (e.g., "npx")
    /// * `args` - Arguments (e.g., ["@zed-industries/claude-code-acp"])
    /// * `env` - Additional environment variables
    pub async fn spawn(command: &str, args: &[String], env: &[(String, String)]) -> Result<Self> {
        info!("Spawning ACP agent: {} {:?}", command, args);

        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()) // Capture stderr for programmatic access
            .kill_on_drop(true);

        for (key, value) in env {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn().context("Failed to spawn ACP agent")?;

        let stdin = child.stdin.take().context("Failed to get stdin")?;
        let stdout = child.stdout.take().context("Failed to get stdout")?;
        let stderr = child.stderr.take().context("Failed to get stderr")?;

        let transport = StdioTransport::new(stdin, stdout);

        // Create shared buffer for stderr lines
        let stderr_lines = Arc::new(Mutex::new(Vec::with_capacity(STDERR_BUFFER_CAPACITY)));
        let stderr_lines_clone = Arc::clone(&stderr_lines);

        // Spawn task to read stderr lines
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        // Remove trailing newline
                        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');

                        // Truncate long lines to 10KB
                        let truncated = if trimmed.len() > STDERR_LINE_MAX_LENGTH {
                            &trimmed[..STDERR_LINE_MAX_LENGTH]
                        } else {
                            trimmed
                        };

                        let mut buffer = stderr_lines_clone.lock().await;

                        // If buffer is full, remove oldest line
                        if buffer.len() >= STDERR_BUFFER_CAPACITY {
                            buffer.remove(0);
                        }

                        buffer.push(truncated.to_string());
                    }
                    Err(e) => {
                        warn!("Error reading stderr: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(Self {
            child,
            transport,
            capabilities: None,
            stderr_lines,
        })
    }

    /// Initialize the ACP agent with protocol handshake
    pub async fn initialize(&mut self, client_capabilities: Value) -> Result<Value> {
        debug!("Initializing ACP agent");

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "initialize".to_string(),
            params: Some(json!({
                "protocolVersion": "1.0",
                "capabilities": client_capabilities,
            })),
            id: json!(1),
        };

        let response = self.transport.send_request(&request).await?;

        if let Some(error) = response.error {
            anyhow::bail!("Agent initialization failed: {}", error.message);
        }

        let result = response.result.context("No result in init response")?;
        self.capabilities = Some(result.clone());

        debug!("Agent initialized with capabilities: {:?}", result);
        Ok(result)
    }

    /// Send a request to the agent
    pub async fn send_request(&mut self, request: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        self.transport.send_request(request).await
    }

    /// Kill the agent subprocess
    pub async fn kill(&mut self) -> Result<()> {
        self.child.kill().await.context("Failed to kill agent")
    }

    /// Get agent capabilities (available after initialization)
    pub fn capabilities(&self) -> Option<&Value> {
        self.capabilities.as_ref()
    }

    /// Get captured stderr lines
    ///
    /// Returns a clone of all stderr lines captured so far from the agent subprocess.
    /// Lines are stored in order of receipt, with oldest first. The buffer is capped
    /// at 500 lines; when full, oldest lines are dropped.
    pub async fn get_stderr_lines(&self) -> Vec<String> {
        self.stderr_lines.lock().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_agent_spawn() {
        // Test that we can spawn a simple subprocess (using cat as a stand-in)
        // Real testing requires the mock ACP agent from /mock-acp-agent
        let result = AgentProcess::spawn("cat", &[], &[]).await;
        assert!(result.is_ok());

        let mut agent = result.unwrap();
        agent.kill().await.ok();
    }

    #[tokio::test]
    #[ignore] // Requires mock-acp-agent to be available
    async fn test_agent_initialize_with_mock() {
        // This test assumes the mock-acp-agent package is available
        // In CI, we'd need to ensure it's installed first

        let args = vec!["mock-acp-agent".to_string()];
        let mut agent = AgentProcess::spawn("npx", &args, &[])
            .await
            .expect("Failed to spawn mock agent");

        let client_caps = json!({
            "tools": true,
            "streaming": true,
        });

        let init_result = timeout(Duration::from_secs(5), agent.initialize(client_caps))
            .await
            .expect("Initialize timed out")
            .expect("Initialize failed");

        assert!(init_result.is_object());
        assert!(agent.capabilities().is_some());

        agent.kill().await.ok();
    }

    #[tokio::test]
    async fn test_stderr_capture_basic() {
        // Spawn a shell command that writes to stderr then exits
        let args = vec![
            "-c".to_string(),
            "echo 'error line 1' >&2 && echo 'error line 2' >&2 && sleep 0.1".to_string(),
        ];
        let mut agent = AgentProcess::spawn("sh", &args, &[])
            .await
            .expect("Failed to spawn");

        // Give time for stderr to be written
        tokio::time::sleep(Duration::from_millis(200)).await;

        let stderr_lines = agent.get_stderr_lines().await;
        assert_eq!(stderr_lines.len(), 2);
        assert_eq!(stderr_lines[0], "error line 1");
        assert_eq!(stderr_lines[1], "error line 2");

        agent.kill().await.ok();
    }

    #[tokio::test]
    async fn test_stderr_capture_empty() {
        // Spawn a command that writes nothing to stderr
        let args = vec!["-c".to_string(), "sleep 0.1".to_string()];
        let mut agent = AgentProcess::spawn("sh", &args, &[])
            .await
            .expect("Failed to spawn");

        tokio::time::sleep(Duration::from_millis(200)).await;

        let stderr_lines = agent.get_stderr_lines().await;
        assert!(stderr_lines.is_empty());

        agent.kill().await.ok();
    }

    #[tokio::test]
    async fn test_stderr_capture_overflow() {
        // Spawn a command that writes more than buffer capacity (500 lines)
        // Write 600 lines to test that only the last 500 are retained
        let args = vec![
            "-c".to_string(),
            "for i in $(seq 1 600); do echo \"stderr line $i\" >&2; done && sleep 0.1".to_string(),
        ];
        let mut agent = AgentProcess::spawn("sh", &args, &[])
            .await
            .expect("Failed to spawn");

        // Give time for all stderr to be written
        tokio::time::sleep(Duration::from_millis(500)).await;

        let stderr_lines = agent.get_stderr_lines().await;
        assert_eq!(
            stderr_lines.len(),
            500,
            "Buffer should be capped at 500 lines"
        );
        // First line in buffer should be line 101 (lines 1-100 dropped)
        assert_eq!(stderr_lines[0], "stderr line 101");
        // Last line should be line 600
        assert_eq!(stderr_lines[499], "stderr line 600");

        agent.kill().await.ok();
    }

    #[tokio::test]
    async fn test_stderr_line_truncation() {
        // Spawn a command that writes a line longer than 10KB
        // Create a line of 15000 characters (15KB) using head -c which is POSIX compliant
        let args = vec![
            "-c".to_string(),
            "head -c 15000 < /dev/zero | tr '\\0' 'X' >&2 && echo '' >&2 && echo 'normal line' >&2 && sleep 0.1".to_string(),
        ];
        let mut agent = AgentProcess::spawn("sh", &args, &[])
            .await
            .expect("Failed to spawn");

        tokio::time::sleep(Duration::from_millis(300)).await;

        let stderr_lines = agent.get_stderr_lines().await;
        assert_eq!(stderr_lines.len(), 2);
        // First line should be truncated to 10KB (10240 bytes)
        assert_eq!(
            stderr_lines[0].len(),
            10240,
            "Long line should be truncated to 10KB"
        );
        // Second line should be normal
        assert_eq!(stderr_lines[1], "normal line");

        agent.kill().await.ok();
    }
}
