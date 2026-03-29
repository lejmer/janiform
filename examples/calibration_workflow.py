import json
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any

from janiform.calibration.staged import CalibrationExecutionMode, run_staged_calibration

from .support._shared import build_calendar_registry, build_calibration_execution_input


def run_example(*, disk_checkpoint_directory: Path) -> dict[str, Any]:
    calendar_registry = build_calendar_registry()

    memory_execution_input = build_calibration_execution_input()
    memory_response = run_staged_calibration(
        memory_execution_input,
        calendar_registry=calendar_registry,
    )

    disk_execution_input = build_calibration_execution_input(
        execution_mode=CalibrationExecutionMode.DISK,
        disk_checkpoint_directory=str(disk_checkpoint_directory),
    )
    disk_response = run_staged_calibration(
        disk_execution_input,
        calendar_registry=calendar_registry,
    )

    checkpoint_files = sorted(
        checkpoint_path.name for checkpoint_path in disk_checkpoint_directory.rglob("*.json")
    )

    return {
        "memory_mode": {
            "request_execution_mode": memory_execution_input.request.execution_mode.value,
            "response_execution_mode": memory_response.execution_mode.value,
            "status": memory_response.status.value,
            "stage_count": len(memory_response.stage_reports),
            "checkpoint_count": len(memory_response.emitted_checkpoints),
        },
        "disk_mode": {
            "request_execution_mode": disk_execution_input.request.execution_mode.value,
            "response_execution_mode": disk_response.execution_mode.value,
            "status": disk_response.status.value,
            "stage_count": len(disk_response.stage_reports),
            "checkpoint_count": len(disk_response.emitted_checkpoints),
            "checkpoint_file_count": len(checkpoint_files),
        },
    }


def main() -> None:
    with TemporaryDirectory(prefix="janiform-calibration-example-") as checkpoint_directory:
        summary = run_example(disk_checkpoint_directory=Path(checkpoint_directory))
        print(json.dumps(summary, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
