use janiform_terminal_ui::{
    ProgressAdapter, ProgressContractError, ProgressEvent, ProgressMeasure,
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
    });
    adapter.observe(ProgressEvent::StageStarted {
        stage: "return components".to_string(),
        stage_number: 2,
        stage_count: 5,
    });
    adapter.observe(ProgressEvent::StageProgress {
        phase: "candidate enumeration".to_string(),
        measure: ProgressMeasure::bounded(1, 4).expect("test progress should be valid"),
        latest_signal: Some("seed baseline".to_string()),
    });
    adapter.observe(ProgressEvent::StageProgress {
        phase: "candidate enumeration".to_string(),
        measure: ProgressMeasure::bounded(3, 4).expect("test progress should be valid"),
        latest_signal: Some("seed basin".to_string()),
    });

    adapter
        .draw_latest()
        .expect("hidden progress should draw successfully");

    let snapshot = adapter.snapshot();
    assert_eq!(snapshot.run_id.as_deref(), Some("fit-20260426"));
    assert_eq!(snapshot.active_stage.as_deref(), Some("return components"));
    assert_eq!(snapshot.phase.as_deref(), Some("candidate enumeration"));
    assert_eq!(snapshot.latest_signal.as_deref(), Some("seed basin"));
    assert_eq!(
        snapshot.measure,
        Some(ProgressMeasure::Bounded {
            completed_units: 3,
            total_units: 4
        })
    );
}
