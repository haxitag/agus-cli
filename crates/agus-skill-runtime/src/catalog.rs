use std::collections::HashMap;
use std::path::Path;

use crate::loader::{discover_skills, LoadError, SkillPackage};
use crate::{builtin_skills_dir, user_skills_dir};

/// Merged view of builtin + user skill packages (user overrides same id).
#[derive(Debug, Clone)]
pub struct SkillCatalog {
    packages: Vec<SkillPackage>,
}

impl SkillCatalog {
    /// Discover skills from builtin dir and user dir; user wins on id collision.
    pub fn discover() -> Result<Self, LoadError> {
        let mut by_id: HashMap<String, SkillPackage> = HashMap::new();

        if let Some(dir) = builtin_skills_dir() {
            for pkg in discover_skills(&dir)? {
                by_id.insert(pkg.manifest.id.clone(), pkg);
            }
        }

        if let Some(dir) = user_skills_dir() {
            if dir.is_dir() {
                for pkg in discover_skills(&dir)? {
                    by_id.insert(pkg.manifest.id.clone(), pkg);
                }
            }
        }

        let mut packages: Vec<_> = by_id.into_values().collect();
        packages.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
        Ok(Self { packages })
    }

    /// Discover from explicit roots (tests / injection).
    pub fn from_roots(roots: &[impl AsRef<Path>]) -> Result<Self, LoadError> {
        let mut by_id: HashMap<String, SkillPackage> = HashMap::new();
        for root in roots {
            if !root.as_ref().is_dir() {
                continue;
            }
            for pkg in discover_skills(root)? {
                by_id.insert(pkg.manifest.id.clone(), pkg);
            }
        }
        let mut packages: Vec<_> = by_id.into_values().collect();
        packages.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
        Ok(Self { packages })
    }

    pub fn list_packages(&self) -> &[SkillPackage] {
        &self.packages
    }

    pub fn find_package(&self, id: &str) -> Option<&SkillPackage> {
        self.packages.iter().find(|p| p.manifest.id == id)
    }
}
