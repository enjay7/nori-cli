# Noridoc: ACP Module

Path: @/codex-rs/acp

### Overview

- Implements Agent Context Protocol (ACP) for Codex to communicate with external AI agent subprocesses
- Provides JSON-RPC 2.0-based IPC over stdin/stdout pipes
- Manages agent lifecycle, initialization handshake, and stderr capture for diagnostic logging

### How it fits into the larger codebase

- Used by `@/codex-rs/core/src/client.rs` to spawn and communicate with ACP-compliant agents
- Enables Codex to delegate AI operations to external providers (Claude, Gemini, etc.) that implement the ACP specification
- Complements the existing OpenAI-style API path in core by providing an alternative subprocess-based agent model
- Provides structured error handling via JSON-RPC error responses that core translates to user-facing messages
- TUI and other clients can access captured stderr for displaying agent diagnostic output

### Core Implementation

**Entry Point:** `AgentProcess::spawn()` in `@/codex-rs/acp/src/agent.rs`

- Creates a tokio subprocess with piped stdin/stdout/stderr
- Spawns a detached tokio task to asynchronously read stderr lines into a thread-safe buffer

**Protocol Flow:**

```
Client              AgentProcess                Agent Subprocess
  |                      |                            |
  |--- spawn() --------->|--- Command::spawn() ------>|
  |                      |                            |
  |--- initialize() ---->|--- JSON-RPC request ------>|
  |                      |<-- JSON-RPC response ------|
  |                      |                            |
  |--- send_request() -->|--- JSON-RPC request ------>|
  |                      |<-- JSON-RPC response ------|
```

**Key Components:**

- `StdioTransport` in `@/codex-rs/acp/src/transport.rs` - Serializes/deserializes JSON-RPC messages over async streams
- `JsonRpcRequest/Response` in `@/codex-rs/acp/src/protocol.rs` - Protocol data structures
- `AcpSession` in `@/codex-rs/acp/src/session.rs` - Session state management placeholder

### Things to Know

**Stderr Capture Implementation:**

- Buffer uses `Arc<Mutex<Vec<String>>>` for thread-safe access between reader task and caller
- Bounded at 500 lines (`STDERR_BUFFER_CAPACITY`) with FIFO eviction when full
- Individual lines truncated to 10KB (`STDERR_LINE_MAX_LENGTH`)
- Access via `agent.get_stderr_lines().await` which clones the current buffer
- Reader task runs until EOF or error, logging warnings via tracing

**Why stderr was changed from inherit to piped:**

Per ACP specification, agents "MAY write UTF-8 strings to stderr for logging purposes" and clients "MAY capture, forward, or ignore this logging". Previous `Stdio::inherit()` sent stderr directly to terminal, making it inaccessible programmatically.

**Threading model:**

The stderr reader task is fire-and-forget (spawned via `tokio::spawn` without joining). It terminates naturally when the subprocess exits and stderr closes.

**Test coverage:**

- Unit tests in `agent.rs` use shell commands to test basic capture, empty stderr, buffer overflow (600 lines), and line truncation
- Integration tests in `@/codex-rs/acp/tests/integration.rs` test stderr capture with the actual mock-acp-agent binary
- `test_gemini_acp_handshake` in integration tests verifies Gemini CLI ACP handshake works correctly (skips if npx unavailable)
- Core-level tests in `@/codex-rs/core/tests/suite/acp_gemini.rs` test `stream_acp` flow with both mock and Gemini agents

Created and maintained by Nori.
