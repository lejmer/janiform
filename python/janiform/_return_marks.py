from __future__ import annotations

from collections.abc import Sequence
from math import isfinite
from typing import Final

from .contracts.base import ValidationErrorFactory


RETURN_MARK_HOUR_OF_WEEK_FACTOR_COUNT: Final[int] = 24 * 7


def validate_return_mark_hour_of_week_factors(
    return_mark_hour_of_week_factors: Sequence[float],
    *,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> None:
    if len(return_mark_hour_of_week_factors) != RETURN_MARK_HOUR_OF_WEEK_FACTOR_COUNT:
        raise error_factory(
            field_path,
            "must contain 168 local hour-of-week factors",
        )

    for factor_index, factor in enumerate(return_mark_hour_of_week_factors):
        if (
            isinstance(factor, bool)
            or not isinstance(factor, int | float)
            or not isfinite(factor)
            or factor <= 0.0
        ):
            raise error_factory(
                f"{field_path}[{factor_index}]",
                "must be finite and strictly positive",
            )
