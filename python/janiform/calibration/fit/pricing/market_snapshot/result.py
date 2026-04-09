"""Public market-snapshot pricing result types."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from typing import Any, Optional

import pandas

from .....pricing import (
    VanillaOptionImpliedVolatilityInversionStatus,
    VanillaOptionPayoffType,
    VanillaOptionPricingFailureStage,
    VanillaOptionPricingStatus,
)
from ..grids import JaniformVanillaOptionGridBatchPricingResult
from ....errors import calibration_error


@dataclass(frozen=True, slots=True)
class JaniformObservedOptionSnapshotNode:
    quote_timestamp_utc: datetime
    expiry_timestamp_utc: datetime
    strike: float
    option_type: VanillaOptionPayoffType
    risk_free_rate: float
    dividend_yield: float
    maturity_interval: float
    observed_bid_price: Optional[float]
    observed_ask_price: Optional[float]
    observed_mid_price: Optional[float]
    model_price: Optional[float]
    model_implied_volatility: Optional[float]
    implied_volatility_inversion_status: Optional[VanillaOptionImpliedVolatilityInversionStatus]
    pricing_status: VanillaOptionPricingStatus
    pricing_failure_stage: Optional[VanillaOptionPricingFailureStage]
    pricing_failure_reason: Optional[str]
    forward_price: Optional[float]
    discount_factor: Optional[float]
    boundary_state_norm: Optional[float]


@dataclass(frozen=True, slots=True)
class JaniformObservedOptionSnapshotPricingResult:
    valuation_time_utc: datetime
    spot: float
    nodes: tuple[JaniformObservedOptionSnapshotNode, ...]
    raw_result: JaniformVanillaOptionGridBatchPricingResult = field(repr=False)

    def to_pandas(self) -> Any:
        rows = [
            {
                "quote_timestamp_utc": node.quote_timestamp_utc,
                "expiry_timestamp_utc": node.expiry_timestamp_utc,
                "strike": node.strike,
                "option_type": node.option_type.value,
                "risk_free_rate": node.risk_free_rate,
                "dividend_yield": node.dividend_yield,
                "maturity_interval": node.maturity_interval,
                "observed_bid_price": node.observed_bid_price,
                "observed_ask_price": node.observed_ask_price,
                "observed_mid_price": node.observed_mid_price,
                "model_price": node.model_price,
                "model_implied_volatility": node.model_implied_volatility,
                "implied_volatility_inversion_status": (
                    None
                    if node.implied_volatility_inversion_status is None
                    else node.implied_volatility_inversion_status.value
                ),
                "pricing_status": node.pricing_status.value,
                "pricing_failure_stage": (
                    None
                    if node.pricing_failure_stage is None
                    else node.pricing_failure_stage.value
                ),
                "pricing_failure_reason": node.pricing_failure_reason,
                "forward_price": node.forward_price,
                "discount_factor": node.discount_factor,
                "boundary_state_norm": node.boundary_state_norm,
            }
            for node in self.nodes
        ]

        return pandas.DataFrame.from_records(rows)

    def to_surface_frame(
        self,
        *,
        value_column: str = "model_implied_volatility",
    ) -> Any:
        frame = self.to_pandas()
        if value_column not in frame.columns:
            raise calibration_error(
                "value_column",
                f"must be one of: {', '.join(str(value) for value in frame.columns)}",
            )

        index_columns = (
            ["expiry_timestamp_utc"]
            if frame["option_type"].nunique(dropna=False) == 1
            else ["option_type", "expiry_timestamp_utc"]
        )

        return (
            frame
            .pivot(index=index_columns, columns="strike", values=value_column)
            .sort_index(axis=0)
            .sort_index(axis=1)
        )


__all__ = [
    "JaniformObservedOptionSnapshotNode",
    "JaniformObservedOptionSnapshotPricingResult",
]
