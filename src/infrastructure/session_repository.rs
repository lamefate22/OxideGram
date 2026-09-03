//! TOML implementation of the saved-session repository.
//!
//! Stores authorized account metadata, encrypted session payload strings, and application settings
//! in `data/config.oxide` using atomic file replacement.

use crate::application::authentication::SessionRepository;
use crate::domain::identity::SavedSession;
use crate::errors::{ConfigError, OxideError};
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
            api_id: session.api_id,
            api_hash: session.api_hash,
            session_string: session.session_string,
        }
    }
}

impl From<&PersistedSession> for SavedSession {
    fn from(session: &PersistedSession) -> Self {
        Self {
            salt: session.salt.clone(),
            api_id: session.api_id,
            api_hash: session.api_hash.clone(),
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
                fs::create_dir_all(parent).await.map_err(ConfigError::Io)?;
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

        if let Err(e) = fs::write(&tmp_path, &toml_string).await {
            let _ = fs::remove_file(&tmp_path).await;
            return Err(ConfigError::Io(e).into());
        }

        if let Err(e) = fs::rename(&tmp_path, &self.path).await {
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
        info!(phone, "Session added to configuration");
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
