//! Android / Termux hardware and sandbox fingerprint collector.
#![allow(dead_code)]

use crate::domain::crypto::{FingerprintError, HardwareFingerprint};
use std::collections::HashMap;
use std::fs;
use std::process::Command;

/// Collector extracting hardware identifiers on Android inside Termux or Android OS.
#[derive(Debug, Default)]
pub struct AndroidTermuxFingerprintCollector;

impl AndroidTermuxFingerprintCollector {
    pub fn new() -> Self {
        Self
    }

    /// Parses the raw output of `/system/bin/getprop` dump into a key-value map.
    ///
    /// The standard dump format produces lines matching: `[prop.name]: [value]`.
    pub fn parse_getprop_dump(text: &str) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for line in text.lines() {
            let trimmed = line.trim();
            let parsed = trimmed
                .strip_prefix('[')
                .and_then(|s| s.split_once("]: ["))
                .and_then(|(k, rest)| {
                    rest.strip_suffix(']')
                        .map(str::trim)
                        .map(|val| (k.trim(), val))
                });

            match parsed {
                Some((k, v)) if !k.is_empty() && !v.is_empty() => {
                    map.insert(k.to_string(), v.to_string());
                }
                _ => {}
            }
        }
        map
    }

    /// Parses Android `build.prop` style `key=value` content into an existing map.
    pub fn parse_build_prop_into(text: &str, map: &mut HashMap<String, String>) {
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim();
                let val = trimmed[eq_pos + 1..].trim();
                if !key.is_empty() && !val.is_empty() && !map.contains_key(key) {
                    map.insert(key.to_string(), val.to_string());
                }
            }
        }
    }

    /// Queries a single property using `/system/bin/getprop` or `getprop`.
    fn query_single_prop(key: &str) -> Option<String> {
        let output = Command::new("/system/bin/getprop")
            .arg(key)
            .output()
            .or_else(|_| Command::new("getprop").arg(key).output())
            .ok()?;
        if output.status.success() {
            let val = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
        None
    }

    /// Collects Android system properties through all available channels.
    ///
    /// 1. Tries a fast batch dump via `/system/bin/getprop`.
    /// 2. Reads world-readable Android `build.prop` files on disk without spawning processes.
    /// 3. Falls back to individual property queries for critical identifiers.
    pub fn collect_android_properties() -> HashMap<String, String> {
        let mut props = HashMap::new();

        // 1. Full property dump via getprop
        let getprop_output = Command::new("/system/bin/getprop")
            .output()
            .or_else(|_| Command::new("getprop").output())
            .ok();

        if let Some(output) = getprop_output.filter(|o| o.status.success()) {
            let text = String::from_utf8_lossy(&output.stdout);
            props = Self::parse_getprop_dump(&text);
        }

        // 2. Direct read of Android build.prop files
        let prop_files = [
            "/system/build.prop",
            "/vendor/build.prop",
            "/product/build.prop",
            "/system_ext/build.prop",
            "/default.prop",
        ];

        for file_path in prop_files {
            if let Ok(content) = fs::read_to_string(file_path) {
                Self::parse_build_prop_into(&content, &mut props);
            }
        }

        // 3. Fallback: query individual critical properties if missing
        let critical_keys = [
            "ro.build.fingerprint",
            "ro.product.model",
            "ro.product.brand",
            "ro.product.manufacturer",
            "ro.product.device",
            "ro.product.board",
            "ro.board.platform",
            "ro.hardware",
            "ro.boot.hardware",
            "ro.build.id",
        ];

        for key in critical_keys {
            if props.contains_key(key) {
                continue;
            }
            if let Some(val) = Self::query_single_prop(key) {
                props.insert(key.to_string(), val);
            }
        }

        props
    }

    /// Extracts SoC Hardware, Model, or ARM Core information from `/proc/cpuinfo`.
    ///
    /// Robust across both older kernels (having `Hardware` / `Processor`) and modern
    /// GKI (Generic Kernel Image) ARM64 Linux kernels (using `CPU implementer` / `CPU part`).
    pub fn parse_cpuinfo(content: &str) -> Option<String> {
        let mut hardware = None;
        let mut model = None;
        let mut implementer = None;
        let mut part = None;
        let mut architecture = None;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("Hardware") && hardware.is_none() {
                hardware = trimmed
                    .find(':')
                    .map(|p| trimmed[p + 1..].trim().to_string());
            } else if trimmed.starts_with("model name") && model.is_none() {
                model = trimmed
                    .find(':')
                    .map(|p| trimmed[p + 1..].trim().to_string());
            } else if trimmed.starts_with("Model") && model.is_none() {
                model = trimmed
                    .find(':')
                    .map(|p| trimmed[p + 1..].trim().to_string());
            } else if trimmed.starts_with("CPU implementer") && implementer.is_none() {
                implementer = trimmed
                    .find(':')
                    .map(|p| trimmed[p + 1..].trim().to_string());
            } else if trimmed.starts_with("CPU part") && part.is_none() {
                part = trimmed
                    .find(':')
                    .map(|p| trimmed[p + 1..].trim().to_string());
            } else if trimmed.starts_with("CPU architecture") && architecture.is_none() {
                architecture = trimmed
                    .find(':')
                    .map(|p| trimmed[p + 1..].trim().to_string());
            }
        }

        if let Some(h) = hardware {
            return Some(h);
        }
        if let Some(m) = model {
            return Some(m);
        }
        if let (Some(imp), Some(prt)) = (implementer, part) {
            let arch = architecture.unwrap_or_else(|| "8".into());
            return Some(format!("arm:imp={imp}:part={prt}:arch={arch}"));
        }
        None
    }

    /// Reads SoC identifiers from the standard Android Linux sysfs (`/sys/devices/soc0`).
    fn extract_soc_info() -> Option<String> {
        let machine = fs::read_to_string("/sys/devices/soc0/machine").ok();
        let soc_id = fs::read_to_string("/sys/devices/soc0/soc_id").ok();
        let family = fs::read_to_string("/sys/devices/soc0/family").ok();

        let parts: Vec<String> = [machine, soc_id, family]
            .into_iter()
            .flatten()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(";"))
        }
    }

    /// Inspects Termux app sandbox installation metadata safely without root.
    fn extract_sandbox_probes() -> Vec<(&'static str, String)> {
        let mut probes = Vec::new();

        let candidate_paths = [
            std::env::var("PREFIX").ok(),
            std::env::var("HOME").ok(),
            Some("/data/data/com.termux/files".into()),
            Some("/data/user/0/com.termux/files".into()),
        ];

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            for path_opt in candidate_paths.into_iter().flatten() {
                let path = std::path::Path::new(&path_opt);
                if let Ok(meta) = fs::metadata(path) {
                    let uid = meta.uid();
                    if uid > 0 {
                        probes.push(("android_app_uid", uid.to_string()));
                        break;
                    }
                }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = candidate_paths;
        }

        // App user name in Termux environment (e.g. u0_a245)
        if let Ok(user) = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")) {
            let trimmed = user.trim().to_string();
            if !trimmed.is_empty() && trimmed.starts_with('u') {
                probes.push(("android_app_user", trimmed));
            }
        }

        probes
    }
}

impl super::fingerprint::FingerprintCollector for AndroidTermuxFingerprintCollector {
    fn collect(&self) -> Result<HardwareFingerprint, FingerprintError> {
        let mut probes: Vec<(&'static str, String)> = Vec::new();

        // 1. Android system properties
        let props = Self::collect_android_properties();

        if let Some(fp) = props.get("ro.build.fingerprint") {
            probes.push(("android_build_fingerprint", fp.clone()));
        }

        if let Some(model) = props.get("ro.product.model") {
            probes.push(("android_product_model", model.clone()));
        }

        if let Some(brand) = props.get("ro.product.brand") {
            probes.push(("android_product_brand", brand.clone()));
        }

        if let Some(manufacturer) = props.get("ro.product.manufacturer") {
            probes.push(("android_product_manufacturer", manufacturer.clone()));
        }

        if let Some(device) = props.get("ro.product.device") {
            probes.push(("android_product_device", device.clone()));
        }

        if let Some(board) = props
            .get("ro.product.board")
            .or_else(|| props.get("ro.board.platform"))
        {
            probes.push(("android_product_board", board.clone()));
        }

        if let Some(hw) = props
            .get("ro.hardware")
            .or_else(|| props.get("ro.boot.hardware"))
        {
            probes.push(("android_hardware", hw.clone()));
        }

        if let Some(build_id) = props.get("ro.build.id") {
            probes.push(("android_build_id", build_id.clone()));
        }

        // 2. CPU hardware info from /proc/cpuinfo
        if let Some(cpu) = fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|c| Self::parse_cpuinfo(&c))
        {
            probes.push(("android_cpu_hardware", cpu));
        }

        // 3. Sysfs SoC hardware info
        if let Some(soc) = Self::extract_soc_info() {
            probes.push(("android_soc_info", soc));
        }

        // 4. Termux sandbox metadata (Android app UID, app user)
        for (k, v) in Self::extract_sandbox_probes() {
            probes.push((k, v));
        }

        // 5. Machine ID if present in Termux/PRoot container
        let machine_id_paths = [
            "/data/data/com.termux/files/usr/etc/machine-id",
            "/etc/machine-id",
            "/var/lib/dbus/machine-id",
        ];
        for mid_path in machine_id_paths {
            if let Ok(mid) = fs::read_to_string(mid_path) {
                let trimmed = mid.trim().to_string();
                if !trimmed.is_empty() {
                    probes.push(("linux_machine_id", trimmed));
                    break;
                }
            }
        }

        HardwareFingerprint::from_probes(probes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_getprop_dump() {
        let sample = r#"
[ro.build.fingerprint]: [google/oriole/oriole:14/AP2A.240805.005/12025142:user/release-keys]
[ro.product.model]: [Pixel 6]
[ro.product.brand]: [google]
[ro.product.manufacturer]: [Google]
[ro.product.device]: [oriole]
[ro.board.platform]: [gs101]
"#;
        let props = AndroidTermuxFingerprintCollector::parse_getprop_dump(sample);
        assert_eq!(
            props.get("ro.build.fingerprint").unwrap(),
            "google/oriole/oriole:14/AP2A.240805.005/12025142:user/release-keys"
        );
        assert_eq!(props.get("ro.product.model").unwrap(), "Pixel 6");
        assert_eq!(props.get("ro.product.brand").unwrap(), "google");
        assert_eq!(props.get("ro.product.manufacturer").unwrap(), "Google");
        assert_eq!(props.get("ro.product.device").unwrap(), "oriole");
        assert_eq!(props.get("ro.board.platform").unwrap(), "gs101");
    }

    #[test]
    fn test_parse_build_prop_into() {
        let sample = r#"
# Sample build.prop
ro.build.fingerprint=samsung/dm3qxxx/dm3q:14/UP1A.231005.007/S918BXXS3BWK5:user/release-keys
ro.product.model=SM-S918B
ro.product.brand=samsung
ro.product.manufacturer=samsung
"#;
        let mut props = HashMap::new();
        AndroidTermuxFingerprintCollector::parse_build_prop_into(sample, &mut props);
        assert_eq!(props.get("ro.product.model").unwrap(), "SM-S918B");
        assert_eq!(props.get("ro.product.brand").unwrap(), "samsung");
    }

    #[test]
    fn test_parse_cpuinfo_arm64_modern() {
        let cpuinfo_arm64 = r#"
processor	: 0
BogoMIPS	: 38.40
Features	: fp asimd evtstrm aes pmull sha1 sha2 crc32 atomics fphp asimdhp
CPU implementer	: 0x51
CPU architecture: 8
CPU variant	: 0xd
CPU part	: 0x805
CPU revision	: 14
"#;
        let cpu = AndroidTermuxFingerprintCollector::parse_cpuinfo(cpuinfo_arm64);
        assert_eq!(cpu, Some("arm:imp=0x51:part=0x805:arch=8".to_string()));
    }

    #[test]
    fn test_parse_cpuinfo_legacy_hardware() {
        let cpuinfo_legacy = r#"
Processor	: ARMv7 Processor rev 4 (v7l)
Hardware	: Qualcomm Technologies, Inc MSM8996
Revision	: 0000
"#;
        let cpu = AndroidTermuxFingerprintCollector::parse_cpuinfo(cpuinfo_legacy);
        assert_eq!(cpu, Some("Qualcomm Technologies, Inc MSM8996".to_string()));
    }

    #[test]
    fn test_parse_cpuinfo_x86_emulator() {
        let cpuinfo_x86 = r#"
processor	: 0
vendor_id	: GenuineIntel
model name	: Intel(R) Core(TM) i7-10700K CPU @ 3.80GHz
"#;
        let cpu = AndroidTermuxFingerprintCollector::parse_cpuinfo(cpuinfo_x86);
        assert_eq!(
            cpu,
            Some("Intel(R) Core(TM) i7-10700K CPU @ 3.80GHz".to_string())
        );
    }
}
