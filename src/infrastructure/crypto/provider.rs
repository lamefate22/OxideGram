//! Hardware-bound master key provider implementation.

use crate::application::authentication::LoginConsole;
use crate::application::master_key::MasterKeyProvider;
use crate::errors::{AuthError, OxideError};
use crate::infrastructure::crypto::device_vault::HardwareDeviceVault;
use crate::infrastructure::crypto::fingerprint::{FingerprintCollector, get_platform_collector};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Hardware-bound implementation of `MasterKeyProvider`.
///
/// Implements a multi-stage cascade:
/// 1. In-memory session cache (avoids re-prompting during cluster run or restarts)
/// 2. Explicit CLI flag `--password`
/// 3. Environment variable `OXIDEGRAM_PASSWORD`
/// 4. Persistent device-bound vault (`data/.device_vault`) unlocked with physical hardware fingerprint
/// 5. Interactive console prompt with optional "Remember on this device" enrollment
pub struct HardwareMasterKeyProvider {
    vault: HardwareDeviceVault,
    collector: Box<dyn FingerprintCollector>,
    explicit_password: Option<String>,
    cached_password: Arc<RwLock<Option<String>>>,
    console: Arc<dyn LoginConsole>,
}

impl HardwareMasterKeyProvider {
    /// Creates a new `HardwareMasterKeyProvider` with default platform collector and standard vault path.
    pub fn new(console: Arc<dyn LoginConsole>, explicit_password: Option<String>) -> Self {
        Self {
            vault: HardwareDeviceVault::default(),
            collector: get_platform_collector(),
            explicit_password,
            cached_password: Arc::new(RwLock::new(None)),
            console,
        }
    }

    /// Creates a custom provider with injected dependencies (useful for testing).
    #[allow(dead_code)]
    pub fn with_components(
        vault: HardwareDeviceVault,
        collector: Box<dyn FingerprintCollector>,
        explicit_password: Option<String>,
        console: Arc<dyn LoginConsole>,
    ) -> Self {
        Self {
            vault,
            collector,
            explicit_password,
            cached_password: Arc::new(RwLock::new(None)),
            console,
        }
    }
}

#[async_trait]
impl MasterKeyProvider for HardwareMasterKeyProvider {
    async fn resolve_master_key(&self) -> Result<String, OxideError> {
        // 1. Check in-memory cache
        if let Some(ref cached) = *self.cached_password.read().await {
            return Ok(cached.clone());
        }

        // 2. Check explicit CLI parameter
        if let Some(ref explicit) = self.explicit_password {
            *self.cached_password.write().await = Some(explicit.clone());
            return Ok(explicit.clone());
        }

        // 3. Check environment variable OXIDEGRAM_PASSWORD
        if let Ok(env_pass) = std::env::var("OXIDEGRAM_PASSWORD") {
            let trimmed = env_pass.trim().to_string();
            if !trimmed.is_empty() {
                info!("Master key resolved from OXIDEGRAM_PASSWORD environment variable");
                *self.cached_password.write().await = Some(trimmed.clone());
                return Ok(trimmed);
            }
        }

        // 4. Check hardware-bound device vault
        if self.vault.exists() {
            match self.collector.collect() {
                Ok(fp) => match self.vault.try_unlock(&fp) {
                    Ok(pass) => {
                        info!("Master key automatically unlocked via hardware-bound vault");
                        *self.cached_password.write().await = Some(pass.clone());
                        return Ok(pass);
                    }
                    Err(crate::domain::crypto::VaultError::DeviceMismatch) => {
                        warn!(
                            "Device mismatch: .device_vault was created on another machine or hardware changed"
                        );
                    }
                    Err(err) => {
                        warn!(error = %err, "Failed to unlock device vault");
                    }
                },
                Err(err) => {
                    warn!(error = %err, "Failed to collect hardware fingerprint for vault unlock");
                }
            }
        }

        // 5. Interactive prompt fallback
        let prompt_result = self
            .console
            .ask_password("Enter your decryption password:")
            .map_err(AuthError::UiError)?;

        let password = prompt_result.trim().to_string();
        if password.is_empty() {
            return Err(AuthError::UiError("Password cannot be empty".into()).into());
        }

        // Offer to remember on this device if vault doesn't already exist
        if !self.vault.exists() {
            let remember = self
                .console
                .ask_confirm("Remember password on this device for automatic unlock?")
                .unwrap_or(false);

            if remember {
                if let Err(err) = self.save_device_key(&password).await {
                    warn!(error = %err, "Failed to save device-bound key vault");
                } else {
                    info!("Master key saved and bound to this device's hardware");
                }
            }
        }

        *self.cached_password.write().await = Some(password.clone());
        Ok(password)
    }

    async fn save_device_key(&self, password: &str) -> Result<(), OxideError> {
        let fp = self.collector.collect().map_err(|e| {
            AuthError::UiError(format!("Failed to collect hardware fingerprint: {e}"))
        })?;

        self.vault
            .store(password, &fp)
            .map_err(|e| AuthError::UiError(format!("Failed to save device vault: {e}")))?;

        Ok(())
    }

    async fn clear_device_key(&self) -> Result<(), OxideError> {
        self.vault
            .clear()
            .map_err(|e| AuthError::UiError(format!("Failed to clear device vault: {e}")))?;
        *self.cached_password.write().await = None;
        Ok(())
    }

    fn has_device_key(&self) -> bool {
        self.vault.exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::crypto::HardwareFingerprint;
    use std::path::PathBuf;

    struct TempVaultGuard(PathBuf);
    impl TempVaultGuard {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("test_prov_vault_{}.json", rand::random::<u64>()));
            Self(path)
        }
    }
    impl Drop for TempVaultGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    struct MockCollector(HardwareFingerprint);
    impl FingerprintCollector for MockCollector {
        fn collect(&self) -> Result<HardwareFingerprint, crate::domain::crypto::FingerprintError> {
            Ok(self.0.clone())
        }
    }

    struct MockConsole {
        password: String,
        remember: bool,
    }
    impl LoginConsole for MockConsole {
        fn ask_text(&self, _: &str) -> Result<String, String> {
            unreachable!()
        }
        fn ask_password(&self, _: &str) -> Result<String, String> {
            Ok(self.password.clone())
        }
        fn ask_integer(&self, _: &str) -> Result<i32, String> {
            unreachable!()
        }
        fn ask_confirm(&self, _: &str) -> Result<bool, String> {
            Ok(self.remember)
        }
        fn ask_autocomplete(&self, _: &str, _: Vec<String>) -> Result<String, String> {
            unreachable!()
        }
    }

    fn test_fingerprint() -> HardwareFingerprint {
        HardwareFingerprint::from_probes(vec![
            ("probe1", "mock_val1".to_string()),
            ("probe2", "mock_val2".to_string()),
        ])
        .unwrap()
    }

    #[tokio::test]
    async fn test_resolves_explicit_password() {
        let temp = TempVaultGuard::new();
        let vault = HardwareDeviceVault::new(&temp.0);
        let collector = Box::new(MockCollector(test_fingerprint()));
        let console = Arc::new(MockConsole {
            password: "wrong".to_string(),
            remember: false,
        });

        let provider = HardwareMasterKeyProvider::with_components(
            vault,
            collector,
            Some("explicit_secret".to_string()),
            console,
        );

        let resolved = provider.resolve_master_key().await.unwrap();
        assert_eq!(resolved, "explicit_secret");
    }

    #[tokio::test]
    async fn test_auto_unlocks_from_hardware_vault() {
        let temp = TempVaultGuard::new();
        let vault = HardwareDeviceVault::new(&temp.0);
        let fp = test_fingerprint();
        vault.store("stored_secret_123", &fp).unwrap();

        let collector = Box::new(MockCollector(fp));
        let console = Arc::new(MockConsole {
            password: "console_never_called".to_string(),
            remember: false,
        });

        let provider = HardwareMasterKeyProvider::with_components(vault, collector, None, console);

        let resolved = provider.resolve_master_key().await.unwrap();
        assert_eq!(resolved, "stored_secret_123");
    }

    #[tokio::test]
    async fn test_prompts_and_enrolls_into_vault() {
        let temp = TempVaultGuard::new();
        let vault = HardwareDeviceVault::new(&temp.0);
        let fp = test_fingerprint();

        let collector = Box::new(MockCollector(fp));
        let console = Arc::new(MockConsole {
            password: "interactive_pass".to_string(),
            remember: true,
        });

        let provider =
            HardwareMasterKeyProvider::with_components(vault.clone(), collector, None, console);

        assert!(!provider.has_device_key());
        let resolved = provider.resolve_master_key().await.unwrap();
        assert_eq!(resolved, "interactive_pass");
        assert!(provider.has_device_key());

        // Clear device key
        provider.clear_device_key().await.unwrap();
        assert!(!provider.has_device_key());
    }
}
