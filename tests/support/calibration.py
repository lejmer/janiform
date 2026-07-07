from copy import deepcopy
from pathlib import Path
from typing import Any

from janiform.calibration.market_data import (
    CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_POLICY,
    CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_SCHEMA_VERSION,
    MarketDataTimestampBatchingSurfaceDiagnosticState,
)
from janiform.calibration.staged import (
    CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
    CalibrationExecutionMode,
    CalibrationOptimizerSettings,
    CalibrationPreparedExecutionPayloadFileReference,
    CalibrationRunDataInputs,
    CalibrationRunSeedSettings,
    CalibrationStage,
    CalibrationStageSeedOverride,
    CalibrationStageSolverSettings,
    CalibrationStagedExecutionRuntimeResources,
    StagedCalibrationExecutionInput,
    StagedCalibrationRequest,
)
from janiform.mean_reversion import MeanReversionFittedParameters


_PREPARED_EXECUTION_PAYLOAD_PATH = (
    Path(__file__).resolve().parents[1]
    / "fixtures"
    / "calibration"
    / "staged_prepared_execution_payload.json"
)


def build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload() -> dict[str, Any]:
    return build_endogenous_mean_reversion_fitted_parameters_payload(kappa=0.0)


def build_empty_mean_reversion_series_summaries_payload() -> dict[str, dict[str, int]]:
    empty_summary: dict[str, int] = {"sample_count": 0, "finite_count": 0}

    return {
        "core_expected_increments": empty_summary.copy(),
        "market_pressure_state_path": empty_summary.copy(),
        "boundary_anchor_values": empty_summary.copy(),
        "deviations": empty_summary.copy(),
        "residuals": empty_summary.copy(),
    }


def build_neutral_return_clock_exposure_parameters_payload() -> dict[str, Any]:
    return {
        "buy_excitation_return_clock_scale": 1.0,
        "sell_excitation_return_clock_scale": 1.0,
    }


def build_neutral_long_variance_parameters_payload() -> dict[str, Any]:
    return {
        "mean_reversion_rate": 1.0,
        "vol_of_vol": 0.0,
        "stationary_return_energy_rate_per_hour": 0.0,
        "return_energy_loading": 0.0,
        "buy_event_loading": 0.0,
        "sell_event_loading": 0.0,
    }


def build_no_anchor_impact_market_pressure_parameters_payload() -> dict[str, Any]:
    return {
        "side_imbalance_levels": [0.0],
        "transition_rates": {
            "upward_transition_rates": [],
            "downward_transition_rates": [],
        },
        "anchor_drift_per_signed_balance": 0.0,
    }


def build_nominal_price_discretization_surface_payload() -> dict[str, Any]:
    return {
        "tick_size": None,
        "tick_size_source": "unavailable",
        "inference_policy": "unit_test_no_tick_grid_inference.v1",
        "observation_policy": "continuous_price_observation.v1",
    }


def build_disabled_timestamp_batching_surface_payload() -> dict[str, Any]:
    return {
        "schema_version": "model.timestamp_batching_observation.v1",
        "mode": "disabled",
        "bucket_width_nanos": None,
        "same_timestamp_continuation_count": None,
        "same_session_observation_transition_count": None,
        "bucket_width_source": "unavailable",
        "inference_policy": (
            "same_session_duplicate_timestamps_with_positive_timestamp_quantum_gcd.v1"
        ),
        "observation_policy": "latent_event_time_equals_observed_timestamp.v1",
        "coarse_interval_compatibility_policy": (
            "observed_timestamp_ownership_constrained_to_"
            "latent_interval_for_direct_coarse_consistency.v1"
        ),
    }


def build_disabled_timestamp_batching_surface_diagnostic_payload(
    *,
    return_components_stage_selected: bool,
) -> dict[str, Any]:
    diagnostic_state = MarketDataTimestampBatchingSurfaceDiagnosticState

    if return_components_stage_selected:
        state = diagnostic_state.DISABLED_NO_DETECTABLE_BATCHING
        message = (
            "timestamp batching surface evaluated: "
            "disabled because no same-session duplicate timestamp quantum was detected"
        )
    else:
        state = diagnostic_state.NOT_EVALUATED_RETURN_COMPONENTS_NOT_SELECTED
        message = (
            "timestamp batching surface not evaluated because return components are not "
            "selected"
        )

    return {
        "schema_version": CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_SCHEMA_VERSION,
        "diagnostic_policy": CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_POLICY,
        "state": state.value,
        "return_components_stage_selected": return_components_stage_selected,
        "timestamp_batching_surface": build_disabled_timestamp_batching_surface_payload(),
        "message": message,
    }


def build_flat_business_time_clock_model_payload() -> dict[str, Any]:
    axis_names = ("intraday", "weekly", "annual")

    return {
        "schema_version": "business_time_clock.fourier_model.v1",
        "coordinate_system": "exchange_local_minute_of_day_weekday_plus_civil_year_phase.v1",
        "coefficient_basis": "log_rate_intercept_then_axis_keyed_cos_sin_harmonic_series.v1",
        "normalization_policy": "fit_open_exposure_weighted_mean_rate_equals_one.v1",
        "selection_policy": "aicc_over_open_minute_poisson_exposure_likelihood.v1",
        "intercept": 0.0,
        "harmonic_series": {
            axis_name: build_zero_order_harmonic_series_payload()
            for axis_name in axis_names
        },
        "selected_objective_value": 0.0,
        "selected_negative_log_likelihood": 0.0,
        "evaluated_candidate_count": 1,
        "fit_open_minute_count": 1,
        "fit_open_event_count": 1,
        "fit_open_exposure_seconds": 60.0,
    }


def build_zero_order_harmonic_series_payload() -> dict[str, Any]:
    return {
        "harmonic_order": 0,
        "cosine_coefficients": [],
        "sine_coefficients": [],
    }


def build_structural_seasonality_parameters_payload(
    *,
    exchange_code: str = "XNYS",
) -> dict[str, Any]:
    return {
        "schema_version": "business_time_clock.fourier_model.v1",
        "exchange_code": exchange_code,
        "clock_model_schema_version": "business_time_clock.fourier_model.v1",
        "clock_model": build_flat_business_time_clock_model_payload(),
    }


def update_model_mean_reversion_payload(model: Any, mean_reversion_payload: dict[str, Any]) -> None:
    _update_model_parameters_mean_reversion_payload(
        model.parameters,
        mean_reversion_payload,
    )

    model_payload = model._model_payload
    if isinstance(model_payload, dict):
        raw_parameters = model_payload.get("parameters")
        if isinstance(raw_parameters, dict):
            _update_model_parameters_mean_reversion_payload(
                raw_parameters,
                mean_reversion_payload,
            )


def _update_model_parameters_mean_reversion_payload(
    parameters: dict[str, Any],
    mean_reversion_payload: dict[str, Any],
) -> None:
    finalization_parameters = parameters.get("finalization")
    if not isinstance(finalization_parameters, dict):
        raise AssertionError("model parameters must contain finalization parameters")

    fitted_parameters = finalization_parameters.get("fitted_parameters")
    if not isinstance(fitted_parameters, dict):
        raise AssertionError("finalization model parameters must contain fitted_parameters")

    return_components_fitted_parameters = fitted_parameters.get(
        "return_components_fitted_parameters"
    )
    if not isinstance(return_components_fitted_parameters, dict):
        raise AssertionError(
            "finalization fitted_parameters must contain return_components_fitted_parameters"
        )

    return_components_fitted_parameters["mean_reversion_fitted_parameters"] = (
        mean_reversion_payload
    )


def stage_report_by_stage(
    raw_stage_reports: list[dict[str, Any]],
    stage: CalibrationStage,
) -> dict[str, Any]:
    for raw_stage_report in raw_stage_reports:
        if raw_stage_report.get("stage") == stage.value:
            return raw_stage_report

    raise AssertionError(f"stage reports must contain {stage.value}")


def append_finalization_stage_report_from_joint_refinement(raw_response: dict[str, Any]) -> None:
    raw_stage_reports = raw_response.get("stage_reports")
    if not isinstance(raw_stage_reports, list):
        raise AssertionError("staged response must contain stage_reports")

    joint_refinement_report = stage_report_by_stage(
        raw_stage_reports,
        CalibrationStage.JOINT_REFINEMENT,
    )

    finalization_report = deepcopy(joint_refinement_report)
    finalization_report["stage_report_kind"] = CalibrationStage.FINALIZATION.value
    finalization_report["stage"] = CalibrationStage.FINALIZATION.value

    raw_stage_reports.append(finalization_report)


def build_endogenous_mean_reversion_fitted_parameters_payload(
    *,
    kappa: float,
) -> dict[str, Any]:
    return MeanReversionFittedParameters(kappa=kappa).to_payload()


def build_valid_execution_input_model(
    *,
    execution_mode: CalibrationExecutionMode = CalibrationExecutionMode.MEMORY,
) -> StagedCalibrationExecutionInput:
    return StagedCalibrationExecutionInput(
        schema_version=CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
        request=StagedCalibrationRequest(
            run_id="run-001",
            data_inputs=CalibrationRunDataInputs(
                exchange_code="XNYS",
                trade_events_data_reference="dataset://synthetic/trades/v1",
                option_surface_data_reference="dataset://synthetic/options/v1",
            ),
            execution_mode=execution_mode,
            selected_stages=(
                CalibrationStage.SEASONALITY,
                CalibrationStage.HAWKES,
                CalibrationStage.RETURN_COMPONENTS,
                CalibrationStage.LONG_VARIANCE,
                CalibrationStage.NEWS_SHOCKS,
                CalibrationStage.RETURN_BACKFIT,
                CalibrationStage.MEAN_REVERSION,
            ),
            seed_settings=CalibrationRunSeedSettings(
                global_seed=17,
                stage_seed_overrides=(
                    CalibrationStageSeedOverride(
                        stage=CalibrationStage.SEASONALITY,
                        seed=101,
                    ),
                ),
            ),
            optimizer_settings_by_stage=(
                _solver_settings(CalibrationStage.SEASONALITY),
                _solver_settings(CalibrationStage.HAWKES),
                _solver_settings(CalibrationStage.RETURN_COMPONENTS),
                _solver_settings(CalibrationStage.LONG_VARIANCE),
                _solver_settings(CalibrationStage.NEWS_SHOCKS),
                _solver_settings(CalibrationStage.RETURN_BACKFIT),
                _solver_settings(CalibrationStage.MEAN_REVERSION),
            ),
        ),
        prepared_payload_file=CalibrationPreparedExecutionPayloadFileReference(
            path=str(_PREPARED_EXECUTION_PAYLOAD_PATH),
        ),
        runtime_resources=(
            CalibrationStagedExecutionRuntimeResources(
                disk_checkpoint_directory="/tmp/janiform-calibration",
            )
            if execution_mode == CalibrationExecutionMode.DISK
            else None
        ),
    )


def build_valid_execution_input_payload() -> dict[str, Any]:
    return build_valid_execution_input_model().to_payload()


def _solver_settings(stage: CalibrationStage) -> CalibrationStageSolverSettings:
    return CalibrationStageSolverSettings(
        stage=stage,
        optimizer_settings=CalibrationOptimizerSettings(
            solver_name="lbfgs",
            max_iterations=250,
            cpu_count=1,
            absolute_tolerance=1e-8,
            relative_tolerance=1e-6,
            constraint_penalty_weight=10.0,
        ),
    )
