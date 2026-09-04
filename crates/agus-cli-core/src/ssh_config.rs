//! Parse OpenSSH `~/.ssh/config` into Agus hosts (best-effort, no ProxyJump expansion).

use std::fs;
use std::path::{Path, PathBuf};

use agus_core_domain::{Environment, Host};

use crate::CliError;

#[derive(Debug, Clone)]
pub struct SshConfigHost {
    pub alias: String,
    pub host: Host,
}

/// Load hosts from an OpenSSH config file. Resolves simple `Include` directives (depth-limited).
pub fn load_ssh_config_hosts(path: &Path) -> Result<Vec<SshConfigHost>, CliError> {
    let mut out = Vec::new();
    let mut visited = std::collections::HashSet::new();
    load_ssh_config_recursive(path, 0, &mut visited, &mut out)?;
    Ok(out)
}

fn load_ssh_config_recursive(
    path: &Path,
    depth: u8,
    visited: &mut std::collections::HashSet<PathBuf>,
    out: &mut Vec<SshConfigHost>,
) -> Result<(), CliError> {
    if depth > 5 {
        return Ok(());
    }
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !visited.insert(canonical.clone()) {
        return Ok(());
    }
    if !path.is_file() {
        return Err(CliError::InvalidInput(format!(
            "ssh config not found: {}",
            path.display()
        )));
    }
    let content = fs::read_to_string(path).map_err(|e| {
        CliError::InvalidInput(format!("failed to read {}: {}", path.display(), e))
    })?;

    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut current_aliases: Vec<String> = Vec::new();
    let mut hostname: Option<String> = None;
    let mut user: Option<String> = None;
    let mut port: Option<u16> = None;
    let mut identity_file: Option<String> = None;

    let flush = |aliases: &mut Vec<String>,
                 hostname: &mut Option<String>,
                 user: &mut Option<String>,
                 port: &mut Option<u16>,
                 identity_file: &mut Option<String>,
                 out: &mut Vec<SshConfigHost>| {
        if aliases.is_empty() {
            return;
        }
        for alias in aliases.drain(..) {
            if alias.contains('*') || alias.contains('?') {
                continue;
            }
            let address = hostname
                .clone()
                .unwrap_or_else(|| alias.clone())
                .trim()
                .to_string();
            if address.is_empty() {
                continue;
            }
            let id = format!("sshconfig:{}", sanitize_id(&alias));
            let mut labels = vec![
                "source:ssh_config".to_string(),
                format!("alias:{}", alias),
            ];
            if address != alias {
                labels.push(format!("hostname:{}", address));
            }
            out.push(SshConfigHost {
                alias: alias.clone(),
                host: Host {
                    id,
                    address,
                    environment: Environment::Prod,
                    labels,
                    user: user.clone().unwrap_or_else(|| "root".to_string()),
                    port: port.unwrap_or(22),
                    identity_file: identity_file.clone(),
                    password: None,
                    group_id: None,
                    health_check_url: None,
                },
            });
        }
        *hostname = None;
        *user = None;
        *port = None;
        *identity_file = None;
    };

    for raw_line in content.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let key = match parts.next() {
            Some(k) => k,
            None => continue,
        };
        let value = parts.collect::<Vec<_>>().join(" ");
        if value.is_empty() {
            continue;
        }
        let key_l = key.to_ascii_lowercase();
        match key_l.as_str() {
            "include" => {
                flush(
                    &mut current_aliases,
                    &mut hostname,
                    &mut user,
                    &mut port,
                    &mut identity_file,
                    out,
                );
                for pattern in value.split_whitespace() {
                    for included in expand_include_globs(base_dir, pattern) {
                        let _ = load_ssh_config_recursive(&included, depth + 1, visited, out);
                    }
                }
            }
            "host" => {
                flush(
                    &mut current_aliases,
                    &mut hostname,
                    &mut user,
                    &mut port,
                    &mut identity_file,
                    out,
                );
                current_aliases = value
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();
            }
            "hostname" => hostname = Some(expand_home(&value)),
            "user" => user = Some(value),
            "port" => {
                if let Ok(p) = value.parse::<u16>() {
                    port = Some(p);
                }
            }
            "identityfile" => identity_file = Some(expand_home(&value)),
            _ => {}
        }
    }
    flush(
        &mut current_aliases,
        &mut hostname,
        &mut user,
        &mut port,
        &mut identity_file,
        out,
    );
    Ok(())
}

fn expand_home(value: &str) -> String {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = dirs_home() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    if value == "~" {
        if let Some(home) = dirs_home() {
            return home.to_string_lossy().to_string();
        }
    }
    value.to_string()
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn expand_include_globs(base_dir: &Path, pattern: &str) -> Vec<PathBuf> {
    let expanded = expand_home(pattern);
    let path = PathBuf::from(&expanded);
    let absolute = if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    };
    // Minimal glob: only support trailing * filename patterns for safety.
    if let Some(parent) = absolute.parent() {
        if let Some(name) = absolute.file_name().and_then(|n| n.to_str()) {
            if name.contains('*') && !name.contains('/') {
                let prefix = name.trim_end_matches('*');
                let mut matches = Vec::new();
                if let Ok(entries) = fs::read_dir(parent) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if !p.is_file() {
                            continue;
                        }
                        if let Some(fname) = p.file_name().and_then(|n| n.to_str()) {
                            if fname.starts_with(prefix) {
                                matches.push(p);
                            }
                        }
                    }
                }
                return matches;
            }
        }
    }
    if absolute.is_file() {
        vec![absolute]
    } else {
        Vec::new()
    }
}

fn sanitize_id(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_basic_host_block() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(
            f,
            "Host myserver\n  HostName 1.2.3.4\n  User ubuntu\n  Port 2222\n  IdentityFile ~/.ssh/id_ed25519\n"
        )
        .unwrap();
        let hosts = load_ssh_config_hosts(&path).unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host.address, "1.2.3.4");
        assert_eq!(hosts[0].host.user, "ubuntu");
        assert_eq!(hosts[0].host.port, 2222);
        assert!(hosts[0]
            .host
            .identity_file
            .as_ref()
            .unwrap()
            .contains(".ssh/id_ed25519"));
    }
}
