use super::*;
use chrono::Utc;

fn project(id: &str) -> Project {
    let now = Utc::now();
    Project {
        id: id.into(),
        name: format!("proj-{id}"),
        path: "/tmp/repo".into(),
        description: None,
        enabled: true,
        default_agent: AgentKind::Claude,
        max_concurrent: None,
        roadmap_enabled: true,
        verify_enabled: true,
        created_at: now,
        updated_at: now,
    }
}

fn task(id: &str, project_id: &str, status: TaskStatus, priority: i64) -> Task {
    let now = Utc::now();
    Task {
        id: id.into(),
        project_id: project_id.into(),
        title: format!("task-{id}"),
        description: "do it".into(),
        status,
        priority,
        agent: AgentKind::Claude,
        parent_id: None,
        depends_on: vec![],
        attempts: 0,
        max_attempts: 3,
        tags: vec![],
        auto_generated: false,
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn settings_roundtrip() {
    let db = Db::open_in_memory().unwrap();
    assert_eq!(db.get_settings().unwrap().max_concurrent, 3);
    let mut s = db.get_settings().unwrap();
    s.max_concurrent = 7;
    db.save_settings(&s).unwrap();
    assert_eq!(db.get_settings().unwrap().max_concurrent, 7);
}

#[test]
fn project_crud() {
    let db = Db::open_in_memory().unwrap();
    db.upsert_project(&project("p1")).unwrap();
    assert_eq!(db.count_projects().unwrap(), 1);
    let mut p = db.get_project("p1").unwrap();
    p.name = "renamed".into();
    db.upsert_project(&p).unwrap();
    assert_eq!(db.get_project("p1").unwrap().name, "renamed");
    db.delete_project("p1").unwrap();
    assert_eq!(db.count_projects().unwrap(), 0);
}

#[test]
fn tasks_filter_and_ordering() {
    let db = Db::open_in_memory().unwrap();
    db.upsert_project(&project("p1")).unwrap();
    db.upsert_task(&task("t1", "p1", TaskStatus::Pending, 10))
        .unwrap();
    db.upsert_task(&task("t2", "p1", TaskStatus::Pending, 100))
        .unwrap();
    db.upsert_task(&task("t3", "p1", TaskStatus::Completed, 50))
        .unwrap();

    let sched = db.schedulable_tasks("p1").unwrap();
    assert_eq!(sched.len(), 2);
    // Highest priority first.
    assert_eq!(sched[0].id, "t2");
    assert_eq!(db.count_schedulable_tasks("p1").unwrap(), 2);
    assert_eq!(db.count_pending_tasks().unwrap(), 2);
}

#[test]
fn cascade_delete_tasks_with_project() {
    let db = Db::open_in_memory().unwrap();
    db.upsert_project(&project("p1")).unwrap();
    db.upsert_task(&task("t1", "p1", TaskStatus::Pending, 10))
        .unwrap();
    db.delete_project("p1").unwrap();
    assert_eq!(db.count_pending_tasks().unwrap(), 0);
}

#[test]
fn session_and_events_and_usage() {
    let db = Db::open_in_memory().unwrap();
    db.upsert_project(&project("p1")).unwrap();
    let now = Utc::now();
    let session = Session {
        id: "s1".into(),
        task_id: None,
        project_id: "p1".into(),
        agent: AgentKind::Claude,
        kind: SessionKind::Task,
        status: SessionStatus::Running,
        agent_session_id: None,
        model: None,
        prompt: "hi".into(),
        result_text: None,
        error: None,
        exit_code: None,
        usage: TokenUsage::default(),
        started_at: Some(now),
        ended_at: None,
        created_at: now,
    };
    db.upsert_session(&session).unwrap();
    assert_eq!(db.count_active_sessions().unwrap(), 1);
    assert_eq!(
        db.count_active_sessions_for_agent(AgentKind::Claude)
            .unwrap(),
        1
    );

    db.insert_event("s1", "assistant", Some("hello"), None, now)
        .unwrap();
    db.insert_event("s1", "result", None, None, now).unwrap();
    assert_eq!(db.list_events("s1").unwrap().len(), 2);

    let usage = TokenUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_cost_usd: 0.5,
        ..Default::default()
    };
    db.insert_usage("u1", AgentKind::Claude, Some("s1"), &usage, now)
        .unwrap();
    let agg = db.usage_for_agent(AgentKind::Claude, None).unwrap();
    assert_eq!(agg.input_tokens, 100);
    assert!((agg.total_cost_usd - 0.5).abs() < 1e-9);

    // Mark session done; no longer active.
    let mut done = session.clone();
    done.status = SessionStatus::Completed;
    db.upsert_session(&done).unwrap();
    assert_eq!(db.count_active_sessions().unwrap(), 0);
}

#[test]
fn orphan_reconciliation() {
    let db = Db::open_in_memory().unwrap();
    db.upsert_project(&project("p1")).unwrap();
    let now = Utc::now();
    for i in 0..3 {
        let s = Session {
            id: format!("s{i}"),
            task_id: None,
            project_id: "p1".into(),
            agent: AgentKind::Claude,
            kind: SessionKind::Task,
            status: SessionStatus::Running,
            agent_session_id: None,
            model: None,
            prompt: String::new(),
            result_text: None,
            error: None,
            exit_code: None,
            usage: TokenUsage::default(),
            started_at: Some(now),
            ended_at: None,
            created_at: now,
        };
        db.upsert_session(&s).unwrap();
    }
    assert_eq!(db.reconcile_orphan_sessions().unwrap(), 3);
    assert_eq!(db.count_active_sessions().unwrap(), 0);
}
