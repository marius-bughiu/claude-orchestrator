//! Higher-level operations used by the host (Tauri commands): project/task
//! creation with validation and defaults, convention scaffolding, etc.

use crate::conventions;
use crate::db::Db;
use crate::error::{CoreError, Result};
use crate::models::*;
use chrono::Utc;
use serde::Deserialize;
use std::path::Path;
use uuid::Uuid;

/// Input for adding a project.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddProjectInput {
    pub path: String,
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub scaffold: bool,
}

/// Input for creating a task.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskInput {
    pub project_id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub agent: Option<AgentKind>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub max_attempts: Option<u32>,
}

fn default_true() -> bool {
    true
}

/// Validate and register a project. Verifies the path is an existing directory;
/// warns (does not fail) if it is not a git repository. Optionally scaffolds the
/// `.orchestrator/` convention files.
pub fn add_project(db: &Db, input: AddProjectInput) -> Result<Project> {
    let path = Path::new(&input.path);
    if !path.is_dir() {
        return Err(CoreError::invalid(format!(
            "{} is not a directory",
            input.path
        )));
    }
    let canonical = path
        .canonicalize()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| input.path.clone());

    // Reject duplicates by path.
    if db.list_projects()?.iter().any(|p| p.path == canonical) {
        return Err(CoreError::invalid("a project with that path already exists"));
    }

    let name = input.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".into())
    });

    let is_git = path.join(".git").exists();
    let now = Utc::now();
    let project = Project {
        id: Uuid::new_v4().to_string(),
        name,
        path: canonical,
        description: input.description,
        enabled: true,
        default_agent: AgentKind::Claude,
        max_concurrent: None,
        roadmap_enabled: true,
        verify_enabled: true,
        created_at: now,
        updated_at: now,
    };
    db.upsert_project(&project)?;

    if input.scaffold {
        let _ = conventions::scaffold(&project.path);
    }
    let _ = is_git; // git status is advisory for now.
    Ok(project)
}

/// Create a task with sensible defaults.
pub fn create_task(db: &Db, input: CreateTaskInput) -> Result<Task> {
    if input.title.trim().is_empty() {
        return Err(CoreError::invalid("task title is required"));
    }
    // Ensure the project exists.
    let project = db.get_project(&input.project_id)?;
    let now = Utc::now();
    let task = Task {
        id: Uuid::new_v4().to_string(),
        project_id: project.id,
        title: input.title,
        description: input.description,
        status: TaskStatus::Pending,
        priority: input.priority.unwrap_or(50),
        agent: input.agent.unwrap_or(project.default_agent),
        parent_id: None,
        depends_on: input.depends_on,
        attempts: 0,
        max_attempts: input.max_attempts.unwrap_or(3),
        tags: input.tags,
        auto_generated: false,
        created_at: now,
        updated_at: now,
    };
    db.upsert_task(&task)?;
    Ok(task)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_project_rejects_nonexistent_path() {
        let db = Db::open_in_memory().unwrap();
        let err = add_project(
            &db,
            AddProjectInput {
                path: "/no/such/dir/xyz".into(),
                name: None,
                description: None,
                scaffold: false,
            },
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::Invalid(_)));
    }

    #[test]
    fn add_project_and_create_task() {
        let db = Db::open_in_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("orch-svc-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let project = add_project(
            &db,
            AddProjectInput {
                path: dir.to_string_lossy().into_owned(),
                name: Some("demo".into()),
                description: None,
                scaffold: true,
            },
        )
        .unwrap();
        assert_eq!(project.name, "demo");
        assert!(conventions::is_initialized(&dir));

        let task = create_task(
            &db,
            CreateTaskInput {
                project_id: project.id.clone(),
                title: "do a thing".into(),
                description: "details".into(),
                priority: Some(100),
                agent: None,
                depends_on: vec![],
                tags: vec!["x".into()],
                max_attempts: None,
            },
        )
        .unwrap();
        assert_eq!(task.priority, 100);
        assert_eq!(task.agent, AgentKind::Claude);
        std::fs::remove_dir_all(&dir).ok();
    }
}
