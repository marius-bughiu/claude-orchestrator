//! Higher-level operations used by the host (Tauri commands): project/task
//! creation with validation and defaults, convention scaffolding, etc.

use crate::config::Settings;
use crate::conventions;
use crate::db::Db;
use crate::error::{CoreError, Result};
use crate::models::*;
use chrono::Utc;
use serde::{Deserialize, Serialize};
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
    pub model: Option<String>,
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
        return Err(CoreError::invalid(
            "a project with that path already exists",
        ));
    }

    let name = input
        .name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| {
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
        allowed_agents: vec![AgentKind::Claude],
        max_concurrent: None,
        roadmap_enabled: true,
        verify_enabled: true,
        default_max_attempts: None,
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

    // If an agent is explicitly requested it must be allowed for this project.
    if let Some(agent) = input.agent {
        if !project.allows(agent) {
            return Err(CoreError::invalid(format!(
                "agent {} is not allowed for this project",
                agent.as_str()
            )));
        }
    }
    let auto_agent = input.agent.is_none();
    let default_attempts = project.effective_max_attempts();
    let now = Utc::now();
    let task = Task {
        id: Uuid::new_v4().to_string(),
        project_id: project.id,
        title: input.title,
        description: input.description,
        status: TaskStatus::Pending,
        priority: input.priority.unwrap_or(50),
        agent: input.agent.unwrap_or(project.default_agent),
        auto_agent,
        model: input.model.filter(|m| !m.trim().is_empty()),
        parent_id: None,
        depends_on: input.depends_on,
        attempts: 0,
        max_attempts: input.max_attempts.unwrap_or(default_attempts),
        tags: input.tags,
        auto_generated: false,
        retry_at: None,
        created_at: now,
        updated_at: now,
    };
    db.upsert_task(&task)?;
    Ok(task)
}

/// Input for creating many tasks at once from a pasted list.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkTaskInput {
    pub project_id: String,
    /// Raw text — one task per line, markdown checklist/bullet markers stripped.
    pub text: String,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub agent: Option<AgentKind>,
}

/// Parse pasted text into task titles: one per non-empty line, stripping common
/// markdown list/checklist markers (`- [ ]`, `- [x]`, `-`, `*`, `1.`). Markdown
/// headings (`#`) and blank lines are skipped.
pub fn parse_bulk_titles(text: &str) -> Vec<String> {
    let mut titles = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Strip a leading bullet marker (`-`, `*`, including a bare one).
        let mut s = line.trim_start_matches(['-', '*']).trim_start();
        // Strip an ordered-list marker like "12. ".
        if let Some(pos) = s.find(". ") {
            if !s[..pos].is_empty() && s[..pos].chars().all(|c| c.is_ascii_digit()) {
                s = s[pos + 2..].trim_start();
            }
        }
        // Strip a checklist box: "[ ] foo" / "[x] foo".
        if s.starts_with("[ ]") || s.starts_with("[x]") || s.starts_with("[X]") {
            s = s[3..].trim_start();
        }
        let title = s.trim();
        if !title.is_empty() {
            titles.push(title.to_string());
        }
    }
    titles
}

/// Create one pending task per line of `input.text`. Returns the created tasks.
pub fn create_tasks_bulk(db: &Db, input: BulkTaskInput) -> Result<Vec<Task>> {
    let titles = parse_bulk_titles(&input.text);
    if titles.is_empty() {
        return Err(CoreError::invalid("no task lines found in the pasted text"));
    }
    let mut created = Vec::new();
    for title in titles {
        let task = create_task(
            db,
            CreateTaskInput {
                project_id: input.project_id.clone(),
                title,
                description: String::new(),
                priority: input.priority,
                agent: input.agent,
                model: None,
                depends_on: vec![],
                tags: vec!["bulk".into()],
                max_attempts: None,
            },
        )?;
        created.push(task);
    }
    Ok(created)
}

/// A portable snapshot of the orchestrator's configuration: global settings and
/// the managed projects. Excludes tasks/sessions/usage (runtime state).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBundle {
    #[serde(default = "default_bundle_version")]
    pub version: u32,
    pub settings: Settings,
    pub projects: Vec<Project>,
}

fn default_bundle_version() -> u32 {
    1
}

/// Result of importing a config bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub projects_imported: u32,
    pub projects_skipped: u32,
    pub settings_applied: bool,
}

/// Export the current settings + projects as a portable bundle.
pub fn export_config(db: &Db) -> Result<ConfigBundle> {
    Ok(ConfigBundle {
        version: 1,
        settings: db.get_settings()?,
        projects: db.list_projects()?,
    })
}

/// Apply a config bundle: replace settings and upsert projects. Projects whose
/// path collides with a *different* existing project are skipped (not clobbered).
pub fn import_config(db: &Db, bundle: ConfigBundle) -> Result<ImportResult> {
    db.save_settings(&bundle.settings)?;
    let existing = db.list_projects()?;
    let mut imported = 0;
    let mut skipped = 0;
    for p in bundle.projects {
        if existing.iter().any(|e| e.path == p.path && e.id != p.id) {
            skipped += 1;
            continue;
        }
        db.upsert_project(&p)?;
        imported += 1;
    }
    Ok(ImportResult {
        projects_imported: imported,
        projects_skipped: skipped,
        settings_applied: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bulk_titles_with_markers() {
        let text =
            "# Heading\n- [ ] First task\n- [x] Second\n* Third\n3. Fourth\nPlain line\n\n   \n- ";
        let titles = parse_bulk_titles(text);
        assert_eq!(
            titles,
            vec!["First task", "Second", "Third", "Fourth", "Plain line"]
        );
    }

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
                model: None,
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

    #[test]
    fn config_export_import_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("orch-cfg-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        add_project(
            &db,
            AddProjectInput {
                path: dir.to_string_lossy().into_owned(),
                name: Some("demo".into()),
                description: None,
                scaffold: false,
            },
        )
        .unwrap();
        let mut s = db.get_settings().unwrap();
        s.max_concurrent = 9;
        db.save_settings(&s).unwrap();

        let bundle = export_config(&db).unwrap();
        assert_eq!(bundle.projects.len(), 1);
        assert_eq!(bundle.settings.max_concurrent, 9);

        // Import into a fresh DB reproduces settings + projects.
        let db2 = Db::open_in_memory().unwrap();
        let res = import_config(&db2, bundle).unwrap();
        assert_eq!(res.projects_imported, 1);
        assert!(res.settings_applied);
        assert_eq!(db2.get_settings().unwrap().max_concurrent, 9);
        assert_eq!(db2.list_projects().unwrap().len(), 1);

        // Re-importing skips the path collision rather than duplicating.
        let bundle2 = export_config(&db).unwrap();
        let res2 = import_config(&db2, bundle2).unwrap();
        assert_eq!(res2.projects_imported, 1); // same id -> upsert, not skipped
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bulk_create_makes_one_task_per_line() {
        let db = Db::open_in_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("orch-bulk-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let project = add_project(
            &db,
            AddProjectInput {
                path: dir.to_string_lossy().into_owned(),
                name: Some("demo".into()),
                description: None,
                scaffold: false,
            },
        )
        .unwrap();
        let created = create_tasks_bulk(
            &db,
            BulkTaskInput {
                project_id: project.id.clone(),
                text: "- [ ] Alpha\n- Beta\n\n1. Gamma".into(),
                priority: Some(70),
                agent: None,
            },
        )
        .unwrap();
        assert_eq!(created.len(), 3);
        assert_eq!(created[0].title, "Alpha");
        assert!(created
            .iter()
            .all(|t| t.priority == 70 && t.tags.contains(&"bulk".to_string())));
        assert_eq!(db.list_tasks(Some(&project.id)).unwrap().len(), 3);
        // Empty text is rejected.
        assert!(create_tasks_bulk(
            &db,
            BulkTaskInput {
                project_id: project.id,
                text: "  \n#only heading".into(),
                priority: None,
                agent: None
            },
        )
        .is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
