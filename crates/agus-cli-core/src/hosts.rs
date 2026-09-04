use agus_core_domain::Host;
use agus_storage::{JsonFileStorage, StorageBackend};

use crate::host_password::{
    host_with_resolved_password, migrate_hosts_passwords, store_host_password,
    SECRET_REF_PREFIX,
};
use crate::{config, CliError};

fn storage() -> Result<Box<dyn StorageBackend>, CliError> {
    let base = config::agus_home()?;
    std::fs::create_dir_all(&base)?;
    Ok(Box::new(agus_storage::JsonFileStorage::new(base)))
}

pub fn migrate_all_host_passwords() -> Result<usize, CliError> {
    let storage = storage()?;
    let mut hosts = storage.load_hosts()?;
    let migrated = migrate_hosts_passwords(&mut hosts)?;
    if migrated > 0 {
        storage.save_hosts(&hosts)?;
    }
    Ok(migrated)
}

pub fn load_hosts() -> Result<Vec<Host>, CliError> {
    let storage = storage()?;
    let mut hosts = storage.load_hosts()?;
    let migrated = migrate_hosts_passwords(&mut hosts)?;
    if migrated > 0 {
        storage.save_hosts(&hosts)?;
    }
    Ok(hosts)
}

pub fn load_hosts_raw() -> Result<Vec<Host>, CliError> {
    let storage = storage()?;
    storage.load_hosts().map_err(Into::into)
}

pub fn save_hosts(hosts: &[Host]) -> Result<(), CliError> {
    let storage = storage()?;
    storage.save_hosts(hosts)?;
    Ok(())
}

pub fn find_host(host_id: &str) -> Result<Host, CliError> {
    let hosts = load_hosts()?;
    hosts
        .into_iter()
        .find(|host| host.id == host_id || host.address == host_id)
        .map(|host| host_with_resolved_password(&host))
        .ok_or_else(|| CliError::InvalidInput(format!("host not found: {host_id}")))
}

pub fn prepare_host_for_storage(mut host: Host) -> Result<Host, CliError> {
    if let Some(password) = host.password.as_ref().map(|value| value.trim().to_string()) {
        if password.is_empty() {
            host.password = None;
        } else if password.starts_with(SECRET_REF_PREFIX) {
            host.password = Some(password);
        } else {
            host.password = Some(store_host_password(&host.id, &password)?);
        }
    }
    Ok(host)
}

pub fn upsert_host(host: Host) -> Result<bool, CliError> {
    let mut host = prepare_host_for_storage(host)?;
    let mut hosts = load_hosts_raw()?;
    if let Some(existing) = hosts.iter_mut().find(|item| item.id == host.id) {
        if host.password.is_none() {
            host.password = existing.password.clone();
        }
        *existing = host;
        save_hosts(&hosts)?;
        return Ok(true);
    }
    hosts.push(host);
    save_hosts(&hosts)?;
    Ok(false)
}

pub fn remove_host(host_id: &str) -> Result<bool, CliError> {
    let mut hosts = load_hosts()?;
    let original_len = hosts.len();
    hosts.retain(|host| host.id != host_id);
    if hosts.len() == original_len {
        return Ok(false);
    }
    save_hosts(&hosts)?;
    Ok(true)
}
