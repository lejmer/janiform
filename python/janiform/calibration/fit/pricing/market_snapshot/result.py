"""Public market-snapshot pricing result types."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from typing import Any, Optional

import pandas

from janiform._pricing_field_names import (
    BOUNDARY_STATE_NORM_FIELD_NAME,
    DISCOUNT_FACTOR_FIELD_NAME,
    DIVIDEND_YIELD_FIELD_NAME,
    EXPIRY_TIMESTAMP_UTC_FIELD_NAME,
    FORWARD_PRICE_FIELD_NAME,
    IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME,
    LOG_FORWARD_MONEYNESS_FIELD_NAME,
    MATURITY_INTERVAL_FIELD_NAME,
    MODEL_IMPLIED_VOLATILITY_FIELD_NAME,
    MODEL_PRICE_FIELD_NAME,
    OBSERVED_ASK_PRICE_FIELD_NAME,
    OBSERVED_BID_PRICE_FIELD_NAME,
    OBSERVED_MID_PRICE_FIELD_NAME,
    OPTION_TYPE_FIELD_NAME,
    PRICING_FAILURE_REASON_FIELD_NAME,
    PRICING_FAILURE_STAGE_FIELD_NAME,
    PRICING_STATUS_FIELD_NAME,
    QUOTE_TIMESTAMP_UTC_FIELD_NAME,
    RISK_FREE_RATE_FIELD_NAME,
    STRIKE_FIELD_NAME,
)
from .....pricing import (
    VanillaOptionImpliedVolatilityInversionStatus,
    VanillaOptionPayoffType,
    VanillaOptionPricingFailureStage,
    VanillaOptionPricingStatus,
)
from .....pricing._moneyness import calculate_optional_log_forward_moneyness
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

    @property
    def log_forward_moneyness(self) -> Optional[float]:
        return calculate_optional_log_forward_moneyness(
            strike=self.strike,
            forward_price=self.forward_price,
        )


@dataclass(frozen=True, slots=True)
class JaniformObservedOptionSnapshotPricingResult:
    valuation_time_utc: datetime
    spot: float
    nodes: tuple[JaniformObservedOptionSnapshotNode, ...]
    raw_result: JaniformVanillaOptionGridBatchPricingResult = field(repr=False)

    def to_pandas(self) -> Any:
        rows = [
            {
                QUOTE_TIMESTAMP_UTC_FIELD_NAME: node.quote_timestamp_utc,
                EXPIRY_TIMESTAMP_UTC_FIELD_NAME: node.expiry_timestamp_utc,
                STRIKE_FIELD_NAME: node.strike,
                OPTION_TYPE_FIELD_NAME: node.option_type.value,
                RISK_FREE_RATE_FIELD_NAME: node.risk_free_rate,
                DIVIDEND_YIELD_FIELD_NAME: node.dividend_yield,
                MATURITY_INTERVAL_FIELD_NAME: node.maturity_interval,
                OBSERVED_BID_PRICE_FIELD_NAME: node.observed_bid_price,
                OBSERVED_ASK_PRICE_FIELD_NAME: node.observed_ask_price,
                OBSERVED_MID_PRICE_FIELD_NAME: node.observed_mid_price,
                MODEL_PRICE_FIELD_NAME: node.model_price,
                MODEL_IMPLIED_VOLATILITY_FIELD_NAME: node.model_implied_volatility,
                IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME: (
                    None
                    if node.implied_volatility_inversion_status is None
                    else node.implied_volatility_inversion_status.value
                ),
                PRICING_STATUS_FIELD_NAME: node.pricing_status.value,
                PRICING_FAILURE_STAGE_FIELD_NAME: (
                    None
                    if node.pricing_failure_stage is None
                    else node.pricing_failure_stage.value
                ),
                PRICING_FAILURE_REASON_FIELD_NAME: node.pricing_failure_reason,
                FORWARD_PRICE_FIELD_NAME: node.forward_price,
                LOG_FORWARD_MONEYNESS_FIELD_NAME: node.log_forward_moneyness,
                DISCOUNT_FACTOR_FIELD_NAME: node.discount_factor,
                BOUNDARY_STATE_NORM_FIELD_NAME: node.boundary_state_norm,
            }
            for node in self.nodes
        ]

        return pandas.DataFrame.from_records(rows)

    def to_surface_df(
        self,
        *,
        value_column: str = MODEL_IMPLIED_VOLATILITY_FIELD_NAME,
    ) -> Any:
        frame = self.to_pandas()
        if value_column not in frame.columns:
            raise calibration_error(
                "value_column",
                f"must be one of: {', '.join(str(value) for value in frame.columns)}",
            )

        index_columns = (
            [EXPIRY_TIMESTAMP_UTC_FIELD_NAME]
            if frame[OPTION_TYPE_FIELD_NAME].nunique(dropna=False) == 1
            else [OPTION_TYPE_FIELD_NAME, EXPIRY_TIMESTAMP_UTC_FIELD_NAME]
        )

        return (
            frame
            .pivot(index=index_columns, columns=STRIKE_FIELD_NAME, values=value_column)
            .sort_index(axis=0)
            .sort_index(axis=1)
        )


__all__ = [
    "JaniformObservedOptionSnapshotNode",
    "JaniformObservedOptionSnapshotPricingResult",
]
