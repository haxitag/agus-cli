//! Agus Ops Skill Runtime
//!
//! Controlled Automation spine:
//! observe / analyze freely → propose changes → **human approval** → execute via existing
//! Agus executor (not from this crate) → verify with fresh evidence.
//!
//! This crate intentionally does **not** SSH or approve. It loads skill packages, evaluates
//! permissions, and produces structured runs / proposals that higher layers gate.

mod catalog;
mod loader;
mod manifest;
mod playbook;
mod proposal;
mod runtime;
mod service;
mod store;

pub use catalog::SkillCatalog;
pub use loader::{discover_skills, load_skill_dir, SkillPackage};
pub use manifest::{
    Permission, RiskClass, SkillManifest, SkillMaturity, SkillTrigger, TriggerKind,
};
pub use playbook::{Playbook, PlaybookStep, StepKind};
pub use proposal::{
    ExecProposal, ProposalStatus, SkillEvidence, SkillReport, VerificationVerdict,
};
pub use runtime::{SkillRun, SkillRunStatus, SkillRuntime};
pub use service::{resolve_agus_home, SkillService, SkillSummary, ServiceError};
pub use store::{SkillRunStore, StoredSkillRun, StoreError};

/// Builtin skill packages shipped with Agus (`skills/` at workspace root, or installed share dir).
pub fn builtin_skills_dir() -> Option<std::path::PathBuf> {
    // Prefer env override for packs / tests / custom installs.
    if let Ok(dir) = std::env::var("AGUS_SKILLS_DIR") {
        let p = std::path::PathBuf::from(dir);
        if p.is_dir() {
            return Some(p);
        }
    }

    // Installed CLI share dir (install_cli.sh copies skills here).
    if let Some(home) = dirs_next_home() {
        for candidate in [
            home.join(".agus").join("share").join("skills"),
            home.join(".local").join("share").join("agus").join("skills"),
        ] {
            if candidate.is_dir() {
                return Some(candidate);
            }
        }
    }

    // Next to the running binary (CLI unpack layout / macOS .app Resources).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let candidates = [
                exe_dir.join("skills"),
                exe_dir.join("../skills"),
                exe_dir.join("../share/agus/skills"),
                exe_dir.join("../Resources/skills"),
                exe_dir.join("../Resources/resources/skills"),
            ];
            for c in candidates {
                if let Ok(canonical) = c.canonicalize() {
                    if canonical.is_dir() {
                        return Some(canonical);
                    }
                } else if c.is_dir() {
                    return Some(c);
                }
            }
        }
    }

    // Walk up from CARGO_MANIFEST_DIR during tests / local source builds.
    let mut dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..4 {
        let candidate = dir.join("skills");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// Default user-extensible skill root: `$AGUS_HOME/skills` or `~/.agus/skills`.
pub fn user_skills_dir() -> Option<std::path::PathBuf> {
    if let Ok(home) = std::env::var("AGUS_HOME") {
        let p = std::path::PathBuf::from(home).join("skills");
        return Some(p);
    }
    dirs_next_home().map(|h| h.join(".agus").join("skills"))
}

fn dirs_next_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
}
