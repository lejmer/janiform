from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any, Optional

from ..._progress_events import (
    BoundedProgressEvent,
    bounded_progress_counts_from_mapping,
    first_bounded_count_measure,
)
from ...contracts.base import require_mapping
from ..errors import calibration_error


@dataclass(frozen=True, slots=True)
class FitProgressEvent(BoundedProgressEvent):
    stage_id: Optional[str]
    stage_label: Optional[str]
    phase_id: Optional[str]
    phase_label: Optional[str]
    completed_units: int
    total_units: int

    @property
    def detail(self) -> str:
        return self.phase_label or self.phase_id or self.stage_label or self.stage_id or "fit"


def fit_progress_event_from_mapping(
    payload: Mapping[str, Any],
) -> Optional[FitProgressEvent]:
    if payload.get("event_class") != "progress":
        return None

    raw_payload = require_mapping(
        payload,
        "payload",
        "progress_event.payload",
        calibration_error,
    )
    bounded_count = first_bounded_count_measure(
        raw_payload,
        field_path="progress_event.payload.measures",
        error_factory=calibration_error,
    )
    if bounded_count is None:
        return None

    stage = _optional_ref(raw_payload, "stage", "stage_id", "progress_event.payload.stage")
    phase = _optional_ref(raw_payload, "phase", "phase_id", "progress_event.payload.phase")
    counts = bounded_progress_counts_from_mapping(
        bounded_count,
        completed_units_field_path="progress_event.payload.measures.completed_units",
        total_units_field_path="progress_event.payload.measures.total_units",
        error_factory=calibration_error,
    )

    return FitProgressEvent(
        stage_id=None if stage is None else stage.identifier,
        stage_label=None if stage is None else stage.label,
        phase_id=None if phase is None else phase.identifier,
        phase_label=None if phase is None else phase.label,
        completed_units=counts.completed_units,
        total_units=counts.total_units,
    )


@dataclass(frozen=True, slots=True)
class _EventRef:
    identifier: str
    label: Optional[str]


def _optional_ref(
    payload: Mapping[str, Any],
    field_name: str,
    identifier_field_name: str,
    field_path: str,
) -> Optional[_EventRef]:
    raw_value = payload.get(field_name)
    if raw_value is None:
        return None

    if not isinstance(raw_value, Mapping):
        raise calibration_error(field_path, "must be an object when provided")

    raw_identifier = raw_value.get(identifier_field_name)
    if not isinstance(raw_identifier, str) or not raw_identifier:
        raise calibration_error(
            f"{field_path}.{identifier_field_name}",
            "must be a non-empty string",
        )

    raw_label = raw_value.get("label")
    if raw_label is not None and not isinstance(raw_label, str):
        raise calibration_error(f"{field_path}.label", "must be a string when provided")

    return _EventRef(identifier=raw_identifier, label=raw_label)


__all__ = [
    "FitProgressEvent",
    "fit_progress_event_from_mapping",
]
