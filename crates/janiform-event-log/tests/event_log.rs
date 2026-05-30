use std::fs;
use std::io::Write;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use janiform_event_log::{
    EventDeliveryOutcome, EventDeliveryPriority, EventLogError, ProgressMeasure, RunArtifactEvent,
    RunEvent, RunEventLogWriter, RunEventPayload, RunLifecycleEvent, RunPhaseRef, RunProgressEvent,
    RunStageRef, RunStatus, RunWarningEvent, WorkflowRef, append_event_to_log,
    bounded_event_channel, default_run_event_paths, load_run_state_snapshot, replay_event_log,
    replay_event_log_allowing_trailing_partial,
};

#[test]
fn writer_persists_replayable_log_and_latest_state_snapshot() {
    let run_directory = temporary_run_directory("writer-persistence");
    let paths = default_run_event_paths(&run_directory);

    let workflow = WorkflowRef::labeled("fit", "Fit");
    let stage = RunStageRef::labeled("return_components", "Fit return distribution");
    let phase = RunPhaseRef::labeled(
        "continuous_nig_basin_optimization",
        "continuous NIG basin optimization",
    );

    let mut writer = RunEventLogWriter::create(
        &paths.event_log_path,
        Some(&paths.latest_state_snapshot_path),
        "run-001",
        workflow.clone(),
    )
    .expect("event writer should be created");

    for event in [
        RunEvent::at(
            "2026-04-26T10:00:00Z",
            "run-001",
            workflow.clone(),
            EventDeliveryPriority::Required,
            RunEventPayload::Lifecycle(RunLifecycleEvent::RunStarted {
                scope_id: "full".to_string(),
                scope_label: Some("Full calibration".to_string()),
                inputs: vec!["ticks.parquet".to_string()],
                output_directory: Some(run_directory.display().to_string()),
                stage_sequence: vec![stage.clone()],
            }),
        ),
        RunEvent::at(
            "2026-04-26T10:00:01Z",
            "run-001",
            workflow.clone(),
            EventDeliveryPriority::Required,
            RunEventPayload::Lifecycle(RunLifecycleEvent::StageStarted {
                stage: stage.clone(),
                stage_index: 1,
                stage_count: 1,
            }),
        ),
        RunEvent::at(
            "2026-04-26T10:00:02Z",
            "run-001",
            workflow.clone(),
            EventDeliveryPriority::DroppableVolatile,
            RunEventPayload::Progress(RunProgressEvent {
                stage: Some(stage.clone()),
                phase: Some(phase.clone()),
                subphase: None,
                measures: vec![ProgressMeasure::EvaluationCount {
                    metric_id: "evaluation_count".to_string(),
                    label: Some("eval".to_string()),
                    completed_evaluations: 1840,
                    evaluation_budget: Some(6000),
                }],
            }),
        ),
        RunEvent::at(
            "2026-04-26T10:00:03Z",
            "run-001",
            workflow.clone(),
            EventDeliveryPriority::Required,
            RunEventPayload::Artifact(RunArtifactEvent {
                artifact_kind: "checkpoint".to_string(),
                path: run_directory
                    .join("checkpoints")
                    .join("return-components.json")
                    .display()
                    .to_string(),
                stage: Some(stage.clone()),
                phase: Some(phase.clone()),
                label: None,
            }),
        ),
        RunEvent::at(
            "2026-04-26T10:00:04Z",
            "run-001",
            workflow.clone(),
            EventDeliveryPriority::Required,
            RunEventPayload::Warning(RunWarningEvent {
                warning_code: "interval_like_ticks".to_string(),
                reason: "Tick data appears interval-like".to_string(),
                stage: Some(stage.clone()),
                phase: None,
                input_surface: None,
                field: None,
                artifact_paths: Vec::new(),
            }),
        ),
        RunEvent::at(
            "2026-04-26T10:00:05Z",
            "run-001",
            workflow.clone(),
            EventDeliveryPriority::Required,
            RunEventPayload::Lifecycle(RunLifecycleEvent::StageCompleted {
                stage,
                stage_index: 1,
                stage_count: 1,
                runtime_seconds: 4.0,
            }),
        ),
        RunEvent::at(
            "2026-04-26T10:00:06Z",
            "run-001",
            workflow,
            EventDeliveryPriority::Required,
            RunEventPayload::Lifecycle(RunLifecycleEvent::RunCompleted {
                total_runtime_seconds: 6.0,
            }),
        ),
    ] {
        writer
            .append_event(&event)
            .expect("event should append to log and snapshot");
    }

    writer.flush().expect("event log should flush");

    let snapshot =
        load_run_state_snapshot(&paths.latest_state_snapshot_path).expect("snapshot should load");
    let replayed = replay_event_log(&paths.event_log_path).expect("event log should replay");

    assert_eq!(snapshot, replayed);
    assert_eq!(snapshot.status, RunStatus::Completed);
    assert_eq!(snapshot.event_count, 7);
    assert_eq!(snapshot.checkpoints.available_count, 1);
    assert_eq!(snapshot.warnings.len(), 1);

    fs::remove_dir_all(run_directory).expect("temporary run directory should be removable");
}

#[test]
fn bounded_channel_drops_only_droppable_volatile_events() {
    let (sender, receiver) = bounded_event_channel(1).expect("channel should be created");

    let workflow = WorkflowRef::new("fit");
    let required_event = RunEvent::at(
        "2026-04-26T10:00:00Z",
        "run-001",
        workflow.clone(),
        EventDeliveryPriority::Required,
        RunEventPayload::Lifecycle(RunLifecycleEvent::RunCompleted {
            total_runtime_seconds: 1.0,
        }),
    );
    let volatile_event = RunEvent::at(
        "2026-04-26T10:00:01Z",
        "run-001",
        workflow.clone(),
        EventDeliveryPriority::DroppableVolatile,
        RunEventPayload::Progress(RunProgressEvent {
            stage: None,
            phase: None,
            subphase: None,
            measures: Vec::new(),
        }),
    );
    let second_required_event = RunEvent::at(
        "2026-04-26T10:00:02Z",
        "run-001",
        workflow,
        EventDeliveryPriority::Required,
        RunEventPayload::Lifecycle(RunLifecycleEvent::RunCompleted {
            total_runtime_seconds: 2.0,
        }),
    );

    assert_eq!(
        sender
            .try_send(required_event)
            .expect("first event should fit in the bounded channel"),
        EventDeliveryOutcome::Delivered
    );
    assert_eq!(
        sender
            .try_send(volatile_event)
            .expect("volatile backpressure should be reported as dropped"),
        EventDeliveryOutcome::DroppedVolatile
    );
    assert!(
        sender.try_send(second_required_event).is_err(),
        "required backpressure should surface as an error"
    );
    assert!(receiver.try_recv().is_ok());
}

#[test]
fn live_replay_ignores_only_malformed_unterminated_trailing_line() {
    let run_directory = temporary_run_directory("live-replay-partial-line");
    fs::create_dir_all(&run_directory).expect("temporary run directory should be created");
    let paths = default_run_event_paths(&run_directory);
    let workflow = WorkflowRef::labeled("fit", "Fit");
    let event = RunEvent::at(
        "2026-04-26T10:00:00Z",
        "run-001",
        workflow,
        EventDeliveryPriority::Required,
        RunEventPayload::Lifecycle(RunLifecycleEvent::RunStarted {
            scope_id: "seasonality_only".to_string(),
            scope_label: Some("Seasonality only".to_string()),
            inputs: vec!["ticks.csv".to_string()],
            output_directory: Some(run_directory.display().to_string()),
            stage_sequence: Vec::new(),
        }),
    );

    let mut file = fs::File::create(&paths.event_log_path).expect("event log should be created");
    append_event_to_log(&mut file, &paths.event_log_path, &event)
        .expect("complete event should write");
    write!(file, "{{\"schema_version\":\"janiform.run_event.v1\"")
        .expect("partial event should write");
    file.flush().expect("event log should flush");

    let live_replayed = replay_event_log_allowing_trailing_partial(&paths.event_log_path)
        .expect("live replay should ignore trailing partial row");
    assert_eq!(live_replayed.event_count, 1);
    assert_eq!(live_replayed.status, RunStatus::Running);

    assert!(
        matches!(
            replay_event_log(&paths.event_log_path),
            Err(EventLogError::ParseEventLogLine { line_number: 2, .. })
        ),
        "strict replay should still reject the partial row"
    );

    fs::remove_dir_all(run_directory).expect("temporary run directory should be removable");
}

#[test]
fn live_replay_rejects_malformed_terminated_line() {
    let run_directory = temporary_run_directory("live-replay-malformed-line");
    fs::create_dir_all(&run_directory).expect("temporary run directory should be created");
    let paths = default_run_event_paths(&run_directory);

    let mut file = fs::File::create(&paths.event_log_path).expect("event log should be created");
    writeln!(file, "{{\"schema_version\":\"janiform.run_event.v1\"")
        .expect("malformed event should write");
    file.flush().expect("event log should flush");

    assert!(
        matches!(
            replay_event_log_allowing_trailing_partial(&paths.event_log_path),
            Err(EventLogError::ParseEventLogLine { line_number: 1, .. })
        ),
        "live replay should reject malformed rows once they are line-terminated"
    );

    fs::remove_dir_all(run_directory).expect("temporary run directory should be removable");
}

fn temporary_run_directory(label: &str) -> std::path::PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after UNIX epoch")
        .as_nanos();

    std::env::temp_dir().join(format!(
        "janiform-event-log-{label}-{}-{timestamp}",
        std::process::id()
    ))
}
