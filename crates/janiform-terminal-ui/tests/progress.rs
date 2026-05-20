use janiform_terminal_ui::{
    ProgressAdapter, ProgressContractError, ProgressEvent, ProgressMeasure, ProgressWorkItemColumn,
    ProgressWorkItemMetric,
};

#[test]
fn bounded_progress_requires_real_positive_totals() {
    assert_eq!(
        ProgressMeasure::bounded(0, 0),
        Err(ProgressContractError::EmptyBoundedTotal)
    );
    assert_eq!(
        ProgressMeasure::bounded(2, 1),
        Err(ProgressContractError::CompletedUnitsExceedTotal)
    );
}

#[test]
fn percent_progress_caps_display_percent_without_units() {
    assert_eq!(
        ProgressMeasure::percent(150),
        ProgressMeasure::Percent { percent: 100 }
    );
}

#[test]
fn adapter_coalesces_structured_events_until_draw() {
    let mut adapter = ProgressAdapter::hidden();

    adapter.observe(ProgressEvent::RunStarted {
        run_id: "fit-20260426".to_string(),
        scope: "staged calibration".to_string(),
        input_count: 3,
        output_path: Some(".janiform/runs/fit".to_string()),
        stage_sequence: vec!["seasonality".to_string(), "return components".to_string()],
        completed_stage_count: 0,
    });

    adapter.observe(ProgressEvent::StageStarted {
        stage: "return components".to_string(),
        stage_number: 2,
        stage_count: 5,
    });

    adapter.observe(ProgressEvent::StageProgress {
        phase: "candidate enumeration".to_string(),
        measure: ProgressMeasure::bounded(1, 4).expect("test progress should be valid"),
        work_item_label: Some("seed baseline".to_string()),
        work_item_progress_percent: Some(25),
        work_item_status: None,
        planned_work_item_labels: Vec::new(),
        work_item_columns: vec![ProgressWorkItemColumn::new("eval", "Eval")],
        work_item_metrics: vec![ProgressWorkItemMetric::new("eval", "1/4")],
        latest_signal: Some("seed baseline".to_string()),
    });

    adapter.observe(ProgressEvent::StageProgress {
        phase: "candidate enumeration".to_string(),
        measure: ProgressMeasure::bounded(3, 4).expect("test progress should be valid"),
        work_item_label: Some("seed basin".to_string()),
        work_item_progress_percent: Some(75),
        work_item_status: None,
        planned_work_item_labels: Vec::new(),
        work_item_columns: vec![ProgressWorkItemColumn::new("eval", "Eval")],
        work_item_metrics: vec![ProgressWorkItemMetric::new("eval", "3/4")],
        latest_signal: Some("seed basin".to_string()),
    });

    adapter.observe(ProgressEvent::StageProgress {
        phase: "candidate enumeration".to_string(),
        measure: ProgressMeasure::bounded(4, 4).expect("test progress should be valid"),
        work_item_label: Some("seed baseline".to_string()),
        work_item_progress_percent: Some(100),
        work_item_status: None,
        planned_work_item_labels: Vec::new(),
        work_item_columns: vec![
            ProgressWorkItemColumn::new("eval", "Eval"),
            ProgressWorkItemColumn::new("assessment", "Assessment"),
        ],
        work_item_metrics: vec![
            ProgressWorkItemMetric::new("eval", "4/4"),
            ProgressWorkItemMetric::new("assessment", "admissible"),
        ],
        latest_signal: Some("seed baseline completed".to_string()),
    });

    adapter
        .draw_latest()
        .expect("hidden progress should draw successfully");

    let snapshot = adapter.snapshot();
    assert_eq!(snapshot.run_id.as_deref(), Some("fit-20260426"));
    assert_eq!(snapshot.active_stage.as_deref(), Some("return components"));
    assert_eq!(snapshot.phase.as_deref(), Some("candidate enumeration"));
    assert_eq!(
        snapshot.latest_signal.as_deref(),
        Some("seed baseline completed")
    );
    assert_eq!(
        snapshot.measure,
        Some(ProgressMeasure::Bounded {
            completed_units: 4,
            total_units: 4
        })
    );
    assert_eq!(
        snapshot
            .work_item_columns
            .iter()
            .map(|column| column.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Eval", "Assessment"]
    );
    assert_eq!(snapshot.work_items[0].label, "seed baseline");
    assert_eq!(snapshot.work_items[0].status, "completed");
    assert_eq!(
        snapshot.work_items[0]
            .metrics
            .get("assessment")
            .map(String::as_str),
        Some("admissible")
    );
}

#[test]
fn adapter_marks_leading_stages_completed_when_run_starts_from_resume() {
    let mut adapter = ProgressAdapter::hidden();

    adapter.observe(ProgressEvent::RunStarted {
        run_id: "fit-20260426".to_string(),
        scope: "staged calibration".to_string(),
        input_count: 1,
        output_path: None,
        stage_sequence: vec![
            "seasonality".to_string(),
            "event activity".to_string(),
            "return distribution".to_string(),
            "mean reversion".to_string(),
            "refine dynamics".to_string(),
        ],
        completed_stage_count: 4,
    });

    let snapshot = adapter.snapshot();

    assert_eq!(
        snapshot
            .stages
            .iter()
            .map(|stage| format!("{:?}", stage.status))
            .collect::<Vec<_>>(),
        vec![
            "Completed",
            "Completed",
            "Completed",
            "Completed",
            "Pending",
        ]
    );
}

#[test]
fn adapter_seeds_planned_work_items_as_waiting() {
    let mut adapter = ProgressAdapter::hidden();

    adapter.observe(ProgressEvent::RunStarted {
        run_id: "fit-20260426".to_string(),
        scope: "staged calibration".to_string(),
        input_count: 1,
        output_path: None,
        stage_sequence: vec!["joint refinement".to_string()],
        completed_stage_count: 0,
    });

    adapter.observe(ProgressEvent::StageStarted {
        stage: "joint refinement".to_string(),
        stage_number: 1,
        stage_count: 1,
    });

    adapter.observe(ProgressEvent::StageProgress {
        phase: "restart optimization".to_string(),
        measure: ProgressMeasure::bounded(0, 3).expect("test progress should be valid"),
        work_item_label: None,
        work_item_progress_percent: None,
        work_item_status: None,
        planned_work_item_labels: vec![
            "baseline".to_string(),
            "activity_aligned".to_string(),
            "midpoint".to_string(),
        ],
        work_item_columns: vec![ProgressWorkItemColumn::new("eval", "Eval")],
        work_item_metrics: Vec::new(),
        latest_signal: Some("active restarts 3".to_string()),
    });

    let snapshot = adapter.snapshot();

    assert_eq!(
        snapshot
            .work_items
            .iter()
            .map(|work_item| (work_item.label.as_str(), work_item.status.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("baseline", "waiting"),
            ("activity_aligned", "waiting"),
            ("midpoint", "waiting")
        ]
    );
    assert_eq!(
        snapshot
            .work_item_columns
            .iter()
            .map(|column| column.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Eval"]
    );
}

#[test]
fn adapter_uses_explicit_work_item_status_when_provided() {
    let mut adapter = ProgressAdapter::hidden();

    adapter.observe(ProgressEvent::RunStarted {
        run_id: "fit-20260426".to_string(),
        scope: "staged calibration".to_string(),
        input_count: 1,
        output_path: None,
        stage_sequence: vec!["joint refinement".to_string()],
        completed_stage_count: 0,
    });

    adapter.observe(ProgressEvent::StageStarted {
        stage: "joint refinement".to_string(),
        stage_number: 1,
        stage_count: 1,
    });

    adapter.observe(ProgressEvent::StageProgress {
        phase: "restart optimization".to_string(),
        measure: ProgressMeasure::bounded(1, 4).expect("test progress should be valid"),
        work_item_label: Some("tick_moment_21".to_string()),
        work_item_progress_percent: Some(25),
        work_item_status: Some("queued".to_string()),
        planned_work_item_labels: Vec::new(),
        work_item_columns: vec![ProgressWorkItemColumn::new("eval", "Eval")],
        work_item_metrics: vec![ProgressWorkItemMetric::new("eval", "1/4")],
        latest_signal: Some("queued restart".to_string()),
    });

    let snapshot = adapter.snapshot();

    assert_eq!(
        snapshot
            .work_items
            .iter()
            .map(|work_item| (work_item.label.as_str(), work_item.status.as_str()))
            .collect::<Vec<_>>(),
        vec![("tick_moment_21", "queued")]
    );
}

#[test]
fn adapter_keeps_completed_work_item_when_late_progress_arrives() {
    let mut adapter = ProgressAdapter::hidden();

    adapter.observe(ProgressEvent::RunStarted {
        run_id: "fit-20260426".to_string(),
        scope: "staged calibration".to_string(),
        input_count: 1,
        output_path: None,
        stage_sequence: vec!["joint refinement".to_string()],
        completed_stage_count: 0,
    });

    adapter.observe(ProgressEvent::StageStarted {
        stage: "joint refinement".to_string(),
        stage_number: 1,
        stage_count: 1,
    });

    adapter.observe(ProgressEvent::StageProgress {
        phase: "option search tier".to_string(),
        measure: ProgressMeasure::percent(20),
        work_item_label: Some("halton_02".to_string()),
        work_item_progress_percent: Some(100),
        work_item_status: None,
        planned_work_item_labels: Vec::new(),
        work_item_columns: vec![ProgressWorkItemColumn::new("eval", "Evals")],
        work_item_metrics: vec![ProgressWorkItemMetric::new("eval", "5/5")],
        latest_signal: Some("halton_02 completed".to_string()),
    });

    adapter.observe(ProgressEvent::StageProgress {
        phase: "option search tier".to_string(),
        measure: ProgressMeasure::percent(20),
        work_item_label: Some("halton_02".to_string()),
        work_item_progress_percent: Some(40),
        work_item_status: None,
        planned_work_item_labels: Vec::new(),
        work_item_columns: vec![ProgressWorkItemColumn::new("eval", "Evals")],
        work_item_metrics: vec![ProgressWorkItemMetric::new("eval", "2/5")],
        latest_signal: Some("late halton_02 pricing callback".to_string()),
    });

    let snapshot = adapter.snapshot();
    let work_item = snapshot
        .work_items
        .iter()
        .find(|work_item| work_item.label == "halton_02")
        .expect("completed work item should remain present");

    assert_eq!(work_item.status, "completed");
    assert!(work_item.progress.starts_with("100%"));
    assert_eq!(
        work_item.metrics.get("eval").map(String::as_str),
        Some("5/5")
    );
}

#[test]
fn adapter_clears_work_items_when_phase_changes() {
    let mut adapter = ProgressAdapter::hidden();

    adapter.observe(ProgressEvent::RunStarted {
        run_id: "fit-20260426".to_string(),
        scope: "staged calibration".to_string(),
        input_count: 1,
        output_path: None,
        stage_sequence: vec!["return components".to_string()],
        completed_stage_count: 0,
    });

    adapter.observe(ProgressEvent::StageStarted {
        stage: "return components".to_string(),
        stage_number: 1,
        stage_count: 1,
    });

    adapter.observe(ProgressEvent::StageProgress {
        phase: "continuous NIG proxy EM".to_string(),
        measure: ProgressMeasure::bounded(1, 1).expect("test progress should be valid"),
        work_item_label: Some("proxy EM search points ready".to_string()),
        work_item_progress_percent: Some(100),
        work_item_status: None,
        planned_work_item_labels: Vec::new(),
        work_item_columns: vec![ProgressWorkItemColumn::new("eval", "Evals")],
        work_item_metrics: vec![ProgressWorkItemMetric::new("eval", "17")],
        latest_signal: Some("proxy EM search points ready".to_string()),
    });

    adapter.observe(ProgressEvent::StageProgress {
        phase: "continuous NIG seed scoring".to_string(),
        measure: ProgressMeasure::bounded(0, 12).expect("test progress should be valid"),
        work_item_label: None,
        work_item_progress_percent: None,
        work_item_status: None,
        planned_work_item_labels: Vec::new(),
        work_item_columns: Vec::new(),
        work_item_metrics: Vec::new(),
        latest_signal: Some("score proxy EM search points".to_string()),
    });

    let snapshot = adapter.snapshot();

    assert_eq!(
        snapshot.phase.as_deref(),
        Some("continuous NIG seed scoring")
    );
    assert_eq!(
        snapshot.latest_signal.as_deref(),
        Some("score proxy EM search points")
    );
    assert!(snapshot.work_items.is_empty());
    assert!(snapshot.work_item_columns.is_empty());
}
