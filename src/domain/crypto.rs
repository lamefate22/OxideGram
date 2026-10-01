//! Domain models, value objects, and error types for hardware-bound cryptographic security.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error states encountered during hardware fingerprint collection.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum FingerprintError {
    #[allow(dead_code)]
    #[error("Failed to query hardware probe '{0}': {1}")]
    ProbeFailed(String, String),

    #[error("Insufficient hardware entropy: collected only {0} valid probes (minimum 2 required)")]
    InsufficientEntropy(usize),

    #[allow(dead_code)]
    #[error("Platform unsupported or inaccessible for hardware fingerprinting: {0}")]
    UnsupportedPlatform(String),
}

/// Normalized, immutable representation of a machine's hardware fingerprint.
///
/// Encapsulates multiple hardware probes combined into a canonical, sorted key-value string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareFingerprint {
    canonical_repr: String,
    probe_count: usize,
}

impl HardwareFingerprint {
    /// Constructs a `HardwareFingerprint` from probed (key, value) pairs.
    ///
    /// Trims values, filters empty entries, sorts deterministically by key, and combines them into
    /// a canonical payload. Requires at least 2 distinct non-empty probes to ensure adequate entropy.
    pub fn from_probes(probes: Vec<(&str, String)>) -> Result<Self, FingerprintError> {
        let mut valid_probes: Vec<(String, String)> = probes
            .into_iter()
            .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))
            .filter(|(k, v)| !k.is_empty() && !v.is_empty())
            .collect();

        if valid_probes.len() < 2 {
            return Err(FingerprintError::InsufficientEntropy(valid_probes.len()));
        }

        valid_probes.sort_by(|a, b| a.0.cmp(&b.0));

        let probe_count = valid_probes.len();
        let canonical_repr = valid_probes
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(";;");

        Ok(Self {
            canonical_repr,
            probe_count,
        })
    }

    /// Returns the underlying canonical string representation.
    #[allow(dead_code)]
    pub fn as_str(&self) -> &str {
        &self.canonical_repr
    }

    /// Returns the underlying canonical bytes used as secret material for key derivation.
    pub fn as_bytes(&self) -> &[u8] {
        self.canonical_repr.as_bytes()
    }

    /// Number of verified hardware probes contained in this fingerprint.
    #[allow(dead_code)]
    pub fn probe_count(&self) -> usize {
        self.probe_count
    }
}

/// Serialized payload of the device-bound encrypted vault file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultPayload {
    /// Random 16-byte salt used for Argon2id key derivation.
    pub salt: String,
    /// Random 12-byte nonce used for AES-256-GCM encryption.
    pub nonce: String,
    /// Encrypted master password ciphertext.
    pub ciphertext: String,
    /// Verification tag or hint derived from hardware fingerprint to detect mismatch.
    pub verification_tag: String,
}

/// Errors occurring during vault lock/unlock operations.
#[derive(Debug, Error)]
pub enum VaultError {
    #[error(
        "Device fingerprint mismatch: this vault was encrypted on a different device or hardware was modified"
    )]
    DeviceMismatch,

    #[error("Vault file is corrupted or could not be deserialized: {0}")]
    Corrupted(String),

    #[error("Key derivation failed: {0}")]
    KeyDerivation(String),

    #[error("Encryption failed: {0}")]
    Encryption(String),

    #[allow(dead_code)]
    #[error("Decryption failed: {0}")]
    Decryption(String),

    #[error("File system I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_fingerprint_creation() {
        let probes = vec![
            ("bios_uuid", "1234-5678-ABCD".to_string()),
            ("machine_guid", "AAAA-BBBB-CCCC".to_string()),
            ("cpu_id", "Intel-Core-i7".to_string()),
        ];

        let fp = HardwareFingerprint::from_probes(probes).unwrap();
        assert_eq!(fp.probe_count(), 3);
        // Canonical string must be sorted deterministically
        assert!(fp.as_str().starts_with("bios_uuid=1234-5678-ABCD"));
        assert!(fp.as_str().contains(";;cpu_id=Intel-Core-i7;;"));
    }

    #[test]
    fn test_rejects_insufficient_probes() {
        let probes = vec![("single_probe", "value".to_string())];
        let err = HardwareFingerprint::from_probes(probes).unwrap_err();
        assert!(matches!(err, FingerprintError::InsufficientEntropy(1)));
    }

    #[test]
    fn test_filters_empty_probe_values() {
        let probes = vec![
            ("probe1", "   ".to_string()),
            ("probe2", "valid".to_string()),
            ("probe3", "".to_string()),
            ("probe4", "valid_too".to_string()),
        ];
        let fp = HardwareFingerprint::from_probes(probes).unwrap();
        assert_eq!(fp.probe_count(), 2);
    }
}
