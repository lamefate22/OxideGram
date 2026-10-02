//! Linux hardware fingerprint collector using machine-id, cpuinfo, and SMBIOS.
#![allow(dead_code)]

use crate::domain::crypto::{FingerprintError, HardwareFingerprint};
use std::fs;

/// Collector extracting hardware identifiers on standard Linux systems.
#[derive(Debug, Default)]
pub struct LinuxFingerprintCollector;

impl LinuxFingerprintCollector {
    pub fn new() -> Self {
        Self
    }

    /// Reads a file safely, trimming whitespace.
    fn read_trimmed_file(path: &str) -> Option<String> {
        let content = fs::read_to_string(path).ok()?;
        let trimmed = content.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    /// Extracts CPU model name and vendor from `/proc/cpuinfo`.
    /// Also supports ARM-based Linux hosts (e.g. Raspberry Pi, aarch64 servers).
    fn extract_cpu_info() -> Option<String> {
        let content = fs::read_to_string("/proc/cpuinfo").ok()?;
        let mut model = None;
        let mut vendor = None;
        let mut hardware = None;
        let mut implementer = None;
        let mut part = None;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("model name") && model.is_none() {
                model = trimmed
                    .find(':')
                    .map(|pos| trimmed[pos + 1..].trim().to_string());
            } else if trimmed.starts_with("vendor_id") && vendor.is_none() {
                vendor = trimmed
                    .find(':')
                    .map(|pos| trimmed[pos + 1..].trim().to_string());
            } else if (trimmed.starts_with("Hardware") || trimmed.starts_with("Model"))
                && hardware.is_none()
            {
                hardware = trimmed
                    .find(':')
                    .map(|pos| trimmed[pos + 1..].trim().to_string());
            } else if trimmed.starts_with("CPU implementer") && implementer.is_none() {
                implementer = trimmed
                    .find(':')
                    .map(|pos| trimmed[pos + 1..].trim().to_string());
            } else if trimmed.starts_with("CPU part") && part.is_none() {
                part = trimmed
                    .find(':')
                    .map(|pos| trimmed[pos + 1..].trim().to_string());
            }
        }

        if let (Some(v), Some(m)) = (vendor.as_deref(), model.as_deref()) {
            Some(format!("{v} {m}"))
        } else if let Some(m) = model {
            Some(m)
        } else if let Some(h) = hardware {
            Some(h)
        } else if let (Some(imp), Some(prt)) = (implementer.as_deref(), part.as_deref()) {
            Some(format!("arm:imp={imp}:part={prt}"))
        } else {
            None
        }
    }
}

impl super::fingerprint::FingerprintCollector for LinuxFingerprintCollector {
    fn collect(&self) -> Result<HardwareFingerprint, FingerprintError> {
        let mut probes: Vec<(&'static str, String)> = Vec::new();

        // 1. Primary unprivileged machine identifier
        if let Some(mid) = Self::read_trimmed_file("/etc/machine-id") {
            probes.push(("linux_machine_id", mid));
        } else if let Some(dbus_mid) = Self::read_trimmed_file("/var/lib/dbus/machine-id") {
            probes.push(("linux_machine_id", dbus_mid));
        }

        // 2. CPU Hardware Information
        if let Some(cpu) = Self::extract_cpu_info() {
            probes.push(("linux_cpu_info", cpu));
        }

        // 3. System Hostname
        if let Some(host) = Self::read_trimmed_file("/etc/hostname") {
            probes.push(("linux_hostname", host));
        } else if let Some(host) = std::env::var("HOSTNAME")
            .ok()
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty())
        {
            probes.push(("linux_hostname", host));
        }

        // 4. SMBIOS UUID (Optional best-effort; unprivileged users might get PermissionDenied)
        if let Some(uuid) = Self::read_trimmed_file("/sys/class/dmi/id/product_uuid") {
            probes.push(("linux_smbios_uuid", uuid));
        }

        // 5. Motherboard product name (Optional best-effort)
        if let Some(board) = Self::read_trimmed_file("/sys/class/dmi/id/board_name") {
            probes.push(("linux_board_name", board));
        }

        HardwareFingerprint::from_probes(probes)
    }
}
