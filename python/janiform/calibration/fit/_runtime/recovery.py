from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Final, Optional

from ...staged import CalibrationStage, CalibrationStageExecutionStatus
from ...staged.contracts import CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION
from ...staged.results import (
    StagedCalibrationResponse,
    select_staged_failure_details,
    validate_staged_calibration_response,
)
from .stage_reports import load_stage_report_payload_from_stage_parameter_artifact


_ARTIFACT_MANIFEST_DUPLICATE_COMMIT_FAILURE_KIND: Final[str] = "artifact_manifest_persist_failed"
_ARTIFACT_MANIFEST_DUPLICATE_COMMIT_MESSAGE_FRAGMENT: Final[str] = "committed artifact set already exists for run"
_RECOVERED_STAGE_REPORT_HANDOFF_POLICY: Final[str] = "committed_artifact_manifest_recovery.v1"


def recover_committed_disk_run_from_duplicate_manifest_failure(
    *,
    staged_response: StagedCalibrationResponse,
    raw_response_payload: dict[str, Any],
    selected_stage_kinds: tuple[CalibrationStage, ...],
    disk_checkpoint_directory: Optional[str],
) -> Optional[tuple[StagedCalibrationResponse, dict[str, Any]]]:
    if staged_response.status != CalibrationStageExecutionStatus.FAILURE:
        return None

    selected_failure, _ = select_staged_failure_details(staged_response)
    if selected_failure is None:
        return None

    if selected_failure.failure_kind != _ARTIFACT_MANIFEST_DUPLICATE_COMMIT_FAILURE_KIND:
        return None

    if (
        selected_failure.error_message is None
        or _ARTIFACT_MANIFEST_DUPLICATE_COMMIT_MESSAGE_FRAGMENT not in selected_failure.error_message
    ):
        return None

    if disk_checkpoint_directory is None:
        return None

    checkpoint_directory = Path(disk_checkpoint_directory)
    run_id = staged_response.run_id
    artifact_manifest_path = checkpoint_directory / "artifacts" / "runs" / run_id / "artifact_manifest.json"
    if not artifact_manifest_path.is_file():
        return None

    artifact_manifest = _load_json_mapping(artifact_manifest_path)
    if artifact_manifest is None:
        return None

    if not _has_complete_stage_parameter_artifacts(
        checkpoint_directory=checkpoint_directory,
        run_id=run_id,
        selected_stage_kinds=selected_stage_kinds,
    ):
        return None

    recovered_raw_response = {
        "schema_version": _non_empty_string(
            raw_response_payload.get("schema_version"),
            fallback=CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION,
        ),
        "failure_triage_policy": _non_empty_string(
            raw_response_payload.get("failure_triage_policy"),
            fallback=staged_response.failure_triage_policy,
        ),
        "run_id": run_id,
        "execution_mode": "disk",
        "status": "success",
        "stage_reports": [
            _recovered_stage_report_payload(stage_kind=stage_kind, run_id=run_id)
            for stage_kind in selected_stage_kinds
        ],
        "emitted_checkpoints": _recovered_checkpoint_payloads(
            artifact_manifest=artifact_manifest,
            selected_stage_kinds=selected_stage_kinds,
        ),
        "diagnostic_artifact_references": _recovered_diagnostic_artifact_references(
            artifact_manifest=artifact_manifest,
            run_id=run_id,
        ),
        "resumed_from_checkpoint_reference": staged_response.resumed_from_checkpoint_reference,
        "resumed_from_last_successful_stage": (
            None
            if staged_response.resumed_from_last_successful_stage is None
            else staged_response.resumed_from_last_successful_stage.value
        ),
        "started_from_stage": (
            None
            if staged_response.started_from_stage is None
            else staged_response.started_from_stage.value
        ),
    }
    recovered_raw_response = {
        key: value for key, value in recovered_raw_response.items() if value is not None
    }

    return validate_staged_calibration_response(recovered_raw_response), recovered_raw_response


def _has_complete_stage_parameter_artifacts(
    *,
    checkpoint_directory: Path,
    run_id: str,
    selected_stage_kinds: tuple[CalibrationStage, ...],
) -> bool:
    for stage_kind in selected_stage_kinds:
        persisted_stage_payload = load_stage_report_payload_from_stage_parameter_artifact(
            checkpoint_directory=checkpoint_directory,
            run_id=run_id,
            stage_kind=stage_kind,
        )
        if persisted_stage_payload is None:
            return False

    return True


def _load_json_mapping(path: Path) -> Optional[dict[str, Any]]:
    try:
        raw_payload = json.loads(path.read_text(encoding="utf-8"))
    except OSError:
        return None
    except json.JSONDecodeError:
        return None

    if not isinstance(raw_payload, dict):
        return None

    return raw_payload


def _recovered_stage_report_payload(
    *,
    stage_kind: CalibrationStage,
    run_id: str,
) -> dict[str, Any]:
    return {
        "stage_report_kind": stage_kind.value,
        "stage": stage_kind.value,
        "status": "success",
        "run_id": run_id,
        "execution_mode": "disk",
        "handoff_policy": _RECOVERED_STAGE_REPORT_HANDOFF_POLICY,
    }


def _recovered_checkpoint_payloads(
    *,
    artifact_manifest: dict[str, Any],
    selected_stage_kinds: tuple[CalibrationStage, ...],
) -> list[dict[str, Any]]:
    raw_artifact_references = artifact_manifest.get("artifact_references")
    if not isinstance(raw_artifact_references, list):
        return []

    checkpoint_reference_by_stage: dict[CalibrationStage, str] = {}
    for raw_artifact_reference in raw_artifact_references:
        if not isinstance(raw_artifact_reference, dict):
            continue

        if raw_artifact_reference.get("artifact_kind") != "stage_checkpoint":
            continue

        raw_stage = raw_artifact_reference.get("stage")
        raw_checkpoint_reference = raw_artifact_reference.get("artifact_reference")
        if not isinstance(raw_stage, str) or not isinstance(raw_checkpoint_reference, str):
            continue

        try:
            stage_kind = CalibrationStage(raw_stage)
        except ValueError:
            continue

        checkpoint_reference_by_stage[stage_kind] = raw_checkpoint_reference

    emitted_checkpoints: list[dict[str, Any]] = []
    for stage_position, stage_kind in enumerate(selected_stage_kinds):
        checkpoint_reference = checkpoint_reference_by_stage.get(stage_kind)
        if checkpoint_reference is None:
            continue

        emitted_checkpoints.append(
            {
                "checkpoint_reference": checkpoint_reference,
                "completed_stage": stage_kind.value,
                "completed_stage_position": stage_position,
                "execution_mode": "disk",
            }
        )

    return emitted_checkpoints


def _recovered_diagnostic_artifact_references(
    *,
    artifact_manifest: dict[str, Any],
    run_id: str,
) -> list[dict[str, Any]]:
    raw_artifact_references = artifact_manifest.get("artifact_references")
    if not isinstance(raw_artifact_references, list):
        return []

    diagnostic_artifact_references: list[dict[str, Any]] = []
    diagnostic_prefix = f"artifact://diagnostics/{run_id}/"
    for raw_artifact_reference in raw_artifact_references:
        if not isinstance(raw_artifact_reference, dict):
            continue

        if raw_artifact_reference.get("artifact_kind") != "diagnostics":
            continue

        raw_reference = raw_artifact_reference.get("artifact_reference")
        if not isinstance(raw_reference, str) or not raw_reference.startswith(diagnostic_prefix):
            continue

        artifact_family = raw_reference.removeprefix(diagnostic_prefix)
        if not artifact_family:
            continue

        diagnostic_artifact_references.append(
            {
                "artifact_family": artifact_family,
                "relative_path": (
                    Path("artifacts")
                    / "runs"
                    / run_id
                    / "diagnostics"
                    / f"{artifact_family}.json"
                ).as_posix(),
            }
        )

    return diagnostic_artifact_references


def _non_empty_string(raw_value: Any, *, fallback: str) -> str:
    if isinstance(raw_value, str) and raw_value.strip():
        return raw_value

    return fallback
