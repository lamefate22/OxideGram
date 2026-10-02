//! TOML implementation of the saved-session repository.
//!
//! Stores authorized account metadata, encrypted session payload strings, and application settings
//! in `data/config.oxide` using atomic file replacement.

use crate::application::authentication::SessionRepository;
use crate::application::master_key::KeyValidator;
use crate::domain::PhoneNumber;
use crate::domain::identity::SavedSession;
use crate::errors::{AuthError, ConfigError, OxideError};
use crate::infrastructure::IO_TIMEOUT;
use crate::infrastructure::crypto;
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info};

/// Decrypted session representation used for export and migration backups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecryptedSessionBackup {
    pub phone: String,
    pub api_id: i32,
    pub api_hash: String,
    pub session_hex: String,
}

/// Root data model representing OxideGram configuration file contents.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SettingsModel {
    /// Map of phone numbers to encrypted session data.
    #[serde(default)]
    sessions: HashMap<String, PersistedSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSession {
    salt: String,
    api_id: i32,
    api_hash: String,
    session_string: String,
}

impl From<SavedSession> for PersistedSession {
    fn from(session: SavedSession) -> Self {
        Self {
            salt: session.salt,
            api_id: session.api_id.raw(),
            api_hash: session.api_hash.to_string(),
            session_string: session.session_string,
        }
    }
}

impl From<&PersistedSession> for SavedSession {
    fn from(session: &PersistedSession) -> Self {
        Self {
            salt: session.salt.clone(),
            api_id: crate::domain::ApiId::new(session.api_id),
            api_hash: crate::domain::ApiHash::new(&session.api_hash),
            session_string: session.session_string.clone(),
        }
    }
}

/// Controller for loading, saving, and querying application settings stored in `.oxide` format.
pub struct OxideConfig {
    path: PathBuf,
    data: SettingsModel,
}

impl Default for OxideConfig {
    fn default() -> Self {
        Self::new("data/config.oxide")
    }
}

impl OxideConfig {
    /// Creates a new `OxideConfig` pointing to the specified path.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            data: SettingsModel::default(),
        }
    }

    /// Internal helper ensuring that the configuration file and parent directory exist.
    async fn ensure_config_exists(&self) -> Result<(), OxideError> {
        if !self.path.exists() {
            if let Some(parent) = self.path.parent() {
                tokio::time::timeout(IO_TIMEOUT, fs::create_dir_all(parent))
                    .await
                    .map_err(|_| {
                        ConfigError::Io(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "Create config directory timed out",
                        ))
                    })?
                    .map_err(ConfigError::Io)?;
            }
            self.save().await?;
        }
        Ok(())
    }

    /// Loads the configuration file from disk. If missing, creates an empty configuration.
    pub async fn load(&mut self) -> Result<(), OxideError> {
        self.ensure_config_exists().await?;
        let content = tokio::time::timeout(IO_TIMEOUT, fs::read_to_string(&self.path))
            .await
            .map_err(|_| {
                ConfigError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Read config file timed out",
                ))
            })?
            .map_err(ConfigError::Io)?;
        let settings: SettingsModel =
            toml::from_str(&content).map_err(ConfigError::Deserialization)?;
        self.data = settings;
        debug!(path = %self.path.display(), sessions = self.data.sessions.len(), "Configuration loaded");
        Ok(())
    }

    /// Atomically saves current settings to disk using a temporary `.tmp` file.
    pub async fn save(&self) -> Result<(), OxideError> {
        let toml_string = toml::to_string_pretty(&self.data).map_err(ConfigError::Serialization)?;
        let tmp_path = self.path.with_extension("tmp");

        let write_res = tokio::time::timeout(IO_TIMEOUT, fs::write(&tmp_path, &toml_string))
            .await
            .map_err(|_| {
                ConfigError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Write temporary config timed out",
                ))
            })?;

        if let Err(e) = write_res {
            let _ = fs::remove_file(&tmp_path).await;
            return Err(ConfigError::Io(e).into());
        }

        let rename_res = tokio::time::timeout(IO_TIMEOUT, fs::rename(&tmp_path, &self.path))
            .await
            .map_err(|_| {
                ConfigError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Rename temporary config timed out",
                ))
            })?;

        if let Err(e) = rename_res {
            let _ = fs::remove_file(&tmp_path).await;
            return Err(ConfigError::Io(e).into());
        }

        debug!(path = %self.path.display(), "Configuration saved");
        Ok(())
    }

    /// Adds a session to the configuration and saves changes.
    pub async fn add_session(
        &mut self,
        phone: String,
        session_data: SavedSession,
    ) -> Result<(), OxideError> {
        info!(phone = %PhoneNumber::new(&phone).masked(), "Session added to configuration");
        self.data.sessions.insert(phone, session_data.into());
        self.save().await
    }

    /// Returns the session associated with the phone number, if present.
    pub fn get_session(&self, phone: &str) -> Option<SavedSession> {
        self.data.sessions.get(phone).map(SavedSession::from)
    }

    /// Removes a session from the configuration and saves changes.
    pub async fn remove_session(&mut self, phone: &str) -> Result<bool, OxideError> {
        if self.data.sessions.remove(phone).is_some() {
            info!(phone = %PhoneNumber::new(phone).masked(), "Session removed from configuration");
            self.save().await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Verifies whether the provided master password can successfully decrypt saved sessions.
    /// Returns `Ok(true)` if valid or if no sessions exist yet.
    /// Returns `Ok(false)` if password fails decryption on any saved session.
    pub async fn verify_password(&self, password: &str) -> Result<bool, OxideError> {
        let Some(first) = self.data.sessions.values().next() else {
            return Ok(true);
        };
        let Ok(salt_bytes) = URL_SAFE_NO_PAD.decode(&first.salt) else {
            return Ok(false);
        };
        let Ok(salt) = salt_bytes.try_into() else {
            return Ok(false);
        };
        match crypto::decrypt_string_async(password, &salt, &first.session_string).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Re-encrypts all stored sessions with a new master password.
    ///
    /// Decrypts all sessions into memory using `old_password` first to guarantee data integrity,
    /// then derives new salts and re-encrypts all session strings with `new_password`.
    pub async fn reencrypt_all_sessions(
        &mut self,
        old_password: &str,
        new_password: &str,
    ) -> Result<usize, OxideError> {
        if self.data.sessions.is_empty() {
            return Ok(0);
        }

        // 1. Decrypt all sessions into memory first
        let mut decrypted_sessions = Vec::new();
        for (phone, persisted) in &self.data.sessions {
            let salt_bytes = URL_SAFE_NO_PAD
                .decode(&persisted.salt)
                .map_err(|e| AuthError::DecryptionFailed(e.to_string()))?;
            let salt: [u8; 16] = salt_bytes
                .try_into()
                .map_err(|_| AuthError::DecryptionFailed("Invalid salt length".into()))?;
            let session_hex =
                crypto::decrypt_string_async(old_password, &salt, &persisted.session_string)
                    .await
                    .map_err(|e| {
                        AuthError::DecryptionFailed(format!(
                            "Failed to decrypt session {phone}: {e}"
                        ))
                    })?;
            decrypted_sessions.push((
                phone.clone(),
                persisted.api_id,
                persisted.api_hash.clone(),
                session_hex,
            ));
        }

        // 2. Re-encrypt all sessions with new password and fresh salts
        let count = decrypted_sessions.len();
        for (phone, api_id, api_hash, session_hex) in decrypted_sessions {
            let new_salt = crypto::generate_salt()?;
            let encrypted =
                crypto::encrypt_string_async(new_password, &new_salt, &session_hex).await?;
            self.data.sessions.insert(
                phone,
                PersistedSession {
                    salt: URL_SAFE_NO_PAD.encode(new_salt),
                    api_id,
                    api_hash,
                    session_string: encrypted,
                },
            );
        }

        // 3. Atomically save the updated config
        self.save().await?;
        info!(
            count,
            "All saved sessions re-encrypted with new master password"
        );
        Ok(count)
    }

    /// Decrypts all saved sessions and exports them to an unencrypted JSON backup file.
    pub async fn export_decrypted_sessions(
        &self,
        password: &str,
        out_path: &Path,
    ) -> Result<usize, OxideError> {
        let mut backups = Vec::new();
        for (phone, persisted) in &self.data.sessions {
            let salt_bytes = URL_SAFE_NO_PAD
                .decode(&persisted.salt)
                .map_err(|e| AuthError::DecryptionFailed(e.to_string()))?;
            let salt: [u8; 16] = salt_bytes
                .try_into()
                .map_err(|_| AuthError::DecryptionFailed("Invalid salt length".into()))?;
            let session_hex =
                crypto::decrypt_string_async(password, &salt, &persisted.session_string)
                    .await
                    .map_err(|e| {
                        AuthError::DecryptionFailed(format!(
                            "Failed to decrypt session {phone}: {e}"
                        ))
                    })?;
            backups.push(DecryptedSessionBackup {
                phone: phone.clone(),
                api_id: persisted.api_id,
                api_hash: persisted.api_hash.clone(),
                session_hex,
            });
        }

        let json =
            serde_json::to_string_pretty(&backups).map_err(ConfigError::JsonSerialization)?;

        if let Some(parent) = out_path.parent().filter(|p| !p.as_os_str().is_empty()) {
            tokio::time::timeout(IO_TIMEOUT, fs::create_dir_all(parent))
                .await
                .map_err(|_| {
                    ConfigError::Io(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "Create export directory timed out",
                    ))
                })?
                .map_err(ConfigError::Io)?;
        }

        tokio::time::timeout(IO_TIMEOUT, fs::write(out_path, &json))
            .await
            .map_err(|_| {
                ConfigError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Write export backup timed out",
                ))
            })?
            .map_err(ConfigError::Io)?;

        info!(count = backups.len(), path = %out_path.display(), "Sessions exported to backup");
        Ok(backups.len())
    }

    /// Imports plaintext sessions from a JSON backup file and re-encrypts them with the master password.
    pub async fn import_decrypted_sessions(
        &mut self,
        password: &str,
        in_path: &Path,
    ) -> Result<usize, OxideError> {
        let content = tokio::time::timeout(IO_TIMEOUT, fs::read_to_string(in_path))
            .await
            .map_err(|_| {
                ConfigError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Read import backup timed out",
                ))
            })?
            .map_err(ConfigError::Io)?;

        let backups: Vec<DecryptedSessionBackup> =
            serde_json::from_str(&content).map_err(ConfigError::JsonDeserialization)?;

        let count = backups.len();
        for item in backups {
            let salt = crypto::generate_salt()?;
            let encrypted =
                crypto::encrypt_string_async(password, &salt, &item.session_hex).await?;
            self.data.sessions.insert(
                item.phone,
                PersistedSession {
                    salt: URL_SAFE_NO_PAD.encode(salt),
                    api_id: item.api_id,
                    api_hash: item.api_hash,
                    session_string: encrypted,
                },
            );
        }

        self.save().await?;
        info!(count, "Sessions imported and encrypted from backup");
        Ok(count)
    }

    /// Creates a snapshot key validator backed by the current configuration's sessions.
    pub fn validator(&self) -> SessionKeyValidator {
        let sessions = self
            .data
            .sessions
            .values()
            .map(|s| (s.salt.clone(), s.session_string.clone()))
            .collect();
        SessionKeyValidator::new(sessions)
    }
}

/// Lightweight snapshot validator that verifies candidate passwords against saved sessions.
#[derive(Debug, Clone, Default)]
pub struct SessionKeyValidator {
    sessions: Vec<(String, String)>,
}

impl SessionKeyValidator {
    /// Creates a new validator with the given `(salt_b64, session_ciphertext)` pairs.
    pub fn new(sessions: Vec<(String, String)>) -> Self {
        Self { sessions }
    }
}

#[async_trait]
impl KeyValidator for SessionKeyValidator {
    async fn validate_key(&self, password: &str) -> Result<bool, OxideError> {
        let Some((salt_b64, session_str)) = self.sessions.first() else {
            return Ok(true);
        };
        let Ok(salt_bytes) = URL_SAFE_NO_PAD.decode(salt_b64) else {
            return Ok(false);
        };
        let Ok(salt) = salt_bytes.try_into() else {
            return Ok(false);
        };
        match crypto::decrypt_string_async(password, &salt, session_str).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

#[async_trait]
impl SessionRepository for OxideConfig {
    fn session_phones(&self) -> Vec<String> {
        self.data.sessions.keys().cloned().collect()
    }

    fn session(&self, phone: &str) -> Option<SavedSession> {
        self.get_session(phone)
    }

    async fn save_session(
        &mut self,
        phone: String,
        session: SavedSession,
    ) -> Result<(), OxideError> {
        self.add_session(phone, session).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ApiHash, ApiId};

    struct TempConfigPath(PathBuf);

    impl TempConfigPath {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("oxidegram_test_{}.toml", rand::random::<u64>()));
            Self(path)
        }
    }

    impl Drop for TempConfigPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            let _ = std::fs::remove_file(self.0.with_extension("tmp"));
        }
    }

    #[tokio::test]
    async fn test_empty_config_creation() {
        let temp = TempConfigPath::new();
        let mut config = OxideConfig::new(temp.0.clone());
        config.load().await.unwrap();

        assert!(config.session_phones().is_empty());
        assert!(temp.0.exists());
    }

    #[tokio::test]
    async fn test_save_and_load_roundtrip() {
        let temp = TempConfigPath::new();
        let mut config = OxideConfig::new(temp.0.clone());
        config.load().await.unwrap();

        let session = SavedSession {
            salt: "salt_abc".into(),
            api_id: ApiId::new(999888),
            api_hash: ApiHash::new("hash12345"),
            session_string: "ciphertext_sample".into(),
        };

        config
            .save_session("+1234567890".into(), session.clone())
            .await
            .unwrap();

        let mut config2 = OxideConfig::new(temp.0.clone());
        config2.load().await.unwrap();

        assert_eq!(config2.session_phones(), vec!["+1234567890"]);
        let loaded = config2.session("+1234567890").unwrap();
        assert_eq!(loaded.api_id.raw(), 999888);
        assert_eq!(loaded.api_hash.as_str(), "hash12345");
        assert_eq!(loaded.salt, "salt_abc");
        assert_eq!(loaded.session_string, "ciphertext_sample");
    }

    #[tokio::test]
    async fn test_corrupted_toml_returns_deserialization_error() {
        let temp = TempConfigPath::new();
        std::fs::write(&temp.0, "[[invalid toml = = {").unwrap();

        let mut config = OxideConfig::new(temp.0.clone());
        let err = config.load().await.unwrap_err();

        assert!(matches!(
            err,
            OxideError::Config(ConfigError::Deserialization(_))
        ));
    }

    #[tokio::test]
    async fn test_verify_password_and_reencrypt_all_sessions() {
        let temp = TempConfigPath::new();
        let mut config = OxideConfig::new(temp.0.clone());
        config.load().await.unwrap();

        let salt = crypto::generate_salt().unwrap();
        let session_str = crypto::encrypt_string_async("pass1", &salt, "sample_hex_data")
            .await
            .unwrap();

        let session = SavedSession {
            salt: URL_SAFE_NO_PAD.encode(salt),
            api_id: ApiId::new(12345),
            api_hash: ApiHash::new("hash123"),
            session_string: session_str,
        };

        config
            .save_session("+111222333".into(), session)
            .await
            .unwrap();

        // 1. Password verification
        assert!(config.verify_password("pass1").await.unwrap());
        assert!(!config.verify_password("wrong_pass").await.unwrap());

        // 2. Validator snapshot test
        let validator = config.validator();
        assert!(validator.validate_key("pass1").await.unwrap());
        assert!(!validator.validate_key("wrong_pass").await.unwrap());

        // 3. Re-encrypt all sessions with new password
        let count = config
            .reencrypt_all_sessions("pass1", "pass2")
            .await
            .unwrap();
        assert_eq!(count, 1);

        // 4. Verification under new password
        assert!(!config.verify_password("pass1").await.unwrap());
        assert!(config.verify_password("pass2").await.unwrap());

        // 5. Verify data integrity after reload
        let mut config_reloaded = OxideConfig::new(temp.0.clone());
        config_reloaded.load().await.unwrap();
        assert!(config_reloaded.verify_password("pass2").await.unwrap());

        let loaded_session = config_reloaded.session("+111222333").unwrap();
        let new_salt_bytes: [u8; 16] = URL_SAFE_NO_PAD
            .decode(&loaded_session.salt)
            .unwrap()
            .try_into()
            .unwrap();
        let decrypted =
            crypto::decrypt_string_async("pass2", &new_salt_bytes, &loaded_session.session_string)
                .await
                .unwrap();
        assert_eq!(decrypted, "sample_hex_data");
    }

    #[tokio::test]
    async fn test_export_and_import_decrypted_sessions() {
        let temp_cfg = TempConfigPath::new();
        let mut config = OxideConfig::new(temp_cfg.0.clone());
        config.load().await.unwrap();

        let salt = crypto::generate_salt().unwrap();
        let session_str = crypto::encrypt_string_async("orig_pass", &salt, "hex_payload_456")
            .await
            .unwrap();

        let session = SavedSession {
            salt: URL_SAFE_NO_PAD.encode(salt),
            api_id: ApiId::new(777),
            api_hash: ApiHash::new("hash777"),
            session_string: session_str,
        };

        config
            .save_session("+999888777".into(), session)
            .await
            .unwrap();

        // Export to temporary JSON
        let export_path =
            std::env::temp_dir().join(format!("oxide_export_{}.json", rand::random::<u64>()));
        let export_count = config
            .export_decrypted_sessions("orig_pass", &export_path)
            .await
            .unwrap();
        assert_eq!(export_count, 1);
        assert!(export_path.exists());

        // Import into clean configuration with different password
        let temp_cfg2 = TempConfigPath::new();
        let mut config2 = OxideConfig::new(temp_cfg2.0.clone());
        config2.load().await.unwrap();

        let import_count = config2
            .import_decrypted_sessions("imported_new_pass", &export_path)
            .await
            .unwrap();
        assert_eq!(import_count, 1);

        assert!(config2.verify_password("imported_new_pass").await.unwrap());
        let imported = config2.session("+999888777").unwrap();
        let imp_salt: [u8; 16] = URL_SAFE_NO_PAD
            .decode(&imported.salt)
            .unwrap()
            .try_into()
            .unwrap();
        let decrypted =
            crypto::decrypt_string_async("imported_new_pass", &imp_salt, &imported.session_string)
                .await
                .unwrap();
        assert_eq!(decrypted, "hex_payload_456");

        let _ = std::fs::remove_file(&export_path);
    }
}
