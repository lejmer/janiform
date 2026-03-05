mod support;

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use janiform_core::verification::{
    GapBehaviorBlockBootstrapConfiguration, GapBehaviorClosedWindowRegime, GapBehaviorDatasetInput,
    GapBehaviorDiagnosticsRequest, GapBehaviorHorizonBucket, GapBehaviorMarketState,
    GapBehaviorMetricSampleSufficiencyPolicy, GapBehaviorMetricStatus,
    GapBehaviorSampleSufficiencyPolicy, GapBehaviorScenarioInput, GapBehaviorThresholdPolicy,
    NewsSemanticsScenarioClass, OpenClosedGapDeterministicReplayMetadata,
    OpenClosedGapIssueTraceabilityMetadata, run_gap_behavior_diagnostics,
};

use support::validation_fixture::{
    OpenClosedGapBehaviorScenarioCaseFixture, load_calendar_regression_fixture_from_manifest,
    load_open_closed_gap_behavior_scenario_fixture_from_manifest,
    validate_open_closed_gap_behavior_calendar_integration,
};

#[test]
fn gap_behavior_metrics_match_controlled_open_closed_fixture() {
    let request = GapBehaviorDiagnosticsRequest {
        analysis_start_utc_nanos: 0,
        analysis_end_utc_nanos: 100,
        empirical_dataset: controlled_dataset("empirical"),
        simulated_dataset: controlled_dataset("simulated"),
        horizon_buckets: vec![GapBehaviorHorizonBucket {
            horizon_label: "10n".to_string(),
            horizon_duration_nanos: 10,
        }],
        bootstrap_configuration: GapBehaviorBlockBootstrapConfiguration {
            confidence_level: 0.95,
            replication_count: 300,
            block_length_interval_count: 3,
            random_seed: 9_011,
        },
        sample_sufficiency_policy: permissive_sample_sufficiency_policy(),
        threshold_policy: GapBehaviorThresholdPolicy::default(),
    };

    let report = run_gap_behavior_diagnostics(request)
        .expect("gap-behavior diagnostics should succeed on controlled fixture");
    assert_eq!(report.horizons.len(), 1);

    let horizon_report = &report.horizons[0];
    let open_jump_metric = &horizon_report.open_jump_abs_tail_expected_shortfall_ratio;
    let closed_gap_metric = &horizon_report.closed_gap_abs_tail_expected_shortfall_ratio;
    let contrast_metric = &horizon_report.closed_gap_vs_open_jump_wasserstein_distance;

    assert_eq!(open_jump_metric.status, GapBehaviorMetricStatus::Available);
    assert_eq!(closed_gap_metric.status, GapBehaviorMetricStatus::Available);
    assert_eq!(contrast_metric.status, GapBehaviorMetricStatus::Available);

    let observed_open_jump_value = open_jump_metric
        .empirical
        .value
        .expect("open-jump metric value should be available");
    let observed_closed_gap_value = closed_gap_metric
        .empirical
        .value
        .expect("closed-gap metric value should be available");
    let observed_contrast_value = contrast_metric
        .empirical
        .value
        .expect("contrast metric value should be available");

    assert!(
        (observed_open_jump_value - 1.034_482_758_620_689_7).abs() <= 1e-12,
        "unexpected open-jump expected-shortfall ratio",
    );
    assert!(
        (observed_closed_gap_value - 1.052_631_578_947_368_4).abs() <= 1e-12,
        "unexpected closed-gap expected-shortfall ratio",
    );
    assert!(
        (observed_contrast_value - (1.0 / 6.0)).abs() <= 1e-12,
        "unexpected open-vs-closed wasserstein distance",
    );

    let empirical_component_summary = horizon_report.component_coupling_summary.empirical;
    assert_eq!(empirical_component_summary.open_window_event_count, 3);
    assert_eq!(empirical_component_summary.closed_window_event_count, 3);
    assert_eq!(
        empirical_component_summary.overnight_closed_window_event_count,
        2
    );
    assert_eq!(
        empirical_component_summary.weekend_closed_window_event_count,
        1
    );
    assert!((empirical_component_summary.activity_coupling_total - 2.1).abs() <= 1e-12);
    assert!((empirical_component_summary.open_jump_total - 0.0).abs() <= 1e-12);
    assert!((empirical_component_summary.closed_gap_total + 5.0).abs() <= 1e-12);
    assert!((empirical_component_summary.overnight_closed_gap_total + 1.0).abs() <= 1e-12);
    assert!((empirical_component_summary.weekend_closed_gap_total + 4.0).abs() <= 1e-12);

    let confidence_interval = open_jump_metric
        .empirical
        .confidence_interval
        .expect("available metric should include confidence interval");
    assert!(
        confidence_interval.lower_bound <= observed_open_jump_value
            && confidence_interval.upper_bound >= observed_open_jump_value,
    );
}

#[test]
fn gap_behavior_report_is_deterministic_with_shared_fixture_metadata() {
    let loaded_calendar_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar regression fixture should load from manifest");
    let loaded_gap_fixture = load_open_closed_gap_behavior_scenario_fixture_from_manifest()
        .expect("open and closed gap behavior fixture should load from manifest");

    validate_open_closed_gap_behavior_calendar_integration(
        &loaded_gap_fixture.fixture_document,
        &loaded_calendar_fixture.fixture_document,
    )
    .expect("open and closed gap fixture should remain integrated with shared calendar fixtures");

    let empirical_dataset = dataset_from_gap_fixture_cases(
        "empirical_fixture",
        loaded_gap_fixture.fixture_document.cases.as_slice(),
    );
    let simulated_dataset = dataset_from_gap_fixture_cases(
        "simulated_fixture",
        loaded_gap_fixture.fixture_document.cases.as_slice(),
    );

    let mut all_event_timestamps: Vec<i64> = empirical_dataset
        .scenarios
        .iter()
        .flat_map(|scenario| {
            scenario
                .event_observations
                .iter()
                .map(|event| event.timestamp_utc_nanos)
        })
        .collect();
    all_event_timestamps.sort_unstable();

    let analysis_start_utc_nanos = *all_event_timestamps
        .first()
        .expect("fixture-derived dataset should contain event timestamps");
    let analysis_end_utc_nanos = all_event_timestamps
        .last()
        .copied()
        .expect("fixture-derived dataset should contain event timestamps")
        .saturating_add(1);

    let request = GapBehaviorDiagnosticsRequest {
        analysis_start_utc_nanos,
        analysis_end_utc_nanos,
        empirical_dataset,
        simulated_dataset,
        horizon_buckets: vec![GapBehaviorHorizonBucket {
            horizon_label: "6h".to_string(),
            horizon_duration_nanos: 6 * 60 * 60 * 1_000_000_000,
        }],
        bootstrap_configuration: GapBehaviorBlockBootstrapConfiguration {
            confidence_level: 0.95,
            replication_count: 200,
            block_length_interval_count: 4,
            random_seed: 44_019,
        },
        sample_sufficiency_policy: permissive_sample_sufficiency_policy(),
        threshold_policy: GapBehaviorThresholdPolicy::default(),
    };

    let first_report = run_gap_behavior_diagnostics(request.clone())
        .expect("first diagnostics run on shared fixture should succeed");
    let second_report = run_gap_behavior_diagnostics(request)
        .expect("second diagnostics run on shared fixture should succeed");

    assert_eq!(first_report, second_report);

    let first_payload =
        serde_json::to_string(&first_report).expect("first diagnostics report should serialize");
    let second_payload =
        serde_json::to_string(&second_report).expect("second diagnostics report should serialize");
    assert_eq!(first_payload, second_payload);

    assert_eq!(
        first_report.empirical_scenarios.len(),
        loaded_gap_fixture.fixture_document.cases.len()
    );
    assert!(first_report.empirical_scenarios.iter().all(|scenario| {
        !scenario
            .deterministic_replay_metadata
            .replay_baseline_id
            .is_empty()
            && !scenario.issue_traceability.issue_id.is_empty()
            && !scenario.issue_traceability.semantic_focus_tags.is_empty()
    }));

    let component_summary = first_report.horizons[0]
        .component_coupling_summary
        .empirical;
    assert!(component_summary.overnight_closed_window_event_count > 0);
    assert!(component_summary.weekend_closed_window_event_count > 0);
}

#[test]
fn gap_behavior_marks_closed_window_metrics_insufficient_when_closed_samples_are_sparse() {
    let request = GapBehaviorDiagnosticsRequest {
        analysis_start_utc_nanos: 0,
        analysis_end_utc_nanos: 100,
        empirical_dataset: sparse_closed_dataset("empirical"),
        simulated_dataset: sparse_closed_dataset("simulated"),
        horizon_buckets: vec![GapBehaviorHorizonBucket {
            horizon_label: "10n".to_string(),
            horizon_duration_nanos: 10,
        }],
        bootstrap_configuration: GapBehaviorBlockBootstrapConfiguration {
            confidence_level: 0.95,
            replication_count: 200,
            block_length_interval_count: 3,
            random_seed: 71_337,
        },
        sample_sufficiency_policy: sparse_closed_gate_sample_sufficiency_policy(),
        threshold_policy: GapBehaviorThresholdPolicy::default(),
    };

    let report = run_gap_behavior_diagnostics(request)
        .expect("gap-behavior diagnostics should succeed on sparse closed-window fixture");
    let horizon_report = &report.horizons[0];

    assert_eq!(
        horizon_report
            .open_jump_abs_tail_expected_shortfall_ratio
            .status,
        GapBehaviorMetricStatus::Available
    );
    assert_eq!(
        horizon_report
            .closed_gap_abs_tail_expected_shortfall_ratio
            .status,
        GapBehaviorMetricStatus::InsufficientData
    );
    assert_eq!(
        horizon_report
            .closed_gap_vs_open_jump_wasserstein_distance
            .status,
        GapBehaviorMetricStatus::InsufficientData
    );
}

fn controlled_dataset(dataset_id: &str) -> GapBehaviorDatasetInput {
    GapBehaviorDatasetInput {
        dataset_id: dataset_id.to_string(),
        scenarios: vec![
            GapBehaviorScenarioInput {
                scenario_id: format!("{dataset_id}_intraday"),
                exchange_code: "XNYS".to_string(),
                scenario_class: NewsSemanticsScenarioClass::KnownOnly,
                deterministic_replay_metadata: deterministic_metadata("intraday"),
                issue_traceability: issue_traceability(
                    "JAN-167",
                    vec!["open_jump", "activity_coupling"],
                ),
                tags: vec!["open_window".to_string(), "intraday".to_string()],
                event_observations: vec![
                    open_event("event_open_0", 1, 0.2, -1.0),
                    open_event("event_open_1", 12, 0.3, 3.0),
                    open_event("event_open_2", 21, 0.4, -2.0),
                ],
            },
            GapBehaviorScenarioInput {
                scenario_id: format!("{dataset_id}_closed"),
                exchange_code: "XNYS".to_string(),
                scenario_class: NewsSemanticsScenarioClass::SurpriseOnly,
                deterministic_replay_metadata: deterministic_metadata("closed"),
                issue_traceability: issue_traceability(
                    "JAN-167",
                    vec!["closed_gap", "overnight", "weekend"],
                ),
                tags: vec!["closed_window".to_string(), "mixed_window".to_string()],
                event_observations: vec![
                    closed_event(
                        "event_closed_0",
                        32,
                        GapBehaviorClosedWindowRegime::Overnight,
                        0.5,
                        -2.0,
                    ),
                    closed_event(
                        "event_closed_1",
                        41,
                        GapBehaviorClosedWindowRegime::Overnight,
                        0.2,
                        1.0,
                    ),
                    closed_event(
                        "event_closed_2",
                        52,
                        GapBehaviorClosedWindowRegime::Weekend,
                        0.5,
                        -4.0,
                    ),
                ],
            },
        ],
    }
}

fn sparse_closed_dataset(dataset_id: &str) -> GapBehaviorDatasetInput {
    GapBehaviorDatasetInput {
        dataset_id: dataset_id.to_string(),
        scenarios: vec![
            GapBehaviorScenarioInput {
                scenario_id: format!("{dataset_id}_open_dense"),
                exchange_code: "XNYS".to_string(),
                scenario_class: NewsSemanticsScenarioClass::KnownOnly,
                deterministic_replay_metadata: deterministic_metadata("open_dense"),
                issue_traceability: issue_traceability("JAN-167", vec!["open_jump"]),
                tags: vec!["open_window".to_string()],
                event_observations: vec![
                    open_event("event_open_0", 2, 0.1, -0.8),
                    open_event("event_open_1", 12, 0.1, 0.9),
                    open_event("event_open_2", 22, 0.1, -1.1),
                    open_event("event_open_3", 32, 0.1, 1.0),
                    open_event("event_open_4", 42, 0.1, -0.7),
                ],
            },
            GapBehaviorScenarioInput {
                scenario_id: format!("{dataset_id}_closed_sparse"),
                exchange_code: "XNYS".to_string(),
                scenario_class: NewsSemanticsScenarioClass::SurpriseOnly,
                deterministic_replay_metadata: deterministic_metadata("closed_sparse"),
                issue_traceability: issue_traceability("JAN-167", vec!["closed_gap", "overnight"]),
                tags: vec!["closed_window".to_string(), "overnight".to_string()],
                event_observations: vec![closed_event(
                    "event_closed_0",
                    72,
                    GapBehaviorClosedWindowRegime::Overnight,
                    0.3,
                    -0.4,
                )],
            },
        ],
    }
}

fn permissive_sample_sufficiency_policy() -> GapBehaviorSampleSufficiencyPolicy {
    GapBehaviorSampleSufficiencyPolicy {
        open_jump_abs_tail_expected_shortfall_ratio: GapBehaviorMetricSampleSufficiencyPolicy {
            minimum_sample_count: 1,
            minimum_effective_sample_count: 1,
            minimum_tail_observation_count: 1,
            minimum_horizon_coverage_ratio: 0.01,
        },
        closed_gap_abs_tail_expected_shortfall_ratio: GapBehaviorMetricSampleSufficiencyPolicy {
            minimum_sample_count: 1,
            minimum_effective_sample_count: 1,
            minimum_tail_observation_count: 1,
            minimum_horizon_coverage_ratio: 0.01,
        },
        closed_gap_vs_open_jump_wasserstein_distance: GapBehaviorMetricSampleSufficiencyPolicy {
            minimum_sample_count: 1,
            minimum_effective_sample_count: 1,
            minimum_tail_observation_count: 0,
            minimum_horizon_coverage_ratio: 0.01,
        },
    }
}

fn sparse_closed_gate_sample_sufficiency_policy() -> GapBehaviorSampleSufficiencyPolicy {
    GapBehaviorSampleSufficiencyPolicy {
        open_jump_abs_tail_expected_shortfall_ratio: GapBehaviorMetricSampleSufficiencyPolicy {
            minimum_sample_count: 3,
            minimum_effective_sample_count: 3,
            minimum_tail_observation_count: 1,
            minimum_horizon_coverage_ratio: 0.1,
        },
        closed_gap_abs_tail_expected_shortfall_ratio: GapBehaviorMetricSampleSufficiencyPolicy {
            minimum_sample_count: 2,
            minimum_effective_sample_count: 2,
            minimum_tail_observation_count: 1,
            minimum_horizon_coverage_ratio: 0.1,
        },
        closed_gap_vs_open_jump_wasserstein_distance: GapBehaviorMetricSampleSufficiencyPolicy {
            minimum_sample_count: 2,
            minimum_effective_sample_count: 2,
            minimum_tail_observation_count: 0,
            minimum_horizon_coverage_ratio: 0.1,
        },
    }
}

fn deterministic_metadata(label: &str) -> OpenClosedGapDeterministicReplayMetadata {
    OpenClosedGapDeterministicReplayMetadata {
        replay_baseline_id: format!("baseline_{label}"),
        event_calendar_binding_id: format!("calendar_binding_{label}"),
        jump_observation_binding_id: format!("jump_obs_{label}"),
        gap_observation_binding_id: format!("gap_obs_{label}"),
        candidate_selection_policy_id: "deterministic_candidate_ranked.v1".to_string(),
        candidate_selection_rank: 0,
    }
}

fn issue_traceability(
    issue_id: &str,
    semantic_focus_tags: Vec<&str>,
) -> OpenClosedGapIssueTraceabilityMetadata {
    OpenClosedGapIssueTraceabilityMetadata {
        issue_id: issue_id.to_string(),
        semantic_focus_tags: semantic_focus_tags
            .into_iter()
            .map(str::to_string)
            .collect(),
    }
}

fn open_event(
    event_id: &str,
    timestamp_utc_nanos: i64,
    activity_coupling_increment: f64,
    open_jump_increment: f64,
) -> janiform_core::verification::GapBehaviorEventObservation {
    janiform_core::verification::GapBehaviorEventObservation {
        event_id: event_id.to_string(),
        timestamp_utc_nanos,
        market_state: GapBehaviorMarketState::Open,
        closed_window_regime: None,
        activity_coupling_increment,
        open_jump_increment,
        closed_gap_increment: 0.0,
    }
}

fn closed_event(
    event_id: &str,
    timestamp_utc_nanos: i64,
    closed_window_regime: GapBehaviorClosedWindowRegime,
    activity_coupling_increment: f64,
    closed_gap_increment: f64,
) -> janiform_core::verification::GapBehaviorEventObservation {
    janiform_core::verification::GapBehaviorEventObservation {
        event_id: event_id.to_string(),
        timestamp_utc_nanos,
        market_state: GapBehaviorMarketState::Closed,
        closed_window_regime: Some(closed_window_regime),
        activity_coupling_increment,
        open_jump_increment: 0.0,
        closed_gap_increment,
    }
}

fn dataset_from_gap_fixture_cases(
    dataset_id: &str,
    fixture_cases: &[OpenClosedGapBehaviorScenarioCaseFixture],
) -> GapBehaviorDatasetInput {
    GapBehaviorDatasetInput {
        dataset_id: dataset_id.to_string(),
        scenarios: fixture_cases
            .iter()
            .map(gap_behavior_scenario_from_fixture_case)
            .collect(),
    }
}

fn gap_behavior_scenario_from_fixture_case(
    fixture_case: &OpenClosedGapBehaviorScenarioCaseFixture,
) -> GapBehaviorScenarioInput {
    let expected_outcome_by_event_id: BTreeMap<&str, _> = fixture_case
        .expected_outcome
        .event_outcomes
        .iter()
        .map(|event_outcome| (event_outcome.event_id.as_str(), event_outcome))
        .collect();

    let event_observations = fixture_case
        .injected_events
        .iter()
        .map(|injected_event| {
            let expected_event_outcome = expected_outcome_by_event_id
                .get(injected_event.event_id.as_str())
                .unwrap_or_else(|| {
                    panic!(
                        "fixture {} missing expected event outcome for event {}",
                        fixture_case.fixture_id, injected_event.event_id
                    )
                });

            let event_market_state =
                parse_market_state(expected_event_outcome.expected_market_state.as_str());
            let closed_window_regime = match event_market_state {
                GapBehaviorMarketState::Open => None,
                GapBehaviorMarketState::Closed => {
                    Some(closed_window_regime_for_fixture_case(fixture_case))
                }
            };

            janiform_core::verification::GapBehaviorEventObservation {
                event_id: injected_event.event_id.clone(),
                timestamp_utc_nanos: parse_timestamp_to_utc_nanos(
                    injected_event.timestamp_utc.as_str(),
                ),
                market_state: event_market_state,
                closed_window_regime,
                activity_coupling_increment: expected_event_outcome
                    .expected_activity_coupling_j_t_increment,
                open_jump_increment: expected_event_outcome.expected_open_jump_m_t_increment,
                closed_gap_increment: expected_event_outcome.expected_closed_gap_g_t_increment,
            }
        })
        .collect();

    GapBehaviorScenarioInput {
        scenario_id: fixture_case.fixture_id.clone(),
        exchange_code: fixture_case.exchange_code.clone(),
        scenario_class: parse_scenario_class(fixture_case.scenario_class.as_str()),
        deterministic_replay_metadata: OpenClosedGapDeterministicReplayMetadata {
            replay_baseline_id: fixture_case
                .deterministic_replay_metadata
                .replay_baseline_id
                .clone(),
            event_calendar_binding_id: fixture_case
                .deterministic_replay_metadata
                .event_calendar_binding_id
                .clone(),
            jump_observation_binding_id: fixture_case
                .deterministic_replay_metadata
                .jump_observation_binding_id
                .clone(),
            gap_observation_binding_id: fixture_case
                .deterministic_replay_metadata
                .gap_observation_binding_id
                .clone(),
            candidate_selection_policy_id: fixture_case
                .deterministic_replay_metadata
                .candidate_selection_policy_id
                .clone(),
            candidate_selection_rank: fixture_case
                .deterministic_replay_metadata
                .candidate_selection_rank as u64,
        },
        issue_traceability: OpenClosedGapIssueTraceabilityMetadata {
            issue_id: fixture_case.issue_traceability.issue_id.clone(),
            semantic_focus_tags: fixture_case.issue_traceability.semantic_focus_tags.clone(),
        },
        tags: fixture_case.tags.clone(),
        event_observations,
    }
}

fn parse_market_state(market_state: &str) -> GapBehaviorMarketState {
    match market_state {
        "open" => GapBehaviorMarketState::Open,
        "closed" => GapBehaviorMarketState::Closed,
        unsupported_market_state => {
            panic!("unsupported market state in fixture: {unsupported_market_state}")
        }
    }
}

fn parse_scenario_class(scenario_class: &str) -> NewsSemanticsScenarioClass {
    match scenario_class {
        "known_only" => NewsSemanticsScenarioClass::KnownOnly,
        "surprise_only" => NewsSemanticsScenarioClass::SurpriseOnly,
        "mixed_news" => NewsSemanticsScenarioClass::MixedNews,
        unsupported_scenario_class => {
            panic!("unsupported scenario_class in fixture: {unsupported_scenario_class}")
        }
    }
}

fn closed_window_regime_for_fixture_case(
    fixture_case: &OpenClosedGapBehaviorScenarioCaseFixture,
) -> GapBehaviorClosedWindowRegime {
    if fixture_case.tags.iter().any(|tag| tag == "weekend") {
        return GapBehaviorClosedWindowRegime::Weekend;
    }
    if fixture_case.tags.iter().any(|tag| tag == "overnight") {
        return GapBehaviorClosedWindowRegime::Overnight;
    }

    GapBehaviorClosedWindowRegime::OtherClosed
}

fn parse_timestamp_to_utc_nanos(timestamp_utc: &str) -> i64 {
    DateTime::parse_from_rfc3339(timestamp_utc)
        .unwrap_or_else(|source| panic!("timestamp parse failed for {timestamp_utc}: {source}"))
        .with_timezone(&Utc)
        .timestamp_nanos_opt()
        .unwrap_or_else(|| panic!("timestamp outside nanosecond range: {timestamp_utc}"))
}
