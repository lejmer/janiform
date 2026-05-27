import pytest

from janiform.calibration import CalibrationValidationError
from janiform.calibration.staged import (
    CalibrationExecutionMode,
    CalibrationStage,
    CalibrationStageExecutionStatus,
    run_staged_calibration,
    validate_staged_calibration_execution_input,
)
from tests.support.calibration import build_valid_execution_input_model


def test_run_staged_calibration_executes_in_memory_mode(calendar_registry) -> None:
    execution_input = build_valid_execution_input_model()

    response = run_staged_calibration(
        execution_input,
        calendar_registry=calendar_registry,
    )

    assert response.execution_mode == CalibrationExecutionMode.MEMORY
    assert response.status == CalibrationStageExecutionStatus.SUCCESS
    assert [report.stage for report in response.stage_reports] == [
        CalibrationStage.SEASONALITY,
        CalibrationStage.HAWKES,
        CalibrationStage.RETURN_COMPONENTS,
        CalibrationStage.LONG_VARIANCE,
        CalibrationStage.MEAN_REVERSION,
    ]
    assert len(response.emitted_checkpoints) == 5


def test_run_staged_calibration_executes_in_disk_mode(calendar_registry, tmp_path) -> None:
    execution_input = build_valid_execution_input_model(execution_mode=CalibrationExecutionMode.DISK)
    execution_input_payload = execution_input.to_payload()
    execution_input_payload["runtime_resources"]["disk_checkpoint_directory"] = str(tmp_path)
    validated_execution_input = validate_staged_calibration_execution_input(execution_input_payload)

    response = run_staged_calibration(
        validated_execution_input,
        calendar_registry=calendar_registry,
    )

    assert response.execution_mode == CalibrationExecutionMode.DISK
    assert response.status == CalibrationStageExecutionStatus.SUCCESS
    assert len(response.emitted_checkpoints) == 5
    assert any(tmp_path.rglob("*.json"))


def test_validation_rejects_disk_runtime_resources_in_memory_mode() -> None:
    execution_input_payload = build_valid_execution_input_model().to_payload()
    execution_input_payload["runtime_resources"] = {
        "disk_checkpoint_directory": "/tmp/should-not-be-used",
    }

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_staged_calibration_execution_input(execution_input_payload)

    assert captured_error.value.failure.field_path == "runtime_resources.disk_checkpoint_directory"
    assert "only supported when execution_mode=disk" in captured_error.value.failure.message
