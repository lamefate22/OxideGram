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

    #[cfg(target_os = "android")]
    {
        Box::new(super::termux::AndroidTermuxFingerprintCollector::new())
    }

    #[cfg(not(any(target_os = "windows", target_os = "android")))]
    {
        if is_android_or_termux_environment() {
            Box::new(super::termux::AndroidTermuxFingerprintCollector::new())
        } else {
            Box::new(super::linux::LinuxFingerprintCollector::new())
        }
    }
}

/// Detects if execution is running on Android OS or inside Termux / Android container.
#[allow(dead_code)]
pub fn is_android_or_termux_environment() -> bool {
    #[cfg(target_os = "android")]
    {
        true
    }
    #[cfg(not(target_os = "android"))]
    {
        is_termux_environment()
            || Path::new("/system/build.prop").exists()
            || Path::new("/system/bin/getprop").exists()
    }
}

/// Detects if execution is running inside Android Termux.
#[allow(dead_code)]
pub fn is_termux_environment() -> bool {
    #[cfg(target_os = "android")]
    {
        true
    }
    #[cfg(not(target_os = "android"))]
    {
        std::env::var("TERMUX_VERSION").is_ok()
            || Path::new("/data/data/com.termux").exists()
            || std::env::var("PREFIX")
                .map(|p| p.contains("com.termux"))
                .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_platform_collector_returns_valid_instance() {
        let collector = get_platform_collector();
        // Just verify trait object instantiates without panic
        assert!(std::any::type_name_of_val(&collector).contains("Box"));
    }

    #[test]
    fn test_environment_check_does_not_panic() {
        let _ = is_android_or_termux_environment();
        let _ = is_termux_environment();
    }
}
