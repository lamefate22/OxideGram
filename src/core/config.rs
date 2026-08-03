//! TOML configuration management for OxideGram.
//!
//! Stores authorized account metadata, encrypted session payload strings, and application settings
//! in `data/config.oxide` using atomic file replacement.

use crate::errors::{ConfigError, OxideError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info};

/// Data model representing an encrypted Telegram user session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionModel {
    /// URL-safe Base64 encoded salt used for Argon2id key derivation.
    pub salt: String,
    /// Telegram application API ID.
    pub api_id: i32,
    /// Telegram application API hash.
    pub api_hash: String,
    /// URL-safe Base64 encoded AES-256-GCM ciphertext containing the session data.
    pub session_string: String,
}

/// Root data model representing OxideGram configuration file contents.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SettingsModel {
    /// Map of phone numbers to encrypted session data.
    #[serde(default)]
    pub sessions: HashMap<String, SessionModel>,
}

/// Controller for loading, saving, and querying application settings stored in `.oxide` format.
pub struct OxideConfig {
    pub path: PathBuf,
    pub data: SettingsModel,
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
                fs::create_dir_all(parent)
                    .await
                    .map_err(ConfigError::IoError)?;
            }
            self.save().await?;
        }
        Ok(())
    }

    /// Loads the configuration file from disk. If missing, creates an empty configuration.
    pub async fn load(&mut self) -> Result<(), OxideError> {
        self.ensure_config_exists().await?;
        let content = fs::read_to_string(&self.path)
            .await
            .map_err(ConfigError::IoError)?;
        let settings: SettingsModel = toml::from_str(&content).map_err(ConfigError::TomlDeError)?;
        self.data = settings;
        debug!(path = %self.path.display(), sessions = self.data.sessions.len(), "Configuration loaded");
        Ok(())
    }

    /// Atomically saves current settings to disk using a temporary `.tmp` file.
    pub async fn save(&self) -> Result<(), OxideError> {
        let toml_string = toml::to_string_pretty(&self.data).map_err(ConfigError::TomlSerError)?;
        let tmp_path = self.path.with_extension("tmp");

        fs::write(&tmp_path, toml_string)
            .await
            .map_err(ConfigError::IoError)?;
        fs::rename(&tmp_path, &self.path)
            .await
            .map_err(ConfigError::IoError)?;
        debug!(path = %self.path.display(), "Configuration saved");
        Ok(())
    }

    /// Adds a session to the configuration and saves changes.
    pub async fn add_session(
        &mut self,
        phone: String,
        session_data: SessionModel,
    ) -> Result<(), OxideError> {
        info!(phone, "Session added to configuration");
        self.data.sessions.insert(phone, session_data);
        self.save().await
    }

    /// Returns a reference to the session associated with the phone number, if present.
    pub fn get_session(&self, phone: &str) -> Option<&SessionModel> {
        self.data.sessions.get(phone)
    }
}
