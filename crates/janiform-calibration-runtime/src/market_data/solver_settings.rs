use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, CalibrationStage, CalibrationStageSolverSettings,
};
use janiform_foundations::numeric::strictly_positive_machine_value;
use janiform_foundations::parallelism::resolved_cpu_count as resolve_cpu_count;

use super::loading::MarketDataLoadingOutput;

pub(crate) const DATA_DERIVED_SOLVER_SETTINGS_POLICY_ID: &str = "missing_stage_solver_settings_are_derived_from_stage_geometry_support_and_machine_resolution.v1";

#[derive(Debug, Clone, Copy, PartialEq)]
struct StageSolverGeometry {
    parameter_dimension: usize,
    objective_term_count: usize,
    support_count: usize,
    candidate_geometry_count: usize,
    scale_reference: f64,
}

pub(crate) fn has_missing_stage_solver_settings(
    selected_stages: &[CalibrationStage],
    configured_solver_settings: &[CalibrationStageSolverSettings],
) -> bool {
    selected_stages.iter().any(|selected_stage| {
        configured_solver_settings
            .iter()
            .all(|configured_stage_settings| configured_stage_settings.stage != *selected_stage)
    })
}

pub(crate) fn resolve_solver_settings_by_stage(
    selected_stages: &[CalibrationStage],
    configured_solver_settings: &[CalibrationStageSolverSettings],
    loading_output: &MarketDataLoadingOutput,
    cpu_count: Option<usize>,
) -> Result<Vec<CalibrationStageSolverSettings>, String> {
    for configured_stage_settings in configured_solver_settings {
        if !selected_stages.contains(&configured_stage_settings.stage) {
            return Err(format!(
                "optimizer_settings_by_stage contains stage '{}' outside the selected workflow scope",
                calibration_stage_name(configured_stage_settings.stage)
            ));
        }
    }

    let resolved_cpu_count = resolve_cpu_count(cpu_count);

    selected_stages
        .iter()
        .copied()
        .map(|stage| {
            configured_solver_settings
                .iter()
                .find(|configured_stage_settings| configured_stage_settings.stage == stage)
                .cloned()
                .map_or_else(
                    || derive_stage_solver_settings(stage, loading_output, resolved_cpu_count),
                    Ok,
                )
        })
        .collect()
}

fn derive_stage_solver_settings(
    stage: CalibrationStage,
    loading_output: &MarketDataLoadingOutput,
    cpu_count: usize,
) -> Result<CalibrationStageSolverSettings, String> {
    let geometry = stage_solver_geometry(stage, loading_output);
    let geometry_complexity_units = stage_geometry_complexity_units(geometry);

    let relative_tolerance = derived_relative_tolerance(geometry);
    let absolute_tolerance = derived_absolute_tolerance(geometry, geometry_complexity_units);

    let max_iterations = match stage {
        CalibrationStage::MeanReversion => {
            derived_mean_reversion_iteration_budget(absolute_tolerance.max(relative_tolerance))
        }
        _ => geometry
            .parameter_dimension
            .max(1)
            .saturating_mul(geometry_complexity_units.max(1)),
    };

    let constraint_penalty_weight =
        geometry_complexity_units as f64 / geometry.parameter_dimension.max(1) as f64;

    let optimizer_settings = CalibrationOptimizerSettings::try_new(
        "lbfgs",
        max_iterations.max(1),
        cpu_count.max(1),
        absolute_tolerance,
        relative_tolerance,
        constraint_penalty_weight,
    )
    .map_err(|source| format!("failed to derive optimizer settings: {source}"))?;

    Ok(CalibrationStageSolverSettings {
        stage,
        optimizer_settings,
    })
}

fn calibration_stage_name(stage: CalibrationStage) -> &'static str {
    match stage {
        CalibrationStage::Seasonality => "seasonality",
        CalibrationStage::Hawkes => "hawkes",
        CalibrationStage::ReturnComponents => "return_components",
        CalibrationStage::MeanReversion => "mean_reversion",
        CalibrationStage::JointRefinement => "joint_refinement",
    }
}

fn stage_solver_geometry(
    stage: CalibrationStage,
    loading_output: &MarketDataLoadingOutput,
) -> StageSolverGeometry {
    match stage {
        CalibrationStage::Seasonality => {
            let timestamp_record_count = loading_output
                .preprocessing_output
                .seasonality_stage_input
                .timestamp_event_records
                .len();
            let overlay_count = loading_output
                .preprocessing_output
                .seasonality_stage_input
                .scheduled_event_overlays
                .len();

            StageSolverGeometry {
                parameter_dimension: 3,
                objective_term_count: 3,
                support_count: timestamp_record_count,
                candidate_geometry_count: overlay_count,
                scale_reference: 1.0,
            }
        }
        CalibrationStage::Hawkes => {
            let candidate_inputs = loading_output
                .preprocessing_output
                .hawkes_candidate_inputs
                .as_deref()
                .unwrap_or(&[]);
            let support_count = candidate_inputs
                .first()
                .map(|candidate_input| candidate_input.source_regime.open_session_tick_count)
                .unwrap_or_default();
            let objective_term_count = candidate_inputs
                .first()
                .map(|candidate_input| {
                    5 + usize::from(candidate_input.source_regime.known_news_open_event_count > 0)
                })
                .unwrap_or(5);

            StageSolverGeometry {
                parameter_dimension: 3,
                objective_term_count,
                support_count,
                candidate_geometry_count: candidate_inputs.len(),
                scale_reference: 1.0,
            }
        }
        CalibrationStage::ReturnComponents => {
            let candidate_generation_input = loading_output
                .preprocessing_output
                .return_components_candidate_generation_input
                .as_ref();
            let support_count = candidate_generation_input.map_or(0, |input| {
                input.observed_open_session_returns.len()
                    + input.realized_known_news_return_samples.len()
                    + input.detected_intraday_jump_candidates.len()
                    + input.observed_close_open_boundary_observations.len()
            });
            let candidate_geometry_count = candidate_generation_input.map_or(0, |input| {
                input.realized_known_news_return_samples.len()
                    + input.detected_intraday_jump_candidates.len()
                    + input.observed_close_open_boundary_observations.len()
            });
            let objective_term_count = candidate_generation_input
                .map_or(1, active_return_components_objective_weight_count);
            let scale_reference =
                candidate_generation_input.map_or(1.0, return_components_scale_reference);

            StageSolverGeometry {
                parameter_dimension: 4,
                objective_term_count,
                support_count,
                candidate_geometry_count,
                scale_reference,
            }
        }
        CalibrationStage::MeanReversion => {
            let observations = loading_output
                .preprocessing_output
                .mean_reversion_open_session_price_observations
                .as_deref()
                .unwrap_or(&[]);
            let scale_reference = observed_mean_reversion_log_price_span(observations);

            StageSolverGeometry {
                parameter_dimension: 2,
                objective_term_count: 4,
                support_count: observations.len(),
                candidate_geometry_count: 4,
                scale_reference,
            }
        }
        CalibrationStage::JointRefinement => {
            let candidate_generation_input = loading_output
                .preprocessing_output
                .return_components_candidate_generation_input
                .as_ref();

            let known_news_sample_count = candidate_generation_input
                .map(|input| input.realized_known_news_return_samples.len())
                .unwrap_or_default();

            let surprise_jump_count = candidate_generation_input
                .map(|input| input.detected_intraday_jump_candidates.len())
                .unwrap_or_default();

            let return_support_count = candidate_generation_input.map_or(0, |input| {
                input.observed_open_session_returns.len()
                    + input.observed_close_open_boundary_observations.len()
            });

            let option_support_count = loading_output
                .option_surface_records
                .as_ref()
                .map_or(0, Vec::len);

            StageSolverGeometry {
                parameter_dimension: 8
                    + usize::from(known_news_sample_count > 0)
                    + usize::from(surprise_jump_count > 0) * 2,
                objective_term_count: 12 + usize::from(option_support_count > 0),
                support_count: return_support_count + option_support_count,
                candidate_geometry_count: loading_output
                    .preprocessing_output
                    .hawkes_candidate_inputs
                    .as_ref()
                    .map_or(0, Vec::len)
                    + surprise_jump_count
                    + option_support_count,
                scale_reference: 1.0,
            }
        }
    }
}

fn active_return_components_objective_weight_count(
    candidate_generation_input: &crate::CalibrationReturnComponentsStageCandidateGenerationInput,
) -> usize {
    [
        candidate_generation_input
            .objective_metric_weights
            .continuous_distribution_fit_weight,
        candidate_generation_input
            .objective_metric_weights
            .surprise_arrival_fit_weight,
        candidate_generation_input
            .objective_metric_weights
            .jump_fit_weight,
        candidate_generation_input
            .objective_metric_weights
            .gap_fit_weight,
        candidate_generation_input
            .objective_metric_weights
            .activity_coupling_fit_weight,
        candidate_generation_input
            .objective_metric_weights
            .price_range_sanity_fit_weight,
    ]
    .into_iter()
    .filter(|weight| *weight > 0.0)
    .count()
    .max(1)
}

fn return_components_scale_reference(
    candidate_generation_input: &crate::CalibrationReturnComponentsStageCandidateGenerationInput,
) -> f64 {
    let open_return_scale = candidate_generation_input
        .observed_open_session_returns
        .iter()
        .map(|observation| observation.log_return.abs())
        .fold(0.0, f64::max);

    let known_jump_scale = candidate_generation_input
        .realized_known_news_return_samples
        .iter()
        .map(|sample| {
            sample
                .open_window_jump_return
                .abs()
                .max(sample.closed_window_gap_return.abs())
        })
        .fold(0.0, f64::max);

    let surprise_jump_scale = candidate_generation_input
        .detected_intraday_jump_candidates
        .iter()
        .map(|candidate| candidate.jump_return.abs())
        .fold(0.0, f64::max);

    let boundary_gap_scale = candidate_generation_input
        .observed_close_open_boundary_observations
        .iter()
        .map(|observation| (observation.open_price.ln() - observation.close_price.ln()).abs())
        .fold(0.0, f64::max);

    candidate_generation_input
        .close_open_gap_detection_policy
        .material_gap_absolute_return_tolerance
        .abs()
        .max(open_return_scale)
        .max(known_jump_scale)
        .max(surprise_jump_scale)
        .max(boundary_gap_scale)
        .max(strictly_positive_machine_value())
}

fn observed_mean_reversion_log_price_span(
    observations: &[crate::CalibrationMeanReversionStageObservation],
) -> f64 {
    let mut minimum_log_price = f64::INFINITY;
    let mut maximum_log_price = f64::NEG_INFINITY;

    for observation in observations {
        minimum_log_price = minimum_log_price.min(observation.observed_log_price);
        maximum_log_price = maximum_log_price.max(observation.observed_log_price);
    }

    if minimum_log_price.is_finite() && maximum_log_price.is_finite() {
        (maximum_log_price - minimum_log_price)
            .abs()
            .max(strictly_positive_machine_value())
    } else {
        1.0
    }
}

fn stage_geometry_complexity_units(geometry: StageSolverGeometry) -> usize {
    geometry
        .objective_term_count
        .max(1)
        .saturating_mul(support_information_depth(geometry.support_count))
        .saturating_add(geometry.candidate_geometry_count.max(1))
}

fn support_information_depth(support_count: usize) -> usize {
    usize::try_from(support_count.max(1).ilog2())
        .unwrap_or(usize::MAX)
        .saturating_add(1)
}

fn derived_relative_tolerance(geometry: StageSolverGeometry) -> f64 {
    let resolution_cell_count = geometry
        .parameter_dimension
        .max(1)
        .saturating_mul(geometry.objective_term_count.max(1))
        .saturating_mul(geometry.support_count.max(1));

    1.0 / resolution_cell_count.max(1) as f64
}

fn derived_absolute_tolerance(
    geometry: StageSolverGeometry,
    geometry_complexity_units: usize,
) -> f64 {
    let scale_reference = geometry
        .scale_reference
        .max(strictly_positive_machine_value());

    let sample_resolution_tolerance = scale_reference * derived_relative_tolerance(geometry);
    let machine_resolution_tolerance =
        scale_reference * geometry_complexity_units.max(1) as f64 * f64::EPSILON;

    sample_resolution_tolerance.max(machine_resolution_tolerance)
}

fn derived_mean_reversion_iteration_budget(tolerance: f64) -> usize {
    let golden_ratio = (1.0 + 5.0_f64.sqrt()) / 2.0;

    let mut interval_width = 1.0_f64;
    let mut iteration_count = 0usize;

    while interval_width > tolerance {
        interval_width /= golden_ratio;
        iteration_count = iteration_count.saturating_add(1);
    }

    iteration_count.max(1)
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::CalibrationStage;

    use super::{
        StageSolverGeometry, derived_absolute_tolerance, derived_mean_reversion_iteration_budget,
        derived_relative_tolerance, has_missing_stage_solver_settings,
        resolve_solver_settings_by_stage, stage_geometry_complexity_units,
    };
    use crate::{
        CalibrationHawkesDirectionalLabelDiagnostics, CalibrationHawkesObservedSourceRegime,
        CalibrationHawkesStageCandidateInput, CalibrationIntradaySeasonalityEstimatorSettings,
        CalibrationIntradaySmoothingKernel, CalibrationReturnComponentsCloseOpenGapDetectionPolicy,
        CalibrationReturnComponentsKnownNewsCalendarInput,
        CalibrationReturnComponentsObjectiveMetricWeights,
        CalibrationReturnComponentsObservedOpenSessionReturn,
        CalibrationReturnComponentsSignalSourceWindow,
        CalibrationReturnComponentsStageCandidateGenerationInput,
        CalibrationSeasonalityStageExecutionInput, CalibrationSeasonalityStageExecutionSettings,
        CalibrationTimestampEventSessionFilter, MarketDataLoadingOutput,
        MarketDataSessionResolutionPayload, MarketDataSourceSummary,
        MarketDataTranslationPreprocessingOutput,
    };

    #[test]
    fn missing_stage_solver_settings_detection_tracks_selected_stages() {
        assert!(has_missing_stage_solver_settings(
            &[CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            &[],
        ));
    }

    #[test]
    fn derived_tolerances_follow_geometry_resolution() {
        let geometry = StageSolverGeometry {
            parameter_dimension: 4,
            objective_term_count: 6,
            support_count: 25,
            candidate_geometry_count: 8,
            scale_reference: 0.5,
        };

        assert_eq!(derived_relative_tolerance(geometry), 1.0 / 600.0);
        assert_eq!(
            derived_absolute_tolerance(geometry, stage_geometry_complexity_units(geometry)),
            0.5 / 600.0
        );
    }

    #[test]
    fn mean_reversion_iteration_budget_shrinks_unit_interval_to_tolerance() {
        let budget = derived_mean_reversion_iteration_budget(0.1);
        assert!(budget > 0);
    }

    #[test]
    fn resolve_solver_settings_derives_missing_entries_from_loading_output() {
        let loading_output = MarketDataLoadingOutput {
            session_resolution: MarketDataSessionResolutionPayload {
                schema_version: "schema".to_string(),
                resolution_kind: "kind".to_string(),
                provenance_kind: "provenance".to_string(),
                selected_policy_id: "policy".to_string(),
                manual_override_applied: false,
                exchange_code: "XNYS".to_string(),
                exchange_name: "NYSE".to_string(),
                resolved_timezone_name: "America/New_York".to_string(),
                timezone_source: "registry".to_string(),
                open_session_duration_hours_per_week: 32.5,
                confidence_score: 1.0,
                confidence_label: "high".to_string(),
                regular_session_windows: Vec::new(),
                recurring_closed_windows: Vec::new(),
                reasons: Vec::new(),
                ambiguity_warnings: Vec::new(),
                metadata_discrepancies: Vec::new(),
                resolved_session_definition_json: "{}".to_string(),
            },
            preprocessing_output: MarketDataTranslationPreprocessingOutput {
                seasonality_stage_input: CalibrationSeasonalityStageExecutionInput {
                    timestamp_event_records: Vec::new(),
                    scheduled_event_overlays: Vec::new(),
                    settings: Some(CalibrationSeasonalityStageExecutionSettings {
                        session_filter: CalibrationTimestampEventSessionFilter::OpenOnly,
                        sparse_hour_event_threshold: 1,
                        intraday_settings: CalibrationIntradaySeasonalityEstimatorSettings {
                            minutes_per_bin: 1,
                            smoothing_kernel: CalibrationIntradaySmoothingKernel::Gaussian,
                            smoothing_bandwidth_bins: 1.0,
                            minimum_bin_events: 1,
                            pseudocount: 1.0,
                        },
                        day_of_week_settings: crate::CalibrationDayOfWeekSeasonalityEstimatorSettings {
                            minimum_bucket_events: 1,
                            pseudocount: 1.0,
                        },
                        day_of_year_settings: crate::CalibrationDayOfYearSeasonalityEstimatorSettings {
                            minimum_bucket_events: 1,
                            pseudocount: 1.0,
                        },
                    }),
                    dataset_id_override: None,
                    estimator_version_override: None,
                },
                hawkes_candidate_inputs: Some(vec![CalibrationHawkesStageCandidateInput {
                    candidate_id: "candidate".to_string(),
                    lambda_bar: 1.0,
                    beta: 1.0,
                    structured_alpha: janiform_model_kernels::hawkes::HawkesStructuredAlphaParameterization {
                        a_self: 1.0,
                        a_cross: 0.0,
                        delta_self: 0.0,
                        delta_cross: 0.0,
                    },
                    structured_rho: janiform_model_kernels::hawkes::HawkesStructuredRhoParameterization {
                        r_self: 1.0,
                        r_cross: 0.0,
                        delta_self_r: 0.0,
                        delta_cross_r: 0.0,
                    },
                    self_dominance_policy: None,
                    stability_numeric_tolerance: 0.0,
                    source_regime: CalibrationHawkesObservedSourceRegime {
                        observation_duration_hours: 6.5,
                        open_session_duration_hours: 6.5,
                        total_tick_count: 10,
                        open_session_tick_count: 10,
                        closed_session_tick_count: 0,
                        open_session_buy_tick_count: 5,
                        open_session_sell_tick_count: 5,
                        open_session_mean_interarrival_seconds: 10.0,
                        open_session_interarrival_coefficient_of_variation: 1.0,
                        open_session_short_gap_threshold_seconds: 5.0,
                        open_session_short_gap_ratio: 0.1,
                        known_news_open_event_count: 0,
                        open_session_known_news_activity_share: 0.0,
                    },
                    directional_label_diagnostics: CalibrationHawkesDirectionalLabelDiagnostics {
                        policy_id: "policy".to_string(),
                        first_inferred_trade_sign: "buy".to_string(),
                        provided_label_count: 0,
                        inferred_label_count: 10,
                        flat_move_carry_count: 0,
                        price_up_inference_count: 5,
                        price_down_inference_count: 5,
                    },
                }]),
                return_components_candidate_generation_input: Some(
                    CalibrationReturnComponentsStageCandidateGenerationInput {
                        known_news_calendar: CalibrationReturnComponentsKnownNewsCalendarInput {
                            source_window: CalibrationReturnComponentsSignalSourceWindow {
                                window_start_utc_nanos: 0,
                                window_end_utc_nanos: 10,
                            },
                            scheduled_event_timestamps_utc_nanos: Vec::new(),
                        },
                        realized_known_news_return_samples: Vec::new(),
                        observed_open_session_returns: vec![
                            CalibrationReturnComponentsObservedOpenSessionReturn {
                                return_timestamp_utc_nanos: 1,
                                log_return: 0.01,
                                post_shock_activity_lift: 0.0,
                                sell_share_proxy: 0.5,
                            },
                        ],
                        detected_intraday_jump_candidates: Vec::new(),
                        observed_close_open_boundary_observations: Vec::new(),
                        close_open_gap_detection_policy:
                            CalibrationReturnComponentsCloseOpenGapDetectionPolicy {
                                material_gap_absolute_return_tolerance: 0.01,
                            },
                        open_session_sell_share_proxy: 0.5,
                        shared_mark_law_template:
                            crate::CalibrationSharedNewsMarkLawConfiguration {
                                gamma_severity_marks:
                                    janiform_model_kernels::news::GammaSeverityMarkConfiguration {
                                        shape_k: 1.0,
                                        scale_theta: 1.0,
                                    },
                                double_exponential_base_shock:
                                    janiform_model_kernels::news::DoubleExponentialBaseShockConfiguration {
                                        probability_positive: 0.5,
                                        eta_plus: 1.0,
                                        eta_minus: 1.0,
                                    },
                                coupled_gap_marks:
                                    janiform_model_kernels::news::CoupledGapMarkConfiguration {
                                        sigma_g: 0.5,
                                    },
                            },
                        objective_metric_weights: CalibrationReturnComponentsObjectiveMetricWeights {
                            continuous_distribution_fit_weight: 1.0,
                            surprise_arrival_fit_weight: 1.0,
                            jump_fit_weight: 1.0,
                            gap_fit_weight: 1.0,
                            activity_coupling_fit_weight: 1.0,
                            price_range_sanity_fit_weight: 1.0,
                        },
                    },
                ),
                mean_reversion_open_session_price_observations: None,
                mean_reversion_observed_session_duration_hours: None,
                session_open_tick_count: 10,
                session_closed_tick_count: 0,
                known_news_event_count: 0,
                known_news_events_outside_source_window_count: 0,
                known_news_open_event_count: 0,
                known_news_closed_event_count: 0,
                material_close_open_gap_count: 0,
            },
            tick_data_summary: MarketDataSourceSummary {
                path: "ticks.csv".to_string(),
                file_format: "csv".to_string(),
                provenance_label: "ticks".to_string(),
                source_row_count: 10,
                retained_row_count: 10,
                dropped_duplicate_row_count: 0,
                dropped_invalid_row_count: 0,
                sort_records_by_timestamp: true,
                applied_timestamp_timezone: None,
                canonical_value_field: None,
                value_mapping_policy: None,
            },
            option_surface_summary: None,
            option_surface_records: None,
            known_news_summary: None,
            reference_price: Some(100.0),
            fit_warning: None,
        };

        let resolved = resolve_solver_settings_by_stage(
            &[CalibrationStage::ReturnComponents],
            &[],
            &loading_output,
            Some(2),
        )
        .expect("solver settings should derive");

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].stage, CalibrationStage::ReturnComponents);
        assert_eq!(resolved[0].optimizer_settings.cpu_count, 2);
    }
}
