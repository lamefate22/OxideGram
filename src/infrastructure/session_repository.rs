//! TOML implementation of the saved-session repository.
//!
//! Stores authorized account metadata, encrypted session payload strings, and application settings
//! in `data/config.oxide` using atomic file replacement.

use crate::application::authentication::SessionRepository;
use crate::domain::PhoneNumber;
use crate::domain::identity::SavedSession;
use crate::errors::{ConfigError, OxideError};
use crate::infrastructure::IO_TIMEOUT;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info};

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
}
