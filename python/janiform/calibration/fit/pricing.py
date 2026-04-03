from __future__ import annotations

from collections.abc import Callable, Sequence
from dataclasses import dataclass
from typing import Any, Optional

from ...contracts.base import (
    require_optional_boolean_value,
    require_optional_finite_number,
    require_optional_mapping_value,
    require_optional_string_value,
    require_str_enum_value,
)
from ...contracts import (
    ContractModel,
    require_finite_number,
    require_finite_scalar,
    require_mapping,
    require_non_negative_finite_scalar,
    require_positive_finite_scalar,
    require_sequence,
)
from ...pricing import VanillaOptionPayoffType
from .._errors import _calibration_error


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionBatch(ContractModel):
    spot: float
    strikes: tuple[float, ...]
    maturity_intervals: tuple[float, ...]
    payoff_types: tuple[VanillaOptionPayoffType, ...]
    risk_free_rates: tuple[float, ...]
    dividend_yields: tuple[float, ...]

    def __post_init__(self) -> None:
        require_positive_finite_scalar(self.spot, "spot", _calibration_error)
        node_count = len(self.strikes)
        if node_count == 0:
            raise _calibration_error("strikes", "must contain at least one option node")

        _validate_aligned_node_count(self.maturity_intervals, node_count, field_path="maturity_intervals")
        _validate_aligned_node_count(self.payoff_types, node_count, field_path="payoff_types")
        _validate_aligned_node_count(self.risk_free_rates, node_count, field_path="risk_free_rates")
        _validate_aligned_node_count(self.dividend_yields, node_count, field_path="dividend_yields")

        for node_index, strike in enumerate(self.strikes):
            require_positive_finite_scalar(strike, f"strikes[{node_index}]", _calibration_error)
        for node_index, maturity_interval in enumerate(self.maturity_intervals):
            require_positive_finite_scalar(
                maturity_interval,
                f"maturity_intervals[{node_index}]",
                _calibration_error,
            )
        for node_index, risk_free_rate in enumerate(self.risk_free_rates):
            require_finite_scalar(
                risk_free_rate,
                f"risk_free_rates[{node_index}]",
                _calibration_error,
            )
        for node_index, dividend_yield in enumerate(self.dividend_yields):
            require_finite_scalar(
                dividend_yield,
                f"dividend_yields[{node_index}]",
                _calibration_error,
            )

    @property
    def node_count(self) -> int:
        return len(self.strikes)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "batch",
    ) -> JaniformVanillaOptionBatch:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return JaniformVanillaOptionBatch(
            spot=require_positive_finite_scalar(raw_payload.get("spot"), f"{field_path}.spot", _calibration_error),
            strikes=_tuple_of_positive_floats(
                raw_payload,
                field_name="strikes",
                field_path=f"{field_path}.strikes",
            ),
            maturity_intervals=_tuple_of_positive_floats(
                raw_payload,
                field_name="maturity_intervals",
                field_path=f"{field_path}.maturity_intervals",
            ),
            payoff_types=_tuple_of_payoff_types(
                raw_payload,
                field_name="payoff_types",
                field_path=f"{field_path}.payoff_types",
            ),
            risk_free_rates=_tuple_of_finite_floats(
                raw_payload,
                field_name="risk_free_rates",
                field_path=f"{field_path}.risk_free_rates",
            ),
            dividend_yields=_tuple_of_finite_floats(
                raw_payload,
                field_name="dividend_yields",
                field_path=f"{field_path}.dividend_yields",
            ),
        )


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionBatchPricingRunDiagnostics(ContractModel):
    transform: tuple[Optional[dict[str, Any]], ...]
    integration_quality: tuple[Optional[dict[str, Any]], ...]
    anchored_recursion: tuple[Optional[dict[str, Any]], ...]

    def __post_init__(self) -> None:
        node_count = len(self.transform)
        _validate_aligned_node_count(
            self.integration_quality,
            node_count,
            field_path="integration_quality",
        )
        _validate_aligned_node_count(
            self.anchored_recursion,
            node_count,
            field_path="anchored_recursion",
        )

    @property
    def node_count(self) -> int:
        return len(self.transform)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> JaniformVanillaOptionBatchPricingRunDiagnostics:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return JaniformVanillaOptionBatchPricingRunDiagnostics(
            transform=_tuple_of_optional_mappings(
                raw_payload,
                field_name="transform",
                field_path=f"{field_path}.transform",
            ),
            integration_quality=_tuple_of_optional_mappings(
                raw_payload,
                field_name="integration_quality",
                field_path=f"{field_path}.integration_quality",
            ),
            anchored_recursion=_tuple_of_optional_mappings(
                raw_payload,
                field_name="anchored_recursion",
                field_path=f"{field_path}.anchored_recursion",
            ),
        )


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionBatchPricingCalibrationHooks(ContractModel):
    option_prices: tuple[float, ...]
    log_moneyness: tuple[float, ...]
    forward_moneyness: tuple[float, ...]
    maturity_intervals: tuple[float, ...]
    transform_status: tuple[Optional[str], ...]
    integration_numeric_quality_is_poor: tuple[Optional[bool], ...]
    integration_convergence_check_passed: tuple[Optional[bool], ...]
    integration_truncation_check_passed: tuple[Optional[bool], ...]

    def __post_init__(self) -> None:
        node_count = len(self.option_prices)
        _validate_aligned_node_count(self.log_moneyness, node_count, field_path="log_moneyness")
        _validate_aligned_node_count(
            self.forward_moneyness,
            node_count,
            field_path="forward_moneyness",
        )
        _validate_aligned_node_count(
            self.maturity_intervals,
            node_count,
            field_path="maturity_intervals",
        )
        _validate_aligned_node_count(
            self.transform_status,
            node_count,
            field_path="transform_status",
        )
        _validate_aligned_node_count(
            self.integration_numeric_quality_is_poor,
            node_count,
            field_path="integration_numeric_quality_is_poor",
        )
        _validate_aligned_node_count(
            self.integration_convergence_check_passed,
            node_count,
            field_path="integration_convergence_check_passed",
        )
        _validate_aligned_node_count(
            self.integration_truncation_check_passed,
            node_count,
            field_path="integration_truncation_check_passed",
        )

    @property
    def node_count(self) -> int:
        return len(self.option_prices)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> JaniformVanillaOptionBatchPricingCalibrationHooks:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return JaniformVanillaOptionBatchPricingCalibrationHooks(
            option_prices=_tuple_of_non_negative_floats(
                raw_payload,
                field_name="option_prices",
                field_path=f"{field_path}.option_prices",
            ),
            log_moneyness=_tuple_of_finite_floats(
                raw_payload,
                field_name="log_moneyness",
                field_path=f"{field_path}.log_moneyness",
            ),
            forward_moneyness=_tuple_of_positive_floats(
                raw_payload,
                field_name="forward_moneyness",
                field_path=f"{field_path}.forward_moneyness",
            ),
            maturity_intervals=_tuple_of_positive_floats(
                raw_payload,
                field_name="maturity_intervals",
                field_path=f"{field_path}.maturity_intervals",
            ),
            transform_status=_tuple_of_optional_strings(
                raw_payload,
                field_name="transform_status",
                field_path=f"{field_path}.transform_status",
            ),
            integration_numeric_quality_is_poor=_tuple_of_optional_booleans(
                raw_payload,
                field_name="integration_numeric_quality_is_poor",
                field_path=f"{field_path}.integration_numeric_quality_is_poor",
            ),
            integration_convergence_check_passed=_tuple_of_optional_booleans(
                raw_payload,
                field_name="integration_convergence_check_passed",
                field_path=f"{field_path}.integration_convergence_check_passed",
            ),
            integration_truncation_check_passed=_tuple_of_optional_booleans(
                raw_payload,
                field_name="integration_truncation_check_passed",
                field_path=f"{field_path}.integration_truncation_check_passed",
            ),
        )


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionBatchPricingResult(ContractModel):
    spot: float
    strikes: tuple[float, ...]
    maturity_intervals: tuple[float, ...]
    payoff_types: tuple[VanillaOptionPayoffType, ...]
    risk_free_rates: tuple[float, ...]
    dividend_yields: tuple[float, ...]
    prices: tuple[float, ...]
    forward_prices: tuple[float, ...]
    discount_factors: tuple[float, ...]
    boundary_state_norms: tuple[float, ...]
    run_diagnostics: Optional[JaniformVanillaOptionBatchPricingRunDiagnostics] = None
    calibration_hooks: Optional[JaniformVanillaOptionBatchPricingCalibrationHooks] = None

    def __post_init__(self) -> None:
        require_positive_finite_scalar(self.spot, "spot", _calibration_error)

        node_count = len(self.strikes)

        _validate_aligned_node_count(self.maturity_intervals, node_count, field_path="maturity_intervals")
        _validate_aligned_node_count(self.payoff_types, node_count, field_path="payoff_types")
        _validate_aligned_node_count(self.risk_free_rates, node_count, field_path="risk_free_rates")
        _validate_aligned_node_count(self.dividend_yields, node_count, field_path="dividend_yields")
        _validate_aligned_node_count(self.prices, node_count, field_path="prices")
        _validate_aligned_node_count(self.forward_prices, node_count, field_path="forward_prices")
        _validate_aligned_node_count(
            self.discount_factors,
            node_count,
            field_path="discount_factors",
        )
        _validate_aligned_node_count(
            self.boundary_state_norms,
            node_count,
            field_path="boundary_state_norms",
        )

        if self.run_diagnostics is not None and self.run_diagnostics.node_count != node_count:
            raise _calibration_error(
                "run_diagnostics",
                f"must contain {node_count} aligned node values",
            )

        if self.calibration_hooks is not None and self.calibration_hooks.node_count != node_count:
            raise _calibration_error(
                "calibration_hooks",
                f"must contain {node_count} aligned node values",
            )

    @property
    def node_count(self) -> int:
        return len(self.strikes)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "batch_pricing_result",
    ) -> JaniformVanillaOptionBatchPricingResult:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        response = require_mapping(
            raw_payload,
            "response",
            f"{field_path}.response",
            _calibration_error,
        )

        return JaniformVanillaOptionBatchPricingResult(
            spot=require_positive_finite_scalar(
                response.get("spot"),
                f"{field_path}.response.spot",
                _calibration_error,
            ),
            strikes=_tuple_of_positive_floats(
                response,
                field_name="strikes",
                field_path=f"{field_path}.response.strikes",
            ),
            maturity_intervals=_tuple_of_positive_floats(
                response,
                field_name="maturity_intervals",
                field_path=f"{field_path}.response.maturity_intervals",
            ),
            payoff_types=_tuple_of_payoff_types(
                response,
                field_name="payoff_types",
                field_path=f"{field_path}.response.payoff_types",
            ),
            risk_free_rates=_tuple_of_finite_floats(
                response,
                field_name="risk_free_rates",
                field_path=f"{field_path}.response.risk_free_rates",
            ),
            dividend_yields=_tuple_of_finite_floats(
                response,
                field_name="dividend_yields",
                field_path=f"{field_path}.response.dividend_yields",
            ),
            prices=_tuple_of_non_negative_floats(
                response,
                field_name="prices",
                field_path=f"{field_path}.response.prices",
            ),
            forward_prices=_tuple_of_positive_floats(
                response,
                field_name="forward_prices",
                field_path=f"{field_path}.response.forward_prices",
            ),
            discount_factors=_tuple_of_positive_floats(
                response,
                field_name="discount_factors",
                field_path=f"{field_path}.response.discount_factors",
            ),
            boundary_state_norms=_tuple_of_non_negative_floats(
                response,
                field_name="boundary_state_norms",
                field_path=f"{field_path}.response.boundary_state_norms",
            ),
            run_diagnostics=(
                JaniformVanillaOptionBatchPricingRunDiagnostics.from_mapping(
                    raw_payload["run_diagnostics"],
                    field_path=f"{field_path}.run_diagnostics",
                )
                if "run_diagnostics" in raw_payload and raw_payload["run_diagnostics"] is not None
                else None
            ),
            calibration_hooks=(
                JaniformVanillaOptionBatchPricingCalibrationHooks.from_mapping(
                    raw_payload["calibration_hooks"],
                    field_path=f"{field_path}.calibration_hooks",
                )
                if "calibration_hooks" in raw_payload and raw_payload["calibration_hooks"] is not None
                else None
            ),
        )


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionSurface(ContractModel):
    spot: float
    strikes: tuple[float, ...]
    maturity_intervals: tuple[float, ...]
    payoff_type: VanillaOptionPayoffType = VanillaOptionPayoffType.CALL
    risk_free_rate: float = 0.0
    dividend_yield: float = 0.0

    def __post_init__(self) -> None:
        require_positive_finite_scalar(self.spot, "spot", _calibration_error)

        if len(self.strikes) == 0:
            raise _calibration_error("strikes", "must contain at least one strike")

        if len(self.maturity_intervals) == 0:
            raise _calibration_error(
                "maturity_intervals",
                "must contain at least one maturity interval",
            )

        for strike_index, strike in enumerate(self.strikes):
            require_positive_finite_scalar(strike, f"strikes[{strike_index}]", _calibration_error)

        for maturity_index, maturity_interval in enumerate(self.maturity_intervals):
            require_positive_finite_scalar(
                maturity_interval,
                f"maturity_intervals[{maturity_index}]",
                _calibration_error,
            )

        require_finite_scalar(self.risk_free_rate, "risk_free_rate", _calibration_error)
        require_finite_scalar(self.dividend_yield, "dividend_yield", _calibration_error)

    @property
    def shape(self) -> tuple[int, int]:
        return len(self.maturity_intervals), len(self.strikes)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "surface",
    ) -> JaniformVanillaOptionSurface:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        payoff_type = require_str_enum_value(
            raw_payload.get("payoff_type", VanillaOptionPayoffType.CALL.value),
            f"{field_path}.payoff_type",
            _calibration_error,
            enum_type=VanillaOptionPayoffType,
            allowed_values_message="must be one of: call, put",
        )

        return JaniformVanillaOptionSurface(
            spot=require_positive_finite_scalar(raw_payload.get("spot"), f"{field_path}.spot", _calibration_error),
            strikes=_tuple_of_positive_floats(
                raw_payload,
                field_name="strikes",
                field_path=f"{field_path}.strikes",
            ),
            maturity_intervals=_tuple_of_positive_floats(
                raw_payload,
                field_name="maturity_intervals",
                field_path=f"{field_path}.maturity_intervals",
            ),
            payoff_type=payoff_type,
            risk_free_rate=require_optional_finite_number(
                raw_payload,
                "risk_free_rate",
                f"{field_path}.risk_free_rate",
                _calibration_error,
                default_value=0.0,
            ),
            dividend_yield=require_optional_finite_number(
                raw_payload,
                "dividend_yield",
                f"{field_path}.dividend_yield",
                _calibration_error,
                default_value=0.0,
            ),
        )


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionSurfacePricingRunDiagnostics(ContractModel):
    transform_grid: tuple[tuple[Optional[dict[str, Any]], ...], ...]
    integration_quality_grid: tuple[tuple[Optional[dict[str, Any]], ...], ...]
    anchored_recursion_grid: tuple[tuple[Optional[dict[str, Any]], ...], ...]

    def __post_init__(self) -> None:
        grid_shape = _grid_shape(self.transform_grid, field_path="transform_grid")

        _validate_matching_grid_shape(
            self.integration_quality_grid,
            grid_shape=grid_shape,
            field_path="integration_quality_grid",
        )
        _validate_matching_grid_shape(
            self.anchored_recursion_grid,
            grid_shape=grid_shape,
            field_path="anchored_recursion_grid",
        )

    @property
    def shape(self) -> tuple[int, int]:
        return _grid_shape(self.transform_grid, field_path="transform_grid")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> JaniformVanillaOptionSurfacePricingRunDiagnostics:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return JaniformVanillaOptionSurfacePricingRunDiagnostics(
            transform_grid=_grid_of_optional_mappings(
                raw_payload,
                field_name="transform_grid",
                field_path=f"{field_path}.transform_grid",
            ),
            integration_quality_grid=_grid_of_optional_mappings(
                raw_payload,
                field_name="integration_quality_grid",
                field_path=f"{field_path}.integration_quality_grid",
            ),
            anchored_recursion_grid=_grid_of_optional_mappings(
                raw_payload,
                field_name="anchored_recursion_grid",
                field_path=f"{field_path}.anchored_recursion_grid",
            ),
        )


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionSurfacePricingCalibrationHooks(ContractModel):
    option_price_grid: tuple[tuple[float, ...], ...]
    log_moneyness_grid: tuple[tuple[float, ...], ...]
    forward_moneyness_grid: tuple[tuple[float, ...], ...]
    maturity_interval_grid: tuple[tuple[float, ...], ...]
    transform_status_grid: tuple[tuple[Optional[str], ...], ...]
    integration_numeric_quality_is_poor_grid: tuple[tuple[Optional[bool], ...], ...]
    integration_convergence_check_passed_grid: tuple[tuple[Optional[bool], ...], ...]
    integration_truncation_check_passed_grid: tuple[tuple[Optional[bool], ...], ...]

    def __post_init__(self) -> None:
        grid_shape = _grid_shape(self.option_price_grid, field_path="option_price_grid")

        _validate_matching_grid_shape(
            self.log_moneyness_grid,
            grid_shape=grid_shape,
            field_path="log_moneyness_grid",
        )
        _validate_matching_grid_shape(
            self.forward_moneyness_grid,
            grid_shape=grid_shape,
            field_path="forward_moneyness_grid",
        )
        _validate_matching_grid_shape(
            self.maturity_interval_grid,
            grid_shape=grid_shape,
            field_path="maturity_interval_grid",
        )
        _validate_matching_grid_shape(
            self.transform_status_grid,
            grid_shape=grid_shape,
            field_path="transform_status_grid",
        )
        _validate_matching_grid_shape(
            self.integration_numeric_quality_is_poor_grid,
            grid_shape=grid_shape,
            field_path="integration_numeric_quality_is_poor_grid",
        )
        _validate_matching_grid_shape(
            self.integration_convergence_check_passed_grid,
            grid_shape=grid_shape,
            field_path="integration_convergence_check_passed_grid",
        )
        _validate_matching_grid_shape(
            self.integration_truncation_check_passed_grid,
            grid_shape=grid_shape,
            field_path="integration_truncation_check_passed_grid",
        )

    @property
    def shape(self) -> tuple[int, int]:
        return _grid_shape(self.option_price_grid, field_path="option_price_grid")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> JaniformVanillaOptionSurfacePricingCalibrationHooks:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return JaniformVanillaOptionSurfacePricingCalibrationHooks(
            option_price_grid=_grid_of_non_negative_floats(
                raw_payload,
                field_name="option_price_grid",
                field_path=f"{field_path}.option_price_grid",
            ),
            log_moneyness_grid=_grid_of_finite_floats(
                raw_payload,
                field_name="log_moneyness_grid",
                field_path=f"{field_path}.log_moneyness_grid",
            ),
            forward_moneyness_grid=_grid_of_positive_floats(
                raw_payload,
                field_name="forward_moneyness_grid",
                field_path=f"{field_path}.forward_moneyness_grid",
            ),
            maturity_interval_grid=_grid_of_positive_floats(
                raw_payload,
                field_name="maturity_interval_grid",
                field_path=f"{field_path}.maturity_interval_grid",
            ),
            transform_status_grid=_grid_of_optional_strings(
                raw_payload,
                field_name="transform_status_grid",
                field_path=f"{field_path}.transform_status_grid",
            ),
            integration_numeric_quality_is_poor_grid=_grid_of_optional_booleans(
                raw_payload,
                field_name="integration_numeric_quality_is_poor_grid",
                field_path=f"{field_path}.integration_numeric_quality_is_poor_grid",
            ),
            integration_convergence_check_passed_grid=_grid_of_optional_booleans(
                raw_payload,
                field_name="integration_convergence_check_passed_grid",
                field_path=f"{field_path}.integration_convergence_check_passed_grid",
            ),
            integration_truncation_check_passed_grid=_grid_of_optional_booleans(
                raw_payload,
                field_name="integration_truncation_check_passed_grid",
                field_path=f"{field_path}.integration_truncation_check_passed_grid",
            ),
        )


@dataclass(frozen=True, slots=True)
class JaniformVanillaOptionSurfacePricingResult(ContractModel):
    spot: float
    strikes: tuple[float, ...]
    maturity_intervals: tuple[float, ...]
    payoff_type: VanillaOptionPayoffType
    risk_free_rate: float
    dividend_yield: float
    price_grid: tuple[tuple[float, ...], ...]
    forward_price_grid: tuple[tuple[float, ...], ...]
    discount_factor_grid: tuple[tuple[float, ...], ...]
    boundary_state_norm_grid: tuple[tuple[float, ...], ...]
    run_diagnostics: Optional[JaniformVanillaOptionSurfacePricingRunDiagnostics] = None
    calibration_hooks: Optional[JaniformVanillaOptionSurfacePricingCalibrationHooks] = None

    def __post_init__(self) -> None:
        require_positive_finite_scalar(self.spot, "spot", _calibration_error)
        require_finite_scalar(self.risk_free_rate, "risk_free_rate", _calibration_error)
        require_finite_scalar(self.dividend_yield, "dividend_yield", _calibration_error)

        grid_shape = _grid_shape(self.price_grid, field_path="price_grid")

        _validate_matching_grid_shape(
            self.forward_price_grid,
            grid_shape=grid_shape,
            field_path="forward_price_grid",
        )
        _validate_matching_grid_shape(
            self.discount_factor_grid,
            grid_shape=grid_shape,
            field_path="discount_factor_grid",
        )
        _validate_matching_grid_shape(
            self.boundary_state_norm_grid,
            grid_shape=grid_shape,
            field_path="boundary_state_norm_grid",
        )

        expected_shape = (len(self.maturity_intervals), len(self.strikes))

        if grid_shape != expected_shape:
            raise _calibration_error(
                "price_grid",
                f"must have shape {expected_shape}, found {grid_shape}",
            )

        if self.run_diagnostics is not None and self.run_diagnostics.shape != expected_shape:
            raise _calibration_error(
                "run_diagnostics",
                f"must have shape {expected_shape}",
            )

        if self.calibration_hooks is not None and self.calibration_hooks.shape != expected_shape:
            raise _calibration_error(
                "calibration_hooks",
                f"must have shape {expected_shape}",
            )

    @property
    def shape(self) -> tuple[int, int]:
        return len(self.maturity_intervals), len(self.strikes)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "surface_pricing_result",
    ) -> JaniformVanillaOptionSurfacePricingResult:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        response = require_mapping(
            raw_payload,
            "response",
            f"{field_path}.response",
            _calibration_error,
        )
        payoff_type = require_str_enum_value(
            response.get("payoff_type"),
            f"{field_path}.response.payoff_type",
            _calibration_error,
            enum_type=VanillaOptionPayoffType,
            allowed_values_message="must be one of: call, put",
        )

        return JaniformVanillaOptionSurfacePricingResult(
            spot=require_positive_finite_scalar(
                response.get("spot"),
                f"{field_path}.response.spot",
                _calibration_error,
            ),
            strikes=_tuple_of_positive_floats(
                response,
                field_name="strikes",
                field_path=f"{field_path}.response.strikes",
            ),
            maturity_intervals=_tuple_of_positive_floats(
                response,
                field_name="maturity_intervals",
                field_path=f"{field_path}.response.maturity_intervals",
            ),
            payoff_type=payoff_type,
            risk_free_rate=require_finite_number(
                response,
                "risk_free_rate",
                f"{field_path}.response.risk_free_rate",
                _calibration_error,
            ),
            dividend_yield=require_finite_number(
                response,
                "dividend_yield",
                f"{field_path}.response.dividend_yield",
                _calibration_error,
            ),
            price_grid=_grid_of_non_negative_floats(
                response,
                field_name="price_grid",
                field_path=f"{field_path}.response.price_grid",
            ),
            forward_price_grid=_grid_of_positive_floats(
                response,
                field_name="forward_price_grid",
                field_path=f"{field_path}.response.forward_price_grid",
            ),
            discount_factor_grid=_grid_of_positive_floats(
                response,
                field_name="discount_factor_grid",
                field_path=f"{field_path}.response.discount_factor_grid",
            ),
            boundary_state_norm_grid=_grid_of_non_negative_floats(
                response,
                field_name="boundary_state_norm_grid",
                field_path=f"{field_path}.response.boundary_state_norm_grid",
            ),
            run_diagnostics=(
                JaniformVanillaOptionSurfacePricingRunDiagnostics.from_mapping(
                    raw_payload["run_diagnostics"],
                    field_path=f"{field_path}.run_diagnostics",
                )
                if "run_diagnostics" in raw_payload and raw_payload["run_diagnostics"] is not None
                else None
            ),
            calibration_hooks=(
                JaniformVanillaOptionSurfacePricingCalibrationHooks.from_mapping(
                    raw_payload["calibration_hooks"],
                    field_path=f"{field_path}.calibration_hooks",
                )
                if "calibration_hooks" in raw_payload and raw_payload["calibration_hooks"] is not None
                else None
            ),
        )


def _tuple_of_positive_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[float, ...]:
    values = require_sequence(raw_payload, field_name, field_path, _calibration_error)
    normalized: list[float] = []
    for value_index, value in enumerate(values):
        normalized.append(
            require_positive_finite_scalar(
                value,
                f"{field_path}[{value_index}]",
                _calibration_error,
            )
        )

    return tuple(normalized)


def _tuple_of_non_negative_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[float, ...]:
    values = require_sequence(raw_payload, field_name, field_path, _calibration_error)
    normalized: list[float] = []
    for value_index, value in enumerate(values):
        normalized.append(
            require_non_negative_finite_scalar(
                value,
                f"{field_path}[{value_index}]",
                _calibration_error,
            )
        )

    return tuple(normalized)


def _tuple_of_finite_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[float, ...]:
    values = require_sequence(raw_payload, field_name, field_path, _calibration_error)
    normalized: list[float] = []
    for value_index, value in enumerate(values):
        normalized.append(
            require_finite_scalar(
                value,
                f"{field_path}[{value_index}]",
                _calibration_error,
            )
        )

    return tuple(normalized)


def _tuple_of_payoff_types(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[VanillaOptionPayoffType, ...]:
    values = require_sequence(raw_payload, field_name, field_path, _calibration_error)
    normalized: list[VanillaOptionPayoffType] = []
    for value_index, value in enumerate(values):
        normalized.append(
            require_str_enum_value(
                value,
                f"{field_path}[{value_index}]",
                _calibration_error,
                enum_type=VanillaOptionPayoffType,
                allowed_values_message="must be one of: call, put",
            )
        )

    return tuple(normalized)


def _tuple_of_optional_mappings(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[Optional[dict[str, Any]], ...]:
    values = require_sequence(raw_payload, field_name, field_path, _calibration_error)
    normalized: list[Optional[dict[str, Any]]] = []
    for value_index, value in enumerate(values):
        normalized.append(
            require_optional_mapping_value(
                value,
                f"{field_path}[{value_index}]",
                _calibration_error,
            )
        )

    return tuple(normalized)


def _tuple_of_optional_strings(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[Optional[str], ...]:
    values = require_sequence(raw_payload, field_name, field_path, _calibration_error)
    normalized: list[Optional[str]] = []
    for value_index, value in enumerate(values):
        normalized.append(
            require_optional_string_value(
                value,
                f"{field_path}[{value_index}]",
                _calibration_error,
            )
        )

    return tuple(normalized)


def _tuple_of_optional_booleans(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[Optional[bool], ...]:
    values = require_sequence(raw_payload, field_name, field_path, _calibration_error)
    normalized: list[Optional[bool]] = []
    for value_index, value in enumerate(values):
        normalized.append(
            require_optional_boolean_value(
                value,
                f"{field_path}[{value_index}]",
                _calibration_error,
            )
        )

    return tuple(normalized)


def _grid_of_positive_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[float, ...], ...]:
    return _grid_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        scalar_parser=lambda value, location: require_positive_finite_scalar(value, location, _calibration_error),
    )


def _grid_of_non_negative_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[float, ...], ...]:
    return _grid_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        scalar_parser=lambda value, location: require_non_negative_finite_scalar(value, location, _calibration_error),
    )


def _grid_of_finite_floats(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[float, ...], ...]:
    return _grid_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        scalar_parser=lambda value, location: require_finite_scalar(
            value,
            location,
            _calibration_error,
        ),
    )


def _grid_of_optional_mappings(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[Optional[dict[str, Any]], ...], ...]:
    return _grid_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        scalar_parser=lambda value, location: require_optional_mapping_value(
            value,
            location,
            _calibration_error,
        ),
    )


def _grid_of_optional_strings(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[Optional[str], ...], ...]:
    return _grid_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        scalar_parser=lambda value, location: require_optional_string_value(
            value,
            location,
            _calibration_error,
        ),
    )


def _grid_of_optional_booleans(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
) -> tuple[tuple[Optional[bool], ...], ...]:
    return _grid_of_scalars(
        raw_payload,
        field_name=field_name,
        field_path=field_path,
        scalar_parser=lambda value, location: require_optional_boolean_value(
            value,
            location,
            _calibration_error,
        ),
    )


def _grid_of_scalars[TScalar](
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
    scalar_parser: Callable[[Any, str], TScalar],
) -> tuple[tuple[TScalar, ...], ...]:
    rows = require_sequence(raw_payload, field_name, field_path, _calibration_error)

    normalized_rows: list[tuple[TScalar, ...]] = []
    expected_column_count: Optional[int] = None

    for row_index, raw_row in enumerate(rows):
        if isinstance(raw_row, str) or not isinstance(raw_row, Sequence):
            raise _calibration_error(f"{field_path}[{row_index}]", "must be a sequence")

        normalized_row = tuple(
            scalar_parser(value, f"{field_path}[{row_index}][{column_index}]")
            for column_index, value in enumerate(raw_row)
        )

        if expected_column_count is None:
            expected_column_count = len(normalized_row)
        elif len(normalized_row) != expected_column_count:
            raise _calibration_error(
                f"{field_path}[{row_index}]",
                f"must contain {expected_column_count} aligned values",
            )

        normalized_rows.append(normalized_row)

    return tuple(normalized_rows)


def _validate_aligned_node_count(
    values: Sequence[Any],
    expected_count: int,
    *,
    field_path: str,
) -> None:
    if len(values) != expected_count:
        raise _calibration_error(
            field_path,
            f"must contain {expected_count} aligned values",
        )


def _grid_shape(
    values: tuple[tuple[Any, ...], ...],
    *,
    field_path: str,
) -> tuple[int, int]:
    row_count = len(values)
    if row_count == 0:
        return 0, 0

    column_count = len(values[0])
    for row_index, row in enumerate(values):
        if len(row) != column_count:
            raise _calibration_error(
                f"{field_path}[{row_index}]",
                f"must contain {column_count} aligned values",
            )

    return row_count, column_count


def _validate_matching_grid_shape(
    values: tuple[tuple[Any, ...], ...],
    *,
    grid_shape: tuple[int, int],
    field_path: str,
) -> None:
    observed_shape = _grid_shape(values, field_path=field_path)
    if observed_shape != grid_shape:
        raise _calibration_error(
            field_path,
            f"must have shape {grid_shape}, found {observed_shape}",
        )


__all__ = [
    "JaniformVanillaOptionBatch",
    "JaniformVanillaOptionBatchPricingCalibrationHooks",
    "JaniformVanillaOptionBatchPricingResult",
    "JaniformVanillaOptionBatchPricingRunDiagnostics",
    "JaniformVanillaOptionSurface",
    "JaniformVanillaOptionSurfacePricingCalibrationHooks",
    "JaniformVanillaOptionSurfacePricingResult",
    "JaniformVanillaOptionSurfacePricingRunDiagnostics",
]
