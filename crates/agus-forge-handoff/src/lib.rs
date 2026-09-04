//! Forge → Agus ops handoff: parse / validate `forge_ops_handoff.json` only (never Markdown).

use agus_core_domain::Environment;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: &str = "1";
pub const HANDOFF_REL: &str = "docs/ops/deploy/forge_ops_handoff.json";
pub const SOURCE: &str = "forge";
pub const KIND: &str = "ops_handoff";

#[derive(Debug, thiserror::Error)]
pub enum HandoffError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid handoff: {0}")]
    Invalid(String),
    #[error("secret rejected: {0}")]
    SecretRejected(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeOpsHandoff {
    pub schema_version: String,
    pub source: String,
    pub kind: String,
    pub generated_at: String,
    pub project: HandoffProject,
    pub delivery: HandoffDelivery,
    pub docs: HandoffDocs,
    pub entrypoints: HandoffEntrypoints,
    pub scripts_agus: HandoffScriptsAgus,
    #[serde(default)]
    pub containers: Option<HandoffContainers>,
    #[serde(default)]
    pub artifacts: Option<HandoffArtifacts>,
    pub agus_import_hints: HandoffAgusHints,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffProject {
    pub project_id: String,
    #[serde(default)]
    pub product_name: String,
    #[serde(default)]
    pub display_name: String,
    pub project_type: String,
    pub repo_root: String,
    #[serde(default)]
    pub works_meta_root: Option<String>,
    #[serde(default)]
    pub one_liner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffDelivery {
    pub delivery_tier: String,
    pub suggested_environment: String,
    #[serde(default)]
    pub persona_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffDocs {
    pub delivery_profile: String,
    pub deployment_runbook: String,
    pub ops_handoff: String,
    #[serde(default)]
    pub handoff_dir: String,
    #[serde(default)]
    pub environments_example: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HandoffEntrypoints {
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub build: Option<String>,
    #[serde(default)]
    pub health_check_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HandoffScriptsAgus {
    #[serde(default)]
    pub build: Option<String>,
    #[serde(default)]
    pub deploy_dev: Option<String>,
    #[serde(default)]
    pub deploy_prod: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HandoffContainers {
    #[serde(default)]
    pub dockerfile: Option<String>,
    #[serde(default)]
    pub compose: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HandoffArtifacts {
    #[serde(default)]
    pub latest_trace_id: Option<String>,
    #[serde(default)]
    pub l5_test_result_rel: Option<String>,
    #[serde(default)]
    pub pipeline_index_rel: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffAgusHints {
    pub local_project_name: String,
    pub repo_path_field: String,
    pub map_suggested_environment_to_host: bool,
    pub do_not_store_secrets_in_repo: bool,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub open_agus_prefills: Option<OpenAgusPrefills>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAgusPrefills {
    #[serde(default)]
    pub scheme: Option<String>,
    pub handoff_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostHint {
    pub suggested_environment: String,
    pub matched_host_ids: Vec<String>,
    pub default_host_id: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDraft {
    pub id: String,
    pub name: String,
    pub repo_path: String,
    pub note: Option<String>,
    pub default_host_id: Option<String>,
    pub suggested_environment: String,
    pub delivery_tier: String,
    pub scripts_missing: ScriptsMissing,
    pub script_paths: ScriptPaths,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScriptsMissing {
    pub build: bool,
    pub deploy_dev: bool,
    pub deploy_prod: bool,
}

impl ScriptsMissing {
    pub fn any(&self) -> bool {
        self.build || self.deploy_dev || self.deploy_prod
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScriptPaths {
    pub build: Option<String>,
    pub deploy_dev: Option<String>,
    pub deploy_prod: Option<String>,
}

/// Resolve handoff path: explicit file, or `{repo}/docs/ops/deploy/forge_ops_handoff.json`.
pub fn resolve_handoff_path(path_or_repo: &Path) -> Result<PathBuf, HandoffError> {
    if path_or_repo.is_file() {
        let name = path_or_repo
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if name.ends_with(".md") || name.ends_with(".markdown") {
            return Err(HandoffError::Invalid(
                "only forge_ops_handoff.json is accepted; do not parse Markdown delivery-profile"
                    .into(),
            ));
        }
        return Ok(path_or_repo.to_path_buf());
    }
    if path_or_repo.is_dir() {
        let candidate = path_or_repo.join(HANDOFF_REL);
        if candidate.is_file() {
            return Ok(candidate);
        }
        return Err(HandoffError::Invalid(format!(
            "missing {HANDOFF_REL} under {}",
            path_or_repo.display()
        )));
    }
    Err(HandoffError::Invalid(format!(
        "path not found: {}",
        path_or_repo.display()
    )))
}

pub fn load_handoff_file(path: &Path) -> Result<ForgeOpsHandoff, HandoffError> {
    let text = fs::read_to_string(path)?;
    parse_handoff_json(&text)
}

pub fn parse_handoff_json(text: &str) -> Result<ForgeOpsHandoff, HandoffError> {
    let trimmed = text.trim_start();
    if trimmed.starts_with('#') || trimmed.starts_with("---") {
        return Err(HandoffError::Invalid(
            "Markdown/YAML front matter rejected; only JSON forge_ops_handoff is accepted".into(),
        ));
    }
    let value: Value = serde_json::from_str(text)?;
    reject_secrets_in_value(&value, "")?;
    let handoff: ForgeOpsHandoff = serde_json::from_value(value)?;
    validate_handoff(&handoff)?;
    Ok(handoff)
}

pub fn validate_handoff(h: &ForgeOpsHandoff) -> Result<(), HandoffError> {
    if h.schema_version != SCHEMA_VERSION {
        return Err(HandoffError::Invalid(format!(
            "schema_version must be {SCHEMA_VERSION}, got {}",
            h.schema_version
        )));
    }
    if h.source != SOURCE {
        return Err(HandoffError::Invalid(format!(
            "source must be {SOURCE}, got {}",
            h.source
        )));
    }
    if h.kind != KIND {
        return Err(HandoffError::Invalid(format!(
            "kind must be {KIND}, got {}",
            h.kind
        )));
    }
    if h.project.project_id.trim().is_empty() {
        return Err(HandoffError::Invalid("project.project_id empty".into()));
    }
    if h.project.repo_root.trim().is_empty() {
        return Err(HandoffError::Invalid("project.repo_root empty".into()));
    }
    parse_suggested_environment(&h.delivery.suggested_environment)?;
    if !h.agus_import_hints.do_not_store_secrets_in_repo {
        return Err(HandoffError::SecretRejected(
            "agus_import_hints.do_not_store_secrets_in_repo must be true".into(),
        ));
    }
    Ok(())
}

pub fn parse_suggested_environment(value: &str) -> Result<Environment, HandoffError> {
    match value.trim().to_lowercase().as_str() {
        "dev" => Ok(Environment::Dev),
        "test" => Ok(Environment::Test),
        "staging" => Ok(Environment::Staging),
        "prod" => Ok(Environment::Prod),
        other => Err(HandoffError::Invalid(format!(
            "unknown suggested_environment: {other}"
        ))),
    }
}

const SECRET_KEY_FRAGMENTS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "api_key",
    "apikey",
    "access_key",
    "private_key",
    "privatekey",
    "token",
    "credential",
    "auth_header",
];

fn reject_secrets_in_value(value: &Value, path: &str) -> Result<(), HandoffError> {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let key_l = k.to_lowercase();
                let next = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                if SECRET_KEY_FRAGMENTS.iter().any(|f| key_l.contains(f)) {
                    // Allow boolean flags like do_not_store_secrets_in_repo
                    if matches!(v, Value::Bool(_)) {
                        continue;
                    }
                    if matches!(v, Value::String(s) if s.is_empty()) {
                        continue;
                    }
                    return Err(HandoffError::SecretRejected(format!(
                        "forbidden secret-like field at {next}"
                    )));
                }
                reject_secrets_in_value(v, &next)?;
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                reject_secrets_in_value(item, &format!("{path}[{i}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn repo_root_hash(repo_root: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(repo_root.as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_string()
}

pub fn pick_hosts_for_environment(
    suggested: &str,
    hosts: &[(String, Environment)],
) -> HostHint {
    let mut warnings = Vec::new();
    let env = match parse_suggested_environment(suggested) {
        Ok(e) => e,
        Err(e) => {
            warnings.push(e.to_string());
            return HostHint {
                suggested_environment: suggested.to_string(),
                matched_host_ids: vec![],
                default_host_id: None,
                warnings,
            };
        }
    };
    let matched: Vec<String> = hosts
        .iter()
        .filter(|(_, e)| *e == env)
        .map(|(id, _)| id.clone())
        .collect();
    let default_host_id = match matched.len() {
        0 => {
            warnings.push(format!(
                "no Host with environment={suggested}; set defaultHostId manually"
            ));
            None
        }
        1 => Some(matched[0].clone()),
        _ => {
            warnings.push(format!(
                "multiple Hosts match environment={suggested}; confirm defaultHostId"
            ));
            Some(matched[0].clone())
        }
    };
    HostHint {
        suggested_environment: suggested.to_string(),
        matched_host_ids: matched,
        default_host_id,
        warnings,
    }
}

pub fn build_project_draft(
    handoff: &ForgeOpsHandoff,
    host_override: Option<&str>,
    hosts: &[(String, Environment)],
) -> ProjectDraft {
    let hint = pick_hosts_for_environment(&handoff.delivery.suggested_environment, hosts);
    let default_host_id = host_override
        .map(|s| s.to_string())
        .or(hint.default_host_id.clone());

    let name = if !handoff.agus_import_hints.local_project_name.trim().is_empty() {
        handoff.agus_import_hints.local_project_name.clone()
    } else if !handoff.project.display_name.trim().is_empty() {
        handoff.project.display_name.clone()
    } else {
        handoff.project.project_id.clone()
    };

    let mut note_parts = Vec::new();
    if !handoff.project.one_liner.trim().is_empty() {
        note_parts.push(handoff.project.one_liner.clone());
    }
    note_parts.push(format!(
        "forge handoff schema={} tier={} env={} generated={}",
        handoff.schema_version,
        handoff.delivery.delivery_tier,
        handoff.delivery.suggested_environment,
        handoff.generated_at
    ));
    if let Some(tid) = handoff
        .artifacts
        .as_ref()
        .and_then(|a| a.latest_trace_id.clone())
    {
        note_parts.push(format!("forge_trace={tid}"));
    }

    let repo = PathBuf::from(&handoff.project.repo_root);
    let build_rel = handoff
        .scripts_agus
        .build
        .clone()
        .or_else(|| handoff.entrypoints.build.clone());
    let deploy_dev = handoff.scripts_agus.deploy_dev.clone();
    let deploy_prod = handoff.scripts_agus.deploy_prod.clone();

    let scripts_missing = ScriptsMissing {
        build: !rel_exists(&repo, build_rel.as_deref().unwrap_or("build.sh")),
        deploy_dev: !rel_exists(
            &repo,
            deploy_dev.as_deref().unwrap_or("deploy.dev.sh"),
        ),
        deploy_prod: !rel_exists(
            &repo,
            deploy_prod.as_deref().unwrap_or("deploy.prod.sh"),
        ),
    };

    ProjectDraft {
        id: handoff.project.project_id.clone(),
        name,
        repo_path: handoff.project.repo_root.clone(),
        note: Some(note_parts.join(" | ")),
        default_host_id,
        suggested_environment: handoff.delivery.suggested_environment.clone(),
        delivery_tier: handoff.delivery.delivery_tier.clone(),
        scripts_missing,
        script_paths: ScriptPaths {
            build: build_rel,
            deploy_dev,
            deploy_prod,
        },
    }
}

fn rel_exists(repo: &Path, rel: &str) -> bool {
    let p = repo.join(rel);
    p.is_file()
}

/// Deep-link: `agus://import-handoff?file=<abs>` or `agus://import-handoff/<path>`.
pub fn parse_import_handoff_deeplink(url: &str) -> Option<PathBuf> {
    let url = url.trim();
    let rest = url.strip_prefix("agus://import-handoff")?;
    let rest = rest.trim_start_matches('/');
    if let Some(q) = rest.strip_prefix('?') {
        for part in q.split('&') {
            if let Some(file) = part
                .strip_prefix("file=")
                .or_else(|| part.strip_prefix("path="))
            {
                let decoded = urlencoding_decode(file);
                return Some(PathBuf::from(decoded));
            }
        }
        return None;
    }
    if rest.is_empty() {
        return None;
    }
    Some(PathBuf::from(urlencoding_decode(rest)))
}

fn urlencoding_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(a), Some(b)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((a << 4 | b) as char);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(' ');
        } else {
            out.push(bytes[i] as char);
        }
        i += 1;
    }
    out
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn handoff_plan_memo(handoff: &ForgeOpsHandoff) -> String {
    format!(
        "Forge ops handoff: project={} type={} tier={} suggested_env={} start={:?} dockerfile={:?} compose={:?} scripts_build={:?} deploy_dev={:?} deploy_prod={:?}",
        handoff.project.project_id,
        handoff.project.project_type,
        handoff.delivery.delivery_tier,
        handoff.delivery.suggested_environment,
        handoff.entrypoints.start,
        handoff.containers.as_ref().and_then(|c| c.dockerfile.as_ref()),
        handoff.containers.as_ref().map(|c| &c.compose),
        handoff.scripts_agus.build,
        handoff.scripts_agus.deploy_dev,
        handoff.scripts_agus.deploy_prod,
    )
}

/// Stub shell scripts when missing (no secrets).
/// Marked with `AGUS_SCRIPT_KIND=stub` so Agus can detect placeholders and refuse silent “success”.
pub fn default_stub_scripts(project_id: &str, start: Option<&str>) -> (String, String, String) {
    let start_cmd = start.unwrap_or("./start.sh");
    let build = format!(
        "#!/usr/bin/env bash\nset -euo pipefail\n# AGUS_SCRIPT_KIND=stub\n# Agus stub from Forge handoff ({project_id})\necho \"[build] stub — replace with real build\" >&2\nexit 1\n"
    );
    let deploy_dev = format!(
        "#!/usr/bin/env bash\nset -euo pipefail\n# AGUS_SCRIPT_KIND=stub\n# Agus stub deploy.dev ({project_id})\necho \"[deploy.dev] stub — replace before deploy; intended start: {start_cmd}\" >&2\nexit 1\n"
    );
    let deploy_prod = format!(
        "#!/usr/bin/env bash\nset -euo pipefail\n# AGUS_SCRIPT_KIND=stub\n# Agus stub deploy.prod ({project_id})\necho \"[deploy.prod] stub — requires real script + approval\" >&2\nexit 1\n"
    );
    (build, deploy_dev, deploy_prod)
}

/// Detect Agus-generated placeholder deploy/build scripts (current + legacy markers).
pub fn is_agus_stub_script(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    content.contains("AGUS_SCRIPT_KIND=stub")
        || content.contains("# Agus stub")
        || lower.contains("agus stub from forge")
        || lower.contains("[build] stub")
        || lower.contains("[deploy.dev] stub")
        || lower.contains("[deploy.prod] stub")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_json() -> String {
        r#"{
          "schema_version": "1",
          "source": "forge",
          "kind": "ops_handoff",
          "generated_at": "2026-07-19T00:00:00Z",
          "project": {
            "project_id": "demo",
            "product_name": "Demo",
            "display_name": "Demo",
            "project_type": "web_app",
            "repo_root": "/tmp/demo",
            "one_liner": "hi"
          },
          "delivery": {
            "delivery_tier": "mvp",
            "suggested_environment": "staging",
            "persona_tags": []
          },
          "docs": {
            "delivery_profile": "docs/product/delivery-profile.md",
            "deployment_runbook": "docs/ops/deploy/deployment-runbook.md",
            "ops_handoff": "docs/ops/deploy/forge_ops_handoff.json"
          },
          "entrypoints": { "start": "start.sh" },
          "scripts_agus": {},
          "agus_import_hints": {
            "local_project_name": "Demo",
            "repo_path_field": "repoPath",
            "map_suggested_environment_to_host": true,
            "do_not_store_secrets_in_repo": true
          }
        }"#
        .to_string()
    }

    #[test]
    fn parses_valid_handoff() {
        let h = parse_handoff_json(&sample_json()).unwrap();
        assert_eq!(h.project.project_id, "demo");
        assert_eq!(h.delivery.suggested_environment, "staging");
    }

    #[test]
    fn rejects_markdown() {
        let err = parse_handoff_json("# title\n").unwrap_err();
        assert!(matches!(err, HandoffError::Invalid(_)));
    }

    #[test]
    fn stub_scripts_are_detectable_and_fail_closed() {
        let (build, deploy_dev, deploy_prod) = default_stub_scripts("demo", Some("./start.sh"));
        assert!(is_agus_stub_script(&build));
        assert!(is_agus_stub_script(&deploy_dev));
        assert!(is_agus_stub_script(&deploy_prod));
        assert!(build.contains("exit 1"));
        assert!(deploy_dev.contains("exit 1"));
        assert!(deploy_prod.contains("exit 1"));
        assert!(!is_agus_stub_script("#!/bin/bash\ndocker compose up -d\n"));
    }

    #[test]
    fn rejects_secret_fields() {
        let mut v: Value = serde_json::from_str(&sample_json()).unwrap();
        v.as_object_mut()
            .unwrap()
            .insert("api_key".into(), Value::String("sk-secret".into()));
        let err = reject_secrets_in_value(&v, "").unwrap_err();
        assert!(matches!(err, HandoffError::SecretRejected(_)));
    }

    #[test]
    fn deeplink_parses_file_query() {
        let p = parse_import_handoff_deeplink(
            "agus://import-handoff?file=%2Ftmp%2Fdemo%2Fdocs%2Fops%2Fdeploy%2Fforge_ops_handoff.json",
        )
        .unwrap();
        assert!(p.to_string_lossy().contains("forge_ops_handoff.json"));
    }

    #[test]
    fn host_pick_single() {
        let hosts = vec![
            ("h1".into(), Environment::Staging),
            ("h2".into(), Environment::Prod),
        ];
        let hint = pick_hosts_for_environment("staging", &hosts);
        assert_eq!(hint.default_host_id.as_deref(), Some("h1"));
    }
}
