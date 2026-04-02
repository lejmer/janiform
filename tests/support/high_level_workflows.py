from pathlib import Path
from typing import Any

import pytest

import janiform.calibration.fit.estimator as fit_estimator_module
from janiform.calibration import StagedCalibrationResponse
from janiform.calibration.fit import JaniformEstimator, JaniformModel
from janiform.calibration.market_data import (
    CalibrationInputFileFormat,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
)
from janiform.calibration.staged import validate_staged_calibration_response
from tests.support.calibration import (
    build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload,
)


def build_full_workflow_model(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    run_id: str,
) -> JaniformModel:
    tick_data_path = _write_tick_data(
        tmp_path / f"{run_id}.csv",
        rows=(
            "2026-02-02T14:30:00Z,100.0,buy",
            "2026-02-02T15:00:00Z,101.0,sell",
            "2026-02-02T20:59:00Z,102.0,buy",
            "2026-02-03T14:30:00Z,99.0,sell",
            "2026-02-03T20:59:00Z,98.0,buy",
            "2026-02-04T14:30:00Z,97.0,sell",
        ),
    )

    monkeypatch.setattr(
        fit_estimator_module,
        "_run_prepared_staged_calibration_with_raw_payload",
        lambda *args, **kwargs: successful_full_model_staged_runner_payload(run_id),
    )

    estimator = JaniformEstimator(
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        tick_data_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        ),
        market_profile="us_equity_cash",
    )

    return estimator.fit(
        run_id=run_id,
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
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
                "normalized_stage_payload": {
                    "composition_schema_version": "seasonality.deterministic.composition.v1",
                    "exchange_code": "XNYS",
                    "timestamp_utc_nanos": [
                        1770042600000000000,
                        1770046200000000000,
                        1770129000000000000,
                    ],
                    "normalized_combined_factor_by_event": [0.95, 1.05, 1.0],
                    "intraday_factor_by_bin": [1.0] * 3,
                    "day_of_week_factor": [1.0] * 7,
                    "day_of_year_factor": [1.0] * 366,
                },
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
                                    "baseline_intensity": 0.0,
                                },
                            },
                            "shared_mark_law": {
                                "gamma_severity_marks": {
                                    "shape_k": 1.5,
                                    "scale_theta": 0.9,
                                },
                                "double_exponential_base_shock": {
                                    "probability_positive": 0.4,
                                    "eta_plus": 1.3,
                                    "eta_minus": 1.1,
                                },
                                "coupled_gap_marks": {
                                    "sigma_g": 0.75,
                                },
                            },
                            "known_component_mark_scales": {
                                "activity_coupling_scale": 0.6,
                                "return_jump_scale": 1.2,
                            },
                            "surprise_component_mark_scales": {
                                "activity_coupling_scale": 0.6,
                                "return_jump_scale": 1.2,
                            },
                        },
                        "surprise_deterministic_intensity_profile": {
                            "weekly_hour_utc_multipliers": [1.0] * 168,
                            "profile_resolution_hours": 1,
                            "fit_policy": (
                                "selected_exogenous_surprise_episode_rate_per_utc_hour_of_week_"
                                "with_baseline_shrinkage.v1"
                            ),
                            "normalization_policy": (
                                "exposure_weighted_mean_one_over_source_window.v1"
                            ),
                        },
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
                        "chosen_anchor_family": "endogenous_core_pressure_level",
                        "selected_candidate_family": "random_walk_null",
                        "calibration_grid": {
                            "calibration_grid_kind": "synthetic_daily_close_grid.v1",
                            "calibration_grid_interval_hours": 24.0,
                            "source_observation_count": 4,
                            "retained_observation_count": 0,
                            "coverage_ratio": 0.0,
                        },
                        "anchor_parameter_estimates": {
                            "anchor_model_kind": "endogenous_core_pressure_level",
                            "reference_interval_hours": 24.0,
                            "pressure_persistence_per_reference_interval": 0.0,
                            "pressure_level_gain": 0.0,
                        },
                        "anchor_origin_level": None,
                        "phi_reference": 1.0,
                        "kappa": 0.0,
                        "residual_sum_of_squares": 0.0,
                        "quasi_log_likelihood": None,
                        "information_criterion_bic": None,
                        "model_selection_policy": "gaussian_quasi_likelihood_bic_with_endogenous_core_pressure_level_candidate_family_profile.v1",
                        "deviation_sample_count": 0,
                        "regression_pair_count": 0,
                        "least_squares_rank": 0,
                        "evaluated_candidates": [
                            {
                                "candidate_family": "random_walk_null",
                                "anchor_model": {
                                    "anchor_model_kind": "endogenous_core_pressure_level",
                                    "reference_interval_hours": 24.0,
                                    "pressure_persistence_per_reference_interval": 0.0,
                                    "pressure_level_gain": 0.0,
                                },
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
                                "core_expected_increments": [],
                                "pressure_state_path": [],
                                "boundary_anchor_values": [],
                                "deviations": [],
                                "residuals": [],
                                "stage_decision_reason": "endogenous_anchor_runtime_projection_required.v1",
                            }
                        ],
                        "stage_decision_reason": "endogenous_anchor_runtime_projection_required.v1",
                    },
                    "return_components_input_transport": "in_memory_output",
                    "return_components_selected_candidate_id": "return_candidate_balanced",
                },
            },
        ],
        "emitted_checkpoints": [],
    }

    return validate_staged_calibration_response(raw_response), raw_response


def _write_tick_data(path: Path, *, rows: tuple[str, ...]) -> Path:
    path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                *rows,
            ]
        ),
        encoding="utf-8",
    )
    return path
