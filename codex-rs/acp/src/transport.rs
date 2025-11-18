//! Stdio transport layer for JSON-RPC communication

use crate::protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as TokioBufReader};
use tokio::io::{AsyncRead, AsyncWrite};

/// Transport layer for stdio communication with ACP agents
pub struct StdioTransport<W: AsyncWrite + Unpin, R: AsyncRead + Unpin> {
    stdin: W,
    stdout: TokioBufReader<R>,
}

impl<W: AsyncWrite + Unpin, R: AsyncRead + Unpin> StdioTransport<W, R> {
    /// Create a new stdio transport from async IO streams
    pub fn new(stdin: W, stdout: R) -> Self {
        Self {
            stdin,
            stdout: TokioBufReader::new(stdout),
        }
    }

    /// Send a JSON-RPC request and return the response
    pub async fn send_request(&mut self, request: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        // Serialize request to JSON and write to stdin
        let json = serde_json::to_string(request)?;
        self.stdin.write_all(json.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;

        // Read response from stdout
        let mut line = String::new();
        self.stdout.read_line(&mut line).await?;

        let response: JsonRpcResponse = serde_json::from_str(&line)?;
        Ok(response)
    }

    /// Send a JSON-RPC notification (no response expected)
    pub async fn send_notification(&mut self, notification: &JsonRpcNotification) -> Result<()> {
        let json = serde_json::to_string(notification)?;
        self.stdin.write_all(json.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    /// Receive a message from stdout (could be notification or response)
    pub async fn receive_message(&mut self) -> Result<String> {
        let mut line = String::new();
        self.stdout.read_line(&mut line).await?;
        Ok(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use tokio::io::duplex;
    use tokio::process::Command;

    #[tokio::test]
    async fn test_stdio_transport_send_receive() {
        // Create a mock subprocess that echoes JSON-RPC responses
        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("Failed to spawn cat");

        let stdin = child.stdin.take().expect("Failed to get stdin");
        let stdout = child.stdout.take().expect("Failed to get stdout");

        let _transport = StdioTransport::new(stdin, stdout);

        // Note: This test just verifies the transport compiles and can be constructed
        // We need a proper mock agent for real request/response testing

        child.kill().await.ok();
    }

    #[tokio::test]
    async fn test_stdio_transport_notification() {
        // Create duplex channel for testing
        let (client_writer, server_reader) = duplex(1024);
        let (_server_writer, client_reader) = duplex(1024);

        // Spawn task to read notification from server side
        let reader_handle = tokio::spawn(async move {
            let mut reader = TokioBufReader::new(server_reader);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            line
        });

        // Create transport with client side
        let mut transport = StdioTransport::new(client_writer, client_reader);

        // Send notification
        let notification = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "session/cancel".to_string(),
            params: None,
        };

        transport.send_notification(&notification).await.unwrap();

        // Verify notification was sent
        let received = reader_handle.await.unwrap();
        assert!(received.contains("session/cancel"));
        assert!(received.contains("\"jsonrpc\":\"2.0\""));
        assert!(!received.contains("\"id\"")); // Notifications have no ID
    }
}
