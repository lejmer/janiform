"""Public current-state estimation contracts for fitted Janiform models."""

from __future__ import annotations

from collections.abc import Iterable, Mapping, Sequence
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import TYPE_CHECKING, Any, Optional, cast

import pandas

from ..._native_api import native_module
from ...contracts import (
    MeanReversionBoundaryState,
    require_datetime_utc,
    require_positive_finite_scalar,
)
from ..market_data.contracts import CalibrationTickDataColumnMapping
from ...simulation import HawkesExcitationState, SeededTickEventSide
from .._errors import CALIBRATION_ERROR_FACTORY, _calibration_error

if TYPE_CHECKING:
    from .model import JaniformModel


@dataclass(frozen=True, slots=True, eq=False)
class CurrentStateEstimationRequest:
    """Observed tick data required to estimate the current fitted-model runtime state."""

    observed_tick_data: pandas.DataFrame | Mapping[str, Sequence[Any]]
    tick_data_column_mapping: Optional[CalibrationTickDataColumnMapping] = None
    observed_price: Optional[float] = None

    def __post_init__(self) -> None:
        if not isinstance(self.observed_tick_data, pandas.DataFrame) and not isinstance(
            self.observed_tick_data,
            Mapping,
        ):
            raise _calibration_error(
                "observed_tick_data",
                "must be a pandas.DataFrame or a mapping of column arrays",
            )

        if (
            self.tick_data_column_mapping is not None
            and not isinstance(
                self.tick_data_column_mapping,
                CalibrationTickDataColumnMapping,
            )
        ):
            raise _calibration_error(
                "tick_data_column_mapping",
                "must be a CalibrationTickDataColumnMapping when provided",
            )

        if self.observed_price is not None:
            require_positive_finite_scalar(
                self.observed_price,
                "observed_price",
                CALIBRATION_ERROR_FACTORY,
            )

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, CurrentStateEstimationRequest):
            return NotImplemented

        return (
            self.tick_data_column_mapping == other.tick_data_column_mapping
            and self.observed_price == other.observed_price
            and _observed_tick_data_equal(
                self.observed_tick_data,
                other.observed_tick_data,
            )
        )


@dataclass(frozen=True, slots=True)
class CurrentStateEstimationResult:
    """Current runtime state derived from a fitted model and observed signed trades."""

    hawkes_excitation_state: HawkesExcitationState
    mean_reversion_boundary_state: MeanReversionBoundaryState

    def __post_init__(self) -> None:
        if not isinstance(self.hawkes_excitation_state, HawkesExcitationState):
            raise _calibration_error(
                "hawkes_excitation_state",
                "must be a HawkesExcitationState",
            )
        if not isinstance(self.mean_reversion_boundary_state, MeanReversionBoundaryState):
            raise _calibration_error(
                "mean_reversion_boundary_state",
                "must be a MeanReversionBoundaryState",
            )


def estimate_current_state_for_model(
    model: JaniformModel,
    *,
    state_time_utc: datetime,
    request: CurrentStateEstimationRequest,
) -> CurrentStateEstimationResult:
    from . import model as fit_model_module

    fit_model_module._require_timezone_aware_datetime(
        state_time_utc,
        field_path="state_time_utc",
    )
    column_mapping = fit_model_module._current_state_tick_data_column_mapping(
        model,
        request=request,
    )
    (
        observed_timestamps_utc,
        observed_trade_prices,
        observed_trade_sides,
    ) = _extract_observed_tick_data_columns(
        request.observed_tick_data,
        column_mapping=column_mapping,
    )
    observed_price = (
        request.observed_price
        if request.observed_price is not None
        else observed_trade_prices[-1]
    )
    observed_log_price = fit_model_module._pricing_observed_log_price_from_spot(
        model,
        spot=observed_price,
    )

    native_payload = native_module().estimate_current_state_from_observed_data(
        fit_model_module._hawkes_kernel_parameters(model).to_payload(),
        fit_model_module._mean_reversion_fitted_parameters(model).to_payload(),
        fit_model_module.datetime_to_utc_nanos(state_time_utc),
        observed_log_price,
        observed_timestamps_utc,
        observed_trade_prices,
        observed_trade_sides,
    )

    return CurrentStateEstimationResult(
        hawkes_excitation_state=HawkesExcitationState.from_mapping(
            dict(native_payload["hawkes_excitation_state"]),
            field_path="hawkes_excitation_state",
        ),
        mean_reversion_boundary_state=MeanReversionBoundaryState.from_mapping(
            dict(native_payload["mean_reversion_boundary_state"]),
            field_path="mean_reversion_boundary_state",
        ),
    )


def _extract_observed_tick_data_columns(
    observed_tick_data: pandas.DataFrame | Mapping[str, Sequence[Any]],
    *,
    column_mapping: CalibrationTickDataColumnMapping,
) -> tuple[tuple[str, ...], tuple[float, ...], tuple[str, ...]]:
    raw_timestamp_values = _extract_tick_data_column(
        observed_tick_data,
        column_name=column_mapping.timestamp_column,
        field_path="observed_tick_data",
    )
    raw_trade_price_values = _extract_tick_data_column(
        observed_tick_data,
        column_name=column_mapping.price_column,
        field_path="observed_tick_data",
    )
    raw_trade_side_values = _extract_tick_data_column(
        observed_tick_data,
        column_name=column_mapping.trade_sign_column,
        field_path="observed_tick_data",
    )

    if column_mapping.price_column is None:
        raise _calibration_error(
            "tick_data_column_mapping.price_column",
            "is required for current-state estimation",
        )
    if column_mapping.trade_sign_column is None:
        raise _calibration_error(
            "tick_data_column_mapping.trade_sign_column",
            "is required for current-state estimation",
        )

    observation_count = len(raw_timestamp_values)
    if observation_count == 0:
        raise _calibration_error(
            "observed_tick_data",
            "must contain at least one observation",
        )
    if len(raw_trade_price_values) != observation_count:
        raise _calibration_error(
            f"observed_tick_data.{column_mapping.price_column}",
            (
                "must contain the same number of rows as "
                f"observed_tick_data.{column_mapping.timestamp_column}"
            ),
        )
    if len(raw_trade_side_values) != observation_count:
        raise _calibration_error(
            f"observed_tick_data.{column_mapping.trade_sign_column}",
            (
                "must contain the same number of rows as "
                f"observed_tick_data.{column_mapping.timestamp_column}"
            ),
        )

    return (
        tuple(
            _normalize_observed_timestamp_utc_string(
                raw_timestamp,
                field_path=f"observed_tick_data.{column_mapping.timestamp_column}[{observation_index}]",
            )
            for observation_index, raw_timestamp in enumerate(raw_timestamp_values)
        ),
        tuple(
            require_positive_finite_scalar(
                raw_trade_price,
                f"observed_tick_data.{column_mapping.price_column}[{observation_index}]",
                CALIBRATION_ERROR_FACTORY,
            )
            for observation_index, raw_trade_price in enumerate(raw_trade_price_values)
        ),
        tuple(
            _normalize_observed_trade_side(
                raw_trade_side,
                field_path=f"observed_tick_data.{column_mapping.trade_sign_column}[{observation_index}]",
            )
            for observation_index, raw_trade_side in enumerate(raw_trade_side_values)
        ),
    )


def _observed_tick_data_equal(
    left: pandas.DataFrame | Mapping[str, Sequence[Any]],
    right: pandas.DataFrame | Mapping[str, Sequence[Any]],
) -> bool:
    if isinstance(left, pandas.DataFrame) and isinstance(right, pandas.DataFrame):
        return left.equals(right)

    if isinstance(left, Mapping) and isinstance(right, Mapping):
        if set(left) != set(right):
            return False

        return all(tuple(left[column_name]) == tuple(right[column_name]) for column_name in left)

    return False


def _extract_tick_data_column(
    observed_tick_data: pandas.DataFrame | Mapping[str, Sequence[Any]],
    *,
    column_name: Optional[str],
    field_path: str,
) -> tuple[Any, ...]:
    if column_name is None:
        raise _calibration_error(
            field_path,
            "requires an explicit tick-data column name",
        )

    if isinstance(observed_tick_data, pandas.DataFrame):
        if column_name not in observed_tick_data.columns:
            raise _calibration_error(
                f"{field_path}.{column_name}",
                "column is required but was not found in observed_tick_data",
            )
        return tuple(cast(Any, observed_tick_data[column_name]).tolist())

    if column_name not in observed_tick_data:
        raise _calibration_error(
            f"{field_path}.{column_name}",
            "column is required but was not found in observed_tick_data",
        )

    raw_column = observed_tick_data[column_name]
    if isinstance(raw_column, str | bytes) or not isinstance(raw_column, Iterable):
        raise _calibration_error(
            f"{field_path}.{column_name}",
            "must be a sequence of column values",
        )

    return tuple(raw_column)


def _normalize_observed_timestamp_utc_string(
    raw_timestamp: Any,
    *,
    field_path: str,
) -> str:
    if isinstance(raw_timestamp, str):
        try:
            parsed_timestamp = datetime.fromisoformat(raw_timestamp.replace("Z", "+00:00"))
        except ValueError as e:
            raise _calibration_error(field_path, "must be a valid ISO 8601 UTC timestamp") from e

        return require_datetime_utc(
            parsed_timestamp,
            field_path,
            CALIBRATION_ERROR_FACTORY,
        ).isoformat()

    if isinstance(raw_timestamp, datetime):
        return require_datetime_utc(
            raw_timestamp,
            field_path,
            CALIBRATION_ERROR_FACTORY,
        ).astimezone(UTC).isoformat()

    raise _calibration_error(
        field_path,
        "must be a timezone-aware datetime or ISO 8601 UTC timestamp string",
    )


def _normalize_observed_trade_side(
    raw_trade_side: Any,
    *,
    field_path: str,
) -> str:
    if isinstance(raw_trade_side, SeededTickEventSide):
        return raw_trade_side.value

    if isinstance(raw_trade_side, str):
        try:
            return SeededTickEventSide(raw_trade_side).value
        except ValueError as e:
            raise _calibration_error(field_path, "must be one of: buy, sell") from e

    raise _calibration_error(field_path, "must be a SeededTickEventSide or trade-side string")
__all__ = [
    "CurrentStateEstimationRequest",
    "CurrentStateEstimationResult",
]
