"""Shared forward-moneyness helpers for public pricing results."""

from __future__ import annotations

from math import log
from typing import Optional

from .._pricing_field_names import FORWARD_PRICE_FIELD_NAME, STRIKE_FIELD_NAME
from ..contracts.base import require_positive_finite_scalar
from .errors import _pricing_error


def calculate_log_forward_moneyness(
    *,
    strike: float,
    forward_price: float,
) -> float:
    require_positive_finite_scalar(strike, STRIKE_FIELD_NAME, _pricing_error)
    require_positive_finite_scalar(forward_price, FORWARD_PRICE_FIELD_NAME, _pricing_error)

    return log(strike / forward_price)


def calculate_optional_log_forward_moneyness(
    *,
    strike: float,
    forward_price: Optional[float],
) -> Optional[float]:
    if forward_price is None:
        return None

    return calculate_log_forward_moneyness(
        strike=strike,
        forward_price=forward_price,
    )
