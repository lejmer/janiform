from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Optional

import numpy as np
import pandas as pd

from ..._pricing_node_validation import validate_option_pricing_node_payload
from ...contracts.collections import (
    grid_of_optional_finite_floats,
    grid_of_scalars,
    grid_shape,
    tuple_of_optional_finite_floats,
    tuple_of_positive_floats,
    tuple_of_scalars,
    validate_aligned_count,
    validate_matching_grid_shape,
)
from ..contracts import (
    PricingIntervalSchedule,
    AnchoredVanillaOptionPricingBoundaryInput,
    VanillaOptionImpliedVolatilityInversionStatus,
    VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType,
    VanillaOptionPricingFailureStage,
    VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingStatus,
    PRICING_ERROR_FACTORY,
    _validate_optional_implied_volatility_inversion_outcome,
    _pricing_error,
)
from ..._pricing_field_names import (
    BOUNDARY_STATE_NORM_FIELD_NAME,
    DISCOUNT_FACTOR_FIELD_NAME,
    FORWARD_PRICE_FIELD_NAME,
    IMPLIED_VOLATILITY_FIELD_NAME,
    IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME,
    MATURITY_INTERVAL_FIELD_NAME,
    MODEL_PRICE_FIELD_NAME,
    OPTION_TYPE_FIELD_NAME,
    PRICING_FAILURE_REASON_FIELD_NAME,
    PRICING_FAILURE_STAGE_FIELD_NAME,
    PRICING_STATUS_FIELD_NAME,
    STRIKE_FIELD_NAME,
)
from ..._timestamps import NANOS_PER_HOUR, hours_to_exact_nanoseconds
from ...contracts import ContractModel
from ...contracts.base import (
    require_finite_number,
    require_mapping,
    require_non_negative_integer,
    require_non_negative_finite_number,
    require_positive_finite_number,
    require_positive_finite_scalar,
    require_sequence,
)
from ...contracts.base import require_str_enum_value
from ...contracts.base import require_optional_string_value
from ...errors import JaniformValidationError
from ...hourly_factors import HourlyFactorContract


type SurfaceNodeRecord = dict[str, Any]
type SurfaceNodeField = tuple[str, tuple[Any, ...]]


@dataclass(frozen=True, slots=True)
class VanillaOptionSurfacePricingNodeTable(ContractModel):
    strikes: tuple[float, ...]
    maturity_intervals: tuple[float, ...]
    option_types: tuple[VanillaOptionPayoffType, ...]
    model_prices: tuple[Optional[float], ...]
    implied_volatilities: tuple[Optional[float], ...]
    implied_volatility_inversion_statuses: tuple[Optional[VanillaOptionImpliedVolatilityInversionStatus], ...]
    pricing_statuses: tuple[VanillaOptionPricingStatus, ...]
    pricing_failure_stages: tuple[Optional[VanillaOptionPricingFailureStage], ...]
    pricing_failure_reasons: tuple[Optional[str], ...]
    forward_prices: tuple[Optional[float], ...]
    discount_factors: tuple[Optional[float], ...]
    boundary_state_norms: tuple[Optional[float], ...]

    def __post_init__(self) -> None:
        expected_count = len(self.strikes)
        if expected_count == 0:
            raise _pricing_error("strikes", "must contain at least one priced surface node")

        aligned_node_fields: tuple[SurfaceNodeField, ...] = (
            ("maturity_intervals", self.maturity_intervals),
            ("option_types", self.option_types),
            ("model_prices", self.model_prices),
            ("implied_volatilities", self.implied_volatilities),
            (
                "implied_volatility_inversion_statuses",
                self.implied_volatility_inversion_statuses,
            ),
            ("pricing_statuses", self.pricing_statuses),
            ("pricing_failure_stages", self.pricing_failure_stages),
            ("pricing_failure_reasons", self.pricing_failure_reasons),
            ("forward_prices", self.forward_prices),
            ("discount_factors", self.discount_factors),
            ("boundary_state_norms", self.boundary_state_norms),
        )

        for field_name, values in aligned_node_fields:
            validate_aligned_count(
                values,
                expected_count,
                field_path=field_name,
                error_factory=PRICING_ERROR_FACTORY,
            )

        for node_index, strike in enumerate(self.strikes):
            require_positive_finite_scalar(
                strike,
                f"strikes[{node_index}]",
                PRICING_ERROR_FACTORY,
            )

        for node_index, maturity_interval in enumerate(self.maturity_intervals):
            require_positive_finite_scalar(
                maturity_interval,
                f"maturity_intervals[{node_index}]",
                PRICING_ERROR_FACTORY,
            )

        for node_index, option_type in enumerate(self.option_types):
            if not isinstance(option_type, VanillaOptionPayoffType):
                raise _pricing_error(
                    f"option_types[{node_index}]",
                    "must be a VanillaOptionPayoffType",
                )

        for node_index, pricing_status in enumerate(self.pricing_statuses):
            if not isinstance(pricing_status, VanillaOptionPricingStatus):
                raise _pricing_error(
                    f"pricing_statuses[{node_index}]",
                    "must be a VanillaOptionPricingStatus",
                )

            _validate_surface_node_payload(
                pricing_status=pricing_status,
                pricing_failure_stage=self.pricing_failure_stages[node_index],
                pricing_failure_reason=self.pricing_failure_reasons[node_index],
                model_price=self.model_prices[node_index],
                implied_volatility=self.implied_volatilities[node_index],
                implied_volatility_inversion_status=self.implied_volatility_inversion_statuses[
                    node_index
                ],
                forward_price=self.forward_prices[node_index],
                discount_factor=self.discount_factors[node_index],
                boundary_state_norm=self.boundary_state_norms[node_index],
                field_path=f"node_table[{node_index}]",
            )

    @property
    def node_count(self) -> int:
        return len(self.strikes)

    def to_records(self) -> tuple[SurfaceNodeRecord, ...]:
        return tuple(
            {
                STRIKE_FIELD_NAME: self.strikes[node_index],
                MATURITY_INTERVAL_FIELD_NAME: self.maturity_intervals[node_index],
                OPTION_TYPE_FIELD_NAME: self.option_types[node_index].value,
                MODEL_PRICE_FIELD_NAME: self.model_prices[node_index],
                IMPLIED_VOLATILITY_FIELD_NAME: self.implied_volatilities[node_index],
                IMPLIED_VOLATILITY_INVERSION_STATUS_FIELD_NAME: (
                    self.implied_volatility_inversion_statuses[node_index].value
                    if self.implied_volatility_inversion_statuses[node_index] is not None
                    else None
                ),
                PRICING_STATUS_FIELD_NAME: self.pricing_statuses[node_index].value,
                PRICING_FAILURE_STAGE_FIELD_NAME: (
                    self.pricing_failure_stages[node_index].value
                    if self.pricing_failure_stages[node_index] is not None
                    else None
                ),
                PRICING_FAILURE_REASON_FIELD_NAME: self.pricing_failure_reasons[node_index],
                FORWARD_PRICE_FIELD_NAME: self.forward_prices[node_index],
                DISCOUNT_FACTOR_FIELD_NAME: self.discount_factors[node_index],
                BOUNDARY_STATE_NORM_FIELD_NAME: self.boundary_state_norms[node_index],
            }
            for node_index in range(self.node_count)
        )

    def to_pandas(self) -> Any:
        return pd.DataFrame.from_records(self.to_records())

    def to_numpy(self) -> Any:
        records = self.to_records()
        if len(records) == 0:
            return np.empty((0, 0), dtype=object)

        column_names = tuple(records[0].keys())

        return np.asarray(
            [
                tuple(record[column_name] for column_name in column_names)
                for record in records
            ],
            dtype=object,
        )

    def to_parquet(self, path: str | Path) -> None:
        self.to_pandas().to_parquet(Path(path), index=False)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "node_table",
    ) -> VanillaOptionSurfacePricingNodeTable:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        return VanillaOptionSurfacePricingNodeTable(
            strikes=tuple_of_positive_floats(
                raw_payload,
                field_name="strikes",
                field_path=f"{field_path}.strikes",
                error_factory=PRICING_ERROR_FACTORY,
            ),
            maturity_intervals=tuple_of_positive_floats(
                raw_payload,
                field_name="maturity_intervals",
                field_path=f"{field_path}.maturity_intervals",
                error_factory=PRICING_ERROR_FACTORY,
            ),
            option_types=tuple_of_scalars(
                raw_payload,
                field_name="option_types",
                field_path=f"{field_path}.option_types",
                error_factory=PRICING_ERROR_FACTORY,
                scalar_parser=lambda value, location: require_str_enum_value(
                    value,
                    location,
                    PRICING_ERROR_FACTORY,
                    enum_type=VanillaOptionPayoffType,
                    allowed_values_message="must be one of: call, put",
                ),
            ),
            model_prices=tuple_of_optional_finite_floats(
                raw_payload,
                field_name="model_prices",
                field_path=f"{field_path}.model_prices",
                error_factory=PRICING_ERROR_FACTORY,
            ),
            implied_volatilities=tuple_of_optional_finite_floats(
                raw_payload,
                field_name="implied_volatilities",
                field_path=f"{field_path}.implied_volatilities",
                error_factory=PRICING_ERROR_FACTORY,
            ),
            implied_volatility_inversion_statuses=tuple_of_scalars(
                raw_payload,
                field_name="implied_volatility_inversion_statuses",
                field_path=f"{field_path}.implied_volatility_inversion_statuses",
                error_factory=PRICING_ERROR_FACTORY,
                scalar_parser=lambda value, location: _require_optional_inversion_status(
                    value,
                    field_path=location,
                ),
            ),
            pricing_statuses=tuple_of_scalars(
                raw_payload,
                field_name="pricing_statuses",
                field_path=f"{field_path}.pricing_statuses",
                error_factory=PRICING_ERROR_FACTORY,
                scalar_parser=lambda value, location: _require_pricing_status(
                    value,
                    field_path=location,
                ),
            ),
            pricing_failure_stages=tuple_of_scalars(
                raw_payload,
                field_name="pricing_failure_stages",
                field_path=f"{field_path}.pricing_failure_stages",
                error_factory=PRICING_ERROR_FACTORY,
                scalar_parser=lambda value, location: _require_optional_failure_stage(
                    value,
                    field_path=location,
                ),
            ),
            pricing_failure_reasons=tuple_of_scalars(
                raw_payload,
                field_name="pricing_failure_reasons",
                field_path=f"{field_path}.pricing_failure_reasons",
                error_factory=PRICING_ERROR_FACTORY,
                scalar_parser=lambda value, location: require_optional_string_value(
                    value,
                    location,
                    PRICING_ERROR_FACTORY,
                ),
            ),
            forward_prices=tuple_of_optional_finite_floats(
                raw_payload,
                field_name="forward_prices",
                field_path=f"{field_path}.forward_prices",
                error_factory=PRICING_ERROR_FACTORY,
            ),
            discount_factors=tuple_of_optional_finite_floats(
                raw_payload,
                field_name="discount_factors",
                field_path=f"{field_path}.discount_factors",
                error_factory=PRICING_ERROR_FACTORY,
            ),
            boundary_state_norms=tuple_of_optional_finite_floats(
                raw_payload,
                field_name="boundary_state_norms",
                field_path=f"{field_path}.boundary_state_norms",
                error_factory=PRICING_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class VanillaOptionSurfacePricingRequest(ContractModel):
    market_inputs: VanillaOptionPricingMarketInputs
    payoff: VanillaOptionPayoffConfiguration
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput
    model_parameters: VanillaOptionPricingModelParameters
    interval_delta: float

    def __post_init__(self) -> None:
        require_positive_finite_scalar(
            self.interval_delta,
            "interval_delta",
            PRICING_ERROR_FACTORY,
        )

        if not isinstance(self.market_inputs, VanillaOptionPricingMarketInputs):
            raise _pricing_error(
                "market_inputs",
                "must be a VanillaOptionPricingMarketInputs",
            )

        if not isinstance(self.payoff, VanillaOptionPayoffConfiguration):
            raise _pricing_error("payoff", "must be a VanillaOptionPayoffConfiguration")

        if not isinstance(self.boundary_input, AnchoredVanillaOptionPricingBoundaryInput):
            raise _pricing_error(
                "boundary_input",
                "must be an AnchoredVanillaOptionPricingBoundaryInput",
            )

        if not isinstance(self.model_parameters, VanillaOptionPricingModelParameters):
            raise _pricing_error(
                "model_parameters",
                "must be a VanillaOptionPricingModelParameters",
            )

        request_interval_duration_nanos = hours_to_exact_nanoseconds(self.interval_delta)
        boundary_interval_duration_nanos = hours_to_exact_nanoseconds(
            self.boundary_input.interval_delta
        )
        if request_interval_duration_nanos != boundary_interval_duration_nanos:
            raise _pricing_error(
                "interval_delta",
                (
                    "must match boundary_input.interval_delta "
                    f"({self.boundary_input.interval_delta})"
                ),
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
    ) -> VanillaOptionSurfacePricingRequest:
        if not isinstance(raw_payload, dict):
            raise _pricing_error("vanilla_option_surface_pricing_request", "must be an object")

        try:
            return VanillaOptionSurfacePricingRequest(
                market_inputs=VanillaOptionPricingMarketInputs.from_mapping(
                    require_mapping(
                        raw_payload,
                        "market_inputs",
                        "market_inputs",
                        PRICING_ERROR_FACTORY,
                    ),
                    field_path="market_inputs",
                ),
                payoff=VanillaOptionPayoffConfiguration.from_mapping(
                    require_mapping(
                        raw_payload,
                        "payoff",
                        "payoff",
                        PRICING_ERROR_FACTORY,
                    ),
                    field_path="payoff",
                ),
                boundary_input=AnchoredVanillaOptionPricingBoundaryInput.from_mapping(
                    require_mapping(
                        raw_payload,
                        "boundary_input",
                        "boundary_input",
                        PRICING_ERROR_FACTORY,
                    ),
                    field_path="boundary_input",
                ),
                model_parameters=VanillaOptionPricingModelParameters.from_mapping(
                    require_mapping(
                        raw_payload,
                        "model_parameters",
                        "model_parameters",
                        PRICING_ERROR_FACTORY,
                    ),
                    field_path="model_parameters",
                ),
                interval_delta=require_positive_finite_number(
                    raw_payload,
                    "interval_delta",
                    "interval_delta",
                    PRICING_ERROR_FACTORY,
                ),
            )
        except JaniformValidationError as e:
            raise _pricing_error(e.failure.field_path, e.failure.message) from e


@dataclass(frozen=True, slots=True)
class VanillaOptionSurfacePricingExecutionInput(ContractModel):
    request: VanillaOptionSurfacePricingRequest
    runtime_interval_schedule: PricingIntervalSchedule
    hourly_factor_contract: HourlyFactorContract
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection = field(
        default_factory=VanillaOptionPricingOptionalOutputSelection
    )
    cpu_count: Optional[int] = None

    def __post_init__(self) -> None:
        if not isinstance(self.request, VanillaOptionSurfacePricingRequest):
            raise _pricing_error(
                "request",
                "must be a VanillaOptionSurfacePricingRequest",
            )

        if not isinstance(self.runtime_interval_schedule, PricingIntervalSchedule):
            raise _pricing_error(
                "runtime_interval_schedule",
                "must be a PricingIntervalSchedule",
            )

        if not isinstance(self.hourly_factor_contract, HourlyFactorContract):
            raise _pricing_error(
                "hourly_factor_contract",
                "must be a HourlyFactorContract runtime artifact",
            )

        if not isinstance(
            self.optional_output_selection,
            VanillaOptionPricingOptionalOutputSelection,
        ):
            raise _pricing_error(
                "optional_output_selection",
                "must be VanillaOptionPricingOptionalOutputSelection",
            )

        if self.hourly_factor_contract.interval_count == 0:
            raise _pricing_error(
                "hourly_factor_contract.interval_count",
                "must contain at least one interval",
            )

        if self.cpu_count is not None:
            if isinstance(self.cpu_count, bool) or not isinstance(self.cpu_count, int):
                raise _pricing_error("cpu_count", "must be an integer or null")

            if self.cpu_count < 0:
                raise _pricing_error("cpu_count", "must be non-negative")

        if self.hourly_factor_contract.interval_duration_nanos != NANOS_PER_HOUR:
            raise _pricing_error(
                "hourly_factor_contract.interval_duration_nanos",
                (
                    "must equal the canonical hourly source interval duration "
                    "(hourly_factor_contract.interval_duration_nanos="
                    f"{self.hourly_factor_contract.interval_duration_nanos})"
                ),
            )

        if self.request.interval_delta != self.runtime_interval_schedule.first_interval_delta_hours:
            raise _pricing_error(
                "request.interval_delta",
                (
                    "must equal runtime_interval_schedule.first_interval_delta_hours "
                    f"({self.runtime_interval_schedule.first_interval_delta_hours})"
                ),
            )

        for maturity_index, maturity_interval in enumerate(
            self.request.market_inputs.maturity_intervals
        ):
            try:
                self.runtime_interval_schedule.terminal_interval_index_for_maturity_interval(
                    maturity_interval
                )
            except JaniformValidationError as error:
                raise _pricing_error(
                    f"request.market_inputs.maturity_intervals[{maturity_index}]",
                    error.failure.message,
                ) from error

    @property
    def shape(self) -> tuple[int, int]:
        return self.request.market_inputs.shape


@dataclass(frozen=True, slots=True)
class VanillaOptionSurfacePricingResponse(ContractModel):
    spot: float
    strikes: tuple[float, ...]
    maturity_intervals: tuple[float, ...]
    payoff_type: VanillaOptionPayoffType
    risk_free_rate: float
    dividend_yield: float
    price_grid: tuple[tuple[Optional[float], ...], ...]
    implied_volatility_grid: tuple[tuple[Optional[float], ...], ...]
    implied_volatility_inversion_status_grid: tuple[
        tuple[Optional[VanillaOptionImpliedVolatilityInversionStatus], ...],
        ...,
    ]
    pricing_status_grid: tuple[tuple[VanillaOptionPricingStatus, ...], ...]
    pricing_failure_stage_grid: tuple[tuple[Optional[VanillaOptionPricingFailureStage], ...], ...]
    pricing_failure_reason_grid: tuple[tuple[Optional[str], ...], ...]
    forward_price_grid: tuple[tuple[Optional[float], ...], ...]
    discount_factor_grid: tuple[tuple[Optional[float], ...], ...]
    boundary_state_norm_grid: tuple[tuple[Optional[float], ...], ...]
    node_table: VanillaOptionSurfacePricingNodeTable

    def __post_init__(self) -> None:
        require_positive_finite_scalar(self.spot, "spot", PRICING_ERROR_FACTORY)

        require_finite_number(
            {"risk_free_rate": self.risk_free_rate},
            "risk_free_rate",
            "risk_free_rate",
            PRICING_ERROR_FACTORY,
        )

        require_finite_number(
            {"dividend_yield": self.dividend_yield},
            "dividend_yield",
            "dividend_yield",
            PRICING_ERROR_FACTORY,
        )

        if not isinstance(self.payoff_type, VanillaOptionPayoffType):
            raise _pricing_error("payoff_type", "must be a VanillaOptionPayoffType")

        if not isinstance(self.node_table, VanillaOptionSurfacePricingNodeTable):
            raise _pricing_error(
                "node_table",
                "must be a VanillaOptionSurfacePricingNodeTable",
            )

        expected_shape = (len(self.maturity_intervals), len(self.strikes))
        observed_grid_shape = grid_shape(
            self.price_grid,
            field_path="price_grid",
            error_factory=PRICING_ERROR_FACTORY,
        )
        if observed_grid_shape != expected_shape:
            raise _pricing_error(
                "price_grid",
                f"must have shape {expected_shape}, found {observed_grid_shape}",
            )

        validate_matching_grid_shape(
            self.implied_volatility_grid,
            grid_shape_value=expected_shape,
            field_path="implied_volatility_grid",
            error_factory=PRICING_ERROR_FACTORY,
        )

        validate_matching_grid_shape(
            self.implied_volatility_inversion_status_grid,
            grid_shape_value=expected_shape,
            field_path="implied_volatility_inversion_status_grid",
            error_factory=PRICING_ERROR_FACTORY,
        )

        validate_matching_grid_shape(
            self.pricing_status_grid,
            grid_shape_value=expected_shape,
            field_path="pricing_status_grid",
            error_factory=PRICING_ERROR_FACTORY,
        )

        validate_matching_grid_shape(
            self.pricing_failure_stage_grid,
            grid_shape_value=expected_shape,
            field_path="pricing_failure_stage_grid",
            error_factory=PRICING_ERROR_FACTORY,
        )

        validate_matching_grid_shape(
            self.pricing_failure_reason_grid,
            grid_shape_value=expected_shape,
            field_path="pricing_failure_reason_grid",
            error_factory=PRICING_ERROR_FACTORY,
        )

        validate_matching_grid_shape(
            self.forward_price_grid,
            grid_shape_value=expected_shape,
            field_path="forward_price_grid",
            error_factory=PRICING_ERROR_FACTORY,
        )

        validate_matching_grid_shape(
            self.discount_factor_grid,
            grid_shape_value=expected_shape,
            field_path="discount_factor_grid",
            error_factory=PRICING_ERROR_FACTORY,
        )

        validate_matching_grid_shape(
            self.boundary_state_norm_grid,
            grid_shape_value=expected_shape,
            field_path="boundary_state_norm_grid",
            error_factory=PRICING_ERROR_FACTORY,
        )

        expected_node_table = _build_node_table(
            strikes=self.strikes,
            maturity_intervals=self.maturity_intervals,
            payoff_type=self.payoff_type,
            price_grid=self.price_grid,
            implied_volatility_grid=self.implied_volatility_grid,
            implied_volatility_inversion_status_grid=self.implied_volatility_inversion_status_grid,
            pricing_status_grid=self.pricing_status_grid,
            pricing_failure_stage_grid=self.pricing_failure_stage_grid,
            pricing_failure_reason_grid=self.pricing_failure_reason_grid,
            forward_price_grid=self.forward_price_grid,
            discount_factor_grid=self.discount_factor_grid,
            boundary_state_norm_grid=self.boundary_state_norm_grid,
        )
        if self.node_table != expected_node_table:
            raise _pricing_error(
                "node_table",
                "must stay aligned with the surface axes and node grids",
            )

    @property
    def shape(self) -> tuple[int, int]:
        return len(self.maturity_intervals), len(self.strikes)

    @property
    def node_count(self) -> int:
        return self.node_table.node_count

    def to_records(self) -> tuple[SurfaceNodeRecord, ...]:
        return self.node_table.to_records()

    def to_pandas(self) -> Any:
        return self.node_table.to_pandas()

    def to_numpy(self) -> Any:
        return self.node_table.to_numpy()

    def to_parquet(self, path: str | Path) -> None:
        self.node_table.to_parquet(path)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "surface_pricing_response",
    ) -> VanillaOptionSurfacePricingResponse:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        strikes = tuple_of_positive_floats(
            raw_payload,
            field_name="strikes",
            field_path=f"{field_path}.strikes",
            error_factory=PRICING_ERROR_FACTORY,
        )

        maturity_intervals = tuple_of_positive_floats(
            raw_payload,
            field_name="maturity_intervals",
            field_path=f"{field_path}.maturity_intervals",
            error_factory=PRICING_ERROR_FACTORY,
        )

        payoff_type = require_str_enum_value(
            raw_payload.get("payoff_type"),
            f"{field_path}.payoff_type",
            PRICING_ERROR_FACTORY,
            enum_type=VanillaOptionPayoffType,
            allowed_values_message="must be one of: call, put",
        )

        price_grid = grid_of_optional_finite_floats(
            raw_payload,
            field_name="price_grid",
            field_path=f"{field_path}.price_grid",
            error_factory=PRICING_ERROR_FACTORY,
        )

        implied_volatility_grid = grid_of_optional_finite_floats(
            raw_payload,
            field_name="implied_volatility_grid",
            field_path=f"{field_path}.implied_volatility_grid",
            error_factory=PRICING_ERROR_FACTORY,
        )

        implied_volatility_inversion_status_grid = grid_of_scalars(
            raw_payload,
            field_name="implied_volatility_inversion_status_grid",
            field_path=f"{field_path}.implied_volatility_inversion_status_grid",
            error_factory=PRICING_ERROR_FACTORY,
            scalar_parser=lambda value, location: _require_optional_inversion_status(
                value,
                field_path=location,
            ),
        )

        pricing_status_grid = grid_of_scalars(
            raw_payload,
            field_name="pricing_status_grid",
            field_path=f"{field_path}.pricing_status_grid",
            error_factory=PRICING_ERROR_FACTORY,
            scalar_parser=lambda value, location: _require_pricing_status(
                value,
                field_path=location,
            ),
        )

        pricing_failure_stage_grid = grid_of_scalars(
            raw_payload,
            field_name="pricing_failure_stage_grid",
            field_path=f"{field_path}.pricing_failure_stage_grid",
            error_factory=PRICING_ERROR_FACTORY,
            scalar_parser=lambda value, location: _require_optional_failure_stage(
                value,
                field_path=location,
            ),
        )

        pricing_failure_reason_grid = grid_of_scalars(
            raw_payload,
            field_name="pricing_failure_reason_grid",
            field_path=f"{field_path}.pricing_failure_reason_grid",
            error_factory=PRICING_ERROR_FACTORY,
            scalar_parser=lambda value, location: require_optional_string_value(
                value,
                location,
                PRICING_ERROR_FACTORY,
            ),
        )

        forward_price_grid = grid_of_optional_finite_floats(
            raw_payload,
            field_name="forward_price_grid",
            field_path=f"{field_path}.forward_price_grid",
            error_factory=PRICING_ERROR_FACTORY,
        )

        discount_factor_grid = grid_of_optional_finite_floats(
            raw_payload,
            field_name="discount_factor_grid",
            field_path=f"{field_path}.discount_factor_grid",
            error_factory=PRICING_ERROR_FACTORY,
        )

        boundary_state_norm_grid = grid_of_optional_finite_floats(
            raw_payload,
            field_name="boundary_state_norm_grid",
            field_path=f"{field_path}.boundary_state_norm_grid",
            error_factory=PRICING_ERROR_FACTORY,
        )

        derived_node_table = _build_node_table(
            strikes=strikes,
            maturity_intervals=maturity_intervals,
            payoff_type=payoff_type,
            price_grid=price_grid,
            implied_volatility_grid=implied_volatility_grid,
            implied_volatility_inversion_status_grid=implied_volatility_inversion_status_grid,
            pricing_status_grid=pricing_status_grid,
            pricing_failure_stage_grid=pricing_failure_stage_grid,
            pricing_failure_reason_grid=pricing_failure_reason_grid,
            forward_price_grid=forward_price_grid,
            discount_factor_grid=discount_factor_grid,
            boundary_state_norm_grid=boundary_state_norm_grid,
        )

        node_table = (
            VanillaOptionSurfacePricingNodeTable.from_mapping(
                require_mapping(
                    raw_payload,
                    "node_table",
                    f"{field_path}.node_table",
                    PRICING_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.node_table",
            )
            if "node_table" in raw_payload and raw_payload["node_table"] is not None
            else derived_node_table
        )

        return VanillaOptionSurfacePricingResponse(
            spot=require_positive_finite_number(
                raw_payload,
                "spot",
                f"{field_path}.spot",
                PRICING_ERROR_FACTORY,
            ),
            strikes=strikes,
            maturity_intervals=maturity_intervals,
            payoff_type=payoff_type,
            risk_free_rate=require_finite_number(
                raw_payload,
                "risk_free_rate",
                f"{field_path}.risk_free_rate",
                PRICING_ERROR_FACTORY,
            ),
            dividend_yield=require_finite_number(
                raw_payload,
                "dividend_yield",
                f"{field_path}.dividend_yield",
                PRICING_ERROR_FACTORY,
            ),
            price_grid=price_grid,
            implied_volatility_grid=implied_volatility_grid,
            implied_volatility_inversion_status_grid=implied_volatility_inversion_status_grid,
            pricing_status_grid=pricing_status_grid,
            pricing_failure_stage_grid=pricing_failure_stage_grid,
            pricing_failure_reason_grid=pricing_failure_reason_grid,
            forward_price_grid=forward_price_grid,
            discount_factor_grid=discount_factor_grid,
            boundary_state_norm_grid=boundary_state_norm_grid,
            node_table=node_table,
        )


@dataclass(frozen=True, slots=True)
class VanillaOptionSurfacePricingRunResult(ContractModel):
    response: VanillaOptionSurfacePricingResponse
    maturity_summaries: tuple[Optional[dict[str, Any]], ...]
    runtime_interval_schedule: PricingIntervalSchedule
    maturity_terminal_interval_indices: tuple[int, ...]

    def __post_init__(self) -> None:
        if not isinstance(self.response, VanillaOptionSurfacePricingResponse):
            raise _pricing_error(
                "response",
                "must be a VanillaOptionSurfacePricingResponse",
            )

        if not isinstance(self.runtime_interval_schedule, PricingIntervalSchedule):
            raise _pricing_error(
                "runtime_interval_schedule",
                "must be a PricingIntervalSchedule",
            )

    def to_records(self) -> tuple[SurfaceNodeRecord, ...]:
        return self.response.to_records()

    def to_pandas(self) -> Any:
        return self.response.to_pandas()

    def to_numpy(self) -> Any:
        return self.response.to_numpy()

    def to_parquet(self, path: str | Path) -> None:
        self.response.to_parquet(path)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "surface_pricing_run_result",
    ) -> VanillaOptionSurfacePricingRunResult:
        if not isinstance(raw_payload, dict):
            raise _pricing_error(field_path, "must be an object")

        raw_maturity_summaries = require_sequence(
            raw_payload,
            "maturity_summaries",
            f"{field_path}.maturity_summaries",
            PRICING_ERROR_FACTORY,
        )

        raw_maturity_terminal_interval_indices = require_sequence(
            raw_payload,
            "maturity_terminal_interval_indices",
            f"{field_path}.maturity_terminal_interval_indices",
            PRICING_ERROR_FACTORY,
        )

        return VanillaOptionSurfacePricingRunResult(
            response=VanillaOptionSurfacePricingResponse.from_mapping(
                require_mapping(
                    raw_payload,
                    "response",
                    f"{field_path}.response",
                    PRICING_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.response",
            ),
            maturity_summaries=tuple(
                (
                    _require_mapping_value(
                        raw_summary,
                        field_path=f"{field_path}.maturity_summaries[{summary_index}]",
                    )
                    if raw_summary is not None
                    else None
                )
                for summary_index, raw_summary in enumerate(raw_maturity_summaries)
            ),
            runtime_interval_schedule=PricingIntervalSchedule.from_mapping(
                require_mapping(
                    raw_payload,
                    "runtime_interval_schedule",
                    f"{field_path}.runtime_interval_schedule",
                    PRICING_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.runtime_interval_schedule",
            ),
            maturity_terminal_interval_indices=tuple(
                require_non_negative_integer(
                    {"value": raw_index},
                    "value",
                    f"{field_path}.maturity_terminal_interval_indices[{index}]",
                    PRICING_ERROR_FACTORY,
                )
                for index, raw_index in enumerate(raw_maturity_terminal_interval_indices)
            ),
        )


def _build_node_table(
    *,
    strikes: tuple[float, ...],
    maturity_intervals: tuple[float, ...],
    payoff_type: VanillaOptionPayoffType,
    price_grid: tuple[tuple[Optional[float], ...], ...],
    implied_volatility_grid: tuple[tuple[Optional[float], ...], ...],
    implied_volatility_inversion_status_grid: tuple[
        tuple[Optional[VanillaOptionImpliedVolatilityInversionStatus], ...],
        ...,
    ],
    pricing_status_grid: tuple[tuple[VanillaOptionPricingStatus, ...], ...],
    pricing_failure_stage_grid: tuple[tuple[Optional[VanillaOptionPricingFailureStage], ...], ...],
    pricing_failure_reason_grid: tuple[tuple[Optional[str], ...], ...],
    forward_price_grid: tuple[tuple[Optional[float], ...], ...],
    discount_factor_grid: tuple[tuple[Optional[float], ...], ...],
    boundary_state_norm_grid: tuple[tuple[Optional[float], ...], ...],
) -> VanillaOptionSurfacePricingNodeTable:
    node_strikes: list[float] = []
    node_maturity_intervals: list[float] = []
    node_option_types: list[VanillaOptionPayoffType] = []
    node_model_prices: list[Optional[float]] = []
    node_implied_volatilities: list[Optional[float]] = []
    node_implied_volatility_inversion_statuses: list[Optional[VanillaOptionImpliedVolatilityInversionStatus]] = []
    node_pricing_statuses: list[VanillaOptionPricingStatus] = []
    node_pricing_failure_stages: list[Optional[VanillaOptionPricingFailureStage]] = []
    node_pricing_failure_reasons: list[Optional[str]] = []
    node_forward_prices: list[Optional[float]] = []
    node_discount_factors: list[Optional[float]] = []
    node_boundary_state_norms: list[Optional[float]] = []

    for maturity_index, maturity_interval in enumerate(maturity_intervals):
        for strike_index, strike in enumerate(strikes):
            node_strikes.append(strike)
            node_maturity_intervals.append(maturity_interval)
            node_option_types.append(payoff_type)
            node_model_prices.append(price_grid[maturity_index][strike_index])
            node_implied_volatilities.append(implied_volatility_grid[maturity_index][strike_index])
            node_implied_volatility_inversion_statuses.append(
                implied_volatility_inversion_status_grid[maturity_index][strike_index]
            )
            node_pricing_statuses.append(pricing_status_grid[maturity_index][strike_index])
            node_pricing_failure_stages.append(
                pricing_failure_stage_grid[maturity_index][strike_index]
            )
            node_pricing_failure_reasons.append(
                pricing_failure_reason_grid[maturity_index][strike_index]
            )
            node_forward_prices.append(forward_price_grid[maturity_index][strike_index])
            node_discount_factors.append(discount_factor_grid[maturity_index][strike_index])
            node_boundary_state_norms.append(boundary_state_norm_grid[maturity_index][strike_index])

    return VanillaOptionSurfacePricingNodeTable(
        strikes=tuple(node_strikes),
        maturity_intervals=tuple(node_maturity_intervals),
        option_types=tuple(node_option_types),
        model_prices=tuple(node_model_prices),
        implied_volatilities=tuple(node_implied_volatilities),
        implied_volatility_inversion_statuses=tuple(node_implied_volatility_inversion_statuses),
        pricing_statuses=tuple(node_pricing_statuses),
        pricing_failure_stages=tuple(node_pricing_failure_stages),
        pricing_failure_reasons=tuple(node_pricing_failure_reasons),
        forward_prices=tuple(node_forward_prices),
        discount_factors=tuple(node_discount_factors),
        boundary_state_norms=tuple(node_boundary_state_norms),
    )


def _require_inversion_status(
    raw_value: Any,
    *,
    field_path: str,
) -> VanillaOptionImpliedVolatilityInversionStatus:
    return require_str_enum_value(
        raw_value,
        field_path,
        PRICING_ERROR_FACTORY,
        enum_type=VanillaOptionImpliedVolatilityInversionStatus,
        allowed_values_message=(
            "must be one of: succeeded, "
            "price_below_no_arbitrage_lower_bound, "
            "price_equals_finite_upper_bound, "
            "price_above_finite_upper_bound"
        ),
    )


def _require_optional_inversion_status(
    raw_value: Any,
    *,
    field_path: str,
) -> Optional[VanillaOptionImpliedVolatilityInversionStatus]:
    if raw_value is None:
        return None

    return _require_inversion_status(raw_value, field_path=field_path)


def _require_pricing_status(
    raw_value: Any,
    *,
    field_path: str,
) -> VanillaOptionPricingStatus:
    return require_str_enum_value(
        raw_value,
        field_path,
        PRICING_ERROR_FACTORY,
        enum_type=VanillaOptionPricingStatus,
        allowed_values_message="must be one of: succeeded, failed",
    )


def _require_optional_failure_stage(
    raw_value: Any,
    *,
    field_path: str,
) -> Optional[VanillaOptionPricingFailureStage]:
    if raw_value is None:
        return None

    return require_str_enum_value(
        raw_value,
        field_path,
        PRICING_ERROR_FACTORY,
        enum_type=VanillaOptionPricingFailureStage,
        allowed_values_message=(
            "must be one of: node_setup, pricing_projection, "
            "risk_neutral_characteristic_function, terminal_affine_measure_change, "
            "spectral_diagnostics, response_diagnostics, response_assembly, "
            "run_result_assembly, worker_execution"
        ),
    )


def _validate_surface_node_payload(
    *,
    pricing_status: VanillaOptionPricingStatus,
    pricing_failure_stage: Optional[VanillaOptionPricingFailureStage],
    pricing_failure_reason: Optional[str],
    model_price: Optional[float],
    implied_volatility: Optional[float],
    implied_volatility_inversion_status: Optional[VanillaOptionImpliedVolatilityInversionStatus],
    forward_price: Optional[float],
    discount_factor: Optional[float],
    boundary_state_norm: Optional[float],
    field_path: str,
) -> None:
    validate_option_pricing_node_payload(
        pricing_status=pricing_status,
        succeeded_pricing_status=VanillaOptionPricingStatus.SUCCEEDED,
        pricing_failure_stage=pricing_failure_stage,
        pricing_failure_reason=pricing_failure_reason,
        model_price=model_price,
        implied_volatility=implied_volatility,
        implied_volatility_inversion_status=implied_volatility_inversion_status,
        forward_price=forward_price,
        discount_factor=discount_factor,
        boundary_state_norm=boundary_state_norm,
        field_path=field_path,
        error_factory=_pricing_error,
        require_non_negative_finite_numeric_field=_require_non_negative_finite_numeric_field,
        require_positive_finite_numeric_field=_require_positive_finite_numeric_field,
        validate_optional_implied_volatility_inversion_outcome=(
            _validate_optional_implied_volatility_inversion_outcome_for_surface_node
        ),
    )


def _require_non_negative_finite_numeric_field(
    value: Optional[float],
    field_path: str,
) -> None:
    require_non_negative_finite_number(
        {"value": value},
        "value",
        field_path,
        PRICING_ERROR_FACTORY,
    )


def _require_positive_finite_numeric_field(
    value: Optional[float],
    field_path: str,
) -> None:
    require_positive_finite_number(
        {"value": value},
        "value",
        field_path,
        PRICING_ERROR_FACTORY,
    )


def _validate_optional_implied_volatility_inversion_outcome_for_surface_node(
    implied_volatility: Optional[float],
    implied_volatility_inversion_status: Optional[VanillaOptionImpliedVolatilityInversionStatus],
    field_path: str,
) -> None:
    _validate_optional_implied_volatility_inversion_outcome(
        implied_volatility=implied_volatility,
        implied_volatility_inversion_status=implied_volatility_inversion_status,
        field_path=field_path,
    )

def _require_mapping_value(
    raw_value: Any,
    *,
    field_path: str,
) -> dict[str, Any]:
    if not isinstance(raw_value, dict):
        raise _pricing_error(field_path, "must be an object")

    return raw_value


def _surface_interval_count_from_maturity_interval(
    maturity_interval: float,
    *,
    interval_duration_nanos: int,
    field_path: str,
) -> int:
    maturity_duration_nanos = hours_to_exact_nanoseconds(maturity_interval)
    if maturity_duration_nanos % interval_duration_nanos != 0:
        raise _pricing_error(
            field_path,
            (
                "must be an exact positive multiple of request.interval_delta "
                f"({maturity_interval} / {interval_duration_nanos / NANOS_PER_HOUR:.12g})"
            ),
        )

    interval_count = maturity_duration_nanos // interval_duration_nanos
    if interval_count <= 0:
        raise _pricing_error(
            field_path,
            "must be a positive exact multiple of request.interval_delta",
        )

    return interval_count


__all__ = [
    "VanillaOptionSurfacePricingExecutionInput",
    "VanillaOptionSurfacePricingNodeTable",
    "VanillaOptionSurfacePricingRequest",
    "VanillaOptionSurfacePricingResponse",
    "VanillaOptionSurfacePricingRunResult",
]
