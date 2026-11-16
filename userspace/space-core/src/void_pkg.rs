//! Void Linux native userspace integration: XBPS package lifecycle & runit service control.

use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct ServiceInfo {
    pub name: String,
    pub enabled: bool,
    pub status: String,
}

/// Wrapper around Void Linux `xbps-*` CLI tools.
pub struct XbpsClient;

impl XbpsClient {
    pub fn list_installed() -> Vec<PackageInfo> {
        let Ok(out) = Command::new("xbps-query").arg("-l").output() else {
            return Vec::new();
        };
        if !out.status.success() {
            return Vec::new();
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut pkgs = Vec::new();
        for line in text.lines() {
            // Format: "ii pkgname-1.2.3_1  Short description"
            let mut parts = line.split_whitespace();
            let _state = parts.next();
            if let Some(pkg_ver) = parts.next() {
                let desc = parts.collect::<Vec<_>>().join(" ");
                let (name, version) = match pkg_ver.rsplit_once('-') {
                    Some((n, v)) => (n.to_string(), v.to_string()),
                    None => (pkg_ver.to_string(), String::new()),
                };
                pkgs.push(PackageInfo {
                    name,
                    version,
                    description: desc,
                });
            }
        }
        pkgs
    }

    pub fn install(pkg: &str) -> Result<String, String> {
        let out = Command::new("xbps-install")
            .args(["-Sy", pkg])
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).into_owned())
        }
    }

    pub fn remove(pkg: &str) -> Result<String, String> {
        let out = Command::new("xbps-remove")
            .args(["-y", pkg])
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).into_owned())
        }
    }
}

/// Wrapper around Void Linux `runit` (`/etc/sv`, `/var/service`, `sv`).
pub struct RunitManager;

impl RunitManager {
    pub fn list_services() -> Vec<ServiceInfo> {
        let mut services = Vec::new();
        let Ok(entries) = fs::read_dir("/etc/sv") else {
            return services;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let enabled = Path::new("/var/service").join(&name).exists();
            let status = if enabled {
                Self::query_status(&name)
            } else {
                "disabled".to_string()
            };
            services.push(ServiceInfo {
                name,
                enabled,
                status,
            });
        }
        services.sort_by(|a, b| a.name.cmp(&b.name));
        services
    }

    pub fn query_status(name: &str) -> String {
        let Ok(out) = Command::new("sv").args(["status", name]).output() else {
            return "unknown".to_string();
        };
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() {
            "unknown".to_string()
        } else {
            s
        }
    }

    pub fn control(name: &str, action: &str) -> Result<String, String> {
        let out = Command::new("sv")
            .args([action, name])
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
}
