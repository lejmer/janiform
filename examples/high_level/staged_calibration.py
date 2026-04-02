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
    StagedCalibrationExecutionInput,
    StagedCalibrationRequest,
    StagedCalibrationResponse,
    run_staged_calibration,
)
from janiform.calendar import CalendarRegistry

from examples._shared import (
    STAGED_PREPARED_EXECUTION_PAYLOAD_PATH,
    calendar_registry_json,
)


def _solver_settings(stage: CalibrationStage) -> CalibrationStageSolverSettings:
    return CalibrationStageSolverSettings(
        stage=stage,
        optimizer_settings=CalibrationOptimizerSettings(
            solver_name="lbfgs",
            max_iterations=250,
            cpu_count=1,
            absolute_tolerance=1.0e-8,
            relative_tolerance=1.0e-6,
            constraint_penalty_weight=10.0,
        ),
    )


def main() -> None:
    execution_input: StagedCalibrationExecutionInput = StagedCalibrationExecutionInput(
        schema_version=CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
        request=StagedCalibrationRequest(
            run_id="example-staged-calibration",
            data_inputs=CalibrationRunDataInputs(
                exchange_code="XNYS",
                trade_events_data_reference="dataset://examples/synthetic/trades/v1",
                option_surface_data_reference="dataset://examples/synthetic/options/v1",
            ),
            execution_mode=CalibrationExecutionMode.MEMORY,
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
            optimizer_settings_by_stage=tuple(
                _solver_settings(stage)
                for stage in (
                    CalibrationStage.SEASONALITY,
                    CalibrationStage.HAWKES,
                    CalibrationStage.RETURN_COMPONENTS,
                    CalibrationStage.MEAN_REVERSION,
                )
            ),
        ),
        prepared_payload_file=CalibrationPreparedExecutionPayloadFileReference(
            path=str(STAGED_PREPARED_EXECUTION_PAYLOAD_PATH),
        ),
    )
    calendar_registry: CalendarRegistry = CalendarRegistry(calendar_registry_json())
    response: StagedCalibrationResponse = run_staged_calibration(
        execution_input,
        calendar_registry=calendar_registry,
    )

    print(f"run status: {response.status.value}")
    print(
        "stage statuses:",
        {
            report.stage.value: report.status.value
            for report in response.stage_reports
        },
    )


if __name__ == "__main__":
    main()
