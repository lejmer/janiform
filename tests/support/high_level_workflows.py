from typing import Any

from janiform.calibration.staged import (
    StagedCalibrationResponse,
    validate_staged_calibration_response,
)
from tests.support.calibration import (
    append_finalization_stage_report_from_joint_refinement,
    build_disabled_timestamp_batching_surface_payload,
    build_empty_mean_reversion_series_summaries_payload,
    build_no_anchor_impact_market_pressure_parameters_payload,
    build_nominal_price_discretization_surface_payload,
    build_neutral_long_variance_parameters_payload,
    build_neutral_return_clock_exposure_parameters_payload,
    build_structural_seasonality_parameters_payload,
    build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload,
)


def successful_full_model_staged_runner_payload(
    run_id: str,
) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
    raw_response = {
        "schema_version": "calibration.orchestration.staged_run.report.v1",
        "failure_triage_policy": "mode_aware_failure_kind_and_checkpoint_context.v1",
        "run_id": run_id,
        "execution_mode": "memory",
        "status": "success",
        "stage_reports": [
            {
                "stage_report_kind": "seasonality",
                "stage": "seasonality",
                "status": "success",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "normalized_stage_payload": build_structural_seasonality_parameters_payload(),
            },
            {
                "stage_report_kind": "hawkes",
                "stage": "hawkes",
                "status": "success",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "fitted_stage_payload": {
                    "schema_version": "calibration.stage_output.hawkes.v1",
                    "selected_candidate_id": "candidate_stable_low_loss",
                    "fitted_parameters": {
                        "kernel_parameters": {
                            "lambda_bar": 0.8,
                            "beta": 1.6,
                            "alpha": {
                                "plus_plus": 0.1,
                                "plus_minus": 0.05,
                                "minus_plus": 0.05,
                                "minus_minus": 0.125,
                            },
                            "rho": {
                                "plus_plus": 0.08,
                                "plus_minus": 0.04,
                                "minus_plus": 0.05,
                                "minus_minus": 0.1,
                            },
                        }
                    },
                },
            },
            {
                "stage_report_kind": "return_components",
                "stage": "return_components",
                "status": "success",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "fitted_stage_payload": {
                    "schema_version": "calibration.stage_output.return_components.v1",
                    "selected_candidate_id": "return_candidate_balanced",
                    "fitted_parameters": {
                        "nig_shared_plus_shift_configuration": {
                            "alpha": 2.3,
                            "beta_0": -0.8,
                            "delta": 0.7,
                            "mu": 0.1,
                            "delta_beta": -0.2,
                        },
                        "news_process_configuration": {
                            "schema_version": "news.process.configuration.v1",
                            "arrivals": {
                                "known": {
                                    "scheduled_event_timestamps_utc_nanos": [],
                                },
                                "surprise": {
                                    "open_baseline_intensity": 0.0,
                                    "closed_baseline_intensity": 0.0,
                                },
                            },
                            "shared_mark_law": {
                                "gamma_severity_marks": {
                                    "shape_k": 1.5,
                                },
                            },
                            "known_component_mark_scales": {
                                "activity_coupling_scale": 0.0,
                                "return_jump_drift": 0.0,
                                "return_jump_volatility": 0.0,
                            },
                            "surprise_component_mark_scales": {
                                "activity_coupling_scale": 0.0,
                                "return_jump_drift": 0.0,
                                "return_jump_volatility": 0.0,
                            },
                        },
                        "surprise_deterministic_intensity_profile": {
                            "weekly_utc_profile_multipliers": [1.0] * 168,
                            "profile_resolution_hours": 1,
                            "fit_policy": (
                                "selected_exogenous_surprise_episode_rate_per_utc_week_profile_bucket_"
                                "with_baseline_shrinkage.v1"
                            ),
                            "normalization_policy": (
                                "exposure_weighted_mean_one_over_source_window.v1"
                            ),
                        },
                        "return_clock_exposure_parameters": (
                            build_neutral_return_clock_exposure_parameters_payload()
                        ),
                        "long_variance_parameters": (
                            build_neutral_long_variance_parameters_payload()
                        ),
                        "market_pressure_parameters": (
                            build_no_anchor_impact_market_pressure_parameters_payload()
                        ),
                        "price_discretization_surface": (
                            build_nominal_price_discretization_surface_payload()
                        ),
                        "timestamp_batching_surface": (
                            build_disabled_timestamp_batching_surface_payload()
                        ),
                        "mean_reversion_fitted_parameters": (
                            build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload()
                        ),
                    },
                },
            },
            {
                "stage_report_kind": "mean_reversion",
                "stage": "mean_reversion",
                "status": "success",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "fitted_stage_payload": {
                    "schema_version": "calibration.stage_output.mean_reversion.v1",
                    "fitted_parameters": (
                        build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload()
                    ),
                    "diagnostics": {
                        "selected_candidate_family": "random_walk_null",
                        "calibration_grid": {
                            "calibration_grid_kind": "synthetic_daily_close_grid.v1",
                            "calibration_grid_interval_hours": 24.0,
                            "source_observation_count": 4,
                            "retained_observation_count": 0,
                            "coverage_ratio": 0.0,
                        },
                        "anchor_origin_level": None,
                        "phi_reference": 1.0,
                        "kappa": 0.0,
                        "residual_sum_of_squares": 0.0,
                        "quasi_log_likelihood": None,
                        "information_criterion_bic": None,
                        "model_selection_policy": "gaussian_quasi_likelihood_bic_mean_reversion_model_selection.v1",
                        "deviation_sample_count": 0,
                        "regression_pair_count": 0,
                        "least_squares_rank": 0,
                        "evaluated_candidates": [
                            {
                                "candidate_family": "random_walk_null",
                                "anchor_origin_level": None,
                                "phi_reference": 1.0,
                                "kappa": 0.0,
                                "residual_sum_of_squares": 0.0,
                                "quasi_log_likelihood": None,
                                "information_criterion_bic": None,
                                "parameter_count": 0,
                                "least_squares_rank": 0,
                                "deviation_sample_count": 0,
                                "regression_pair_count": 0,
                                "series_summaries": (
                                    build_empty_mean_reversion_series_summaries_payload()
                                ),
                                "stage_decision_reason": "endogenous_anchor_runtime_projection_required.v1",
                            }
                        ],
                        "evaluated_grids": [
                            {
                                "calibration_grid": {
                                    "calibration_grid_kind": "synthetic_daily_close_grid.v1",
                                    "calibration_grid_interval_hours": 24.0,
                                    "source_observation_count": 4,
                                    "retained_observation_count": 0,
                                    "coverage_ratio": 0.0,
                                },
                                "evaluated_candidates": [
                                    {
                                        "candidate_family": "random_walk_null",
                                        "anchor_origin_level": None,
                                        "phi_reference": 1.0,
                                        "kappa": 0.0,
                                        "residual_sum_of_squares": 0.0,
                                        "quasi_log_likelihood": None,
                                        "information_criterion_bic": None,
                                        "parameter_count": 0,
                                        "least_squares_rank": 0,
                                        "deviation_sample_count": 0,
                                        "regression_pair_count": 0,
                                        "series_summaries": (
                                            build_empty_mean_reversion_series_summaries_payload()
                                        ),
                                        "stage_decision_reason": "endogenous_anchor_runtime_projection_required.v1",
                                    }
                                ],
                                "stage_decision_reason": "endogenous_anchor_runtime_projection_required.v1",
                            }
                        ],
                        "stage_decision_reason": "endogenous_anchor_runtime_projection_required.v1",
                    },
                    "return_components_input_transport": "in_memory_output",
                    "return_components_selected_candidate_id": "return_candidate_balanced",
                },
            },
            {
                "stage_report_kind": "joint_refinement",
                "stage": "joint_refinement",
                "status": "success",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "fitted_stage_payload": {
                    "schema_version": "calibration.stage_output.joint_refinement.v1",
                    "selected_candidate_id": "joint_refinement_baseline",
                    "fitted_parameters": {
                        "hawkes_fitted_parameters": {
                            "kernel_parameters": {
                                "lambda_bar": 0.8,
                                "beta": 1.6,
                                "alpha": {
                                    "plus_plus": 0.2,
                                    "plus_minus": 0.1,
                                    "minus_plus": 0.1,
                                    "minus_minus": 0.25,
                                },
                                "rho": {
                                    "plus_plus": 0.08,
                                    "plus_minus": 0.04,
                                    "minus_plus": 0.05,
                                    "minus_minus": 0.1,
                                },
                            },
                            "structured_alpha": {
                                "a_self": 0.35,
                                "a_cross": 0.06,
                                "delta_self": 0.07,
                                "delta_cross": 0.04,
                            },
                            "structured_rho": {
                                "r_self": 0.3,
                                "r_cross": 0.08,
                                "delta_self_r": 0.04,
                                "delta_cross_r": 0.05,
                            },
                            "self_dominance_policy": {
                                "minimum_self_to_cross_ratio": 1.0,
                            },
                            "stability_numeric_tolerance": 0.0,
                        },
                        "return_components_fitted_parameters": {
                            "nig_shared_plus_shift_configuration": {
                                "alpha": 2.3,
                                "beta_0": -0.8,
                                "delta": 0.7,
                                "mu": 0.1,
                                "delta_beta": -0.2,
                            },
                            "news_process_configuration": {
                                "schema_version": "news.process.configuration.v1",
                                "arrivals": {
                                    "known": {
                                        "scheduled_event_timestamps_utc_nanos": [],
                                    },
                                    "surprise": {
                                        "open_baseline_intensity": 0.02,
                                    "closed_baseline_intensity": 0.02,
                                    },
                                },
                                "shared_mark_law": {
                                    "gamma_severity_marks": {
                                        "shape_k": 1.4,
                                    },
                                },
                                "known_component_mark_scales": {
                                    "activity_coupling_scale": 0.6,
                                    "return_jump_drift": 0.0,
                                    "return_jump_volatility": 1.0,
                                },
                                "surprise_component_mark_scales": {
                                    "activity_coupling_scale": 0.6,
                                    "return_jump_drift": 0.0,
                                    "return_jump_volatility": 1.0,
                                },
                            },
                            "surprise_deterministic_intensity_profile": {
                                "weekly_utc_profile_multipliers": [
                                    2.0 if hour_index == 14 else 1.0 for hour_index in range(168)
                                ],
                                "profile_resolution_hours": 1,
                                "fit_policy": (
                                    "selected_exogenous_surprise_episode_rate_per_utc_week_profile_bucket_"
                                    "with_baseline_shrinkage.v1"
                                ),
                                "normalization_policy": (
                                    "exposure_weighted_mean_one_over_source_window.v1"
                                ),
                            },
                            "return_clock_exposure_parameters": (
                                build_neutral_return_clock_exposure_parameters_payload()
                            ),
                            "long_variance_parameters": (
                                build_neutral_long_variance_parameters_payload()
                            ),
                            "market_pressure_parameters": (
                                build_no_anchor_impact_market_pressure_parameters_payload()
                            ),
                            "price_discretization_surface": (
                                build_nominal_price_discretization_surface_payload()
                            ),
                            "timestamp_batching_surface": (
                                build_disabled_timestamp_batching_surface_payload()
                            ),
                            "mean_reversion_fitted_parameters": (
                                build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload()
                            ),
                        },
                    },
                    "baseline_hawkes_fitted_parameters": {
                        "kernel_parameters": {
                            "lambda_bar": 0.8,
                            "beta": 1.6,
                            "alpha": {
                                "plus_plus": 0.2,
                                "plus_minus": 0.1,
                                "minus_plus": 0.1,
                                "minus_minus": 0.25,
                            },
                            "rho": {
                                "plus_plus": 0.08,
                                "plus_minus": 0.04,
                                "minus_plus": 0.05,
                                "minus_minus": 0.1,
                            },
                        },
                        "structured_alpha": {
                            "a_self": 0.35,
                            "a_cross": 0.06,
                            "delta_self": 0.07,
                            "delta_cross": 0.04,
                        },
                        "structured_rho": {
                            "r_self": 0.3,
                            "r_cross": 0.08,
                            "delta_self_r": 0.04,
                            "delta_cross_r": 0.05,
                        },
                        "self_dominance_policy": {
                            "minimum_self_to_cross_ratio": 1.0,
                        },
                        "stability_numeric_tolerance": 0.0,
                    },
                    "baseline_return_components_fitted_parameters": {
                        "nig_shared_plus_shift_configuration": {
                            "alpha": 2.3,
                            "beta_0": -0.8,
                            "delta": 0.7,
                            "mu": 0.1,
                            "delta_beta": -0.2,
                        },
                        "news_process_configuration": {
                            "schema_version": "news.process.configuration.v1",
                            "arrivals": {
                                "known": {
                                    "scheduled_event_timestamps_utc_nanos": [],
                                },
                                "surprise": {
                                    "open_baseline_intensity": 0.02,
                                    "closed_baseline_intensity": 0.02,
                                },
                            },
                            "shared_mark_law": {
                                "gamma_severity_marks": {
                                    "shape_k": 1.4,
                                },
                            },
                            "known_component_mark_scales": {
                                "activity_coupling_scale": 0.6,
                                "return_jump_drift": 0.0,
                                "return_jump_volatility": 1.0,
                            },
                            "surprise_component_mark_scales": {
                                "activity_coupling_scale": 0.6,
                                "return_jump_drift": 0.0,
                                "return_jump_volatility": 1.0,
                            },
                        },
                        "surprise_deterministic_intensity_profile": {
                            "weekly_utc_profile_multipliers": [
                                2.0 if hour_index == 14 else 1.0 for hour_index in range(168)
                            ],
                            "profile_resolution_hours": 1,
                            "fit_policy": (
                                "selected_exogenous_surprise_episode_rate_per_utc_week_profile_bucket_"
                                "with_baseline_shrinkage.v1"
                            ),
                            "normalization_policy": (
                                "exposure_weighted_mean_one_over_source_window.v1"
                            ),
                        },
                        "return_clock_exposure_parameters": (
                            build_neutral_return_clock_exposure_parameters_payload()
                        ),
                        "long_variance_parameters": (
                            build_neutral_long_variance_parameters_payload()
                        ),
                        "market_pressure_parameters": (
                            build_no_anchor_impact_market_pressure_parameters_payload()
                        ),
                        "price_discretization_surface": (
                            build_nominal_price_discretization_surface_payload()
                        ),
                        "timestamp_batching_surface": (
                            build_disabled_timestamp_batching_surface_payload()
                        ),
                        "mean_reversion_fitted_parameters": (
                            build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload()
                        ),
                    },
                    "objective_breakdown": {
                        "aggregated_likelihood_fit": 1.0,
                        "aggregated_return_shape_fit": 1.0,
                        "aggregated_return_scale_fit": 1.0,
                        "business_time_return_scale_fit": 1.0,
                        "intraday_return_level_fit": 1.0,
                        "activity_shape_fit": 1.0,
                        "activity_level_fit": 1.0,
                        "rv_activity_coupling_fit": 1.0,
                        "price_span_sanity_fit": 1.0,
                        "weighted_objective": 1.0,
                        "hard_constraint_penalty": 0.0,
                    },
                    "optimizer_diagnostics": {
                        "local_objective": 1.0,
                        "final_objective": 1.0,
                        "local_evaluation_count": 0,
                        "final_simulation_seeds": [17],
                        "stop_reason": "baseline_seed",
                    },
                },
            },
        ],
        "emitted_checkpoints": [],
    }

    append_finalization_stage_report_from_joint_refinement(raw_response)

    return validate_staged_calibration_response(raw_response), raw_response
