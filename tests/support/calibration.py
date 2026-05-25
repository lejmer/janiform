from copy import deepcopy
from pathlib import Path
from typing import Any

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
from janiform.mean_reversion import (
    MeanReversionAnchorModel,
    MeanReversionFittedParameters,
)


_PREPARED_EXECUTION_PAYLOAD_PATH = (
    Path(__file__).resolve().parents[1]
    / "fixtures"
    / "calibration"
    / "staged_prepared_execution_payload.json"
)


def build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload() -> dict[str, Any]:
    return build_endogenous_mean_reversion_fitted_parameters_payload(kappa=0.0)


def build_neutral_return_clock_exposure_parameters_payload() -> dict[str, Any]:
    return {
        "buy_excitation_return_clock_scale": 1.0,
        "sell_excitation_return_clock_scale": 1.0,
    }


def build_nominal_tick_grid_observation_model_payload() -> dict[str, Any]:
    return {
        "tick_size": None,
        "tick_size_source": "unavailable",
        "inference_policy": "unit_test_no_tick_grid_inference.v1",
        "observation_policy": "continuous_price_observation.v1",
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
    return MeanReversionFittedParameters(
        kappa=kappa,
        anchor_model=MeanReversionAnchorModel.zero_gain_endogenous_core_pressure_level(),
    ).to_payload()


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
