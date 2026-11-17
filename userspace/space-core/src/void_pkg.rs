//! PetraOS userspace package & service management helpers.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub installed: bool,
}

#[derive(Debug, Clone)]
pub struct ServiceInfo {
    pub name: String,
    pub enabled: bool,
    pub status: String,
}

/// Software catalog & installation manager for Applications Manager.
pub struct PackageManager;

impl PackageManager {
    fn parse_pkg_line(line: &str, installed: bool) -> Option<PackageInfo> {
        let mut parts = line.split_whitespace();
        let state = parts.next()?;
        let pkg_ver = parts.next()?;
        let desc = parts.collect::<Vec<_>>().join(" ");
        let (name, version) = match pkg_ver.rsplit_once('-') {
            Some((n, v)) => (n.to_string(), v.to_string()),
            None => (pkg_ver.to_string(), String::new()),
        };
        let is_installed = installed || state.starts_with("[*]") || state == "ii";
        Some(PackageInfo {
            name,
            version,
            description: desc,
            installed: is_installed,
        })
    }

    pub fn list_installed() -> Vec<PackageInfo> {
        let Ok(out) = Command::new("xbps-query").arg("-l").output() else {
            return Vec::new();
        };
        if !out.status.success() {
            return Vec::new();
        }
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .filter_map(|line| Self::parse_pkg_line(line, true))
            .collect()
    }

    /// Lists installed packages merged with any locally cached repository catalog.
    pub fn list_catalog() -> Vec<PackageInfo> {
        let installed = Self::list_installed();
        let installed_names: HashSet<String> = installed.iter().map(|p| p.name.clone()).collect();
        let mut catalog = installed;

        if let Ok(out) = Command::new("xbps-query").args(["-Rs", ""]).output() {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                for line in text.lines().take(512) {
                    if let Some(pkg) = Self::parse_pkg_line(line, false) {
                        if !installed_names.contains(&pkg.name) {
                            catalog.push(pkg);
                        }
                    }
                }
            }
        }
        catalog.sort_by(|a, b| a.name.cmp(&b.name));
        catalog
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

/// Wrapper around `runit` (`/etc/sv`, `/var/service`, `sv`).
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
