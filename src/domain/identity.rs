use crate::domain::types::{ApiHash, ApiId};

/// Encrypted credentials required to restore a Telegram session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedSession {
    pub salt: String,
    pub api_id: ApiId,
    pub api_hash: ApiHash,
    pub session_string: String,
}
