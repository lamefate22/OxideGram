//! Android / Termux hardware and sandbox fingerprint collector.
#![allow(dead_code)]

use crate::domain::crypto::{FingerprintError, HardwareFingerprint};
use std::fs;
use std::process::Command;

/// Collector extracting hardware identifiers on Android inside Termux.
#[derive(Debug, Default)]
pub struct AndroidTermuxFingerprintCollector;

impl AndroidTermuxFingerprintCollector {
    pub fn new() -> Self {
        Self
    }

    /// Queries an Android system property via `getprop`.
    fn getprop(prop_name: &str) -> Option<String> {
        let output = Command::new("getprop").arg(prop_name).output().ok()?;
        if output.status.success() {
            let val = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
        None
    }

    /// Queries Android Secure ID via `settings get secure android_id` (best-effort).
    fn get_android_id() -> Option<String> {
        let output = Command::new("/system/bin/settings")
            .args(["get", "secure", "android_id"])
            .output()
            .ok()?;
        if output.status.success() {
            let val = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !val.is_empty() && val != "null" {
                return Some(val);
            }
        }
        None
    }

    /// Extracts SoC Hardware and Processor info from `/proc/cpuinfo`.
    fn extract_cpu_hardware() -> Option<String> {
        let content = fs::read_to_string("/proc/cpuinfo").ok()?;
        let mut hardware = None;
        let mut processor = None;

        for line in content.lines() {
            if line.starts_with("Hardware") && hardware.is_none() {
                hardware = line.find(':').map(|pos| line[pos + 1..].trim().to_string());
            } else if line.starts_with("Processor") && processor.is_none() {
                processor = line.find(':').map(|pos| line[pos + 1..].trim().to_string());
            }
        }

        match (hardware, processor) {
            (Some(h), Some(p)) => Some(format!("{h} ({p})")),
            (Some(h), None) => Some(h),
            (None, Some(p)) => Some(p),
            (None, None) => None,
        }
    }

    /// Inspects Termux app sandbox installation metadata on the file system.
    fn extract_termux_sandbox_meta() -> Option<String> {
        let path = "/data/data/com.termux/files";
        if let Ok(meta) = fs::metadata(path) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                return Some(format!(
                    "uid={}:ino={}:dev={}",
                    meta.uid(),
                    meta.ino(),
                    meta.dev()
                ));
            }
            #[cfg(not(unix))]
            {
                let _ = meta;
                None
            }
        } else {
            None
        }
    }
}

impl super::fingerprint::FingerprintCollector for AndroidTermuxFingerprintCollector {
    fn collect(&self) -> Result<HardwareFingerprint, FingerprintError> {
        let mut probes: Vec<(&'static str, String)> = Vec::new();

        // 1. Android unique OS and hardware build fingerprint
        if let Some(fp) = Self::getprop("ro.build.fingerprint") {
            probes.push(("android_build_fingerprint", fp));
        }

        // 2. Android device product model
        if let Some(model) = Self::getprop("ro.product.model") {
            probes.push(("android_product_model", model));
        }

        // 3. Android device board platform
        if let Some(board) = Self::getprop("ro.product.board") {
            probes.push(("android_product_board", board));
        }

        // 4. Android SoC hardware specification
        if let Some(cpu) = Self::extract_cpu_hardware() {
            probes.push(("android_cpu_hardware", cpu));
        }

        // 5. Termux Sandbox identity (UID and filesystem inode)
        if let Some(meta) = Self::extract_termux_sandbox_meta() {
            probes.push(("termux_sandbox_id", meta));
        }

        // 6. Android Secure ID (Optional best-effort)
        if let Some(aid) = Self::get_android_id() {
            probes.push(("android_secure_id", aid));
        }

        HardwareFingerprint::from_probes(probes)
    }
}
