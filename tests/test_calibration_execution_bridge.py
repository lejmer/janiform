import json

import pytest

from janiform._native_api import native_module
from janiform.calibration import (
    CalibrationExecutionMode,
    CalibrationStage,
    CalibrationStageExecutionStatus,
    CalibrationValidationError,
    deserialize_staged_calibration_response,
    serialize_staged_calibration_execution_input,
    validate_staged_calibration_execution_input,
)
from tests.calibration_test_support import build_valid_execution_input_model


def test_native_bridge_executes_staged_calibration_in_memory_mode(calendar_registry) -> None:
    native = native_module()
    execution_input = build_valid_execution_input_model()

    response_json = native.run_staged_calibration(
        serialize_staged_calibration_execution_input(execution_input),
        calendar_registry._native_registry,
    )
    response = deserialize_staged_calibration_response(response_json)

    assert response.execution_mode == CalibrationExecutionMode.MEMORY
    assert response.status == CalibrationStageExecutionStatus.SUCCESS
    assert [report.stage for report in response.stage_reports] == [
        CalibrationStage.SEASONALITY,
        CalibrationStage.HAWKES,
        CalibrationStage.RETURN_COMPONENTS,
    ]
    assert len(response.emitted_checkpoints) == 3


def test_native_bridge_executes_staged_calibration_in_disk_mode(calendar_registry, tmp_path) -> None:
    native = native_module()
    execution_input = build_valid_execution_input_model(execution_mode=CalibrationExecutionMode.DISK)
    execution_input_payload = execution_input.to_payload()
    execution_input_payload["runtime_resources"]["disk_checkpoint_directory"] = str(tmp_path)
    validated_execution_input = validate_staged_calibration_execution_input(execution_input_payload)

    response_json = native.run_staged_calibration(
        serialize_staged_calibration_execution_input(validated_execution_input),
        calendar_registry._native_registry,
    )
    response = deserialize_staged_calibration_response(response_json)

    assert response.execution_mode == CalibrationExecutionMode.DISK
    assert response.status == CalibrationStageExecutionStatus.SUCCESS
    assert len(response.emitted_checkpoints) == 3
    assert any(tmp_path.rglob("*.json"))


def test_native_bridge_rejects_provenance_only_execution_request(calendar_registry) -> None:
    native = native_module()
    request = build_valid_execution_input_model().request
    provenance_only_payload = {
        "schema_version": "calibration.orchestration.staged_execution_input.v1",
        "request": request.to_payload(),
    }

    with pytest.raises(native.CalibrationExecutionError) as captured_error:
        native.run_staged_calibration(
            json.dumps(provenance_only_payload),
            calendar_registry._native_registry,
        )

    assert "requires explicit stage input" in str(captured_error.value)


def test_validation_rejects_disk_runtime_resources_in_memory_mode() -> None:
    execution_input_payload = build_valid_execution_input_model().to_payload()
    execution_input_payload["runtime_resources"] = {
        "disk_checkpoint_directory": "/tmp/should-not-be-used",
    }

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_staged_calibration_execution_input(execution_input_payload)

    assert captured_error.value.failure.field_path == "runtime_resources.disk_checkpoint_directory"
    assert "only supported when execution_mode=disk" in captured_error.value.failure.message
