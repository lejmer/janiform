from dataclasses import dataclass
from typing import Any, Optional

from .artifacts import WorkbenchArtifacts


@dataclass(frozen=True, slots=True)
class WorkbenchFamilyMetricSummary:
    family_id: str
    normalized_distance: Optional[float]
    raw_payload: dict[str, Any]


@dataclass(frozen=True, slots=True)
class WorkbenchCandidateSnapshot:
    candidate_id: str
    stage: str
    evaluation_tier: str
    candidate_family: str
    candidate_source: str
    admissible: bool
    valid: bool
    admissibility_state: str
    validity_state: str
    pareto_rank: Optional[int]
    lexicographic_rank: Optional[int]
    scalar_tie_break: Optional[float]
    rejection_reason: Optional[str]
    objective_summary: dict[str, Any]
    canonical_coordinates: dict[str, float]
    family_metrics: tuple[WorkbenchFamilyMetricSummary, ...]
    parameter_snapshot: dict[str, Any]
    simulation_summary: dict[str, Any]
    consistency_summary: dict[str, Any]
    lineage: dict[str, Any]
    runtime_summary: dict[str, Any]
    created_at_utc: Optional[str]
    is_selected: bool
    raw_payload: dict[str, Any]


@dataclass(frozen=True, slots=True)
class WorkbenchCandidateStore:
    candidates: tuple[WorkbenchCandidateSnapshot, ...]
    selected_candidate_id: Optional[str]

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
                candidate_family=_candidate_family(stage, entry),
                candidate_source="artifact_frontier",
                admissible=bool(entry.get("admissible")),
                valid=bool(entry.get("valid")),
                admissibility_state=_admissibility_state(bool(entry.get("admissible"))),
                validity_state=_validity_state(bool(entry.get("valid"))),
                pareto_rank=_coerce_optional_int(entry.get("pareto_rank")),
                lexicographic_rank=_coerce_optional_int(entry.get("lexicographic_rank")),
                scalar_tie_break=_coerce_optional_float(entry.get("scalar_tie_break")),
                rejection_reason=_coerce_optional_string(entry.get("rejection_reason")),
                objective_summary=_objective_summary_from_candidate_payload(entry),
                canonical_coordinates=_coerce_float_mapping(entry.get("canonical_coordinates")),
                family_metrics=_family_metric_summaries(entry.get("family_metrics")),
                parameter_snapshot=parameter_snapshot,
                simulation_summary=_simulation_summary_from_payload(entry),
                consistency_summary=_consistency_summary_from_payload(entry),
                lineage=_lineage_summary_from_payload(entry, stage=stage),
                runtime_summary=_runtime_summary_from_payload(entry),
                created_at_utc=_created_at_utc_from_payload(entry),
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


def candidate_store_from_stage_report_payloads(
    stage_report_payloads: tuple[dict[str, Any], ...],
) -> WorkbenchCandidateStore:
    selected_stage_parameters = _selected_stage_parameter_snapshots_from_stage_reports(
        stage_report_payloads
    )
    selected_candidate_id = _selected_candidate_id_from_stage_reports(stage_report_payloads)

    candidates: list[WorkbenchCandidateSnapshot] = []

    for stage_report_payload in stage_report_payloads:
        stage_kind = _coerce_optional_string(stage_report_payload.get("stage_report_kind"))
        fitted_stage_payload = stage_report_payload.get("fitted_stage_payload")
        if stage_kind is None or not isinstance(fitted_stage_payload, dict):
            continue

        match stage_kind:
            case "hawkes":
                candidates.extend(
                    _hawkes_candidate_snapshots(
                        fitted_stage_payload,
                        selected_stage_parameters,
                        selected_candidate_id,
                    )
                )
            case "return_components":
                candidates.extend(
                    _return_components_candidate_snapshots(
                        fitted_stage_payload,
                        selected_stage_parameters,
                        selected_candidate_id,
                    )
                )
            case "joint_refinement":
                candidates.extend(
                    _joint_candidate_snapshots(
                        fitted_stage_payload,
                        selected_stage_parameters,
                        selected_candidate_id,
                    )
                )
            case _:
                continue

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


def _selected_stage_parameter_snapshots(
    model_payload: Optional[dict[str, Any]],
) -> dict[str, dict[str, Any]]:
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


def _selected_stage_parameter_snapshots_from_stage_reports(
    stage_report_payloads: tuple[dict[str, Any], ...],
) -> dict[str, dict[str, Any]]:
    snapshots: dict[str, dict[str, Any]] = {}
    for stage_report_payload in stage_report_payloads:
        stage_kind = _coerce_optional_string(stage_report_payload.get("stage_report_kind"))
        fitted_stage_payload = stage_report_payload.get("fitted_stage_payload")
        if stage_kind is None or not isinstance(fitted_stage_payload, dict):
            continue

        candidate_id = _coerce_optional_string(fitted_stage_payload.get("selected_candidate_id"))
        if candidate_id is None:
            continue

        snapshots[candidate_id] = {
            "stage": stage_kind,
            "payload": dict(fitted_stage_payload),
        }

    return snapshots


def _selected_candidate_id_from_stage_reports(
    stage_report_payloads: tuple[dict[str, Any], ...],
) -> Optional[str]:
    selected_candidate_id: Optional[str] = None
    for stage_report_payload in stage_report_payloads:
        fitted_stage_payload = stage_report_payload.get("fitted_stage_payload")
        if not isinstance(fitted_stage_payload, dict):
            continue

        candidate_id = _coerce_optional_string(fitted_stage_payload.get("selected_candidate_id"))
        if candidate_id is not None:
            selected_candidate_id = candidate_id

    return selected_candidate_id


def _hawkes_candidate_snapshots(
    fitted_stage_payload: dict[str, Any],
    selected_stage_parameters: dict[str, dict[str, Any]],
    selected_candidate_id: Optional[str],
) -> tuple[WorkbenchCandidateSnapshot, ...]:
    raw_candidates = fitted_stage_payload.get("candidate_diagnostics")
    if not isinstance(raw_candidates, list):
        return ()

    sortable_candidates = [
        raw_candidate
        for raw_candidate in raw_candidates
        if isinstance(raw_candidate, dict)
        and isinstance(raw_candidate.get("candidate_id"), str)
    ]
    sortable_candidates.sort(
        key=lambda raw_candidate: (
            _coerce_optional_float(raw_candidate.get("penalized_objective_value"))
            or float("inf"),
            str(raw_candidate.get("candidate_id")),
        )
    )

    snapshots: list[WorkbenchCandidateSnapshot] = []
    for rank, raw_candidate in enumerate(sortable_candidates, start=1):
        candidate_id = str(raw_candidate["candidate_id"])

        admissible = _coerce_optional_int(raw_candidate.get("constraint_violation_count")) == 0

        objective_terms = raw_candidate.get("objective_terms")

        snapshots.append(
            WorkbenchCandidateSnapshot(
                candidate_id=candidate_id,
                stage="hawkes",
                evaluation_tier="exact_block",
                candidate_family="hawkes",
                candidate_source="stage_report",
                admissible=admissible,
                valid=admissible,
                admissibility_state=_admissibility_state(admissible),
                validity_state=_validity_state(admissible),
                pareto_rank=rank,
                lexicographic_rank=rank,
                scalar_tie_break=_coerce_optional_float(raw_candidate.get("penalized_objective_value")),
                rejection_reason=None if admissible else "constraint_violation",
                objective_summary=_objective_summary_from_candidate_payload(raw_candidate),
                canonical_coordinates={},
                family_metrics=_metric_summaries_from_objective_terms(objective_terms),
                parameter_snapshot=_parameter_snapshot_for_candidate(
                    candidate_id,
                    selected_stage_parameters,
                    default_payload=dict(raw_candidate),
                ),
                simulation_summary=_simulation_summary_from_payload(raw_candidate),
                consistency_summary=_consistency_summary_from_payload(raw_candidate),
                lineage=_lineage_summary_from_payload(raw_candidate, stage="hawkes"),
                runtime_summary=_runtime_summary_from_payload(raw_candidate),
                created_at_utc=_created_at_utc_from_payload(raw_candidate),
                is_selected=candidate_id == selected_candidate_id,
                raw_payload=dict(raw_candidate),
            )
        )

    return tuple(snapshots)


def _return_components_candidate_snapshots(
    fitted_stage_payload: dict[str, Any],
    selected_stage_parameters: dict[str, dict[str, Any]],
    selected_candidate_id: Optional[str],
) -> tuple[WorkbenchCandidateSnapshot, ...]:
    raw_candidates = fitted_stage_payload.get("candidate_diagnostics")
    if not isinstance(raw_candidates, list):
        return ()

    sortable_candidates = [
        raw_candidate
        for raw_candidate in raw_candidates
        if isinstance(raw_candidate, dict)
        and isinstance(raw_candidate.get("candidate_id"), str)
    ]
    sortable_candidates.sort(
        key=lambda raw_candidate: (
            _coerce_optional_float(raw_candidate.get("penalized_objective_value"))
            or float("inf"),
            str(raw_candidate.get("candidate_id")),
        )
    )

    snapshots: list[WorkbenchCandidateSnapshot] = []
    for rank, raw_candidate in enumerate(sortable_candidates, start=1):
        candidate_id = str(raw_candidate["candidate_id"])

        valid = _coerce_optional_int(raw_candidate.get("constraint_violation_count")) == 0

        price_range_sanity_diagnostic = raw_candidate.get("price_range_sanity_diagnostic")
        price_range_flagged = (
            isinstance(price_range_sanity_diagnostic, dict)
            and bool(price_range_sanity_diagnostic.get("is_flagged"))
        )

        admissible = valid and not price_range_flagged

        snapshots.append(
            WorkbenchCandidateSnapshot(
                candidate_id=candidate_id,
                stage="return_components",
                evaluation_tier="exact_block",
                candidate_family="return_components",
                candidate_source="stage_report",
                admissible=admissible,
                valid=valid,
                admissibility_state=_admissibility_state(admissible),
                validity_state=_validity_state(valid),
                pareto_rank=rank,
                lexicographic_rank=rank,
                scalar_tie_break=_coerce_optional_float(raw_candidate.get("penalized_objective_value")),
                rejection_reason=None if admissible else "constraint_or_admissibility_failure",
                objective_summary=_objective_summary_from_candidate_payload(raw_candidate),
                canonical_coordinates={},
                family_metrics=_metric_summaries_from_objective_terms(
                    raw_candidate.get("objective_terms")
                ),
                parameter_snapshot=_parameter_snapshot_for_candidate(
                    candidate_id,
                    selected_stage_parameters,
                    default_payload=dict(raw_candidate),
                ),
                simulation_summary=_simulation_summary_from_payload(raw_candidate),
                consistency_summary=_consistency_summary_from_payload(raw_candidate),
                lineage=_lineage_summary_from_payload(raw_candidate, stage="return_components"),
                runtime_summary=_runtime_summary_from_payload(raw_candidate),
                created_at_utc=_created_at_utc_from_payload(raw_candidate),
                is_selected=candidate_id == selected_candidate_id,
                raw_payload=dict(raw_candidate),
            )
        )

    return tuple(snapshots)


def _joint_candidate_snapshots(
    fitted_stage_payload: dict[str, Any],
    selected_stage_parameters: dict[str, dict[str, Any]],
    selected_candidate_id: Optional[str],
) -> tuple[WorkbenchCandidateSnapshot, ...]:
    raw_candidates = fitted_stage_payload.get("candidate_frontier")
    if not isinstance(raw_candidates, list):
        return ()

    snapshots: list[WorkbenchCandidateSnapshot] = []
    for raw_candidate in raw_candidates:
        if not isinstance(raw_candidate, dict):
            continue

        candidate_id = _coerce_optional_string(raw_candidate.get("candidate_id"))
        if candidate_id is None:
            continue

        objective_breakdown = raw_candidate.get("objective_breakdown")

        canonical_coordinates = _coerce_float_mapping(
            _mapping_from_coordinate_pairs(raw_candidate.get("canonical_coordinates"))
        )

        snapshots.append(
            WorkbenchCandidateSnapshot(
                candidate_id=candidate_id,
                stage="joint_refinement",
                evaluation_tier="tick_joint",
                candidate_family="joint_refinement",
                candidate_source="stage_report",
                admissible=bool(raw_candidate.get("admissible")),
                valid=bool(raw_candidate.get("valid")),
                admissibility_state=_admissibility_state(bool(raw_candidate.get("admissible"))),
                validity_state=_validity_state(bool(raw_candidate.get("valid"))),
                pareto_rank=_coerce_optional_int(raw_candidate.get("pareto_rank")),
                lexicographic_rank=_coerce_optional_int(raw_candidate.get("lexicographic_rank")),
                scalar_tie_break=_coerce_optional_float(raw_candidate.get("scalar_tie_break")),
                rejection_reason=_coerce_optional_string(raw_candidate.get("invalid_reason")),
                objective_summary=_objective_summary_from_candidate_payload(raw_candidate),
                canonical_coordinates=canonical_coordinates,
                family_metrics=_metric_summaries_from_objective_breakdown(objective_breakdown),
                parameter_snapshot=_parameter_snapshot_for_candidate(
                    candidate_id,
                    selected_stage_parameters,
                    default_payload={
                        "canonical_coordinates": canonical_coordinates,
                        "objective_breakdown": objective_breakdown,
                    },
                ),
                simulation_summary=_simulation_summary_from_payload(raw_candidate),
                consistency_summary=_consistency_summary_from_payload(raw_candidate),
                lineage=_lineage_summary_from_payload(raw_candidate, stage="joint_refinement"),
                runtime_summary=_runtime_summary_from_payload(raw_candidate),
                created_at_utc=_created_at_utc_from_payload(raw_candidate),
                is_selected=candidate_id == selected_candidate_id,
                raw_payload=dict(raw_candidate),
            )
        )

    return tuple(snapshots)


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


def _metric_summaries_from_objective_terms(
    raw_terms: Any,
) -> tuple[WorkbenchFamilyMetricSummary, ...]:
    if not isinstance(raw_terms, list):
        return ()

    metrics: list[WorkbenchFamilyMetricSummary] = []
    for raw_term in raw_terms:
        if not isinstance(raw_term, dict):
            continue

        metric_id = _coerce_optional_string(raw_term.get("term_id")) or "unknown"

        metrics.append(
            WorkbenchFamilyMetricSummary(
                family_id=metric_id,
                normalized_distance=_coerce_optional_float(raw_term.get("normalized_error")),
                raw_payload=dict(raw_term),
            )
        )

    return tuple(metrics)


def _metric_summaries_from_objective_breakdown(
    raw_breakdown: Any,
) -> tuple[WorkbenchFamilyMetricSummary, ...]:
    if not isinstance(raw_breakdown, dict):
        return ()

    metrics: list[WorkbenchFamilyMetricSummary] = []
    for key, value in raw_breakdown.items():
        if not isinstance(key, str) or not isinstance(value, int | float):
            continue

        metrics.append(
            WorkbenchFamilyMetricSummary(
                family_id=key,
                normalized_distance=float(value),
                raw_payload={key: float(value)},
            )
        )

    return tuple(metrics)


def _parameter_snapshot_for_candidate(
    candidate_id: str,
    selected_stage_parameters: dict[str, dict[str, Any]],
    *,
    default_payload: dict[str, Any],
) -> dict[str, Any]:
    selected_parameters = selected_stage_parameters.get(candidate_id)
    if selected_parameters is not None:
        return selected_parameters

    return default_payload


def _mapping_from_coordinate_pairs(raw_coordinates: Any) -> dict[str, Any]:
    if isinstance(raw_coordinates, dict):
        return dict(raw_coordinates)
    if not isinstance(raw_coordinates, list):
        return {}

    mapping: dict[str, Any] = {}
    for raw_entry in raw_coordinates:
        if (
            isinstance(raw_entry, list | tuple)
            and len(raw_entry) == 2
            and isinstance(raw_entry[0], str)
        ):
            mapping[raw_entry[0]] = raw_entry[1]

    return mapping


def _coerce_float_mapping(raw_mapping: Any) -> dict[str, float]:
    if not isinstance(raw_mapping, dict):
        return {}

    coerced: dict[str, float] = {}
    for key, value in raw_mapping.items():
        if isinstance(key, str) and isinstance(value, int | float):
            coerced[key] = float(value)

    return coerced


def _coerce_optional_float(value: Any) -> Optional[float]:
    if isinstance(value, int | float):
        return float(value)

    return None


def _coerce_optional_int(value: Any) -> Optional[int]:
    if isinstance(value, int):
        return value

    return None


def _coerce_optional_string(value: Any) -> Optional[str]:
    if isinstance(value, str):
        return value

    return None


def _candidate_family(stage: str, raw_payload: dict[str, Any]) -> str:
    candidate_family = _coerce_optional_string(raw_payload.get("candidate_family"))
    if candidate_family is not None:
        return candidate_family

    return stage


def _admissibility_state(admissible: bool) -> str:
    return "admissible" if admissible else "rejected"


def _validity_state(valid: bool) -> str:
    return "valid" if valid else "invalid"


def _objective_summary_from_candidate_payload(raw_payload: dict[str, Any]) -> dict[str, Any]:
    return {
        "objective_value": _coerce_optional_float(
            raw_payload.get("scalar_tie_break")
            if raw_payload.get("scalar_tie_break") is not None
            else raw_payload.get("penalized_objective_value")
        ),
        "pareto_rank": _coerce_optional_int(raw_payload.get("pareto_rank")),
        "lexicographic_rank": _coerce_optional_int(raw_payload.get("lexicographic_rank")),
        "scalar_tie_break": _coerce_optional_float(raw_payload.get("scalar_tie_break")),
    }


def _simulation_summary_from_payload(raw_payload: dict[str, Any]) -> dict[str, Any]:
    simulation_summary = raw_payload.get("simulation_summary")
    if isinstance(simulation_summary, dict):
        return dict(simulation_summary)

    extracted_summary: dict[str, Any] = {}
    for field_name in ("event_count", "mean_absolute_return", "realized_variance"):
        field_value = raw_payload.get(field_name)
        if isinstance(field_value, int | float):
            extracted_summary[field_name] = float(field_value)

    return extracted_summary


def _consistency_summary_from_payload(raw_payload: dict[str, Any]) -> dict[str, Any]:
    consistency_summary = raw_payload.get("consistency_summary")
    if isinstance(consistency_summary, dict):
        return dict(consistency_summary)

    extracted_summary: dict[str, Any] = {}
    for field_name in ("coarse_objective_value", "tick_objective_value", "normalized_delta"):
        field_value = raw_payload.get(field_name)
        if isinstance(field_value, int | float):
            extracted_summary[field_name] = float(field_value)

    return extracted_summary


def _lineage_summary_from_payload(raw_payload: dict[str, Any], *, stage: str) -> dict[str, Any]:
    lineage: dict[str, Any] = {"originating_stage": stage}

    for field_name in (
        "parent_candidate_id",
        "proposal_method",
        "refinement_round",
        "candidate_family",
        "candidate_source",
        "batch_label",
        "neighborhood_label",
    ):
        field_value = raw_payload.get(field_name)
        if isinstance(field_value, str | int | float | bool):
            lineage[field_name] = field_value

    return lineage


def _runtime_summary_from_payload(raw_payload: dict[str, Any]) -> dict[str, Any]:
    runtime_summary: dict[str, Any] = {}
    for field_name in ("runtime_seconds", "elapsed_seconds", "candidate_index"):
        field_value = raw_payload.get(field_name)
        if isinstance(field_value, int | float):
            runtime_summary[field_name] = float(field_value)

    return runtime_summary


def _created_at_utc_from_payload(raw_payload: dict[str, Any]) -> Optional[str]:
    for field_name in ("created_at_utc", "evaluated_at_utc", "occurred_at_utc"):
        field_value = raw_payload.get(field_name)
        if isinstance(field_value, str):
            return field_value

    return None


__all__ = [
    "WorkbenchCandidateSnapshot",
    "WorkbenchCandidateStore",
    "WorkbenchFamilyMetricSummary",
    "candidate_store_from_artifacts",
    "candidate_store_from_stage_report_payloads",
]
