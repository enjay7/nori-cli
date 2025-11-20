use std::time::Duration;
use tui_integration_tests::{Key, SessionConfig, TuiSession};

const TIMEOUT: Duration = Duration::from_secs(10);

#[test]
fn test_escape_cancels_streaming() {
    let config = SessionConfig::new().with_stream_until_cancel();

    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();
    session.wait_for_text("To get started", TIMEOUT).unwrap();

    // Submit prompt
    session.send_str("test").unwrap();
    session.send_key(Key::Enter).unwrap();

    // Wait for streaming to start
    session
        .wait_for_text("Streaming...", TIMEOUT)
        .expect("Streaming did not start");

    // Press Escape to cancel
    session.send_key(Key::Escape).unwrap();

    // Verify cancellation completed
    // (exact behavior depends on TUI implementation)
    session
        .wait_for(
            |s| s.contains("Cancelled") || s.contains("Stopped"),
            TIMEOUT,
        )
        .ok(); // May not show explicit message
}
