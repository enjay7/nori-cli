//! Session state management

// Placeholder for session implementation
pub struct AcpSession;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Created,
    Active,
    Cancelled,
    Completed,
    Failed,
}
