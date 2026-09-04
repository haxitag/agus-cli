//! One-click import of hosts (and keys) from other terminal tools.
//!
//! Supported sources (kept intentionally small):
//! - FinalShell (`~/Library/FinalShell` on macOS)
//! - OpenSSH `~/.ssh/config`

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use agus_core_domain::{Environment, Host};
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::finalshell::{decode_finalshell_password, looks_like_finalshell_password};
use crate::hosts::{load_hosts_raw, upsert_host};
use crate::ssh_config::load_ssh_config_hosts;
use crate::{config, CliError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalImportSource {
    Finalshell,
    SshConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalImportCandidate {
    pub source: TerminalImportSource,
    pub source_ref: String,
    pub display_name: String,
    pub auth_summary: String,
    pub already_exists: bool,
    pub host: Host,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalImportResult {
    pub imported: usize,
    pub updated: usize,
    pub skipped: usize,
}

#[derive(Debug, Deserialize)]
struct FinalShellConn {
    id: Option<String>,
    name: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    user_name: Option<String>,
    password: Option<String>,
    secret_key_id: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    authentication_type: Option<i32>,
    #[serde(default)]
    delete_time: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct FinalShellConfig {
    #[serde(default)]
    secret_key_list: Vec<FinalShellSecretKey>,
}

#[derive(Debug, Deserialize)]
struct FinalShellSecretKey {
    id: String,
    name: Option<String>,
    key_data: String,
    #[serde(default)]
    delete_time: Option<i64>,
}

pub fn default_finalshell_root() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    #[cfg(target_os = "macos")]
    {
        let p = home.join("Library/FinalShell");
        if p.is_dir() {
            return Some(p);
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let p = PathBuf::from(appdata).join("finalshell");
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    let linux = home.join(".finalshell");
    if linux.is_dir() {
        return Some(linux);
    }
    None
}

pub fn default_ssh_config_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".ssh/config")
}

pub fn preview_finalshell_import(root: Option<PathBuf>) -> Result<Vec<TerminalImportCandidate>, CliError> {
    let root = root
        .or_else(default_finalshell_root)
        .ok_or_else(|| CliError::InvalidInput("FinalShell config directory not found".into()))?;
    let conn_dir = root.join("conn");
    if !conn_dir.is_dir() {
        return Err(CliError::InvalidInput(format!(
            "FinalShell conn directory not found: {}",
            conn_dir.display()
        )));
    }

    let key_map = materialize_finalshell_keys(&root)?;
    let existing = load_hosts_raw().unwrap_or_default();
    let existing_ids: std::collections::HashSet<_> =
        existing.iter().map(|h| h.id.clone()).collect();
    let existing_addrs: std::collections::HashSet<_> =
        existing.iter().map(|h| format!("{}@{}:{}", h.user, h.address, h.port)).collect();

    let mut candidates = Vec::new();
    for entry in fs::read_dir(&conn_dir).map_err(|e| {
        CliError::InvalidInput(format!("failed to read {}: {}", conn_dir.display(), e))
    })? {
        let entry = entry.map_err(|e| CliError::InvalidInput(e.to_string()))?;
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.ends_with("_connect_config.json") {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|e| {
            CliError::InvalidInput(format!("failed to read {}: {}", path.display(), e))
        })?;
        let conn: FinalShellConn = serde_json::from_str(&text).map_err(|e| {
            CliError::InvalidInput(format!("invalid FinalShell config {}: {}", path.display(), e))
        })?;
        if conn.delete_time.unwrap_or(0) > 0 {
            continue;
        }
        let address = conn.host.unwrap_or_default().trim().to_string();
        if address.is_empty() {
            continue;
        }
        let fs_id = conn
            .id
            .clone()
            .unwrap_or_else(|| name.trim_end_matches("_connect_config.json").to_string());
        let display_name = conn
            .name
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| address.clone());
        let user = conn
            .user_name
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "root".to_string());
        let port = conn.port.unwrap_or(22);

        let secret_key_id = conn
            .secret_key_id
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let mut identity_file = secret_key_id
            .as_ref()
            .and_then(|id| key_map.get(id).cloned());
        let mut password = None;
        let mut auth_summary = "无凭据（导入后需补全）".to_string();

        if identity_file.is_some() {
            auth_summary = format!(
                "私钥: {}",
                secret_key_id.as_deref().unwrap_or("imported")
            );
        } else if let Some(raw) = conn.password.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty())
        {
            let plain = if looks_like_finalshell_password(raw) {
                decode_finalshell_password(raw).ok_or_else(|| {
                    CliError::InvalidInput(format!(
                        "failed to decrypt FinalShell password for {}",
                        display_name
                    ))
                })?
            } else {
                raw.to_string()
            };
            auth_summary = "密码认证".to_string();
            password = Some(plain);
            identity_file = None;
        }

        // Prefer key when both present (matches FinalShell key-auth sessions).
        if secret_key_id.is_some() && key_map.contains_key(secret_key_id.as_ref().unwrap()) {
            identity_file = key_map.get(secret_key_id.as_ref().unwrap()).cloned();
            password = None;
            auth_summary = format!("私钥: {}", secret_key_id.as_deref().unwrap_or(""));
        }

        let host_id = format!("finalshell:{}", fs_id);
        let already = existing_ids.contains(&host_id)
            || existing_addrs.contains(&format!("{user}@{address}:{port}"));

        candidates.push(TerminalImportCandidate {
            source: TerminalImportSource::Finalshell,
            source_ref: path.to_string_lossy().to_string(),
            display_name,
            auth_summary,
            already_exists: already,
            host: Host {
                id: host_id,
                address,
                environment: Environment::Prod,
                labels: vec![
                    "source:finalshell".to_string(),
                    format!("fs_id:{}", fs_id),
                ],
                user,
                port,
                identity_file,
                password,
                group_id: None,
                health_check_url: None,
            },
        });
    }

    candidates.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(candidates)
}

pub fn preview_ssh_config_import(
    config_path: Option<PathBuf>,
) -> Result<Vec<TerminalImportCandidate>, CliError> {
    let path = config_path.unwrap_or_else(default_ssh_config_path);
    let parsed = load_ssh_config_hosts(&path)?;
    let existing = load_hosts_raw().unwrap_or_default();
    let existing_ids: std::collections::HashSet<_> =
        existing.iter().map(|h| h.id.clone()).collect();
    let existing_addrs: std::collections::HashSet<_> = existing
        .iter()
        .map(|h| format!("{}@{}:{}", h.user, h.address, h.port))
        .collect();

    let mut candidates = Vec::new();
    for item in parsed {
        let already = existing_ids.contains(&item.host.id)
            || existing_addrs.contains(&format!(
                "{}@{}:{}",
                item.host.user, item.host.address, item.host.port
            ));
        let auth_summary = item
            .host
            .identity_file
            .as_ref()
            .map(|p| format!("私钥: {}", Path::new(p).file_name().and_then(|n| n.to_str()).unwrap_or(p)))
            .unwrap_or_else(|| "默认密钥 / 待补全".to_string());
        candidates.push(TerminalImportCandidate {
            source: TerminalImportSource::SshConfig,
            source_ref: path.to_string_lossy().to_string(),
            display_name: item.alias,
            auth_summary,
            already_exists: already,
            host: item.host,
        });
    }
    Ok(candidates)
}

pub fn commit_terminal_import(
    candidates: &[TerminalImportCandidate],
    overwrite: bool,
) -> Result<TerminalImportResult, CliError> {
    let mut imported = 0usize;
    let mut updated = 0usize;
    let mut skipped = 0usize;
    for candidate in candidates {
        if candidate.already_exists && !overwrite {
            skipped += 1;
            continue;
        }
        let updated_existing = upsert_host(candidate.host.clone())?;
        if updated_existing {
            updated += 1;
        } else {
            imported += 1;
        }
    }
    Ok(TerminalImportResult {
        imported,
        updated,
        skipped,
    })
}

/// Look up a FinalShell connection matching address/port/(optional user) and return
/// the materialized private-key path when that session uses key auth.
pub fn lookup_finalshell_identity(
    address: &str,
    port: u16,
    user: Option<&str>,
) -> Result<Option<String>, CliError> {
    let Some(root) = default_finalshell_root() else {
        return Ok(None);
    };
    let conn_dir = root.join("conn");
    if !conn_dir.is_dir() {
        return Ok(None);
    }
    let key_map = materialize_finalshell_keys(&root)?;
    let address = address.trim();
    let user_norm = user.map(|u| u.trim().to_lowercase()).filter(|u| !u.is_empty());

    let mut best: Option<(i32, String)> = None;
    for entry in fs::read_dir(&conn_dir).map_err(|e| {
        CliError::InvalidInput(format!("failed to read {}: {}", conn_dir.display(), e))
    })? {
        let entry = entry.map_err(|e| CliError::InvalidInput(e.to_string()))?;
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.ends_with("_connect_config.json") {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|e| {
            CliError::InvalidInput(format!("failed to read {}: {}", path.display(), e))
        })?;
        let conn: FinalShellConn = match serde_json::from_str(&text) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if conn.delete_time.unwrap_or(0) > 0 {
            continue;
        }
        let host = conn.host.as_deref().unwrap_or("").trim();
        if host != address {
            continue;
        }
        if conn.port.unwrap_or(22) != port {
            continue;
        }
        if let Some(ref want_user) = user_norm {
            let got = conn
                .user_name
                .as_deref()
                .unwrap_or("root")
                .trim()
                .to_lowercase();
            if got != *want_user {
                continue;
            }
        }
        let secret_key_id = conn
            .secret_key_id
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let Some(id) = secret_key_id else {
            continue;
        };
        let Some(key_path) = key_map.get(&id).cloned() else {
            continue;
        };
        // Prefer explicit key-auth sessions (FinalShell authentication_type=2).
        let score = match conn.authentication_type {
            Some(2) => 2,
            Some(_) => 1,
            None => 1,
        };
        match &best {
            Some((best_score, _)) if *best_score >= score => {}
            _ => best = Some((score, key_path)),
        }
    }
    Ok(best.map(|(_, path)| path))
}

fn materialize_finalshell_keys(root: &Path) -> Result<HashMap<String, String>, CliError> {
    let config_path = root.join("config.json");
    if !config_path.is_file() {
        return Ok(HashMap::new());
    }
    let text = fs::read_to_string(&config_path).map_err(|e| {
        CliError::InvalidInput(format!("failed to read {}: {}", config_path.display(), e))
    })?;
    let cfg: FinalShellConfig = serde_json::from_str(&text).map_err(|e| {
        CliError::InvalidInput(format!("invalid FinalShell config.json: {}", e))
    })?;

    let out_dir = config::agus_home()?.join("imported-keys").join("finalshell");
    fs::create_dir_all(&out_dir).map_err(|e| {
        CliError::InvalidInput(format!("failed to create {}: {}", out_dir.display(), e))
    })?;

    let mut map = HashMap::new();
    for key in cfg.secret_key_list {
        if key.delete_time.unwrap_or(0) > 0 {
            continue;
        }
        let raw = base64::engine::general_purpose::STANDARD
            .decode(key.key_data.trim())
            .map_err(|e| CliError::InvalidInput(format!("invalid key_data for {}: {}", key.id, e)))?;
        let safe_name = key
            .name
            .as_deref()
            .unwrap_or("key")
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let path = out_dir.join(format!("{}_{}", key.id, safe_name));
        fs::write(&path, &raw).map_err(|e| {
            CliError::InvalidInput(format!("failed to write {}: {}", path.display(), e))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
        map.insert(key.id, path.to_string_lossy().to_string());
    }
    Ok(map)
}
