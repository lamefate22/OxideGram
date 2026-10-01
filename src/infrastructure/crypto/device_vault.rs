//! Hardware-bound encrypted password vault implementation.
//!
//! Stores the user's master password encrypted with AES-256-GCM, using a 256-bit key
//! derived via Argon2id from the host machine's unique hardware fingerprint.

use crate::domain::crypto::{HardwareFingerprint, VaultError, VaultPayload};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use argon2::Argon2;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::{TryRng, rngs::SysRng};
use std::fs;
use std::path::{Path, PathBuf};

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// File-based vault encrypting and storing master credentials bound to physical hardware.
#[derive(Debug, Clone)]
pub struct HardwareDeviceVault {
    path: PathBuf,
}

impl Default for HardwareDeviceVault {
    fn default() -> Self {
        Self::new("data/.device_vault")
    }
}

impl HardwareDeviceVault {
    /// Creates a new `HardwareDeviceVault` at the designated path.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Checks if a vault file currently exists on disk.
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// Derives a 32-byte key from the hardware fingerprint and a salt using Argon2id.
    fn derive_vault_key(
        fingerprint: &HardwareFingerprint,
        salt: &[u8; SALT_LEN],
    ) -> Result<[u8; KEY_LEN], VaultError> {
        let mut key = [0u8; KEY_LEN];
        Argon2::default()
            .hash_password_into(fingerprint.as_bytes(), salt, &mut key)
            .map_err(|e| VaultError::KeyDerivation(e.to_string()))?;
        Ok(key)
    }

    /// Encrypts and stores the master password using the provided hardware fingerprint.
    pub fn store(
        &self,
        password: &str,
        fingerprint: &HardwareFingerprint,
    ) -> Result<(), VaultError> {
        if let Some(parent) = self.path.parent().filter(|p| !p.exists()) {
            fs::create_dir_all(parent)?;
        }

        // 1. Generate cryptographic salt and nonce
        let mut salt = [0u8; SALT_LEN];
        SysRng
            .try_fill_bytes(&mut salt)
            .map_err(|e| VaultError::Encryption(format!("RNG salt error: {e}")))?;

        let mut nonce_bytes = [0u8; NONCE_LEN];
        SysRng
            .try_fill_bytes(&mut nonce_bytes)
            .map_err(|e| VaultError::Encryption(format!("RNG nonce error: {e}")))?;

        // 2. Derive key from hardware fingerprint
        let key = Self::derive_vault_key(fingerprint, &salt)?;

        // 3. Encrypt password with AES-256-GCM
        let cipher = Aes256Gcm::new((&key).into());
        let nonce = Nonce::from(nonce_bytes);
        let ciphertext = cipher
            .encrypt(&nonce, password.as_bytes())
            .map_err(|e| VaultError::Encryption(e.to_string()))?;

        // 4. Compute verification tag (first 8 bytes of key in hex)
        let verification_tag = hex::encode(&key[..8]);

        let payload = VaultPayload {
            salt: URL_SAFE_NO_PAD.encode(salt),
            nonce: URL_SAFE_NO_PAD.encode(nonce_bytes),
            ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
            verification_tag,
        };

        let json = serde_json::to_string_pretty(&payload)
            .map_err(|e| VaultError::Corrupted(e.to_string()))?;

        fs::write(&self.path, json)?;

        // 5. Enforce restrictive permissions on Unix/Termux (chmod 0600: read/write strictly by owner)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::metadata(&self.path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o600);
                let _ = fs::set_permissions(&self.path, perms);
            }
        }

        Ok(())
    }

    /// Attempts to decrypt the stored master password using the provided hardware fingerprint.
    ///
    /// Returns `VaultError::DeviceMismatch` if the key derived from the fingerprint cannot
    /// decrypt the ciphertext (e.g. copied to another machine or hardware modified).
    pub fn try_unlock(&self, fingerprint: &HardwareFingerprint) -> Result<String, VaultError> {
        if !self.exists() {
            return Err(VaultError::Corrupted("Vault file not found".into()));
        }

        let content = fs::read_to_string(&self.path)?;
        let payload: VaultPayload = serde_json::from_str(&content)
            .map_err(|e| VaultError::Corrupted(format!("Invalid vault JSON: {e}")))?;

        let salt_bytes = URL_SAFE_NO_PAD
            .decode(&payload.salt)
            .map_err(|e| VaultError::Corrupted(format!("Invalid salt base64: {e}")))?;
        if salt_bytes.len() != SALT_LEN {
            return Err(VaultError::Corrupted("Salt length mismatch".into()));
        }
        let mut salt = [0u8; SALT_LEN];
        salt.copy_from_slice(&salt_bytes);

        let nonce_bytes = URL_SAFE_NO_PAD
            .decode(&payload.nonce)
            .map_err(|e| VaultError::Corrupted(format!("Invalid nonce base64: {e}")))?;
        if nonce_bytes.len() != NONCE_LEN {
            return Err(VaultError::Corrupted("Nonce length mismatch".into()));
        }
        let nonce_arr: [u8; NONCE_LEN] = nonce_bytes
            .try_into()
            .map_err(|_| VaultError::Corrupted("Nonce array conversion error".into()))?;

        let ciphertext = URL_SAFE_NO_PAD
            .decode(&payload.ciphertext)
            .map_err(|e| VaultError::Corrupted(format!("Invalid ciphertext base64: {e}")))?;

        // Derive key from candidate hardware fingerprint
        let key = Self::derive_vault_key(fingerprint, &salt)?;

        // Check verification tag first for quick mismatch detection
        let expected_tag = hex::encode(&key[..8]);
        if expected_tag != payload.verification_tag {
            return Err(VaultError::DeviceMismatch);
        }

        // Authenticated AES-256-GCM decryption
        let cipher = Aes256Gcm::new((&key).into());
        let nonce = Nonce::from(nonce_arr);
        let decrypted_bytes = cipher
            .decrypt(&nonce, ciphertext.as_ref())
            .map_err(|_| VaultError::DeviceMismatch)?;

        let password = String::from_utf8(decrypted_bytes).map_err(|e| {
            VaultError::Corrupted(format!("Decrypted password is not valid UTF-8: {e}"))
        })?;

        Ok(password)
    }

    /// Deletes the vault file from disk, clearing saved credentials.
    pub fn clear(&self) -> Result<(), VaultError> {
        if self.exists() {
            fs::remove_file(&self.path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempVaultGuard(PathBuf);
    impl TempVaultGuard {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("test_vault_{}.json", rand::random::<u64>()));
            Self(path)
        }
    }
    impl Drop for TempVaultGuard {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn test_vault_store_and_unlock_roundtrip() {
        let temp = TempVaultGuard::new();
        let vault = HardwareDeviceVault::new(&temp.0);

        let fp = HardwareFingerprint::from_probes(vec![
            ("probe1", "val1".to_string()),
            ("probe2", "val2".to_string()),
        ])
        .unwrap();

        let password = "MyUltraSecretPassword123!";
        vault.store(password, &fp).unwrap();
        assert!(vault.exists());

        let unlocked = vault.try_unlock(&fp).unwrap();
        assert_eq!(unlocked, password);
    }

    #[test]
    fn test_vault_rejects_different_hardware_fingerprint() {
        let temp = TempVaultGuard::new();
        let vault = HardwareDeviceVault::new(&temp.0);

        let fp_device_a = HardwareFingerprint::from_probes(vec![
            ("uuid", "device-A-uuid".to_string()),
            ("board", "Gigabyte-B450".to_string()),
        ])
        .unwrap();

        let fp_device_b = HardwareFingerprint::from_probes(vec![
            ("uuid", "device-B-uuid".to_string()),
            ("board", "ASUS-ROG-X570".to_string()),
        ])
        .unwrap();

        vault.store("password", &fp_device_a).unwrap();

        // Attempting to unlock on Device B must return DeviceMismatch
        let res = vault.try_unlock(&fp_device_b);
        assert!(matches!(res, Err(VaultError::DeviceMismatch)));
    }

    #[test]
    fn test_vault_clear_removes_file() {
        let temp = TempVaultGuard::new();
        let vault = HardwareDeviceVault::new(&temp.0);

        let fp = HardwareFingerprint::from_probes(vec![
            ("k1", "v1".to_string()),
            ("k2", "v2".to_string()),
        ])
        .unwrap();

        vault.store("pass", &fp).unwrap();
        assert!(vault.exists());
        vault.clear().unwrap();
        assert!(!vault.exists());
    }
}
