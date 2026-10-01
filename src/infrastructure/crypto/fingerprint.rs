//! Trait and factory for platform-specific hardware fingerprint collectors.

use crate::domain::crypto::{FingerprintError, HardwareFingerprint};
use std::path::Path;

/// Contract for collecting hardware machine identifiers on a host.
pub trait FingerprintCollector: Send + Sync {
    /// Collects hardware probes and constructs a validated `HardwareFingerprint`.
    fn collect(&self) -> Result<HardwareFingerprint, FingerprintError>;
}

/// Detects the current platform and returns the appropriate hardware collector.
pub fn get_platform_collector() -> Box<dyn FingerprintCollector> {
    #[cfg(target_os = "windows")]
    {
        Box::new(super::windows::WindowsFingerprintCollector::new())
    }

    #[cfg(target_os = "linux")]
    {
        if is_termux_environment() {
            Box::new(super::termux::AndroidTermuxFingerprintCollector::new())
        } else {
            Box::new(super::linux::LinuxFingerprintCollector::new())
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Box::new(super::linux::LinuxFingerprintCollector::new())
    }
}

/// Detects if execution is running inside Android Termux.
#[allow(dead_code)]
pub fn is_termux_environment() -> bool {
    std::env::var("TERMUX_VERSION").is_ok()
        || Path::new("/data/data/com.termux").exists()
        || std::env::var("PREFIX")
            .map(|p| p.contains("com.termux"))
            .unwrap_or(false)
}
