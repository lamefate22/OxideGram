//! Strongly-typed domain identifiers and value objects (Newtype pattern).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Strongly typed Telegram Chat identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChatId(pub i64);

impl ChatId {
    pub const fn new(id: i64) -> Self {
        Self(id)
    }

    pub const fn raw(self) -> i64 {
        self.0
    }
}

impl fmt::Display for ChatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<i64> for ChatId {
    fn from(id: i64) -> Self {
        Self(id)
    }
}

impl From<ChatId> for i64 {
    fn from(chat_id: ChatId) -> Self {
        chat_id.0
    }
}

/// Strongly typed Telegram Sender identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SenderId(pub i64);

impl SenderId {
    pub const fn new(id: i64) -> Self {
        Self(id)
    }

    pub const fn raw(self) -> i64 {
        self.0
    }
}

impl fmt::Display for SenderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<i64> for SenderId {
    fn from(id: i64) -> Self {
        Self(id)
    }
}

impl From<SenderId> for i64 {
    fn from(sender_id: SenderId) -> Self {
        sender_id.0
    }
}

/// Strongly typed Telegram API identifier (API_ID).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApiId(pub i32);

impl ApiId {
    pub const fn new(id: i32) -> Self {
        Self(id)
    }

    pub const fn raw(self) -> i32 {
        self.0
    }
}

impl fmt::Display for ApiId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<i32> for ApiId {
    fn from(id: i32) -> Self {
        Self(id)
    }
}

impl From<ApiId> for i32 {
    fn from(api_id: ApiId) -> Self {
        api_id.0
    }
}

/// Strongly typed Telegram API hash (API_HASH).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApiHash(pub String);

impl ApiHash {
    pub fn new(hash: impl Into<String>) -> Self {
        Self(hash.into())
    }

    #[allow(dead_code)]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ApiHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ApiHash {
    fn from(hash: String) -> Self {
        Self(hash)
    }
}

impl From<&str> for ApiHash {
    fn from(hash: &str) -> Self {
        Self(hash.to_string())
    }
}

/// Strongly typed user phone number with PII masking support.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PhoneNumber(pub String);

impl PhoneNumber {
    pub fn new(phone: impl Into<String>) -> Self {
        Self(phone.into())
    }

    #[allow(dead_code)]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns clean numeric string suitable for filenames without `+` or spaces.
    pub fn clean_digits(&self) -> String {
        self.0.replace('+', "").trim().to_string()
    }

    /// Returns masked phone number to protect PII in logs (e.g. "+123***89").
    pub fn masked(&self) -> String {
        let trimmed = self.0.trim();
        if trimmed.len() <= 6 {
            return "***".to_string();
        }
        let prefix = &trimmed[..4];
        let suffix = &trimmed[trimmed.len() - 2..];
        format!("{prefix}***{suffix}")
    }
}

impl fmt::Display for PhoneNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for PhoneNumber {
    fn from(phone: String) -> Self {
        Self(phone)
    }
}

impl From<&str> for PhoneNumber {
    fn from(phone: &str) -> Self {
        Self(phone.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_number_masks_pii_safely() {
        let phone = PhoneNumber::new("+1234567890");
        assert_eq!(phone.as_str(), "+1234567890");
        assert_eq!(phone.masked(), "+123***90");
        assert_eq!(PhoneNumber::new("+79998887766").masked(), "+799***66");
        assert_eq!(PhoneNumber::new("1234").masked(), "***");
        assert_eq!(PhoneNumber::new("").masked(), "***");
    }

    #[test]
    fn phone_number_cleans_digits() {
        assert_eq!(
            PhoneNumber::new(" +1234567890 ").clean_digits(),
            "1234567890"
        );
    }

    #[test]
    fn newtype_conversions() {
        let chat = ChatId::from(42);
        assert_eq!(chat.raw(), 42);
        assert_eq!(i64::from(chat), 42);

        let sender = SenderId::from(99);
        assert_eq!(sender.raw(), 99);

        let api_id = ApiId::from(12345);
        assert_eq!(api_id.raw(), 12345);

        let api_hash = ApiHash::from("abc");
        assert_eq!(api_hash.as_str(), "abc");
    }
}
