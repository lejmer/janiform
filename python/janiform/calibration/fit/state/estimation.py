from __future__ import annotations

from collections.abc import Iterable, Mapping, Sequence
from datetime import UTC, datetime
from typing import TYPE_CHECKING, Any, cast

import pandas

from ...._native_api import native_module
from ...._timestamps import datetime_to_utc_nanos
from ....contracts import require_datetime_utc, require_positive_finite_scalar, require_timezone_aware_datetime
from ....mean_reversion import MeanReversionBoundaryState
from ....simulation import SeededTickEventSide, HawkesExcitationState
from ...errors import calibration_error
from ...market_data.contracts import CalibrationTickDataColumnMapping
from .contracts import CurrentStateEstimationRequest, CurrentStateEstimationResult

if TYPE_CHECKING:
    from ..model import JaniformModel


def estimate_current_state_for_model(
    model: JaniformModel,
    *,
    state_time_utc: datetime,
    request: CurrentStateEstimationRequest,
) -> CurrentStateEstimationResult:
    require_timezone_aware_datetime(
        state_time_utc,
        field_path="state_time_utc",
        error_factory=calibration_error,
    )
    column_mapping = model.current_state_tick_data_column_mapping(
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
    observed_log_price = model.pricing_observed_log_price_from_spot(
        spot=observed_price,
    )

    native_payload = native_module().estimate_current_state_from_observed_data(
        model.hawkes_kernel_parameters().to_payload(),
        model.mean_reversion_fitted_parameters().to_payload(),
        datetime_to_utc_nanos(state_time_utc),
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
        raise calibration_error(
            "tick_data_column_mapping.price_column",
            "is required for current-state estimation",
        )
    if column_mapping.trade_sign_column is None:
        raise calibration_error(
            "tick_data_column_mapping.trade_sign_column",
            "is required for current-state estimation",
        )

    observation_count = len(raw_timestamp_values)
    if observation_count == 0:
        raise calibration_error(
            "observed_tick_data",
            "must contain at least one observation",
        )
    if len(raw_trade_price_values) != observation_count:
        raise calibration_error(
            f"observed_tick_data.{column_mapping.price_column}",
            (
                "must contain the same number of rows as "
                f"observed_tick_data.{column_mapping.timestamp_column}"
            ),
        )
    if len(raw_trade_side_values) != observation_count:
        raise calibration_error(
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
                calibration_error,
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


def _extract_tick_data_column(
    observed_tick_data: pandas.DataFrame | Mapping[str, Sequence[Any]],
    *,
    column_name: str | None,
    field_path: str,
) -> tuple[Any, ...]:
    if column_name is None:
        raise calibration_error(
            field_path,
            "requires an explicit tick-data column name",
        )

    if isinstance(observed_tick_data, pandas.DataFrame):
        if column_name not in observed_tick_data.columns:
            raise calibration_error(
                f"{field_path}.{column_name}",
                "column is required but was not found in observed_tick_data",
            )
        return tuple(cast(Any, observed_tick_data[column_name]).tolist())

    if column_name not in observed_tick_data:
        raise calibration_error(
            f"{field_path}.{column_name}",
            "column is required but was not found in observed_tick_data",
        )

    raw_column = observed_tick_data[column_name]
    if isinstance(raw_column, str | bytes) or not isinstance(raw_column, Iterable):
        raise calibration_error(
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
        except ValueError as error:
            raise calibration_error(field_path, "must be a valid ISO 8601 UTC timestamp") from error

        return require_datetime_utc(
            parsed_timestamp,
            field_path,
            calibration_error,
        ).isoformat()

    if isinstance(raw_timestamp, datetime):
        return require_datetime_utc(
            raw_timestamp,
            field_path,
            calibration_error,
        ).astimezone(UTC).isoformat()

    raise calibration_error(
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
        except ValueError as error:
            raise calibration_error(field_path, "must be one of: buy, sell") from error

    raise calibration_error(field_path, "must be a SeededTickEventSide or trade-side string")


__all__ = [
    "estimate_current_state_for_model",
]
