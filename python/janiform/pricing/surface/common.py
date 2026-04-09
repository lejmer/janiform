from __future__ import annotations

from collections.abc import Callable, Mapping, Sequence
from typing import Any, Optional

from ...contracts.base import (
    require_finite_scalar,
    require_non_negative_finite_scalar,
    require_positive_finite_scalar,
    require_sequence,
)
from ...contracts.base import ValidationErrorFactory, require_optional_finite_number


def tuple_of_positive_floats(
    raw_payload: Mapping[str, Any],
    *,
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> tuple[float, ...]:
    return tuple_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=error_factory,
        scalar_parser=lambda value, location: require_positive_finite_scalar(
            value,
            location,
            error_factory,
        ),
    )


def tuple_of_non_negative_floats(
    raw_payload: Mapping[str, Any],
    *,
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> tuple[float, ...]:
    return tuple_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=error_factory,
        scalar_parser=lambda value, location: require_non_negative_finite_scalar(
            value,
            location,
            error_factory,
        ),
    )


def tuple_of_finite_floats(
    raw_payload: Mapping[str, Any],
    *,
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> tuple[float, ...]:
    return tuple_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=error_factory,
        scalar_parser=lambda value, location: require_finite_scalar(
            value,
            location,
            error_factory,
        ),
    )


def tuple_of_optional_finite_floats(
    raw_payload: Mapping[str, Any],
    *,
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> tuple[Optional[float], ...]:
    return tuple_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=error_factory,
        scalar_parser=lambda value, location: require_optional_finite_number(
            {"value": value},
            "value",
            location,
            error_factory,
        ),
    )


def tuple_of_scalars[TScalar](
    raw_payload: Mapping[str, Any],
    *,
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
    scalar_parser: Callable[[Any, str], TScalar],
) -> tuple[TScalar, ...]:
    values = require_sequence(raw_payload, field_name, field_path, error_factory)

    normalized: list[TScalar] = []
    for value_index, value in enumerate(values):
        normalized.append(scalar_parser(value, f"{field_path}[{value_index}]"))

    return tuple(normalized)


def grid_of_positive_floats(
    raw_payload: Mapping[str, Any],
    *,
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> tuple[tuple[float, ...], ...]:
    return grid_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=error_factory,
        scalar_parser=lambda value, location: require_positive_finite_scalar(
            value,
            location,
            error_factory,
        ),
    )


def grid_of_non_negative_floats(
    raw_payload: Mapping[str, Any],
    *,
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> tuple[tuple[float, ...], ...]:
    return grid_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=error_factory,
        scalar_parser=lambda value, location: require_non_negative_finite_scalar(
            value,
            location,
            error_factory,
        ),
    )


def grid_of_optional_finite_floats(
    raw_payload: Mapping[str, Any],
    *,
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> tuple[tuple[Optional[float], ...], ...]:
    return grid_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        error_factory=error_factory,
        scalar_parser=lambda value, location: require_optional_finite_number(
            {"value": value},
            "value",
            location,
            error_factory,
        ),
    )


def grid_of_scalars[TScalar](
    raw_payload: Mapping[str, Any],
    *,
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
    scalar_parser: Callable[[Any, str], TScalar],
) -> tuple[tuple[TScalar, ...], ...]:
    rows = require_sequence(raw_payload, field_name, field_path, error_factory)

    normalized_rows: list[tuple[TScalar, ...]] = []
    expected_column_count: Optional[int] = None

    for row_index, raw_row in enumerate(rows):
        if isinstance(raw_row, str) or not isinstance(raw_row, Sequence):
            raise error_factory(f"{field_path}[{row_index}]", "must be a sequence")

        normalized_row = tuple(
            scalar_parser(value, f"{field_path}[{row_index}][{column_index}]")
            for column_index, value in enumerate(raw_row)
        )

        if expected_column_count is None:
            expected_column_count = len(normalized_row)
        elif len(normalized_row) != expected_column_count:
            raise error_factory(
                f"{field_path}[{row_index}]",
                f"must contain {expected_column_count} aligned values",
            )

        normalized_rows.append(normalized_row)

    return tuple(normalized_rows)


def grid_shape(
    values: tuple[tuple[Any, ...], ...],
    *,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> tuple[int, int]:
    row_count = len(values)
    if row_count == 0:
        return 0, 0

    column_count = len(values[0])
    for row_index, row in enumerate(values):
        if len(row) != column_count:
            raise error_factory(
                f"{field_path}[{row_index}]",
                f"must contain {column_count} aligned values",
            )

    return row_count, column_count


def validate_matching_grid_shape(
    values: tuple[tuple[Any, ...], ...],
    *,
    grid_shape_value: tuple[int, int],
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> None:
    observed_shape = grid_shape(
        values,
        field_path=field_path,
        error_factory=error_factory,
    )
    if observed_shape != grid_shape_value:
        raise error_factory(
            field_path,
            f"must have shape {grid_shape_value}, found {observed_shape}",
        )


def validate_aligned_count(
    values: Sequence[Any],
    expected_count: int,
    *,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> None:
    if len(values) != expected_count:
        raise error_factory(
            field_path,
            f"must contain {expected_count} aligned values",
        )
