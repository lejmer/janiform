from __future__ import annotations

from abc import ABC, abstractmethod
from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any, Optional

from .contracts.base import (
    ValidationErrorFactory,
    require_non_negative_integer,
    require_positive_integer,
)


class BoundedProgressEvent(ABC):
    completed_units: int
    total_units: int

    @property
    @abstractmethod
    def detail(self) -> str:
        raise NotImplementedError


@dataclass(frozen=True, slots=True)
class BoundedProgressCounts:
    completed_units: int
    total_units: int


def bounded_progress_counts_from_mapping(
    payload: Mapping[str, Any],
    *,
    completed_units_field_path: str,
    total_units_field_path: str,
    error_factory: ValidationErrorFactory,
) -> BoundedProgressCounts:
    completed_units = require_non_negative_integer(
        payload,
        "completed_units",
        completed_units_field_path,
        error_factory,
    )
    total_units = require_positive_integer(
        payload,
        "total_units",
        total_units_field_path,
        error_factory,
    )

    if completed_units > total_units:
        raise error_factory(
            completed_units_field_path,
            "must be less than or equal to total_units",
        )

    return BoundedProgressCounts(
        completed_units=completed_units,
        total_units=total_units,
    )


def first_bounded_count_measure(
    payload: Mapping[str, Any],
    *,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> Optional[Mapping[str, Any]]:
    raw_measures = payload.get("measures")
    if not isinstance(raw_measures, list):
        raise error_factory(field_path, "must be a list when provided")

    for raw_measure in raw_measures:
        if not isinstance(raw_measure, Mapping):
            raise error_factory(field_path, "must contain objects")

        if raw_measure.get("measure_kind") == "bounded_count":
            return raw_measure

    return None


__all__ = [
    "BoundedProgressEvent",
    "BoundedProgressCounts",
    "bounded_progress_counts_from_mapping",
    "first_bounded_count_measure",
]
