from dataclasses import dataclass
from typing import Any

from .artifacts import WorkbenchArtifacts


@dataclass(frozen=True, slots=True)
class WorkbenchFamilyMetricSummary:
    family_id: str
    normalized_distance: float | None
    raw_payload: dict[str, Any]


@dataclass(frozen=True, slots=True)
class WorkbenchCandidateSnapshot:
    candidate_id: str
    stage: str
    evaluation_tier: str
    admissible: bool
    valid: bool
    pareto_rank: int | None
    lexicographic_rank: int | None
    scalar_tie_break: float | None
    rejection_reason: str | None
    canonical_coordinates: dict[str, float]
    family_metrics: tuple[WorkbenchFamilyMetricSummary, ...]
    parameter_snapshot: dict[str, Any]
    is_selected: bool
    raw_payload: dict[str, Any]


@dataclass(frozen=True, slots=True)
class WorkbenchCandidateStore:
    candidates: tuple[WorkbenchCandidateSnapshot, ...]
    selected_candidate_id: str | None

    @property
    def by_id(self) -> dict[str, WorkbenchCandidateSnapshot]:
        return {candidate.candidate_id: candidate for candidate in self.candidates}


def candidate_store_from_artifacts(artifacts: WorkbenchArtifacts) -> WorkbenchCandidateStore:
    selected_stage_parameters = _selected_stage_parameter_snapshots(artifacts.model_payload)
    selected_candidate_id = artifacts.selected_candidate_id
    entries = artifacts.candidate_frontier.get("frontier_entries", [])
    if not isinstance(entries, list):
        return WorkbenchCandidateStore(candidates=(), selected_candidate_id=selected_candidate_id)

    candidates: list[WorkbenchCandidateSnapshot] = []
    for entry in entries:
        if not isinstance(entry, dict):
            continue

        candidate_id = entry.get("candidate_id")
        stage = entry.get("stage")
        evaluation_tier = entry.get("evaluation_tier")
        if not isinstance(candidate_id, str) or not isinstance(stage, str) or not isinstance(evaluation_tier, str):
            continue

        parameter_snapshot = dict(entry.get("canonical_coordinates", {}))
        selected_parameters = selected_stage_parameters.get(candidate_id)
        if selected_parameters is not None:
            parameter_snapshot = {
                "canonical_coordinates": dict(entry.get("canonical_coordinates", {})),
                "selected_stage_parameters": selected_parameters,
            }

        candidates.append(
            WorkbenchCandidateSnapshot(
                candidate_id=candidate_id,
                stage=stage,
                evaluation_tier=evaluation_tier,
                admissible=bool(entry.get("admissible")),
                valid=bool(entry.get("valid")),
                pareto_rank=_coerce_optional_int(entry.get("pareto_rank")),
                lexicographic_rank=_coerce_optional_int(entry.get("lexicographic_rank")),
                scalar_tie_break=_coerce_optional_float(entry.get("scalar_tie_break")),
                rejection_reason=_coerce_optional_string(entry.get("rejection_reason")),
                canonical_coordinates=_coerce_float_mapping(entry.get("canonical_coordinates")),
                family_metrics=_family_metric_summaries(entry.get("family_metrics")),
                parameter_snapshot=parameter_snapshot,
                is_selected=candidate_id == selected_candidate_id,
                raw_payload=dict(entry),
            )
        )

    candidates.sort(
        key=lambda candidate: (
            candidate.lexicographic_rank if candidate.lexicographic_rank is not None else 10**9,
            candidate.pareto_rank if candidate.pareto_rank is not None else 10**9,
            candidate.scalar_tie_break if candidate.scalar_tie_break is not None else float("inf"),
            candidate.candidate_id,
        )
    )
    return WorkbenchCandidateStore(
        candidates=tuple(candidates),
        selected_candidate_id=selected_candidate_id,
    )


def _selected_stage_parameter_snapshots(model_payload: dict[str, Any] | None) -> dict[str, dict[str, Any]]:
    if not isinstance(model_payload, dict):
        return {}

    raw_parameters = model_payload.get("parameters")
    if not isinstance(raw_parameters, dict):
        return {}

    snapshots: dict[str, dict[str, Any]] = {}
    for stage_name, stage_payload in raw_parameters.items():
        if not isinstance(stage_payload, dict):
            continue

        candidate_id = stage_payload.get("selected_candidate_id")
        if not isinstance(candidate_id, str):
            continue

        snapshots[candidate_id] = {
            "stage": stage_name,
            "payload": dict(stage_payload),
        }

    return snapshots


def _family_metric_summaries(raw_metrics: Any) -> tuple[WorkbenchFamilyMetricSummary, ...]:
    if not isinstance(raw_metrics, list):
        return ()

    metrics: list[WorkbenchFamilyMetricSummary] = []
    for raw_metric in raw_metrics:
        if not isinstance(raw_metric, dict):
            continue

        family_id = raw_metric.get("target_family")
        if not isinstance(family_id, str):
            family_id = "unknown"

        metrics.append(
            WorkbenchFamilyMetricSummary(
                family_id=family_id,
                normalized_distance=_coerce_optional_float(raw_metric.get("normalized_distance")),
                raw_payload=dict(raw_metric),
            )
        )

    return tuple(metrics)


def _coerce_float_mapping(raw_mapping: Any) -> dict[str, float]:
    if not isinstance(raw_mapping, dict):
        return {}

    coerced: dict[str, float] = {}
    for key, value in raw_mapping.items():
        if isinstance(key, str) and isinstance(value, int | float):
            coerced[key] = float(value)

    return coerced


def _coerce_optional_float(value: Any) -> float | None:
    if isinstance(value, int | float):
        return float(value)

    return None


def _coerce_optional_int(value: Any) -> int | None:
    if isinstance(value, int):
        return value

    return None


def _coerce_optional_string(value: Any) -> str | None:
    if isinstance(value, str):
        return value

    return None


__all__ = [
    "WorkbenchCandidateSnapshot",
    "WorkbenchCandidateStore",
    "WorkbenchFamilyMetricSummary",
    "candidate_store_from_artifacts",
]
