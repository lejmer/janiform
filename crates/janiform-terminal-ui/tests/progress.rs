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
fn adapter_coalesces_structured_events_until_draw() {
    let mut adapter = ProgressAdapter::hidden();
    adapter.observe(ProgressEvent::RunStarted {
        run_id: "fit-20260426".to_string(),
        scope: "staged calibration".to_string(),
        input_count: 3,
        output_path: Some(".janiform/runs/fit".to_string()),
        stage_sequence: vec!["seasonality".to_string(), "return components".to_string()],
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
fn adapter_seeds_planned_work_items_as_waiting() {
    let mut adapter = ProgressAdapter::hidden();
    adapter.observe(ProgressEvent::RunStarted {
        run_id: "fit-20260426".to_string(),
        scope: "staged calibration".to_string(),
        input_count: 1,
        output_path: None,
        stage_sequence: vec!["joint refinement".to_string()],
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
