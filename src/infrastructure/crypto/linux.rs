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
    fn extract_cpu_info() -> Option<String> {
        let content = fs::read_to_string("/proc/cpuinfo").ok()?;
        let mut model = None;
        let mut vendor = None;

        for line in content.lines() {
            if line.starts_with("model name") && model.is_none() {
                model = line.find(':').map(|pos| line[pos + 1..].trim().to_string());
            } else if line.starts_with("vendor_id") && vendor.is_none() {
                vendor = line.find(':').map(|pos| line[pos + 1..].trim().to_string());
            }
            if model.is_some() && vendor.is_some() {
                break;
            }
        }

        match (vendor, model) {
            (Some(v), Some(m)) => Some(format!("{v} {m}")),
            (Some(v), None) => Some(v),
            (None, Some(m)) => Some(m),
            (None, None) => None,
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
