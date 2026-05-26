"""Observed-options physical distribution diagnostic contracts."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from typing import Any, Final, Optional

import pandas

from janiform._pricing_field_names import (
    DIVIDEND_YIELD_FIELD_NAME,
    EXPIRY_TIMESTAMP_UTC_FIELD_NAME,
    MARKET_FORWARD_PHYSICAL_IMPLIED_VOLATILITY_FIELD_NAME,
    MARKET_FORWARD_PHYSICAL_IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME,
    MARKET_FORWARD_PHYSICAL_PRICE_FIELD_NAME,
    MATURITY_INTERVAL_FIELD_NAME,
    MODEL_Q_IMPLIED_VOLATILITY_FIELD_NAME,
    OBSERVED_Q_IMPLIED_VOLATILITY_FIELD_NAME,
    OBSERVED_Q_IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME,
    OPTION_TYPE_FIELD_NAME,
    PHYSICAL_FORWARD_NORMALIZED_IMPLIED_VOLATILITY_FIELD_NAME,
    PHYSICAL_FORWARD_NORMALIZED_IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME,
    PHYSICAL_FORWARD_NORMALIZED_PRICE_FIELD_NAME,
    P_SIMULATED_FORWARD_PRICE_FIELD_NAME,
    Q_FORWARD_PRICE_FIELD_NAME,
    QUOTE_TIMESTAMP_UTC_FIELD_NAME,
    RISK_FREE_RATE_FIELD_NAME,
    SAMPLE_COUNT_FIELD_NAME,
    STRIKE_FIELD_NAME,
    VALUATION_TIME_UTC_FIELD_NAME,
)
from janiform.pricing import (
    VanillaOptionImpliedVolatilityInversionStatus,
    VanillaOptionPayoffType,
)
from janiform.pricing.errors import _pricing_error

from .internal.common import native_datetime, optional_float


DEFAULT_PHYSICAL_DISTRIBUTION_PATH_COUNT: Final[int] = 1024
DEFAULT_PHYSICAL_DISTRIBUTION_SEED: Final[int] = 0
_MAX_U64: Final[int] = (1 << 64) - 1


@dataclass(frozen=True, slots=True)
class ObservedOptionsPhysicalDistributionDiagnosticsRequest:
    path_count: int = DEFAULT_PHYSICAL_DISTRIBUTION_PATH_COUNT
    seed: int = DEFAULT_PHYSICAL_DISTRIBUTION_SEED

    def to_payload(self) -> dict[str, int]:
        if self.path_count <= 0:
            raise _pricing_error("physical_distribution_diagnostics.path_count", "must be positive")
        if self.seed < 0 or self.seed > _MAX_U64:
            raise _pricing_error("physical_distribution_diagnostics.seed", "must fit in u64")

        return {
            "path_count": self.path_count,
            "seed": self.seed,
        }


@dataclass(frozen=True, slots=True)
class ObservedOptionsPhysicalDistributionDiagnosticsReport:
    schema_version: str
    valuation_count: int
    node_count: int
    path_count_per_valuation: int
    path_partition_count: int
    worker_count: int
    maximum_horizon_interval_count: int

    @staticmethod
    def from_native_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> ObservedOptionsPhysicalDistributionDiagnosticsReport:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        return ObservedOptionsPhysicalDistributionDiagnosticsReport(
            schema_version=str(raw_payload["schema_version"]),
            valuation_count=int(raw_payload["valuation_count"]),
            node_count=int(raw_payload["node_count"]),
            path_count_per_valuation=int(raw_payload["path_count_per_valuation"]),
            path_partition_count=int(raw_payload["path_partition_count"]),
            worker_count=int(raw_payload["worker_count"]),
            maximum_horizon_interval_count=int(raw_payload["maximum_horizon_interval_count"]),
        )


@dataclass(frozen=True, slots=True)
class ObservedOptionsPhysicalDistributionDiagnosticsNode:
    valuation_time_utc: datetime
    quote_timestamp_utc: datetime
    expiry_timestamp_utc: datetime
    strike: float
    option_type: VanillaOptionPayoffType
    risk_free_rate: float
    dividend_yield: float
    maturity_interval: float
    q_forward_price: float
    p_simulated_forward_price: float
    model_q_implied_volatility: Optional[float]
    observed_q_implied_volatility: Optional[float]
    observed_q_implied_volatility_inversion_status: Optional[
        VanillaOptionImpliedVolatilityInversionStatus
    ]
    sample_count: int
    physical_forward_normalized_price: float
    physical_forward_normalized_implied_volatility: Optional[float]
    physical_forward_normalized_implied_volatility_inversion_status: (
        VanillaOptionImpliedVolatilityInversionStatus
    )
    market_forward_physical_price: float
    market_forward_physical_implied_volatility: Optional[float]
    market_forward_physical_implied_volatility_inversion_status: (
        VanillaOptionImpliedVolatilityInversionStatus
    )

    @staticmethod
    def from_native_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> ObservedOptionsPhysicalDistributionDiagnosticsNode:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        return ObservedOptionsPhysicalDistributionDiagnosticsNode(
            valuation_time_utc=native_datetime(
                raw_payload.get("valuation_time_utc"),
                field_path=f"{field_path}.valuation_time_utc",
            ),
            quote_timestamp_utc=native_datetime(
                raw_payload.get("quote_timestamp_utc"),
                field_path=f"{field_path}.quote_timestamp_utc",
            ),
            expiry_timestamp_utc=native_datetime(
                raw_payload.get("expiry_timestamp_utc"),
                field_path=f"{field_path}.expiry_timestamp_utc",
            ),
            strike=float(raw_payload["strike"]),
            option_type=VanillaOptionPayoffType(raw_payload["option_type"]),
            risk_free_rate=float(raw_payload["risk_free_rate"]),
            dividend_yield=float(raw_payload["dividend_yield"]),
            maturity_interval=float(raw_payload["maturity_interval"]),
            q_forward_price=float(raw_payload["q_forward_price"]),
            p_simulated_forward_price=float(raw_payload["p_simulated_forward_price"]),
            model_q_implied_volatility=optional_float(
                raw_payload.get("model_q_implied_volatility")
            ),
            observed_q_implied_volatility=optional_float(
                raw_payload.get("observed_q_implied_volatility")
            ),
            observed_q_implied_volatility_inversion_status=(
                None
                if raw_payload.get("observed_q_implied_volatility_inversion_status") is None
                else VanillaOptionImpliedVolatilityInversionStatus(
                    raw_payload["observed_q_implied_volatility_inversion_status"]
                )
            ),
            sample_count=int(raw_payload["sample_count"]),
            physical_forward_normalized_price=float(
                raw_payload["physical_forward_normalized_price"]
            ),
            physical_forward_normalized_implied_volatility=optional_float(
                raw_payload.get("physical_forward_normalized_implied_volatility")
            ),
            physical_forward_normalized_implied_volatility_inversion_status=(
                VanillaOptionImpliedVolatilityInversionStatus(
                    raw_payload[
                        "physical_forward_normalized_implied_volatility_inversion_status"
                    ]
                )
            ),
            market_forward_physical_price=float(raw_payload["market_forward_physical_price"]),
            market_forward_physical_implied_volatility=optional_float(
                raw_payload.get("market_forward_physical_implied_volatility")
            ),
            market_forward_physical_implied_volatility_inversion_status=(
                VanillaOptionImpliedVolatilityInversionStatus(
                    raw_payload["market_forward_physical_implied_volatility_inversion_status"]
                )
            ),
        )


@dataclass(frozen=True, slots=True)
class ObservedOptionsPhysicalDistributionDiagnosticsOutput:
    schema_version: str
    request: ObservedOptionsPhysicalDistributionDiagnosticsRequest
    report: ObservedOptionsPhysicalDistributionDiagnosticsReport
    nodes: tuple[ObservedOptionsPhysicalDistributionDiagnosticsNode, ...]

    @staticmethod
    def from_native_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "physical_distribution",
    ) -> Optional[ObservedOptionsPhysicalDistributionDiagnosticsOutput]:
        if raw_payload is None:
            return None

        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        raw_request = raw_payload.get("request")
        if not isinstance(raw_request, dict):
            raise _pricing_error(f"{field_path}.request", "must be an object")

        raw_nodes = raw_payload.get("nodes")
        if not isinstance(raw_nodes, list):
            raise _pricing_error(f"{field_path}.nodes", "must be an array")

        return ObservedOptionsPhysicalDistributionDiagnosticsOutput(
            schema_version=str(raw_payload["schema_version"]),
            request=ObservedOptionsPhysicalDistributionDiagnosticsRequest(
                path_count=int(raw_request["path_count"]),
                seed=int(raw_request["seed"]),
            ),
            report=ObservedOptionsPhysicalDistributionDiagnosticsReport.from_native_mapping(
                raw_payload.get("report"),
                field_path=f"{field_path}.report",
            ),
            nodes=tuple(
                ObservedOptionsPhysicalDistributionDiagnosticsNode.from_native_mapping(
                    raw_node,
                    field_path=f"{field_path}.nodes[{node_position}]",
                )
                for node_position, raw_node in enumerate(raw_nodes)
            ),
        )

    def to_pandas(self) -> Any:
        rows = [
            {
                VALUATION_TIME_UTC_FIELD_NAME: node.valuation_time_utc,
                QUOTE_TIMESTAMP_UTC_FIELD_NAME: node.quote_timestamp_utc,
                EXPIRY_TIMESTAMP_UTC_FIELD_NAME: node.expiry_timestamp_utc,
                STRIKE_FIELD_NAME: node.strike,
                OPTION_TYPE_FIELD_NAME: node.option_type.value,
                RISK_FREE_RATE_FIELD_NAME: node.risk_free_rate,
                DIVIDEND_YIELD_FIELD_NAME: node.dividend_yield,
                MATURITY_INTERVAL_FIELD_NAME: node.maturity_interval,
                Q_FORWARD_PRICE_FIELD_NAME: node.q_forward_price,
                P_SIMULATED_FORWARD_PRICE_FIELD_NAME: node.p_simulated_forward_price,
                MODEL_Q_IMPLIED_VOLATILITY_FIELD_NAME: node.model_q_implied_volatility,
                OBSERVED_Q_IMPLIED_VOLATILITY_FIELD_NAME: node.observed_q_implied_volatility,
                OBSERVED_Q_IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME: (
                    None
                    if node.observed_q_implied_volatility_inversion_status is None
                    else node.observed_q_implied_volatility_inversion_status.value
                ),
                SAMPLE_COUNT_FIELD_NAME: node.sample_count,
                PHYSICAL_FORWARD_NORMALIZED_PRICE_FIELD_NAME: (
                    node.physical_forward_normalized_price
                ),
                PHYSICAL_FORWARD_NORMALIZED_IMPLIED_VOLATILITY_FIELD_NAME: (
                    node.physical_forward_normalized_implied_volatility
                ),
                PHYSICAL_FORWARD_NORMALIZED_IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME: (
                    node.physical_forward_normalized_implied_volatility_inversion_status.value
                ),
                MARKET_FORWARD_PHYSICAL_PRICE_FIELD_NAME: node.market_forward_physical_price,
                MARKET_FORWARD_PHYSICAL_IMPLIED_VOLATILITY_FIELD_NAME: (
                    node.market_forward_physical_implied_volatility
                ),
                MARKET_FORWARD_PHYSICAL_IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME: (
                    node.market_forward_physical_implied_volatility_inversion_status.value
                ),
            }
            for node in self.nodes
        ]

        return pandas.DataFrame.from_records(rows)


def physical_distribution_diagnostics_request_payload(
    diagnostics: bool | ObservedOptionsPhysicalDistributionDiagnosticsRequest,
) -> Optional[dict[str, int]]:
    if diagnostics is False:
        return None
    if diagnostics is True:
        return ObservedOptionsPhysicalDistributionDiagnosticsRequest().to_payload()
    if isinstance(diagnostics, ObservedOptionsPhysicalDistributionDiagnosticsRequest):
        return diagnostics.to_payload()

    raise _pricing_error(
        "physical_distribution_diagnostics",
        "must be a bool or ObservedOptionsPhysicalDistributionDiagnosticsRequest",
    )


__all__ = [
    "DEFAULT_PHYSICAL_DISTRIBUTION_PATH_COUNT",
    "DEFAULT_PHYSICAL_DISTRIBUTION_SEED",
    "ObservedOptionsPhysicalDistributionDiagnosticsNode",
    "ObservedOptionsPhysicalDistributionDiagnosticsOutput",
    "ObservedOptionsPhysicalDistributionDiagnosticsReport",
    "ObservedOptionsPhysicalDistributionDiagnosticsRequest",
    "physical_distribution_diagnostics_request_payload",
]
