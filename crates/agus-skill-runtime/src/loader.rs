use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::manifest::SkillManifest;
use crate::playbook::Playbook;

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("yaml: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("invalid skill at {0}: {1}")]
    Invalid(PathBuf, String),
}

#[derive(Debug, Clone)]
pub struct SkillPackage {
    pub root: PathBuf,
    pub manifest: SkillManifest,
    pub playbook: Playbook,
    pub analyze_prompt: Option<String>,
}

pub fn load_skill_dir(dir: impl AsRef<Path>) -> Result<SkillPackage, LoadError> {
    let root = dir.as_ref().to_path_buf();
    let manifest_path = root.join("AGUS_SKILL.toml");
    if !manifest_path.is_file() {
        return Err(LoadError::Invalid(
            root,
            "missing AGUS_SKILL.toml".into(),
        ));
    }
    let manifest_raw = fs::read_to_string(&manifest_path)?;
    let manifest: SkillManifest = toml::from_str(&manifest_raw)?;

    if manifest.id.trim().is_empty() {
        return Err(LoadError::Invalid(root, "manifest.id empty".into()));
    }
    if manifest.permissions.is_empty() {
        return Err(LoadError::Invalid(
            root.clone(),
            "manifest.permissions empty".into(),
        ));
    }
    // Execute listed alone without propose is invalid — must go through proposal gate.
    if manifest
        .permissions
        .iter()
        .any(|p| matches!(p, crate::Permission::Execute))
        && !manifest
            .permissions
            .iter()
            .any(|p| matches!(p, crate::Permission::ProposeExecute))
    {
        return Err(LoadError::Invalid(
            root,
            "permission execute requires propose_execute".into(),
        ));
    }

    let playbook_path = root.join("playbook.yaml");
    if !playbook_path.is_file() {
        return Err(LoadError::Invalid(root, "missing playbook.yaml".into()));
    }
    let playbook_raw = fs::read_to_string(&playbook_path)?;
    let playbook: Playbook = serde_yaml::from_str(&playbook_raw)?;
    if playbook.steps.is_empty() {
        return Err(LoadError::Invalid(root, "playbook.steps empty".into()));
    }

    let prompt_path = root.join("prompts").join("analyze.md");
    let analyze_prompt = if prompt_path.is_file() {
        Some(fs::read_to_string(prompt_path)?)
    } else {
        None
    };

    Ok(SkillPackage {
        root,
        manifest,
        playbook,
        analyze_prompt,
    })
}

/// Discover skill packages under a root (one directory per skill).
pub fn discover_skills(root: impl AsRef<Path>) -> Result<Vec<SkillPackage>, LoadError> {
    let root = root.as_ref();
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(root)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.join("AGUS_SKILL.toml").is_file() {
            out.push(load_skill_dir(&path)?);
        }
    }
    Ok(out)
}
