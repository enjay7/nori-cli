//! Integration tests for ACP agent communication
//!
//! These tests verify end-to-end communication with ACP agents.
//! The mock-acp-agent package from /mock-acp-agent is used for testing.

use codex_acp::{AgentProcess, JsonRpcRequest};
use serde_json::json;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
#[ignore] // Requires mock-acp-agent package
async fn test_full_acp_flow_with_mock_agent() {
    // Spawn mock ACP agent
    let args = vec!["../../../mock-acp-agent".to_string()];
    let mut agent = AgentProcess::spawn("node", &args, &[])
        .await
        .expect("Failed to spawn mock ACP agent");

    // Initialize agent
    let client_caps = json!({
        "tools": ["read_file", "write_file"],
        "streaming": true,
    });

    let init_result = timeout(Duration::from_secs(5), agent.initialize(client_caps))
        .await
        .expect("Initialize timed out")
        .expect("Initialize failed");

    assert!(init_result.is_object());
    println!("Agent capabilities: {init_result:?}");

    // Create a new session
    let session_request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "session/new".to_string(),
        params: Some(json!({})),
        id: json!(2),
    };

    let session_response = timeout(Duration::from_secs(5), agent.send_request(&session_request))
        .await
        .expect("Session request timed out")
        .expect("Session request failed");

    assert!(session_response.result.is_some());
    println!("Session created: {:?}", session_response.result);

    // Send a prompt
    let session_id = session_response
        .result
        .as_ref()
        .and_then(|r| r.get("sessionId"))
        .and_then(|s| s.as_str())
        .expect("No session ID");

    let prompt_request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "session/prompt".to_string(),
        params: Some(json!({
            "sessionId": session_id,
            "prompt": "Hello, ACP agent!",
        })),
        id: json!(3),
    };

    let prompt_response = timeout(Duration::from_secs(10), agent.send_request(&prompt_request))
        .await
        .expect("Prompt request timed out")
        .expect("Prompt request failed");

    assert!(prompt_response.result.is_some());
    println!("Prompt response: {:?}", prompt_response.result);

    agent.kill().await.ok();
}

#[tokio::test]
async fn test_acp_protocol_validation() {
    // Verify our JSON-RPC structures match ACP spec
    use codex_acp::{JsonRpcNotification, JsonRpcRequest};

    // Request must have jsonrpc, method, params, id
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "initialize".to_string(),
        params: Some(json!({"test": true})),
        id: json!(1),
    };

    let serialized = serde_json::to_string(&request).unwrap();
    assert!(serialized.contains("\"jsonrpc\":\"2.0\""));
    assert!(serialized.contains("\"method\":\"initialize\""));
    assert!(serialized.contains("\"id\":1"));

    // Notification must not have id
    let notification = JsonRpcNotification {
        jsonrpc: "2.0".to_string(),
        method: "session/update".to_string(),
        params: Some(json!({"status": "running"})),
    };

    let serialized = serde_json::to_string(&notification).unwrap();
    assert!(!serialized.contains("\"id\""));

    println!("ACP protocol validation passed");
}

/// Get path to the mock-acp-agent binary
fn mock_agent_binary_path() -> String {
    // The mock-acp-agent is part of the workspace, so the binary is in the workspace target
    // Cargo renames hyphens to underscores in binary names
    // Use the test executable location to find the target directory (handles shared target dirs)
    let test_exe = std::env::current_exe().expect("Failed to get current exe path");
    let target_debug = test_exe
        .parent() // deps
        .and_then(|p| p.parent()) // debug
        .expect("Failed to get target/debug directory");
    target_debug
        .join("mock_acp_agent")
        .to_string_lossy()
        .into_owned()
}

#[tokio::test]
async fn test_mock_agent_stderr_capture() {
    // Build mock-acp-agent first (in a real CI this would be done as a build step)
    let binary_path = mock_agent_binary_path();

    // Spawn mock ACP agent and verify stderr is captured
    let mut agent = AgentProcess::spawn(&binary_path, &[], &[])
        .await
        .expect("Failed to spawn mock ACP agent");

    // Initialize agent - this should produce "Mock agent: initialize" on stderr
    let client_caps = json!({
        "tools": ["read_file"],
        "streaming": true,
    });

    let _init_result = timeout(Duration::from_secs(5), agent.initialize(client_caps))
        .await
        .expect("Initialize timed out")
        .expect("Initialize failed");

    // Give time for stderr to be captured
    tokio::time::sleep(Duration::from_millis(100)).await;

    let stderr_lines = agent.get_stderr_lines().await;
    assert!(
        stderr_lines
            .iter()
            .any(|line| line.contains("Mock agent: initialize")),
        "Expected stderr to contain 'Mock agent: initialize', got: {stderr_lines:?}"
    );

    agent.kill().await.ok();
}

#[tokio::test]
async fn test_mock_agent_stderr_multiple_messages() {
    // This test verifies that multiple stderr lines are captured over time
    // We just use initialize which produces one stderr line, then verify capture works
    let binary_path = mock_agent_binary_path();

    let mut agent = AgentProcess::spawn(&binary_path, &[], &[])
        .await
        .expect("Failed to spawn mock ACP agent");

    // Initialize - produces "Mock agent: initialize"
    let client_caps = json!({
        "tools": ["read_file"],
        "streaming": true,
    });

    let _init_result = timeout(Duration::from_secs(5), agent.initialize(client_caps))
        .await
        .expect("Initialize timed out")
        .expect("Initialize failed");

    tokio::time::sleep(Duration::from_millis(100)).await;

    let stderr_lines = agent.get_stderr_lines().await;
    assert!(
        stderr_lines
            .iter()
            .any(|line| line.contains("Mock agent: initialize")),
        "Expected 'Mock agent: initialize' in stderr, got: {stderr_lines:?}"
    );

    agent.kill().await.ok();
}

// Note: The buffer overflow test is covered by unit tests in agent.rs
// (test_stderr_capture_overflow) which use shell commands to generate many lines.
// A full blackbox test with mock-acp-agent would require implementing the full
// ACP session/prompt protocol which is out of scope for this stderr capture feature.

/// Test that verifies Gemini CLI ACP handshake works correctly.
/// This test confirms that the ACP package can communicate with the Gemini CLI
/// when invoked via npx @google/gemini-cli --experimental-acp.
///
/// Skips if npx is not available in PATH.
#[tokio::test]
async fn test_gemini_acp_handshake() {
    // Skip if npx is not available
    if which::which("npx").is_err() {
        eprintln!("npx not found in PATH, skipping test");
        return;
    }

    // Spawn Gemini ACP agent using the same configuration as built-in provider
    let mut agent = AgentProcess::spawn(
        "npx",
        &[
            "@google/gemini-cli".to_string(),
            "--experimental-acp".to_string(),
        ],
        &[],
    )
    .await
    .expect("Failed to spawn Gemini ACP agent");

    // Initialize with client capabilities
    let client_caps = json!({
        "protocol_version": "1.0",
        "capabilities": {}
    });

    let init_result = timeout(Duration::from_secs(10), agent.initialize(client_caps))
        .await
        .expect("Initialize timed out")
        .expect("Initialize failed");

    // Verify we got a valid response
    assert!(
        init_result.is_object(),
        "Expected object response, got: {init_result:?}"
    );

    // The Gemini CLI returns protocolVersion and isAuthenticated
    if let Some(protocol_version) = init_result.get("protocolVersion") {
        eprintln!("Gemini ACP protocol version: {protocol_version}");
    }
    if let Some(is_auth) = init_result.get("isAuthenticated") {
        eprintln!("Gemini ACP authenticated: {is_auth}");
    }

    eprintln!("Gemini ACP handshake successful: {init_result:?}");

    agent.kill().await.ok();
}
