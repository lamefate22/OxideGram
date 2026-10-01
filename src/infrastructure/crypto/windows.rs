//! Windows hardware fingerprint collector using registry queries and SMBIOS.

use crate::domain::crypto::{FingerprintError, HardwareFingerprint};
use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// Windows flag to prevent flashing a console window when spawning helper processes.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Collector extracting hardware identifiers on Windows systems.
#[derive(Debug, Default)]
pub struct WindowsFingerprintCollector;

impl WindowsFingerprintCollector {
    pub fn new() -> Self {
        Self
    }

    /// Helper executing a system command silently and extracting trimmed stdout.
    fn run_command_silent(program: &str, args: &[&str]) -> Option<String> {
        let mut cmd = Command::new(program);
        cmd.args(args);
        #[cfg(target_os = "windows")]
        cmd.creation_flags(CREATE_NO_WINDOW);

        match cmd.output() {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if stdout.is_empty() {
                    None
                } else {
                    Some(stdout)
                }
            }
            _ => None,
        }
    }

    /// Queries a string value from the Windows Registry via `reg query`.
    fn query_reg_value(key: &str, value_name: &str) -> Option<String> {
        let output = Self::run_command_silent("reg", &["query", key, "/v", value_name])?;
        // reg query outputs:
        // HKEY_LOCAL_MACHINE\...
        //     ValueName    REG_SZ    ValueData
        for line in output.lines() {
            let line = line.trim();
            if line.starts_with(value_name) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    return Some(parts[2..].join(" "));
                }
            }
        }
        None
    }

    /// Queries the volume serial number of C: drive.
    fn query_volume_serial() -> Option<String> {
        let output = Self::run_command_silent("cmd", &["/c", "vol", "C:"])?;
        // Output usually contains: "Volume Serial Number is XXXX-XXXX" or localized "Серийный номер тома: XXXX-XXXX"
        for line in output.lines() {
            if let Some(pos) = line.rfind(':') {
                let candidate = line[pos + 1..].trim();
                if candidate.contains('-') && candidate.len() <= 12 {
                    return Some(candidate.to_string());
                }
            }
        }
        None
    }

    /// Queries SMBIOS system UUID via PowerShell (best-effort).
    fn query_smbios_uuid() -> Option<String> {
        Self::run_command_silent(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "(Get-CimInstance -Class Win32_ComputerSystemProduct).UUID",
            ],
        )
    }
}

impl super::fingerprint::FingerprintCollector for WindowsFingerprintCollector {
    fn collect(&self) -> Result<HardwareFingerprint, FingerprintError> {
        let mut probes: Vec<(&'static str, String)> = Vec::new();

        // 1. Windows Cryptography MachineGuid (Primary unprivileged unique OS installation ID)
        if let Some(guid) =
            Self::query_reg_value(r"HKLM\SOFTWARE\Microsoft\Cryptography", "MachineGuid")
        {
            probes.push(("win_machine_guid", guid));
        }

        // 2. Motherboard Product
        if let Some(board) =
            Self::query_reg_value(r"HKLM\HARDWARE\DESCRIPTION\System\BIOS", "BaseBoardProduct")
        {
            probes.push(("win_baseboard_product", board));
        }

        // 3. Motherboard Manufacturer
        if let Some(mfg) = Self::query_reg_value(
            r"HKLM\HARDWARE\DESCRIPTION\System\BIOS",
            "BaseBoardManufacturer",
        ) {
            probes.push(("win_baseboard_mfg", mfg));
        }

        // 4. Central Processor Identifier
        if let Some(cpu) = Self::query_reg_value(
            r"HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0",
            "Identifier",
        ) {
            probes.push(("win_cpu_identifier", cpu));
        }

        // 5. Volume Serial Number
        if let Some(vol) = Self::query_volume_serial() {
            probes.push(("win_volume_serial", vol));
        }

        // 6. Computer Name
        if let Some(cname) = std::env::var("COMPUTERNAME")
            .ok()
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
        {
            probes.push(("win_computer_name", cname));
        }

        // 7. SMBIOS UUID (Optional best-effort)
        if let Some(uuid) = Self::query_smbios_uuid() {
            probes.push(("win_smbios_uuid", uuid));
        }

        HardwareFingerprint::from_probes(probes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::crypto::fingerprint::FingerprintCollector;

    #[test]
    #[cfg(target_os = "windows")]
    fn test_windows_collector_extracts_fingerprint() {
        let collector = WindowsFingerprintCollector::new();
        let fp = collector
            .collect()
            .expect("Windows fingerprint collection failed");
        assert!(fp.probe_count() >= 2);
        assert!(fp.as_str().contains("win_machine_guid"));
    }
}
