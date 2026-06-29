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
        allowed_agents: vec![AgentKind::Claude],
        max_concurrent: None,
        roadmap_enabled: true,
        verify_enabled: true,
        default_max_attempts: None,
        mcp_config: None,
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
        auto_agent: true,
        model: None,
        parent_id: None,
        depends_on: vec![],
        attempts: 0,
        max_attempts: 3,
        tags: vec![],
        auto_generated: false,
        retry_at: None,
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
fn project_default_max_attempts_roundtrips() {
    let db = Db::open_in_memory().unwrap();
    let mut p = project("p1");
    assert_eq!(p.effective_max_attempts(), 3); // None -> default 3
    p.default_max_attempts = Some(5);
    db.upsert_project(&p).unwrap();
    let loaded = db.get_project("p1").unwrap();
    assert_eq!(loaded.default_max_attempts, Some(5));
    assert_eq!(loaded.effective_max_attempts(), 5);
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
        branch: None,
        pr_url: None,
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
fn usage_series_buckets_by_day() {
    let db = Db::open_in_memory().unwrap();
    db.upsert_project(&project("p1")).unwrap();
    let day1 = "2026-06-01T10:00:00Z"
        .parse::<chrono::DateTime<Utc>>()
        .unwrap();
    let day1b = "2026-06-01T18:00:00Z"
        .parse::<chrono::DateTime<Utc>>()
        .unwrap();
    let day2 = "2026-06-02T09:00:00Z"
        .parse::<chrono::DateTime<Utc>>()
        .unwrap();
    let u = TokenUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_cost_usd: 1.0,
        num_turns: 2,
        ..Default::default()
    };
    db.insert_usage("u1", AgentKind::Claude, None, &u, day1)
        .unwrap();
    db.insert_usage("u2", AgentKind::Claude, None, &u, day1b)
        .unwrap();
    db.insert_usage("u3", AgentKind::Gemini, None, &u, day2)
        .unwrap();

    let series = db.usage_series("day", None, 30).unwrap();
    assert_eq!(series.len(), 2);
    assert_eq!(series[0].period, "2026-06-01");
    assert_eq!(series[0].input_tokens, 200);
    assert!((series[0].cost_usd - 2.0).abs() < 1e-9);
    assert_eq!(series[1].period, "2026-06-02");

    // Agent-scoped.
    let claude = db.usage_series("day", Some(AgentKind::Claude), 30).unwrap();
    assert_eq!(claude.len(), 1);
    assert_eq!(claude[0].input_tokens, 200);

    // Month granularity collapses both days.
    let months = db.usage_series("month", None, 30).unwrap();
    assert_eq!(months.len(), 1);
    assert_eq!(months[0].period, "2026-06");
    assert_eq!(months[0].input_tokens, 300);
}

#[test]
fn agent_stats_compares_agents() {
    let db = Db::open_in_memory().unwrap();
    db.upsert_project(&project("p1")).unwrap();
    let base = "2026-06-01T10:00:00Z".parse::<DateTime<Utc>>().unwrap();

    let mk = |id: &str, agent: AgentKind, status: SessionStatus, cost: f64, secs: i64| Session {
        id: id.into(),
        task_id: None,
        project_id: "p1".into(),
        agent,
        kind: SessionKind::Task,
        status,
        agent_session_id: None,
        model: None,
        prompt: String::new(),
        result_text: None,
        error: None,
        exit_code: None,
        usage: TokenUsage {
            total_cost_usd: cost,
            ..Default::default()
        },
        branch: None,
        pr_url: None,
        started_at: Some(base),
        ended_at: Some(base + chrono::Duration::seconds(secs)),
        created_at: base,
    };

    db.upsert_session(&mk(
        "a",
        AgentKind::Claude,
        SessionStatus::Completed,
        1.0,
        10,
    ))
    .unwrap();
    db.upsert_session(&mk("b", AgentKind::Claude, SessionStatus::Failed, 0.5, 20))
        .unwrap();
    db.upsert_session(&mk(
        "c",
        AgentKind::Gemini,
        SessionStatus::Completed,
        0.2,
        4,
    ))
    .unwrap();
    // Verify/roadmap sessions are excluded from the stats.
    let mut verify = mk("d", AgentKind::Claude, SessionStatus::Completed, 9.0, 99);
    verify.kind = SessionKind::Verify;
    db.upsert_session(&verify).unwrap();

    let stats = db.agent_stats().unwrap();
    let claude = stats.iter().find(|s| s.agent == AgentKind::Claude).unwrap();
    assert_eq!(claude.sessions, 2);
    assert_eq!(claude.completed, 1);
    assert_eq!(claude.failed, 1);
    assert!((claude.success_rate - 0.5).abs() < 1e-9);
    assert!((claude.total_cost_usd - 1.5).abs() < 1e-9);
    assert!((claude.avg_cost_usd - 0.75).abs() < 1e-9);
    assert!((claude.avg_duration_secs - 15.0).abs() < 1e-9);

    let gemini = stats.iter().find(|s| s.agent == AgentKind::Gemini).unwrap();
    assert_eq!(gemini.sessions, 1);
    assert!((gemini.success_rate - 1.0).abs() < 1e-9);
}

#[test]
fn task_rollup_aggregates_sessions() {
    let db = Db::open_in_memory().unwrap();
    db.upsert_project(&project("p1")).unwrap();
    db.upsert_task(&task("t1", "p1", TaskStatus::Completed, 50))
        .unwrap();
    let base = "2026-06-01T10:00:00Z".parse::<DateTime<Utc>>().unwrap();
    let mk = |id: &str, cost: f64, tokens_in: u64, secs: i64| Session {
        id: id.into(),
        task_id: Some("t1".into()),
        project_id: "p1".into(),
        agent: AgentKind::Claude,
        kind: SessionKind::Task,
        status: SessionStatus::Completed,
        agent_session_id: None,
        model: None,
        prompt: String::new(),
        result_text: None,
        error: None,
        exit_code: None,
        usage: TokenUsage {
            input_tokens: tokens_in,
            total_cost_usd: cost,
            ..Default::default()
        },
        branch: None,
        pr_url: None,
        started_at: Some(base),
        ended_at: Some(base + chrono::Duration::seconds(secs)),
        created_at: base,
    };
    db.upsert_session(&mk("s1", 0.5, 100, 30)).unwrap();
    db.upsert_session(&mk("s2", 0.25, 50, 10)).unwrap();
    let r = db.task_rollup("t1").unwrap();
    assert_eq!(r.sessions, 2);
    assert!((r.total_cost_usd - 0.75).abs() < 1e-9);
    assert_eq!(r.total_tokens, 150);
    assert!((r.total_duration_secs - 40.0).abs() < 1e-9);
}

#[test]
fn activity_log_insert_and_scope() {
    let db = Db::open_in_memory().unwrap();
    db.upsert_project(&project("p1")).unwrap();
    db.upsert_project(&project("p2")).unwrap();
    let now = Utc::now();
    db.insert_activity("task", "info", "Completed: A", Some("p1"), None, None, now)
        .unwrap();
    db.insert_activity("pr", "info", "Merged PR #1", Some("p2"), None, None, now)
        .unwrap();
    db.insert_activity(
        "github",
        "info",
        "Imported 3 issues",
        Some("p1"),
        None,
        None,
        now,
    )
    .unwrap();

    let all = db.list_activity(50, None).unwrap();
    assert_eq!(all.len(), 3);
    // Newest first.
    assert_eq!(all[0].message, "Imported 3 issues");
    // Project name is joined in.
    assert_eq!(all[0].project_name.as_deref(), Some("proj-p1"));

    let p1 = db.list_activity(50, Some("p1")).unwrap();
    assert_eq!(p1.len(), 2);
    assert!(p1.iter().all(|e| e.project_id.as_deref() == Some("p1")));
}

#[test]
fn activity_log_prunes_to_retention() {
    let db = Db::open_in_memory().unwrap();
    db.upsert_project(&project("p1")).unwrap();
    let now = Utc::now();
    for i in 0..10 {
        db.insert_activity(
            "task",
            "info",
            &format!("msg {i}"),
            Some("p1"),
            None,
            None,
            now,
        )
        .unwrap();
    }
    assert_eq!(db.list_activity(100, None).unwrap().len(), 10);
    let removed = db.prune_activity(4).unwrap();
    assert_eq!(removed, 6);
    let kept = db.list_activity(100, None).unwrap();
    assert_eq!(kept.len(), 4);
    // The newest are retained.
    assert_eq!(kept[0].message, "msg 9");
    assert_eq!(kept[3].message, "msg 6");
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
            branch: None,
            pr_url: None,
            started_at: Some(now),
            ended_at: None,
            created_at: now,
        };
        db.upsert_session(&s).unwrap();
    }
    assert_eq!(db.reconcile_orphan_sessions().unwrap(), 3);
    assert_eq!(db.count_active_sessions().unwrap(), 0);
}
