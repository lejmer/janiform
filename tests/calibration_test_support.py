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
from janiform.contracts import (
    MeanReversionAnchorModel,
    MeanReversionFittedParameters,
)


_PREPARED_EXECUTION_PAYLOAD_PATH = (
    Path(__file__).resolve().parent / "fixtures" / "calibration" / "staged_prepared_execution_payload.json"
)


def build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload() -> dict[str, Any]:
    return build_endogenous_mean_reversion_fitted_parameters_payload(kappa=0.0)


def update_model_mean_reversion_payload(model: Any, mean_reversion_payload: dict[str, Any]) -> None:
    model.parameters["return_components"]["fitted_parameters"][
        "mean_reversion_fitted_parameters"
    ] = mean_reversion_payload
    model.parameters["mean_reversion"]["fitted_parameters"] = mean_reversion_payload

    joint_refinement_parameters = model.parameters.get("joint_refinement")

    if isinstance(joint_refinement_parameters, dict):
        fitted_parameters = joint_refinement_parameters.get("fitted_parameters")
        if isinstance(fitted_parameters, dict):
            return_components_fitted_parameters = fitted_parameters.get(
                "return_components_fitted_parameters"
            )
            if isinstance(return_components_fitted_parameters, dict):
                return_components_fitted_parameters["mean_reversion_fitted_parameters"] = (
                    mean_reversion_payload
                )

        baseline_return_components = joint_refinement_parameters.get(
            "baseline_return_components_fitted_parameters"
        )
        if isinstance(baseline_return_components, dict):
            baseline_return_components["mean_reversion_fitted_parameters"] = mean_reversion_payload


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
