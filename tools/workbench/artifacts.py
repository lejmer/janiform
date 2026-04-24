import json

from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional


ARTIFACT_FILE_NAMES: dict[str, str] = {
    "observed_targets": "observed_targets.json",
    "candidate_frontier": "candidate_frontier.json",
    "coarse_tick_consistency": "coarse_tick_consistency.json",
    "selection_rationale": "selection_rationale.json",
    "simulation_summary": "simulation_summary.json",
    "search_surface_manifest": "search_surface_manifest.json",
    "runtime_profile": "runtime_profile.json",
}


@dataclass(frozen=True, slots=True)
class WorkbenchArtifacts:
    source_path: Path
    source_kind: str
    metadata: dict[str, Any]
    fit_report_payload: Optional[dict[str, Any]]
    model_payload: Optional[dict[str, Any]]
    observed_targets: dict[str, Any]
    candidate_frontier: dict[str, Any]
    coarse_tick_consistency: dict[str, Any]
    selection_rationale: dict[str, Any]
    simulation_summary: dict[str, Any]
    search_surface_manifest: dict[str, Any]
    runtime_profile: dict[str, Any]

    @property
    def selected_candidate_id(self) -> Optional[str]:
        decision = self.selection_rationale.get("decision")
        if isinstance(decision, dict):
            selected_candidate_id = decision.get("selected_candidate_id")
            if isinstance(selected_candidate_id, str):
                return selected_candidate_id

        return None


def load_workbench_artifacts(path: str | Path) -> WorkbenchArtifacts:
    input_path = Path(path).expanduser().resolve()
    if input_path.is_dir():
        return _load_from_run_directory(input_path)
    if input_path.is_file():
        return _load_from_document(input_path)

    raise FileNotFoundError(f"workbench input path `{input_path}` does not exist")


def build_summary_payload(artifacts: WorkbenchArtifacts) -> dict[str, Any]:
    frontier_entries = _artifact_list(artifacts.candidate_frontier, "frontier_entries")
    consistency_metrics = _artifact_list(artifacts.coarse_tick_consistency, "metrics")
    runtime_entries = _artifact_list(artifacts.runtime_profile, "entries")
    selected_candidate_id = artifacts.selected_candidate_id
    selected_candidate_entry = next(
        (
            entry
            for entry in frontier_entries
            if entry.get("candidate_id") == selected_candidate_id
        ),
        None,
    )
    admissible_candidates = [
        entry for entry in frontier_entries if bool(entry.get("admissible"))
    ]
    maximum_normalized_delta = max(
        (
            float(metric.get("normalized_delta", 0.0))
            for metric in consistency_metrics
            if isinstance(metric.get("normalized_delta"), int | float)
        ),
        default=0.0,
    )

    return {
        "source_path": str(artifacts.source_path),
        "source_kind": artifacts.source_kind,
        "selected_candidate_id": selected_candidate_id,
        "target_count": len(_artifact_list(artifacts.observed_targets, "targets")),
        "candidate_count": len(frontier_entries),
        "admissible_candidate_count": len(admissible_candidates),
        "simulation_summary_count": len(
            _artifact_list(artifacts.simulation_summary, "summaries")
        ),
        "coarse_tick_metric_count": len(consistency_metrics),
        "maximum_normalized_coarse_tick_delta": maximum_normalized_delta,
        "runtime_seconds_total": sum(
            float(entry.get("elapsed_seconds", 0.0))
            for entry in runtime_entries
            if isinstance(entry.get("elapsed_seconds"), int | float)
        ),
        "runtime_component_count": len(runtime_entries),
        "candidate_counts_by_stage": _candidate_counts_by_stage(frontier_entries),
        "selected_candidate_family_metrics": _selected_candidate_family_metrics(
            selected_candidate_entry
        ),
        "metadata": artifacts.metadata,
    }


def _load_from_run_directory(run_directory: Path) -> WorkbenchArtifacts:
    fit_report_path = run_directory / "fit_report.json"
    model_path = run_directory / "model.json"
    model_payload = _load_json_file(model_path) if model_path.is_file() else None
    if fit_report_path.is_file():
        payload = _load_json_file(fit_report_path)
        return _build_workbench_artifacts_from_fit_report(
            source_path=run_directory,
            source_kind="run_directory",
            run_directory=run_directory,
            fit_report_payload=payload,
            model_payload=model_payload,
        )

    diagnostics_directory = _resolve_diagnostics_directory(run_directory)
    bundle = {
        artifact_family: _load_json_file(diagnostics_directory / file_name)
        for artifact_family, file_name in ARTIFACT_FILE_NAMES.items()
    }
    metadata = {"run_directory": str(run_directory)}

    artifact_manifest = _load_artifact_manifest(run_directory)
    if artifact_manifest is not None:
        metadata.update(_extract_metadata_from_artifact_manifest(artifact_manifest))

    return _build_workbench_artifacts(
        source_path=run_directory,
        source_kind="run_directory",
        metadata=metadata,
        fit_report_payload=None,
        model_payload=model_payload,
        bundle=bundle,
    )


def _load_from_document(document_path: Path) -> WorkbenchArtifacts:
    payload = _load_json_file(document_path)
    bundle = _extract_diagnostic_bundle(payload)
    if bundle is not None:
        return _build_workbench_artifacts(
            source_path=document_path,
            source_kind="json_document",
            metadata=_extract_metadata_from_document(payload),
            fit_report_payload=payload,
            model_payload=None,
            bundle=bundle,
        )

    if document_path.name == "fit_report.json":
        return _build_workbench_artifacts_from_fit_report(
            source_path=document_path,
            source_kind="json_document",
            run_directory=document_path.parent,
            fit_report_payload=payload,
            model_payload=None,
        )

    if document_path.name in ARTIFACT_FILE_NAMES.values():
        diagnostics_directory = document_path.parent
        run_directory = diagnostics_directory.parent
        if diagnostics_directory.name != "diagnostics":
            run_directory = diagnostics_directory

        return _load_from_run_directory(run_directory)

    raise ValueError(
        "workbench input JSON must contain a diagnostic_artifact_bundle, a staged_response "
        "with a diagnostic_artifact_bundle, or be one of the diagnostics JSON files"
    )


def _build_workbench_artifacts(
    *,
    source_path: Path,
    source_kind: str,
    metadata: dict[str, Any],
    fit_report_payload: Optional[dict[str, Any]],
    model_payload: Optional[dict[str, Any]],
    bundle: dict[str, Any],
) -> WorkbenchArtifacts:
    return WorkbenchArtifacts(
        source_path=source_path,
        source_kind=source_kind,
        metadata=metadata,
        fit_report_payload=fit_report_payload,
        model_payload=model_payload,
        observed_targets=_require_artifact_mapping(bundle, "observed_targets"),
        candidate_frontier=_require_artifact_mapping(bundle, "candidate_frontier"),
        coarse_tick_consistency=_require_artifact_mapping(bundle, "coarse_tick_consistency"),
        selection_rationale=_require_artifact_mapping(bundle, "selection_rationale"),
        simulation_summary=_require_artifact_mapping(bundle, "simulation_summary"),
        search_surface_manifest=_require_artifact_mapping(bundle, "search_surface_manifest"),
        runtime_profile=_require_artifact_mapping(bundle, "runtime_profile"),
    )


def _build_workbench_artifacts_from_fit_report(
    *,
    source_path: Path,
    source_kind: str,
    run_directory: Path,
    fit_report_payload: dict[str, Any],
    model_payload: Optional[dict[str, Any]],
) -> WorkbenchArtifacts:
    run_id = _extract_run_id_from_fit_report(fit_report_payload)
    bundle = _extract_diagnostic_bundle(fit_report_payload)
    if bundle is None:
        bundle = _load_bundle_from_run_directory(run_directory, run_id=run_id)

    metadata = _extract_metadata_from_document(fit_report_payload)
    metadata["run_directory"] = str(run_directory)

    artifact_manifest = _load_artifact_manifest(run_directory, run_id=run_id)
    if artifact_manifest is not None:
        metadata.update(_extract_metadata_from_artifact_manifest(artifact_manifest))

    return _build_workbench_artifacts(
        source_path=source_path,
        source_kind=source_kind,
        metadata=metadata,
        fit_report_payload=fit_report_payload,
        model_payload=model_payload,
        bundle=bundle,
    )


def _extract_diagnostic_bundle(payload: dict[str, Any]) -> Optional[dict[str, Any]]:
    if all(artifact_family in payload for artifact_family in ARTIFACT_FILE_NAMES):
        return payload

    nested_bundle = payload.get("diagnostic_artifact_bundle")
    if isinstance(nested_bundle, dict):
        return nested_bundle

    staged_response = payload.get("staged_response")
    if isinstance(staged_response, dict):
        staged_response_bundle = staged_response.get("diagnostic_artifact_bundle")
        if isinstance(staged_response_bundle, dict):
            return staged_response_bundle

    return None


def _extract_run_id_from_fit_report(payload: dict[str, Any]) -> Optional[str]:
    run_id = payload.get("run_id")
    if isinstance(run_id, str):
        return run_id

    staged_response = payload.get("staged_response")
    if not isinstance(staged_response, dict):
        return None

    staged_run_id = staged_response.get("run_id")
    if isinstance(staged_run_id, str):
        return staged_run_id

    return None


def _extract_metadata_from_document(payload: dict[str, Any]) -> dict[str, Any]:
    metadata: dict[str, Any] = {}

    summary = payload.get("summary")
    if isinstance(summary, dict):
        metadata["summary"] = summary

    run_id = payload.get("run_id")
    if isinstance(run_id, str):
        metadata["run_id"] = run_id

    staged_response = payload.get("staged_response")
    if isinstance(staged_response, dict):
        staged_run_id = staged_response.get("run_id")
        status = staged_response.get("status")
        execution_mode = staged_response.get("execution_mode")
        if isinstance(staged_run_id, str):
            metadata["run_id"] = staged_run_id
        if isinstance(status, str):
            metadata["status"] = status
        if isinstance(execution_mode, str):
            metadata["execution_mode"] = execution_mode

    return metadata


def _resolve_diagnostics_directory(run_directory: Path, *, run_id: Optional[str] = None) -> Path:
    direct_diagnostics_directory = run_directory / "diagnostics"
    if direct_diagnostics_directory.is_dir():
        return direct_diagnostics_directory

    artifact_store_run_directory = run_directory / "artifacts" / "runs" / run_directory.name
    artifact_store_diagnostics_directory = artifact_store_run_directory / "diagnostics"
    if artifact_store_diagnostics_directory.is_dir():
        return artifact_store_diagnostics_directory

    if run_id is not None:
        fit_report_run_directory = run_directory / "artifacts" / "runs" / run_id
        fit_report_diagnostics_directory = fit_report_run_directory / "diagnostics"
        if fit_report_diagnostics_directory.is_dir():
            return fit_report_diagnostics_directory

    return run_directory


def _load_bundle_from_run_directory(
    run_directory: Path,
    *,
    run_id: Optional[str] = None,
) -> dict[str, Any]:
    diagnostics_directory = _resolve_diagnostics_directory(run_directory, run_id=run_id)
    return {
        artifact_family: _load_json_file(diagnostics_directory / file_name)
        for artifact_family, file_name in ARTIFACT_FILE_NAMES.items()
    }


def _load_artifact_manifest(
    run_directory: Path,
    *,
    run_id: Optional[str] = None,
) -> Optional[dict[str, Any]]:
    manifest_candidates = [
        run_directory / "artifacts" / "runs" / run_directory.name / "artifact_manifest.json"
    ]
    if run_id is not None:
        manifest_candidates.append(
            run_directory / "artifacts" / "runs" / run_id / "artifact_manifest.json"
        )

    for artifact_manifest_path in manifest_candidates:
        if artifact_manifest_path.is_file():
            return _load_json_file(artifact_manifest_path)

    return None


def _extract_metadata_from_artifact_manifest(payload: dict[str, Any]) -> dict[str, Any]:
    metadata: dict[str, Any] = {}

    run_id = payload.get("run_id")
    if isinstance(run_id, str):
        metadata["run_id"] = run_id

    execution_mode = payload.get("execution_mode")
    if isinstance(execution_mode, str):
        metadata["execution_mode"] = execution_mode

    schema_version = payload.get("schema_version")
    if isinstance(schema_version, str):
        metadata["artifact_manifest_schema_version"] = schema_version

    index_metadata = payload.get("index_metadata")
    if isinstance(index_metadata, dict):
        metadata["artifact_index_metadata"] = index_metadata

    return metadata


def _candidate_counts_by_stage(frontier_entries: list[dict[str, Any]]) -> dict[str, int]:
    counts: dict[str, int] = {}
    for entry in frontier_entries:
        stage = entry.get("stage")
        if isinstance(stage, str):
            counts[stage] = counts.get(stage, 0) + 1

    return counts


def _selected_candidate_family_metrics(
    candidate_entry: Optional[dict[str, Any]],
) -> dict[str, float]:
    if candidate_entry is None:
        return {}

    family_metrics = candidate_entry.get("family_metrics", [])
    if not isinstance(family_metrics, list):
        return {}

    selected_metrics: dict[str, float] = {}
    for family_metric in family_metrics:
        if not isinstance(family_metric, dict):
            continue

        target_family = family_metric.get("target_family")
        normalized_distance = family_metric.get("normalized_distance")
        if isinstance(target_family, str) and isinstance(
            normalized_distance,
            int | float,
        ):
            selected_metrics[target_family] = float(normalized_distance)

    return selected_metrics


def _artifact_list(payload: dict[str, Any], field_name: str) -> list[dict[str, Any]]:
    raw_items = payload.get(field_name, [])
    if isinstance(raw_items, list):
        return [item for item in raw_items if isinstance(item, dict)]

    return []


def _require_artifact_mapping(bundle: dict[str, Any], artifact_family: str) -> dict[str, Any]:
    artifact_payload = bundle.get(artifact_family)
    if not isinstance(artifact_payload, dict):
        raise ValueError(
            f"artifact family `{artifact_family}` is missing from the diagnostic bundle"
        )

    return artifact_payload


def _load_json_file(path: Path) -> dict[str, Any]:
    try:
        raw_payload = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise FileNotFoundError(f"expected diagnostics file `{path}`") from error
    except json.JSONDecodeError as error:
        raise ValueError(f"diagnostics file `{path}` does not contain valid JSON") from error

    if not isinstance(raw_payload, dict):
        raise ValueError(f"diagnostics file `{path}` must contain a JSON object")

    return raw_payload
