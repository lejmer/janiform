use super::*;

#[test]
fn proxy_em_progress_signal_uses_global_eval_best_and_short_conv_label() {
    let progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigProxyEmSeedGeneration,
        None,
        Some(17),
        None,
        Some(1.25),
        Some(0.5),
    );

    let progress_event = progress_event_from_staged_event(
        &CalibrationStagedRunObserverEvent::StageProgress(Box::new(progress)),
    )
    .expect("proxy EM progress should map to terminal progress");

    match progress_event {
        Some(ProgressEvent::StageProgress { latest_signal, .. }) => assert_eq!(
            latest_signal.as_deref(),
            Some("evals 17, best 1.250000, conv. 0.500000")
        ),
        _ => panic!("expected stage progress event"),
    }
}

#[test]
fn work_item_phase_start_keeps_latest_signal_when_parallel_metrics_are_absent() {
    let progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        Some("score proxy EM search points"),
        None,
        None,
        None,
        None,
    );

    let progress_event = progress_event_from_staged_event(
        &CalibrationStagedRunObserverEvent::StageProgress(Box::new(progress)),
    )
    .expect("phase-start progress should map to terminal progress");

    match progress_event {
        Some(ProgressEvent::StageProgress { latest_signal, .. }) => assert_eq!(
            latest_signal.as_deref(),
            Some("score proxy EM search points")
        ),
        _ => panic!("expected stage progress event"),
    }
}

#[test]
fn work_item_phase_start_does_not_create_placeholder_table_row() {
    let progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigBasinOptimization,
        Some("optimize basin seed candidates"),
        None,
        None,
        None,
        None,
    );

    let progress_event = progress_event_from_staged_event(
        &CalibrationStagedRunObserverEvent::StageProgress(Box::new(progress)),
    )
    .expect("phase-start progress should remain displayable");

    match progress_event {
        Some(ProgressEvent::StageProgress {
            work_item_label,
            latest_signal,
            ..
        }) => {
            assert_eq!(work_item_label, None);
            assert_eq!(
                latest_signal.as_deref(),
                Some("optimize basin seed candidates")
            );
        }
        _ => panic!("expected stage progress event"),
    }
}

#[test]
fn basin_optimization_eval_progress_stays_on_phase_line_without_table_row() {
    let progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigBasinOptimization,
        Some("basin_seed_003"),
        Some(3),
        Some(8),
        None,
        None,
    );

    let progress_event = progress_event_from_staged_event(
        &CalibrationStagedRunObserverEvent::StageProgress(Box::new(progress)),
    )
    .expect("basin optimization progress should map to terminal progress");

    match progress_event {
        Some(ProgressEvent::StageProgress {
            work_item_label,
            work_item_columns,
            work_item_metrics,
            latest_signal,
            ..
        }) => {
            assert_eq!(work_item_label, None);
            assert_eq!(work_item_columns, Vec::new());
            assert_eq!(work_item_metrics, Vec::new());
            assert_eq!(latest_signal.as_deref(), Some("basin_seed_003, evals 3/8"));
        }
        _ => panic!("expected stage progress event"),
    }
}

#[test]
fn hawkes_likelihood_progress_uses_shared_work_item_table() {
    let mut progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        Some("hawkes_candidate_07"),
        Some(40),
        Some(100),
        Some(-0.25),
        None,
    );

    progress.stage = CalibrationStage::Hawkes;
    progress.progress_phase = CalibrationStageProgressPhase::Hawkes(
        HawkesStageProgressPhase::LikelihoodShortlistEvaluation,
    );
    progress.current_global_loss = Some(-0.125);
    progress.evaluation_progress_percent = Some(40);

    let progress_event = progress_event_from_staged_event(
        &CalibrationStagedRunObserverEvent::StageProgress(Box::new(progress)),
    )
    .expect("Hawkes likelihood progress should map to terminal progress");

    match progress_event {
        Some(ProgressEvent::StageProgress {
            work_item_label,
            work_item_columns,
            work_item_metrics,
            ..
        }) => {
            assert_eq!(work_item_label.as_deref(), Some("hawkes_candidate_07"));
            assert_eq!(
                work_item_columns
                    .into_iter()
                    .map(|column| (column.key, column.label))
                    .collect::<Vec<_>>(),
                vec![
                    ("eval".to_string(), "Evals".to_string()),
                    ("current_objective".to_string(), "Cur. loss".to_string()),
                    ("final_objective".to_string(), "Final loss".to_string()),
                ]
            );
            assert_eq!(
                work_item_metrics
                    .into_iter()
                    .map(|metric| (metric.key, metric.value))
                    .collect::<Vec<_>>(),
                vec![
                    ("eval".to_string(), "40/100".to_string()),
                    ("current_objective".to_string(), "-0.125000".to_string()),
                ]
            );
        }
        _ => panic!("expected stage progress event"),
    }
}

#[test]
fn return_backfit_delta_handoff_progress_uses_stage_signal() {
    let mut progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        Some("apply delta variance handoff"),
        None,
        None,
        None,
        None,
    );

    progress.stage = CalibrationStage::ReturnBackfit;
    progress.progress_phase = CalibrationStageProgressPhase::ReturnBackfit(
        ReturnBackfitStageProgressPhase::DeltaVarianceHandoff,
    );
    progress.aggregate_progress_percent = Some(67);

    let progress_event = progress_event_from_staged_event(
        &CalibrationStagedRunObserverEvent::StageProgress(Box::new(progress)),
    )
    .expect("return-backfit delta handoff progress should map to terminal progress");

    match progress_event {
        Some(ProgressEvent::StageProgress {
            phase,
            measure,
            work_item_label,
            latest_signal,
            ..
        }) => {
            assert_eq!(phase, "delta variance handoff");
            assert_eq!(measure, UiProgressMeasure::percent(67));
            assert_eq!(work_item_label, None);
            assert_eq!(
                latest_signal.as_deref(),
                Some("apply delta variance handoff")
            );
        }
        _ => panic!("expected stage progress event"),
    }
}

#[test]
fn zero_total_phase_progress_uses_unbounded_terminal_measure() {
    let mut progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        Some("score proxy EM search points"),
        None,
        None,
        None,
        None,
    );
    progress.total_units = 0;

    let progress_event = progress_event_from_staged_event(
        &CalibrationStagedRunObserverEvent::StageProgress(Box::new(progress)),
    )
    .expect("zero-total progress should remain displayable");

    match progress_event {
        Some(ProgressEvent::StageProgress {
            measure,
            latest_signal,
            ..
        }) => {
            assert_eq!(measure, UiProgressMeasure::Unbounded);
            assert_eq!(
                latest_signal.as_deref(),
                Some("score proxy EM search points")
            );
        }
        _ => panic!("expected stage progress event"),
    }
}

#[test]
fn resumed_run_started_event_marks_preceding_terminal_stages_completed() {
    let progress_event =
        progress_event_from_staged_event(&CalibrationStagedRunObserverEvent::RunStarted {
            run_id: "test-run".to_string(),
            execution_mode: janiform_calibration_artifacts::CalibrationExecutionMode::Disk,
            stage_count: 5,
            stage_sequence: vec![
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
                CalibrationStage::MeanReversion,
                CalibrationStage::JointRefinement,
            ],
            completed_stage_count: 4,
            resumed_from_checkpoint_reference: Some(
                "checkpoint://test-run/mean_reversion".to_string(),
            ),
            resumed_from_last_successful_stage: Some(CalibrationStage::MeanReversion),
            started_from_stage: Some(CalibrationStage::JointRefinement),
        })
        .expect("run-started event should map to terminal progress");

    match progress_event {
        Some(ProgressEvent::RunStarted {
            completed_stage_count,
            ..
        }) => assert_eq!(completed_stage_count, 4),
        _ => panic!("expected run started progress event"),
    }
}

#[test]
fn restart_progress_table_includes_losses_and_exact_evaluator_counts() {
    let mut progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        Some("halton_01"),
        Some(12),
        Some(59),
        Some(0.5),
        None,
    );

    progress.stage = CalibrationStage::JointRefinement;

    progress.progress_phase = CalibrationStageProgressPhase::JointRefinement(
        JointRefinementStageProgressPhase::RestartOptimization,
    );

    progress.current_restart_best_global_loss = Some(0.5);

    progress.exact_closed_form_buy_count = Some(3);
    progress.exact_closed_form_sell_count = Some(4);
    progress.exact_tilted_mixed_count = Some(5);

    assert_eq!(
        work_item_table_columns(&progress)
            .into_iter()
            .map(|column| (column.key, column.label))
            .collect::<Vec<_>>(),
        vec![
            ("eval".to_string(), "Evals".to_string()),
            ("current_objective".to_string(), "Trial loss".to_string()),
            ("best_objective".to_string(), "Best loss".to_string()),
            ("final_objective".to_string(), "Final loss".to_string()),
            ("exact_buy".to_string(), "CF-Buy".to_string()),
            ("exact_sell".to_string(), "CF-Sell".to_string()),
            ("exact_mixed".to_string(), "Mix".to_string()),
        ]
    );

    assert_eq!(
        work_item_table_metrics(&progress)
            .into_iter()
            .map(|metric| (metric.key, metric.value))
            .collect::<Vec<_>>(),
        vec![
            ("eval".to_string(), "12/59".to_string()),
            ("best_objective".to_string(), "0.500000".to_string()),
            ("exact_buy".to_string(), "3".to_string()),
            ("exact_sell".to_string(), "4".to_string()),
            ("exact_mixed".to_string(), "5".to_string()),
        ]
    );
}

#[test]
fn restart_progress_signal_uses_stable_global_summary_fields() {
    let mut progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        Some("tick_moment_08"),
        Some(17),
        Some(80),
        Some(22.364253),
        None,
    );

    progress.stage = CalibrationStage::JointRefinement;
    progress.progress_phase = CalibrationStageProgressPhase::JointRefinement(
        JointRefinementStageProgressPhase::RestartOptimization,
    );
    progress.current_global_loss = Some(74.277624);
    progress.current_restart_best_global_loss = Some(51.125);
    progress.active_restart_count = Some(4);
    progress.cumulative_local_evaluation_count = Some(127);
    progress.best_restart_id = Some("incumbent_local_search".to_string());

    let progress_event = progress_event_from_staged_event(
        &CalibrationStagedRunObserverEvent::StageProgress(Box::new(progress)),
    )
    .expect("restart progress should map to terminal progress");

    match progress_event {
        Some(ProgressEvent::StageProgress {
            latest_signal,
            work_item_label,
            work_item_metrics,
            ..
        }) => {
            assert_eq!(
                latest_signal.as_deref(),
                Some(
                    "active restarts 4, cumulative evals 127, best loss 22.364253, best candidate incumbent_local_search"
                )
            );
            assert_eq!(work_item_label.as_deref(), Some("tick_moment_08"));
            assert_eq!(
                work_item_metrics
                    .iter()
                    .map(|metric| (metric.key.as_str(), metric.value.as_str()))
                    .collect::<Vec<_>>(),
                vec![
                    ("eval", "17/80"),
                    ("current_objective", "74.277624"),
                    ("best_objective", "51.125000"),
                ]
            );
        }
        _ => panic!("expected stage progress event"),
    }
}

#[test]
fn restart_progress_signal_uses_placeholders_for_unavailable_summary_fields() {
    let mut progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        Some("tick_moment_08"),
        Some(17),
        Some(80),
        None,
        None,
    );

    progress.stage = CalibrationStage::JointRefinement;
    progress.progress_phase = CalibrationStageProgressPhase::JointRefinement(
        JointRefinementStageProgressPhase::RestartOptimization,
    );
    progress.current_global_loss = Some(74.277624);
    progress.current_restart_best_global_loss = Some(51.125);
    progress.active_restart_count = Some(4);

    let progress_event = progress_event_from_staged_event(
        &CalibrationStagedRunObserverEvent::StageProgress(Box::new(progress)),
    )
    .expect("restart progress should map to terminal progress");

    match progress_event {
        Some(ProgressEvent::StageProgress { latest_signal, .. }) => {
            assert_eq!(
                latest_signal.as_deref(),
                Some("active restarts 4, cumulative evals -, best loss -, best candidate -")
            );
        }
        _ => panic!("expected stage progress event"),
    }
}

#[test]
fn restart_finalization_metrics_report_final_loss_without_replacing_search_losses() {
    let mut progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        Some("tick_moment_08"),
        Some(47),
        Some(47),
        Some(22.364253),
        None,
    );

    progress.stage = CalibrationStage::JointRefinement;
    progress.progress_phase = CalibrationStageProgressPhase::JointRefinement(
        JointRefinementStageProgressPhase::RestartOptimization,
    );
    progress.evaluation_progress_percent = Some(100);
    progress.current_global_loss = Some(74.277624);
    progress.current_restart_best_global_loss = Some(51.125);
    progress.current_final_global_loss = Some(63.75);

    assert_eq!(
        work_item_table_metrics(&progress)
            .into_iter()
            .map(|metric| (metric.key, metric.value))
            .collect::<Vec<_>>(),
        vec![
            ("eval".to_string(), "47/47".to_string()),
            ("final_objective".to_string(), "63.750000".to_string()),
        ]
    );
}

#[test]
fn restart_evaluation_timing_progress_writes_metric_update_event() {
    let mut progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        Some("tick_moment_21"),
        Some(12),
        Some(59),
        Some(0.5),
        None,
    );

    progress.stage = CalibrationStage::JointRefinement;
    progress.progress_phase = CalibrationStageProgressPhase::JointRefinement(
        JointRefinementStageProgressPhase::RestartOptimization,
    );
    progress.restart_evaluation_timing_diagnostic =
        Some(JointRefinementStageRestartEvaluationTimingDiagnostic {
            restart_id: "tick_moment_21".to_string(),
            schedule_category: "competitive".to_string(),
            candidate_evaluation_count: 12,
            search_direction_position: Some(3),
            search_direction_label: "interpolation.nig.log_alpha".to_string(),
            elapsed_millis: 1234,
            objective: Some(0.5),
            admissibility_blocker_count: Some(1),
            max_standardized_residual: Some(2.0),
            exact_closed_form_buy_count: Some(3),
            exact_closed_form_sell_count: Some(4),
            exact_tilted_mixed_count: Some(5),
        });

    let events = run_events_from_staged_event(
        &CalibrationStagedRunObserverEvent::StageProgress(Box::new(progress)),
        WorkflowRef::labeled("fit", "Fit"),
        "test-scope",
        "test scope",
        &[],
        "/tmp/janiform-test-output",
    )
    .expect("progress event should serialize");

    assert_eq!(events.len(), 2);
    assert!(matches!(
        events[1].payload,
        RunEventPayload::MetricUpdate(RunMetricUpdateEvent { .. })
    ));

    let RunEventPayload::MetricUpdate(metric_update) = &events[1].payload else {
        panic!("expected metric update event");
    };
    assert_eq!(
        metric_update
            .measures
            .iter()
            .map(|measure| match measure {
                EventProgressMeasure::Text { metric_id, .. }
                | EventProgressMeasure::EvaluationCount { metric_id, .. }
                | EventProgressMeasure::ObjectiveValue { metric_id, .. }
                | EventProgressMeasure::Scalar { metric_id, .. } => metric_id.as_str(),
                EventProgressMeasure::BoundedCount { metric_id, .. }
                | EventProgressMeasure::Convergence { metric_id, .. }
                | EventProgressMeasure::CandidateId { metric_id, .. }
                | EventProgressMeasure::PricingGeometry { metric_id, .. }
                | EventProgressMeasure::SimulationIntervalCount { metric_id, .. }
                | EventProgressMeasure::OutputChunkCount { metric_id, .. }
                | EventProgressMeasure::AnalyticsCheckCount { metric_id, .. } => {
                    metric_id.as_str()
                }
            })
            .collect::<Vec<_>>(),
        vec![
            "joint_refinement.restart_eval.restart_id",
            "joint_refinement.restart_eval.schedule_category",
            "joint_refinement.restart_eval.candidate_evaluation_count",
            "joint_refinement.restart_eval.search_direction_label",
            "joint_refinement.restart_eval.elapsed_millis",
            "joint_refinement.restart_eval.search_direction_position",
            "joint_refinement.restart_eval.objective",
            "joint_refinement.restart_eval.admissibility_blocker_count",
            "joint_refinement.restart_eval.max_standardized_residual",
            "joint_refinement.restart_eval.exact_closed_form_buy_count",
            "joint_refinement.restart_eval.exact_closed_form_sell_count",
            "joint_refinement.restart_eval.exact_tilted_mixed_count",
        ],
    );
}

#[test]
fn option_refinement_progress_table_includes_pricing_interval_and_rows() {
    let mut progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        Some("restart_01"),
        Some(1),
        Some(27),
        None,
        None,
    );

    progress.stage = CalibrationStage::JointRefinement;

    progress.progress_phase = CalibrationStageProgressPhase::JointRefinement(
        JointRefinementStageProgressPhase::OptionSearchTierRefinement,
    );

    progress.current_pricing_interval_index = Some(24);
    progress.total_pricing_interval_count = Some(943);
    progress.completed_pricing_row_count = Some(1);
    progress.total_pricing_row_count = Some(27);

    assert_eq!(
        work_item_table_columns(&progress)
            .into_iter()
            .map(|column| (column.key, column.label))
            .collect::<Vec<_>>(),
        vec![
            ("eval".to_string(), "Evals".to_string()),
            ("pricing_interval".to_string(), "Interval".to_string()),
            ("pricing_rows".to_string(), "Rows".to_string()),
        ]
    );

    assert_eq!(
        work_item_table_metrics(&progress)
            .into_iter()
            .map(|metric| (metric.key, metric.value))
            .collect::<Vec<_>>(),
        vec![
            ("eval".to_string(), "1/27".to_string()),
            ("pricing_interval".to_string(), "25/943".to_string()),
            ("pricing_rows".to_string(), "1/27".to_string()),
        ]
    );
}

#[test]
fn option_refinement_phase_start_seeds_planned_rows_without_active_work_item() {
    let mut progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        None,
        None,
        None,
        None,
        None,
    );

    progress.stage = CalibrationStage::JointRefinement;
    progress.progress_phase = CalibrationStageProgressPhase::JointRefinement(
        JointRefinementStageProgressPhase::OptionSearchTierRefinement,
    );
    progress.aggregate_progress_percent = Some(0);
    progress.planned_work_item_labels = vec!["halton_02".to_string(), "halton_14".to_string()];

    let progress_event = progress_event_from_staged_event(
        &CalibrationStagedRunObserverEvent::StageProgress(Box::new(progress)),
    )
    .expect("option refinement phase-start progress should map to terminal progress");

    match progress_event {
        Some(ProgressEvent::StageProgress {
            work_item_label,
            planned_work_item_labels,
            work_item_columns,
            work_item_metrics,
            latest_signal,
            ..
        }) => {
            assert_eq!(work_item_label, None);
            assert_eq!(
                planned_work_item_labels,
                vec!["halton_02".to_string(), "halton_14".to_string()]
            );
            assert_eq!(work_item_columns, Vec::new());
            assert_eq!(work_item_metrics, Vec::new());
            assert_eq!(latest_signal, None);
        }
        _ => panic!("expected stage progress event"),
    }
}

#[test]
fn option_refinement_interval_metric_uses_completed_count_without_row_detail() {
    let mut progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        Some("restart_01"),
        Some(3),
        Some(27),
        None,
        None,
    );

    progress.stage = CalibrationStage::JointRefinement;

    progress.progress_phase = CalibrationStageProgressPhase::JointRefinement(
        JointRefinementStageProgressPhase::OptionSearchTierRefinement,
    );

    progress.current_pricing_interval_index = Some(24);
    progress.total_pricing_interval_count = Some(47);

    assert_eq!(
        work_item_table_metrics(&progress)
            .into_iter()
            .map(|metric| (metric.key, metric.value))
            .collect::<Vec<_>>(),
        vec![
            ("eval".to_string(), "3/27".to_string()),
            ("pricing_interval".to_string(), "24/47".to_string()),
        ]
    );
}

#[test]
fn option_selection_progress_table_includes_single_pricing_eval() {
    let mut progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        Some("tick_moment_04"),
        Some(1),
        Some(1),
        None,
        None,
    );

    progress.stage = CalibrationStage::JointRefinement;
    progress.progress_phase = CalibrationStageProgressPhase::JointRefinement(
        JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
    );
    progress.evaluation_progress_percent = Some(37);
    progress.current_pricing_interval_index = Some(145);
    progress.total_pricing_interval_count = Some(734);

    assert_eq!(
        work_item_table_columns(&progress)
            .into_iter()
            .map(|column| (column.key, column.label))
            .collect::<Vec<_>>(),
        vec![
            ("eval".to_string(), "Evals".to_string()),
            ("pricing_interval".to_string(), "Interval".to_string()),
        ]
    );

    assert_eq!(
        work_item_table_metrics(&progress)
            .into_iter()
            .map(|metric| (metric.key, metric.value))
            .collect::<Vec<_>>(),
        vec![
            ("eval".to_string(), "1/1".to_string()),
            ("pricing_interval".to_string(), "145/734".to_string()),
        ]
    );

    let progress_event = progress_event_from_staged_event(
        &CalibrationStagedRunObserverEvent::StageProgress(Box::new(progress)),
    )
    .expect("option selection progress should map to terminal progress");

    match progress_event {
        Some(ProgressEvent::StageProgress {
            work_item_progress_percent,
            latest_signal,
            ..
        }) => {
            assert_eq!(work_item_progress_percent, Some(37));
            assert_eq!(latest_signal, None);
        }
        _ => panic!("expected stage progress event"),
    }
}

#[test]
fn option_refinement_progress_signal_prefers_best_loss_over_work_item_label() {
    let mut progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        Some("halton_02"),
        Some(3),
        Some(5),
        Some(22.364253),
        None,
    );

    progress.stage = CalibrationStage::JointRefinement;
    progress.progress_phase = CalibrationStageProgressPhase::JointRefinement(
        JointRefinementStageProgressPhase::OptionSearchTierRefinement,
    );
    progress.current_global_loss = Some(74.277624);
    progress.current_pricing_interval_index = Some(47);
    progress.total_pricing_interval_count = Some(47);

    let progress_event = progress_event_from_staged_event(
        &CalibrationStagedRunObserverEvent::StageProgress(Box::new(progress)),
    )
    .expect("option refinement progress should map to terminal progress");

    match progress_event {
        Some(ProgressEvent::StageProgress { latest_signal, .. }) => {
            assert_eq!(latest_signal.as_deref(), Some("best loss 22.364253"));
        }
        _ => panic!("expected stage progress event"),
    }
}

#[test]
fn option_refinement_progress_signal_omits_row_scoped_eval_count() {
    let mut progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        Some("halton_02"),
        Some(0),
        Some(21),
        None,
        None,
    );

    progress.stage = CalibrationStage::JointRefinement;
    progress.progress_phase = CalibrationStageProgressPhase::JointRefinement(
        JointRefinementStageProgressPhase::OptionSearchTierRefinement,
    );
    progress.current_pricing_interval_index = Some(0);
    progress.total_pricing_interval_count = Some(47);

    let progress_event = progress_event_from_staged_event(
        &CalibrationStagedRunObserverEvent::StageProgress(Box::new(progress)),
    )
    .expect("option refinement progress should map to terminal progress");

    match progress_event {
        Some(ProgressEvent::StageProgress { latest_signal, .. }) => {
            assert_eq!(latest_signal, None);
        }
        _ => panic!("expected stage progress event"),
    }
}

#[test]
fn finalization_pre_evaluation_progress_uses_named_phase_and_signal() {
    let mut progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        Some("resolve stage inputs"),
        None,
        None,
        None,
        None,
    );

    progress.stage = CalibrationStage::Finalization;
    progress.progress_phase = CalibrationStageProgressPhase::JointRefinement(
        JointRefinementStageProgressPhase::StageInputResolution,
    );

    let progress_event = progress_event_from_staged_event(
        &CalibrationStagedRunObserverEvent::StageProgress(Box::new(progress)),
    )
    .expect("finalization setup progress should map to terminal progress");

    match progress_event {
        Some(ProgressEvent::StageProgress {
            phase,
            work_item_label,
            latest_signal,
            ..
        }) => {
            assert_eq!(phase, "stage input resolution");
            assert_eq!(work_item_label, None);
            assert_eq!(latest_signal.as_deref(), Some("resolve stage inputs"));
        }
        _ => panic!("expected stage progress event"),
    }
}

#[test]
fn stage_progress_measure_uses_aggregate_percent_when_available() {
    let mut progress = return_components_progress_event(
        ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        Some("restart_01"),
        Some(1),
        Some(27),
        None,
        None,
    );

    progress.aggregate_progress_percent = Some(40);

    assert_eq!(
        stage_progress_measure(&progress).expect("progress measure should validate"),
        UiProgressMeasure::percent(40),
    );
}

fn return_components_progress_event(
    phase: ReturnComponentsStageProgressPhase,
    current_item_label: Option<&str>,
    current_evaluation_count: Option<usize>,
    total_evaluation_budget: Option<usize>,
    best_global_loss: Option<f64>,
    current_convergence_ratio: Option<f64>,
) -> CalibrationStagedRunStageProgressEvent {
    CalibrationStagedRunStageProgressEvent {
        run_id: "test-run".to_string(),
        stage: CalibrationStage::ReturnComponents,
        stage_index: 3,
        stage_count: 5,
        progress_phase: CalibrationStageProgressPhase::ReturnComponents(phase),
        completed_units: 0,
        total_units: 1,
        stage_progress_percent: None,
        aggregate_progress_percent: None,
        current_item_label: current_item_label.map(str::to_string),
        current_restart_id: None,
        current_work_item_status: None,
        planned_work_item_labels: Vec::new(),
        active_restart_count: None,
        completed_outer_cycles: None,
        total_outer_cycles: None,
        current_evaluation_count,
        total_evaluation_budget,
        evaluation_progress_percent: None,
        current_pricing_interval_index: None,
        total_pricing_interval_count: None,
        completed_pricing_row_count: None,
        total_pricing_row_count: None,
        valid_completed_units: None,
        cumulative_local_evaluation_count: None,
        current_global_loss: None,
        current_restart_best_global_loss: None,
        current_final_global_loss: None,
        current_candidate_assessment: None,
        best_restart_id: None,
        best_global_loss,
        current_coarse_consistency_fit: None,
        best_coarse_consistency_fit: None,
        exact_closed_form_buy_count: None,
        exact_closed_form_sell_count: None,
        exact_tilted_mixed_count: None,
        restart_evaluation_timing_diagnostic: None,
        current_convergence_ratio,
    }
}
