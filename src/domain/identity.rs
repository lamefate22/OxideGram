//! Saved Telegram account state.

/// Encrypted credentials required to restore a Telegram session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedSession {
    pub salt: String,
    pub api_id: i32,
    pub api_hash: String,
    pub session_string: String,
}
