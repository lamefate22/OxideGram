//! Hardware-bound master key provider implementation.

use crate::application::authentication::LoginConsole;
use crate::application::master_key::{KeyValidator, MasterKeyProvider};
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
    validator: Option<Arc<dyn KeyValidator>>,
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
            validator: None,
        }
    }

    /// Attaches an optional key validator to verify candidate passwords.
    pub fn with_validator(mut self, validator: Arc<dyn KeyValidator>) -> Self {
        self.validator = Some(validator);
        self
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
            validator: None,
        }
    }

    /// Explicitly saves the master key into the persistent device-bound vault.
    pub async fn save_device_key(&self, password: &str) -> Result<(), OxideError> {
        <Self as MasterKeyProvider>::save_device_key(self, password).await
    }

    /// Explicitly clears the device-bound vault.
    pub async fn clear_device_key(&self) -> Result<(), OxideError> {
        <Self as MasterKeyProvider>::clear_device_key(self).await
    }

    /// Checks if a device vault file exists.
    pub fn has_device_key(&self) -> bool {
        <Self as MasterKeyProvider>::has_device_key(self)
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
        let mut device_mismatch = false;
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
                        device_mismatch = true;
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
        let mut attempts = 0;
        let password = loop {
            let prompt_result = self
                .console
                .ask_password("Enter your decryption password:")
                .map_err(AuthError::UiError)?;

            let candidate = prompt_result.trim().to_string();
            if candidate.is_empty() {
                return Err(AuthError::UiError("Password cannot be empty".into()).into());
            }

            if let Some(ref validator) = self.validator {
                match validator.validate_key(&candidate).await {
                    Ok(true) => break candidate,
                    Ok(false) => {
                        attempts += 1;
                        if attempts >= 3 {
                            return Err(AuthError::DecryptionFailed(
                                "Incorrect password entered 3 times".into(),
                            )
                            .into());
                        }
                        self.console.print(&format!(
                            "[error] Incorrect password (attempt {attempts}/3). Please try again."
                        ));
                    }
                    Err(e) => {
                        warn!(error = %e, "Password validation error, proceeding with input");
                        break candidate;
                    }
                }
            } else {
                break candidate;
            }
        };

        // Offer to remember or re-bind on this device
        if device_mismatch {
            let rebind = self
                .console
                .ask_confirm(
                    "Hardware mismatch detected (vault was created on another device). Re-bind automatic unlock to this machine?",
                )
                .unwrap_or(false);

            if rebind {
                if let Err(err) = self.save_device_key(&password).await {
                    warn!(error = %err, "Failed to re-bind device-bound key vault");
                } else {
                    info!("Master key re-bound to this device's hardware successfully");
                    self.console.print(
                        "[success] Device vault successfully re-bound to this machine! Automatic unlock is now active.",
                    );
                }
            }
        } else if !self.vault.exists() {
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

        *self.cached_password.write().await = Some(password.to_string());
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

    #[tokio::test]
    async fn test_device_mismatch_offers_rebind_and_succeeds() {
        let temp = TempVaultGuard::new();
        let vault = HardwareDeviceVault::new(&temp.0);

        let fp_old = HardwareFingerprint::from_probes(vec![
            ("probe1", "machine_a".to_string()),
            ("probe2", "cpu_a".to_string()),
        ])
        .unwrap();

        let fp_new = HardwareFingerprint::from_probes(vec![
            ("probe1", "machine_b".to_string()),
            ("probe2", "cpu_b".to_string()),
        ])
        .unwrap();

        // 1. Vault originally created on Machine A
        vault.store("migrated_password", &fp_old).unwrap();

        // 2. Transferred to Machine B: hardware mismatch occurs
        let collector_b = Box::new(MockCollector(fp_new.clone()));
        let console_b = Arc::new(MockConsole {
            password: "migrated_password".to_string(),
            remember: true, // Agree to re-bind to Machine B
        });

        let provider_b =
            HardwareMasterKeyProvider::with_components(vault.clone(), collector_b, None, console_b);

        let resolved = provider_b.resolve_master_key().await.unwrap();
        assert_eq!(resolved, "migrated_password");

        // 3. Verify that Machine B can now auto-unlock without console input
        struct PanicConsole;
        impl LoginConsole for PanicConsole {
            fn ask_text(&self, _: &str) -> Result<String, String> {
                unreachable!()
            }
            fn ask_password(&self, _: &str) -> Result<String, String> {
                unreachable!()
            }
            fn ask_integer(&self, _: &str) -> Result<i32, String> {
                unreachable!()
            }
            fn ask_confirm(&self, _: &str) -> Result<bool, String> {
                unreachable!()
            }
            fn ask_autocomplete(&self, _: &str, _: Vec<String>) -> Result<String, String> {
                unreachable!()
            }
        }

        let provider_next = HardwareMasterKeyProvider::with_components(
            vault,
            Box::new(MockCollector(fp_new)),
            None,
            Arc::new(PanicConsole),
        );

        let auto_unlocked = provider_next.resolve_master_key().await.unwrap();
        assert_eq!(auto_unlocked, "migrated_password");
    }

    #[tokio::test]
    async fn test_validator_retries_on_wrong_password() {
        struct SequentialConsole {
            passwords: std::sync::Mutex<Vec<String>>,
        }
        impl LoginConsole for SequentialConsole {
            fn ask_text(&self, _: &str) -> Result<String, String> {
                unreachable!()
            }
            fn ask_password(&self, _: &str) -> Result<String, String> {
                let mut list = self.passwords.lock().unwrap();
                Ok(list.remove(0))
            }
            fn ask_integer(&self, _: &str) -> Result<i32, String> {
                unreachable!()
            }
            fn ask_confirm(&self, _: &str) -> Result<bool, String> {
                Ok(false)
            }
            fn ask_autocomplete(&self, _: &str, _: Vec<String>) -> Result<String, String> {
                unreachable!()
            }
        }

        struct TestValidator;
        #[async_trait]
        impl KeyValidator for TestValidator {
            async fn validate_key(&self, password: &str) -> Result<bool, OxideError> {
                Ok(password == "correct_pass")
            }
        }

        let temp = TempVaultGuard::new();
        let vault = HardwareDeviceVault::new(&temp.0);
        let fp = test_fingerprint();
        let collector = Box::new(MockCollector(fp));

        let console = Arc::new(SequentialConsole {
            passwords: std::sync::Mutex::new(vec![
                "wrong_first".to_string(),
                "correct_pass".to_string(),
            ]),
        });

        let provider = HardwareMasterKeyProvider::with_components(vault, collector, None, console)
            .with_validator(Arc::new(TestValidator));

        let resolved = provider.resolve_master_key().await.unwrap();
        assert_eq!(resolved, "correct_pass");
    }
}
