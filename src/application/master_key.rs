//! Application port and contracts for master key resolution.

use crate::errors::OxideError;
use async_trait::async_trait;

/// Port for resolving, caching, storing, and clearing the application master encryption key.
#[async_trait]
pub trait MasterKeyProvider: Send + Sync {
    /// Resolves the master key using cache, explicit flag, environment, hardware vault, or interactive prompt.
    async fn resolve_master_key(&self) -> Result<String, OxideError>;

    /// Saves the master key into the persistent device-bound vault.
    async fn save_device_key(&self, password: &str) -> Result<(), OxideError>;

    /// Clears any persistent device-bound vault from the system.
    async fn clear_device_key(&self) -> Result<(), OxideError>;

    /// Checks if a persistent device-bound key is present on this machine.
    fn has_device_key(&self) -> bool;
}
