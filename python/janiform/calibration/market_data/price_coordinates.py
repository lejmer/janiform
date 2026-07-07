from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from math import isfinite
from typing import Any, Optional

from ...contracts import (
    ContractModel,
    require_finite_number,
    require_mapping,
    require_non_empty_string,
    require_non_negative_integer,
    require_positive_finite_number,
)
from ..errors import calibration_error
from .contracts import CalibrationTickDataColumnMapping, CalibrationTickPriceInputPolicy


DEFAULT_OPTION_FORWARD_PARITY_WARNING_RELATIVE_THRESHOLD = 0.0025


class PriceCoordinatePolicy(StrEnum):
    AUTO = "auto"
    SINGLE = "single"
    CONTINUOUS_FIT_RAW_OPTION_ANCHOR = "continuous_fit_raw_option_anchor"


def coerce_price_coordinate_policy(
    price_coordinate_policy: PriceCoordinatePolicy | str,
) -> PriceCoordinatePolicy:
    if isinstance(price_coordinate_policy, PriceCoordinatePolicy):
        return price_coordinate_policy

    if isinstance(price_coordinate_policy, str):
        try:
            return PriceCoordinatePolicy(price_coordinate_policy)
        except ValueError as error:
            raise calibration_error(
                "price_coordinate_policy",
                "must be one of: auto, single, continuous_fit_raw_option_anchor",
            ) from error

    raise calibration_error(
        "price_coordinate_policy",
        "must be a PriceCoordinatePolicy or string",
    )


@dataclass(frozen=True, slots=True)
class OptionForwardParityWarningPolicy(ContractModel):
    relative_threshold: float = DEFAULT_OPTION_FORWARD_PARITY_WARNING_RELATIVE_THRESHOLD

    def __post_init__(self) -> None:
        if not isfinite(self.relative_threshold) or self.relative_threshold <= 0.0:
            raise calibration_error(
                "relative_threshold",
                "must be finite and strictly positive",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> OptionForwardParityWarningPolicy:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        return OptionForwardParityWarningPolicy(
            relative_threshold=(
                require_positive_finite_number(
                    raw_payload,
                    "relative_threshold",
                    f"{field_path}.relative_threshold",
                    calibration_error,
                )
                if "relative_threshold" in raw_payload
                and raw_payload["relative_threshold"] is not None
                else DEFAULT_OPTION_FORWARD_PARITY_WARNING_RELATIVE_THRESHOLD
            ),
        )


@dataclass(frozen=True, slots=True)
class ResolvedPriceCoordinate(ContractModel):
    coordinate_role: str
    coordinate_kind: str
    tick_data_column_mapping: CalibrationTickDataColumnMapping
    tick_price_input_policy: Optional[CalibrationTickPriceInputPolicy] = None

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> ResolvedPriceCoordinate:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        raw_tick_price_input_policy = raw_payload.get("tick_price_input_policy")
        if raw_tick_price_input_policy is not None and not isinstance(raw_tick_price_input_policy, str):
            raise calibration_error(
                f"{field_path}.tick_price_input_policy",
                "must be a string when provided",
            )

        return ResolvedPriceCoordinate(
            coordinate_role=require_non_empty_string(
                raw_payload,
                "coordinate_role",
                f"{field_path}.coordinate_role",
                calibration_error,
            ),
            coordinate_kind=require_non_empty_string(
                raw_payload,
                "coordinate_kind",
                f"{field_path}.coordinate_kind",
                calibration_error,
            ),
            tick_data_column_mapping=CalibrationTickDataColumnMapping.from_mapping(
                require_mapping(
                    raw_payload,
                    "tick_data_column_mapping",
                    f"{field_path}.tick_data_column_mapping",
                    calibration_error,
                ),
                field_path=f"{field_path}.tick_data_column_mapping",
            ),
            tick_price_input_policy=(
                CalibrationTickPriceInputPolicy(raw_tick_price_input_policy)
                if raw_tick_price_input_policy is not None
                else None
            ),
        )


@dataclass(frozen=True, slots=True)
class ResolvedRollAdjustmentEvidence(ContractModel):
    adjusted_price_column: str
    raw_price_column: str
    adjustment_column: str
    checked_row_count: int
    max_absolute_relation_error: float

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> ResolvedRollAdjustmentEvidence:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        return ResolvedRollAdjustmentEvidence(
            adjusted_price_column=require_non_empty_string(
                raw_payload,
                "adjusted_price_column",
                f"{field_path}.adjusted_price_column",
                calibration_error,
            ),
            raw_price_column=require_non_empty_string(
                raw_payload,
                "raw_price_column",
                f"{field_path}.raw_price_column",
                calibration_error,
            ),
            adjustment_column=require_non_empty_string(
                raw_payload,
                "adjustment_column",
                f"{field_path}.adjustment_column",
                calibration_error,
            ),
            checked_row_count=require_non_negative_integer(
                raw_payload,
                "checked_row_count",
                f"{field_path}.checked_row_count",
                calibration_error,
            ),
            max_absolute_relation_error=require_finite_number(
                raw_payload,
                "max_absolute_relation_error",
                f"{field_path}.max_absolute_relation_error",
                calibration_error,
            ),
        )


@dataclass(frozen=True, slots=True)
class OptionForwardParityCheck(ContractModel):
    model_forward_price: float
    observed_forward_price: Optional[float]
    absolute_delta: Optional[float]
    relative_delta: Optional[float]
    observed_bid_ask_forward_lower: Optional[float]
    observed_bid_ask_forward_upper: Optional[float]
    relative_warning_threshold: float
    paired_strike_count: int
    status: str
    warning: Optional[str] = None

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> OptionForwardParityCheck:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        return OptionForwardParityCheck(
            model_forward_price=require_finite_number(
                raw_payload,
                "model_forward_price",
                f"{field_path}.model_forward_price",
                calibration_error,
            ),
            observed_forward_price=_optional_finite_number(
                raw_payload,
                "observed_forward_price",
                f"{field_path}.observed_forward_price",
            ),
            absolute_delta=_optional_finite_number(
                raw_payload,
                "absolute_delta",
                f"{field_path}.absolute_delta",
            ),
            relative_delta=_optional_finite_number(
                raw_payload,
                "relative_delta",
                f"{field_path}.relative_delta",
            ),
            observed_bid_ask_forward_lower=_optional_finite_number(
                raw_payload,
                "observed_bid_ask_forward_lower",
                f"{field_path}.observed_bid_ask_forward_lower",
            ),
            observed_bid_ask_forward_upper=_optional_finite_number(
                raw_payload,
                "observed_bid_ask_forward_upper",
                f"{field_path}.observed_bid_ask_forward_upper",
            ),
            relative_warning_threshold=require_positive_finite_number(
                raw_payload,
                "relative_warning_threshold",
                f"{field_path}.relative_warning_threshold",
                calibration_error,
            ),
            paired_strike_count=require_non_negative_integer(
                raw_payload,
                "paired_strike_count",
                f"{field_path}.paired_strike_count",
                calibration_error,
            ),
            status=require_non_empty_string(
                raw_payload,
                "status",
                f"{field_path}.status",
                calibration_error,
            ),
            warning=(
                require_non_empty_string(
                    raw_payload,
                    "warning",
                    f"{field_path}.warning",
                    calibration_error,
                )
                if raw_payload.get("warning") is not None
                else None
            ),
        )


@dataclass(frozen=True, slots=True)
class ResolvedPriceCoordinateSet(ContractModel):
    policy: PriceCoordinatePolicy
    dynamics_fit: ResolvedPriceCoordinate
    option_valuation: ResolvedPriceCoordinate
    option_underlier_identity: Optional[str] = None
    tick_underlier_identity: Optional[str] = None
    roll_adjustment_evidence: Optional[ResolvedRollAdjustmentEvidence] = None
    parity_forward_check: Optional[OptionForwardParityCheck] = None

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> ResolvedPriceCoordinateSet:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        return ResolvedPriceCoordinateSet(
            policy=coerce_price_coordinate_policy(
                require_non_empty_string(
                    raw_payload,
                    "policy",
                    f"{field_path}.policy",
                    calibration_error,
                )
            ),
            dynamics_fit=ResolvedPriceCoordinate.from_mapping(
                require_mapping(
                    raw_payload,
                    "dynamics_fit",
                    f"{field_path}.dynamics_fit",
                    calibration_error,
                ),
                field_path=f"{field_path}.dynamics_fit",
            ),
            option_valuation=ResolvedPriceCoordinate.from_mapping(
                require_mapping(
                    raw_payload,
                    "option_valuation",
                    f"{field_path}.option_valuation",
                    calibration_error,
                ),
                field_path=f"{field_path}.option_valuation",
            ),
            option_underlier_identity=_optional_non_empty_string(
                raw_payload,
                "option_underlier_identity",
                f"{field_path}.option_underlier_identity",
            ),
            tick_underlier_identity=_optional_non_empty_string(
                raw_payload,
                "tick_underlier_identity",
                f"{field_path}.tick_underlier_identity",
            ),
            roll_adjustment_evidence=(
                ResolvedRollAdjustmentEvidence.from_mapping(
                    raw_payload["roll_adjustment_evidence"],
                    field_path=f"{field_path}.roll_adjustment_evidence",
                )
                if raw_payload.get("roll_adjustment_evidence") is not None
                else None
            ),
            parity_forward_check=(
                OptionForwardParityCheck.from_mapping(
                    raw_payload["parity_forward_check"],
                    field_path=f"{field_path}.parity_forward_check",
                )
                if raw_payload.get("parity_forward_check") is not None
                else None
            ),
        )


def _optional_finite_number(
    raw_payload: dict[str, Any],
    field_name: str,
    field_path: str,
) -> Optional[float]:
    if raw_payload.get(field_name) is None:
        return None

    return require_finite_number(
        raw_payload,
        field_name,
        field_path,
        calibration_error,
    )


def _optional_non_empty_string(
    raw_payload: dict[str, Any],
    field_name: str,
    field_path: str,
) -> Optional[str]:
    if raw_payload.get(field_name) is None:
        return None

    return require_non_empty_string(
        raw_payload,
        field_name,
        field_path,
        calibration_error,
    )


__all__ = [
    "DEFAULT_OPTION_FORWARD_PARITY_WARNING_RELATIVE_THRESHOLD",
    "OptionForwardParityCheck",
    "OptionForwardParityWarningPolicy",
    "PriceCoordinatePolicy",
    "ResolvedPriceCoordinate",
    "ResolvedPriceCoordinateSet",
    "ResolvedRollAdjustmentEvidence",
    "coerce_price_coordinate_policy",
]
