from __future__ import annotations

from collections.abc import Mapping
from copy import deepcopy
import json
from pathlib import Path
from typing import Any, Optional

from ...errors import calibration_error
from ...staged.contracts import CalibrationStage


def raw_stage_report_payloads(raw_response_payload: dict[str, Any]) -> tuple[dict[str, Any], ...]:
    raw_stage_reports = raw_response_payload.get("stage_reports", ())
    if not isinstance(raw_stage_reports, list):
        raise calibration_error("stage_reports", "must be a sequence")

    stage_payloads: list[dict[str, Any]] = []
    for stage_index, raw_stage_report in enumerate(raw_stage_reports):
        if not isinstance(raw_stage_report, dict):
            raise calibration_error(
                f"stage_reports[{stage_index}]",
                "must be an object",
            )

        stage_payloads.append(dict(raw_stage_report))

    return tuple(stage_payloads)


def resolved_stage_report_payloads(
    raw_response_payload: dict[str, Any],
    *,
    selected_stage_kinds: tuple[CalibrationStage, ...],
    run_id: str,
    disk_checkpoint_directory: Optional[str],
    require_complete_selection: bool,
) -> tuple[dict[str, Any], ...]:
    stage_report_payloads = raw_stage_report_payloads(raw_response_payload)
    stage_report_payload_by_kind = {
        _require_calibration_stage(
            stage_report_payload.get("stage_report_kind"),
            field_path="stage_report_kind",
        ): stage_report_payload
        for stage_report_payload in stage_report_payloads
    }

    if disk_checkpoint_directory is not None:
        for stage_kind in selected_stage_kinds:
            current_stage_report_payload = stage_report_payload_by_kind.get(stage_kind)
            if (
                current_stage_report_payload is not None
                and stage_report_payload_contains_stage_parameters(
                    stage_kind=stage_kind,
                    stage_report_payload=current_stage_report_payload,
                )
            ):
                continue

            persisted_stage_report_payload = load_stage_report_payload_from_stage_parameter_artifact(
                checkpoint_directory=Path(disk_checkpoint_directory),
                run_id=run_id,
                stage_kind=stage_kind,
            )
            if persisted_stage_report_payload is not None:
                stage_report_payload_by_kind[stage_kind] = persisted_stage_report_payload

    if require_complete_selection:
        missing_stage_kinds = tuple(
            stage_kind for stage_kind in selected_stage_kinds if stage_kind not in stage_report_payload_by_kind
        )
        if missing_stage_kinds:
            missing_stages = ", ".join(stage_kind.value for stage_kind in missing_stage_kinds)
            raise RuntimeError(
                "Fit completed successfully but persisted stage payload reconstruction was incomplete for "
                f"run `{run_id}`. Missing stages: {missing_stages}"
            )

    return tuple(
        deepcopy(stage_report_payload_by_kind[stage_kind])
        for stage_kind in selected_stage_kinds
        if stage_kind in stage_report_payload_by_kind
    )


def stage_report_payload_contains_stage_parameters(
    *,
    stage_kind: CalibrationStage,
    stage_report_payload: Mapping[str, Any],
) -> bool:
    if stage_kind == CalibrationStage.SEASONALITY:
        return isinstance(stage_report_payload.get("normalized_stage_payload"), Mapping)

    return isinstance(stage_report_payload.get("fitted_stage_payload"), Mapping)


def load_stage_report_payload_from_stage_parameter_artifact(
    *,
    checkpoint_directory: Path,
    run_id: str,
    stage_kind: CalibrationStage,
) -> Optional[dict[str, Any]]:
    artifact_path = stage_parameter_artifact_path(
        checkpoint_directory=checkpoint_directory,
        run_id=run_id,
        stage_kind=stage_kind,
    )
    if not artifact_path.exists():
        return None

    try:
        raw_payload = json.loads(artifact_path.read_text(encoding="utf-8"))
    except OSError as error:
        raise RuntimeError(
            f"Unable to read persisted stage artifact `{artifact_path}` for run `{run_id}`: {error}"
        ) from error
    except json.JSONDecodeError as error:
        raise RuntimeError(
            f"Persisted stage artifact `{artifact_path}` for run `{run_id}` is not valid JSON: {error.msg}"
        ) from error

    if not isinstance(raw_payload, Mapping):
        raise RuntimeError(
            f"Persisted stage artifact `{artifact_path}` for run `{run_id}` must be a JSON object"
        )

    execution_mode = raw_payload.get("execution_mode")
    if not isinstance(execution_mode, str) or not execution_mode.strip():
        raise RuntimeError(
            f"Persisted stage artifact `{artifact_path}` for run `{run_id}` is missing execution_mode"
        )

    stage_output_schema_version = raw_payload.get("stage_output_schema_version")
    if stage_kind != CalibrationStage.SEASONALITY and (
        not isinstance(stage_output_schema_version, str) or not stage_output_schema_version.strip()
    ):
        raise RuntimeError(
            f"Persisted stage artifact `{artifact_path}` for run `{run_id}` is missing stage_output_schema_version"
        )

    stage_parameters = raw_payload.get("stage_parameters")
    if not isinstance(stage_parameters, Mapping):
        raise RuntimeError(
            f"Persisted stage artifact `{artifact_path}` for run `{run_id}` is missing stage_parameters"
        )

    payload_kind = _require_calibration_stage(
        stage_parameters.get("payload_kind"),
        field_path="stage_parameters.payload_kind",
    )
    if payload_kind != stage_kind:
        raise RuntimeError(
            f"Persisted stage artifact `{artifact_path}` for run `{run_id}` has payload_kind "
            f"`{payload_kind.value}` but expected `{stage_kind.value}`"
        )

    stage_payload = stage_parameters.get("payload")
    if not isinstance(stage_payload, Mapping):
        raise RuntimeError(
            f"Persisted stage artifact `{artifact_path}` for run `{run_id}` is missing payload"
        )

    synthetic_stage_report_payload = {
        "stage_report_kind": stage_kind.value,
        "stage": stage_kind.value,
        "status": "success",
        "run_id": run_id,
        "execution_mode": execution_mode,
        "handoff_policy": "stage_parameter_artifact_payload_reconstruction.v1",
    }

    if stage_kind == CalibrationStage.SEASONALITY:
        synthetic_stage_report_payload["normalized_stage_payload"] = deepcopy(dict(stage_payload))
        return synthetic_stage_report_payload

    fitted_stage_payload = deepcopy(dict(stage_payload))
    fitted_stage_payload["schema_version"] = stage_output_schema_version
    synthetic_stage_report_payload["fitted_stage_payload"] = fitted_stage_payload

    return synthetic_stage_report_payload


def stage_parameter_artifact_path(
    *,
    checkpoint_directory: Path,
    run_id: str,
    stage_kind: CalibrationStage,
) -> Path:
    return checkpoint_directory / "artifacts" / "runs" / run_id / f"{stage_kind.value}.stage_parameter.artifact.json"


def _require_calibration_stage(
    raw_value: Any,
    *,
    field_path: str,
) -> CalibrationStage:
    if not isinstance(raw_value, str):
        raise calibration_error(field_path, "must be a string")

    try:
        return CalibrationStage(raw_value)
    except ValueError as error:
        valid_values = ", ".join(stage.value for stage in CalibrationStage)
        raise calibration_error(field_path, f"must be one of: {valid_values}") from error
