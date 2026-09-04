//! Host SSH password resolution, encryption, and FinalShell migration.

use agus_secret_store::create_secret_store;
use agus_core_domain::Host;

use crate::finalshell::{decode_finalshell_password, looks_like_finalshell_password};
use crate::CliError;

pub const SECRET_REF_PREFIX: &str = "__secret__:";

fn secret_key_for_host(host_id: &str) -> String {
    format!("host.{host_id}.ssh_password")
}

fn secret_ref_for_host(host_id: &str) -> String {
    format!("{SECRET_REF_PREFIX}{}", secret_key_for_host(host_id))
}

fn is_secret_ref(value: &str) -> bool {
    value.starts_with(SECRET_REF_PREFIX)
}

fn secret_key_from_ref(value: &str) -> Option<&str> {
    value.strip_prefix(SECRET_REF_PREFIX)
}

/// Resolve a stored password field to plaintext for SSH authentication.
pub fn resolve_host_password(_host_id: &str, stored: Option<&str>) -> Option<String> {
    let Some(raw) = stored.map(str::trim).filter(|value| !value.is_empty()) else {
        return None;
    };

    if let Some(key) = secret_key_from_ref(raw) {
        let store = create_secret_store();
        return store.get_secret(key).ok();
    }

    if looks_like_finalshell_password(raw) {
        return decode_finalshell_password(raw);
    }

    Some(raw.to_string())
}

/// Encrypt plaintext password into secret store; returns the hosts.json marker value.
pub fn store_host_password(host_id: &str, plaintext: &str) -> Result<String, CliError> {
    let key = secret_key_for_host(host_id);
    let store = create_secret_store();
    store
        .store_secret(&key, plaintext)
        .map_err(|err| CliError::Config(format!("failed to store host password: {err}")))?;
    Ok(secret_ref_for_host(host_id))
}

/// Migrate one host: decrypt FinalShell blobs / plaintext into secret store references.
pub fn migrate_host_password(host: &mut Host) -> Result<bool, CliError> {
    let Some(raw) = host.password.as_ref().map(|value| value.trim().to_string()) else {
        return Ok(false);
    };
    if raw.is_empty() {
        host.password = None;
        return Ok(false);
    }
    if is_secret_ref(&raw) {
        return Ok(false);
    }

    let plaintext = if looks_like_finalshell_password(&raw) {
        decode_finalshell_password(&raw).ok_or_else(|| {
            CliError::Config(format!(
                "failed to decrypt FinalShell password for host '{}'",
                host.id
            ))
        })?
    } else {
        raw
    };

    host.password = Some(store_host_password(&host.id, &plaintext)?);
    Ok(true)
}

/// Migrate all hosts in-place when passwords are plaintext or FinalShell-encrypted.
pub fn migrate_hosts_passwords(hosts: &mut [Host]) -> Result<usize, CliError> {
    let mut migrated = 0usize;
    for host in hosts.iter_mut() {
        if migrate_host_password(host)? {
            migrated += 1;
        }
    }
    Ok(migrated)
}

/// Apply resolved password onto a host record for SSH execution.
pub fn host_with_resolved_password(host: &Host) -> Host {
    let mut resolved = host.clone();
    resolved.password = resolve_host_password(&host.id, host.password.as_deref());
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env_lock;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_home() -> (String, std::path::PathBuf) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home = std::env::temp_dir().join(format!("agus-host-pass-{stamp}"));
        let _ = std::fs::remove_dir_all(&home);
        std::env::set_var("AGUS_HOME", &home);
        (format!("host-{stamp}"), home)
    }

    #[test]
    fn migrates_plaintext_to_secret_ref() {
        let _lock = env_lock();
        let (host_id, home) = temp_home();
        let mut host = Host {
            id: host_id.clone(),
            address: "127.0.0.1".to_string(),
            user: "root".to_string(),
            port: 22,
            environment: agus_core_domain::Environment::Dev,
            labels: vec![],
            identity_file: None,
            password: Some("plain-secret".to_string()),
            group_id: None,
            health_check_url: None,
        };
        assert!(migrate_host_password(&mut host).unwrap());
        assert!(host.password.as_deref().unwrap().starts_with(SECRET_REF_PREFIX));
        assert_eq!(
            resolve_host_password(&host_id, host.password.as_deref()).as_deref(),
            Some("plain-secret")
        );
        std::env::remove_var("AGUS_HOME");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn migrates_finalshell_blob() {
        let _lock = env_lock();
        let (host_id, home) = temp_home();
        let mut host = Host {
            id: host_id.clone(),
            address: "127.0.0.1".to_string(),
            user: "root".to_string(),
            port: 22,
            environment: agus_core_domain::Environment::Dev,
            labels: vec![],
            identity_file: None,
            password: Some("OGNqLj1Le11Br3AIelAiPaDJpfhBzmEN".to_string()),
            group_id: None,
            health_check_url: None,
        };
        assert!(migrate_host_password(&mut host).unwrap());
        assert_eq!(
            resolve_host_password(&host_id, host.password.as_deref()).as_deref(),
            Some("beac3d85988e")
        );
        std::env::remove_var("AGUS_HOME");
        let _ = std::fs::remove_dir_all(home);
    }
}
