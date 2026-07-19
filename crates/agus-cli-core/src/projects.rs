//! LocalProject persistence (`projects.json`) + Forge handoff upsert.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agus_core_domain::Environment;
use agus_forge_handoff::{
    build_project_draft, default_stub_scripts, load_handoff_file, parse_suggested_environment,
    repo_root_hash, resolve_handoff_path, ForgeOpsHandoff, HostHint, ProjectDraft, ScriptsMissing,
    HANDOFF_REL,
};
use serde::{Deserialize, Serialize};

use crate::{audit, config, hosts, CliError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectScripts {
    pub build: String,
    pub deploy_dev: String,
    pub deploy_prod: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalProject {
    pub id: String,
    pub name: String,
    pub repo_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_host_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remote_scan_dirs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_dir: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scripts: Option<ProjectScripts>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertFromHandoffResult {
    pub project: LocalProject,
    pub host_hint: HostHint,
    pub draft: ProjectDraft,
    pub created: bool,
    pub scripts_written: bool,
    pub handoff_path: String,
    pub repo_root_hash: String,
    pub warnings: Vec<String>,
    pub dry_run: bool,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn projects_path() -> Result<PathBuf, CliError> {
    Ok(config::ensure_home_dir()?.join("projects.json"))
}

pub fn load_projects() -> Result<Vec<LocalProject>, CliError> {
    let path = projects_path()?;
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path)?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&text)?)
}

pub fn save_projects(projects: &[LocalProject]) -> Result<(), CliError> {
    let path = projects_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(projects)?;
    fs::write(&path, format!("{body}\n"))?;
    Ok(())
}

fn host_pairs() -> Result<Vec<(String, Environment)>, CliError> {
    let list = hosts::load_hosts()?;
    Ok(list
        .into_iter()
        .map(|h| (h.id, h.environment))
        .collect())
}

fn write_stub_scripts(repo: &Path, scripts: &ProjectScripts) -> Result<(), CliError> {
    write_exec(repo.join("build.sh"), &scripts.build)?;
    write_exec(repo.join("deploy.dev.sh"), &scripts.deploy_dev)?;
    write_exec(repo.join("deploy.prod.sh"), &scripts.deploy_prod)?;
    Ok(())
}

fn write_exec(path: PathBuf, content: &str) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms)?;
    }
    Ok(())
}

/// Upsert LocalProject from Forge handoff JSON (or repo root containing it).
pub fn upsert_from_handoff(
    path_or_repo: &Path,
    host_override: Option<&str>,
    write_missing_scripts: bool,
    dry_run: bool,
) -> Result<UpsertFromHandoffResult, CliError> {
    let handoff_path = resolve_handoff_path(path_or_repo)
        .map_err(|e| CliError::InvalidInput(e.to_string()))?;
    let handoff = load_handoff_file(&handoff_path)
        .map_err(|e| CliError::InvalidInput(e.to_string()))?;

    let hosts = host_pairs()?;
    let draft = build_project_draft(&handoff, host_override, &hosts);
    let mut host_hint =
        agus_forge_handoff::pick_hosts_for_environment(&draft.suggested_environment, &hosts);
    if let Some(h) = host_override {
        host_hint.default_host_id = Some(h.to_string());
    }

    let mut warnings = host_hint.warnings.clone();
    if draft.scripts_missing.any() {
        warnings.push(format!(
            "missing deploy scripts under {} (build={} deploy_dev={} deploy_prod={}); use --write-scripts",
            draft.repo_path,
            draft.scripts_missing.build,
            draft.scripts_missing.deploy_dev,
            draft.scripts_missing.deploy_prod
        ));
    }

    let repo_path = PathBuf::from(&draft.repo_path);
    if !repo_path.is_dir() {
        return Err(CliError::InvalidInput(format!(
            "repo_root does not exist: {}",
            draft.repo_path
        )));
    }

    let mut projects = load_projects()?;
    let existing_idx = projects.iter().position(|p| p.id == draft.id);
    let created = existing_idx.is_none();
    let now = now_ms();

    let mut scripts_written = false;
    let scripts = if write_missing_scripts && draft.scripts_missing.any() {
        let (build, deploy_dev, deploy_prod) = default_stub_scripts(
            &draft.id,
            handoff.entrypoints.start.as_deref(),
        );
        let scripts = ProjectScripts {
            build,
            deploy_dev,
            deploy_prod,
        };
        if !dry_run {
            write_stub_scripts(&repo_path, &scripts)?;
            scripts_written = true;
        }
        Some(scripts)
    } else {
        existing_idx
            .and_then(|i| projects[i].scripts.clone())
    };

    let project = LocalProject {
        id: draft.id.clone(),
        name: draft.name.clone(),
        repo_path: draft.repo_path.clone(),
        default_host_id: draft.default_host_id.clone(),
        note: draft.note.clone(),
        remote_scan_dirs: Vec::new(),
        remote_dir: None,
        created_at: existing_idx
            .map(|i| projects[i].created_at)
            .unwrap_or(now),
        updated_at: now,
        scripts,
    };

    if !dry_run {
        if let Some(i) = existing_idx {
            let keep_remote = projects[i].remote_scan_dirs.clone();
            let keep_remote_dir = projects[i].remote_dir.clone();
            let mut p = project.clone();
            if p.remote_scan_dirs.is_empty() {
                p.remote_scan_dirs = keep_remote;
            }
            if p.remote_dir.is_none() {
                p.remote_dir = keep_remote_dir;
            }
            if p.scripts.is_none() {
                p.scripts = projects[i].scripts.clone();
            }
            projects[i] = p.clone();
            save_projects(&projects)?;
            let hash = repo_root_hash(&p.repo_path);
            audit::write_audit_log(
                "business",
                &format!(
                    "import_forge_ops_handoff project={} schema={} repo_hash={} host={:?} path={} created=false scripts_written={}",
                    p.id,
                    handoff.schema_version,
                    hash,
                    p.default_host_id,
                    handoff_path.display(),
                    scripts_written
                ),
            )?;
            return Ok(UpsertFromHandoffResult {
                project: p,
                host_hint,
                draft,
                created: false,
                scripts_written,
                handoff_path: handoff_path.to_string_lossy().to_string(),
                repo_root_hash: hash,
                warnings,
                dry_run: false,
            });
        }
        projects.push(project.clone());
        save_projects(&projects)?;
        let hash = repo_root_hash(&project.repo_path);
        audit::write_audit_log(
            "business",
            &format!(
                "import_forge_ops_handoff project={} schema={} repo_hash={} host={:?} path={} created=true scripts_written={}",
                project.id,
                handoff.schema_version,
                hash,
                project.default_host_id,
                handoff_path.display(),
                scripts_written
            ),
        )?;
    }

    Ok(UpsertFromHandoffResult {
        project,
        host_hint,
        created,
        scripts_written,
        handoff_path: handoff_path.to_string_lossy().to_string(),
        repo_root_hash: repo_root_hash(&draft.repo_path),
        warnings,
        dry_run,
        draft,
    })
}

pub fn load_handoff_for_repo(repo: &Path) -> Result<Option<ForgeOpsHandoff>, CliError> {
    let candidate = repo.join(HANDOFF_REL);
    if !candidate.is_file() {
        return Ok(None);
    }
    Ok(Some(
        load_handoff_file(&candidate).map_err(|e| CliError::InvalidInput(e.to_string()))?,
    ))
}

pub fn ensure_suggested_env_valid(env: &str) -> Result<Environment, CliError> {
    parse_suggested_environment(env).map_err(|e| CliError::InvalidInput(e.to_string()))
}

pub fn scripts_missing_summary(m: &ScriptsMissing) -> String {
    format!(
        "build={} deploy_dev={} deploy_prod={}",
        m.build, m.deploy_dev, m.deploy_prod
    )
}
