//! Bot script loader scanning `data/bots/` directory for executable `.lua` scripts.

use crate::domain::automation::BotScript;
use crate::errors::{OxideError, ScriptError};
use std::path::{Path, PathBuf};
use tokio::fs;

/// Scanner for dynamic bot script discovery.
pub struct FileSystemScriptCatalog {
    pub bots_dir: PathBuf,
}

impl Default for FileSystemScriptCatalog {
    fn default() -> Self {
        Self::new("data/bots")
    }
}

impl FileSystemScriptCatalog {
    /// Creates a catalog pointing to the specified directory.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            bots_dir: path.as_ref().to_path_buf(),
        }
    }

    /// Ensures that the bot scripts directory exists on disk.
    pub async fn ensure_bots_dir_exists(&self) -> Result<(), OxideError> {
        if !self.bots_dir.exists() {
            fs::create_dir_all(&self.bots_dir)
                .await
                .map_err(|source| ScriptError::ScriptIo {
                    path: self.bots_dir.clone(),
                    source,
                })?;
        }
        Ok(())
    }

    /// Searches the `data/bots` directory for available `.lua` bot scripts.
    pub async fn search_bots(&self) -> Result<Vec<BotScript>, OxideError> {
        self.ensure_bots_dir_exists().await?;
        let mut discovered = Vec::new();

        let mut entries =
            fs::read_dir(&self.bots_dir)
                .await
                .map_err(|source| ScriptError::ScriptIo {
                    path: self.bots_dir.clone(),
                    source,
                })?;

        while let Some(entry) =
            entries
                .next_entry()
                .await
                .map_err(|source| ScriptError::ScriptIo {
                    path: self.bots_dir.clone(),
                    source,
                })?
        {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("lua") {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown_bot");

                discovered.push(BotScript {
                    name: stem.to_string(),
                    path,
                });
            }
        }

        Ok(discovered)
    }
}
