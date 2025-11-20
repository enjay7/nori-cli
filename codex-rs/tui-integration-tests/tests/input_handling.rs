use std::time::Duration;
use tui_integration_tests::{Key, TuiSession};

const TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn test_ctrl_c_clears_input() {
    let mut session = TuiSession::spawn(24, 80).unwrap();
    session.wait_for_text("To get started", TIMEOUT).unwrap();

    // Type some text
    session.send_str("draft message").unwrap();
    session.wait_for_text("draft message", TIMEOUT).unwrap();

    // Ctrl-C should clear
    session.send_key(Key::Ctrl('c')).unwrap();

    // Verify cleared
    session
        .wait_for(|s| !s.contains("draft message"), TIMEOUT)
        .expect("Input was not cleared");
}

#[test]
fn test_backspace() {
    let mut session = TuiSession::spawn(24, 80).unwrap();
    session.wait_for_text("To get started", TIMEOUT).unwrap();

    session.send_str("Hello").unwrap();
    session.wait_for_text("Hello", TIMEOUT).unwrap();

    // Backspace twice
    session.send_key(Key::Backspace).unwrap();
    session.send_key(Key::Backspace).unwrap();

    // Should have "Hel" remaining
    session.wait_for_text("Hel", TIMEOUT).unwrap();
    session.wait_for(|s| !s.contains("Hello"), TIMEOUT).unwrap();
}
