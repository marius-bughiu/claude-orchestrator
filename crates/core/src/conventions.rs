//! Per-project orchestration conventions.
//!
//! Each managed project may contain an `.orchestrator/` directory whose files
//! steer autonomous behavior. All files are optional — sensible defaults are
//! embedded here and used when a file is absent, so any git repo works out of the
//! box, while power users can override behavior per project.
//!
//! Files:
//! - `config.json` — per-project orchestrator config (currently advisory).
//! - `roadmap.md` — prompt for the roadmap loop (generates new tasks when the
//!   queue empties).
//! - `verify.md` — prompt for the verifier (judges whether a finished task met
//!   its goal).
//! - `task.md` — preamble prepended to every task prompt for this project.

use crate::error::Result;
use std::fs;
use std::path::{Path, PathBuf};

pub const DIR_NAME: &str = ".orchestrator";

pub const DEFAULT_ROADMAP: &str = include_str!("templates/roadmap.md");
pub const DEFAULT_VERIFY: &str = include_str!("templates/verify.md");
pub const DEFAULT_TASK: &str = include_str!("templates/task.md");
pub const DEFAULT_CONFIG: &str = include_str!("templates/config.json");
pub const DEFAULT_README: &str = include_str!("templates/README.md");

/// The `.orchestrator` directory inside a project.
pub fn dir(project_path: impl AsRef<Path>) -> PathBuf {
    project_path.as_ref().join(DIR_NAME)
}

fn read_file(project_path: &Path, name: &str) -> Option<String> {
    fs::read_to_string(dir(project_path).join(name)).ok()
}

/// Roadmap-loop prompt: file contents if present, else the embedded default.
pub fn roadmap_prompt(project_path: impl AsRef<Path>) -> String {
    read_file(project_path.as_ref(), "roadmap.md").unwrap_or_else(|| DEFAULT_ROADMAP.to_string())
}

/// Verification prompt: file contents if present, else the embedded default.
pub fn verify_prompt(project_path: impl AsRef<Path>) -> String {
    read_file(project_path.as_ref(), "verify.md").unwrap_or_else(|| DEFAULT_VERIFY.to_string())
}

/// Optional preamble prepended to every task prompt for a project.
pub fn task_preamble(project_path: impl AsRef<Path>) -> Option<String> {
    read_file(project_path.as_ref(), "task.md")
}

/// True if the project already has an `.orchestrator` directory.
pub fn is_initialized(project_path: impl AsRef<Path>) -> bool {
    dir(project_path).is_dir()
}

/// Write the default convention files into a project, without overwriting any
/// that already exist. Returns the list of files created (relative paths).
pub fn scaffold(project_path: impl AsRef<Path>) -> Result<Vec<String>> {
    let base = dir(&project_path);
    fs::create_dir_all(&base)?;
    let files = [
        ("README.md", DEFAULT_README),
        ("config.json", DEFAULT_CONFIG),
        ("roadmap.md", DEFAULT_ROADMAP),
        ("verify.md", DEFAULT_VERIFY),
        ("task.md", DEFAULT_TASK),
    ];
    let mut created = Vec::new();
    for (name, contents) in files {
        let path = base.join(name);
        if !path.exists() {
            fs::write(&path, contents)?;
            created.push(format!("{DIR_NAME}/{name}"));
        }
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_used_when_missing() {
        let dir = std::env::temp_dir().join(format!("orch-conv-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!is_initialized(&dir));
        assert_eq!(roadmap_prompt(&dir), DEFAULT_ROADMAP);
        assert!(task_preamble(&dir).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scaffold_creates_then_preserves() {
        let dir = std::env::temp_dir().join(format!("orch-conv-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let created = scaffold(&dir).unwrap();
        assert!(created.iter().any(|f| f.ends_with("roadmap.md")));
        assert!(is_initialized(&dir));
        // Override a file, re-scaffold, ensure it is not clobbered.
        let roadmap = super::dir(&dir).join("roadmap.md");
        std::fs::write(&roadmap, "custom").unwrap();
        let created2 = scaffold(&dir).unwrap();
        assert!(created2.is_empty());
        assert_eq!(std::fs::read_to_string(&roadmap).unwrap(), "custom");
        std::fs::remove_dir_all(&dir).ok();
    }
}
