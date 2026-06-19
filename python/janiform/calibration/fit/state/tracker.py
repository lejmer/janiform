from __future__ import annotations

from collections.abc import Callable, Mapping, Sequence
from datetime import datetime, timedelta
from pathlib import Path
from typing import TYPE_CHECKING, Any, Optional

import pandas

import janiform._native_api as native_api_module

from ...._timestamps import datetime_to_utc_nanos
from ...._workflow_progress import WorkflowProgressMode
from ....contracts.base import require_datetime_utc, require_positive_finite_scalar
from ....forecast import ForecastMeasure, ForecastTableFormat
from ....forecast.result import DEFAULT_FORECAST_DISTRIBUTION_NODE_COUNT
from ...errors import calibration_error
from ...market_data.contracts import CalibrationTickDataColumnMapping
from ._estimation import _extract_observed_tick_data_columns
from .contracts import (
    CurrentStateEstimationRequest,
    CurrentStateEstimationResult,
    CurrentStateTrackerSnapshot,
)

if TYPE_CHECKING:
    from ....forecast import JaniformForecast
    from ....model import JaniformModel


class JaniformStateTracker:
    """Incremental fitted-model state tracker for conditional forecasts."""

    def __init__(
        self,
        model: JaniformModel,
        *,
        tick_data_column_mapping: Optional[CalibrationTickDataColumnMapping] = None,
        initial_snapshot: Optional[CurrentStateTrackerSnapshot] = None,
    ) -> None:
        if (
            tick_data_column_mapping is not None
            and not isinstance(tick_data_column_mapping, CalibrationTickDataColumnMapping)
        ):
            raise calibration_error(
                "tick_data_column_mapping",
                "must be a CalibrationTickDataColumnMapping when provided",
            )

        if initial_snapshot is not None and not isinstance(
            initial_snapshot,
            CurrentStateTrackerSnapshot,
        ):
            raise calibration_error(
                "initial_snapshot",
                "must be a CurrentStateTrackerSnapshot when provided",
            )

        self._model = model
        self._tick_data_column_mapping = tick_data_column_mapping
        self._snapshot = initial_snapshot

    @property
    def snapshot(self) -> Optional[CurrentStateTrackerSnapshot]:
        return self._snapshot

    @property
    def current_state(self) -> CurrentStateEstimationResult:
        return self._require_snapshot().state

    @property
    def current_price(self) -> float:
        return self._require_snapshot().observed_price

    @property
    def state_time_utc(self) -> datetime:
        return self._require_snapshot().state_time_utc

    def update(
        self,
        observed_tick_data: Optional[pandas.DataFrame | Mapping[str, Sequence[Any]]] = None,
        *,
        state_time_utc: Optional[datetime] = None,
        observed_price: Optional[float] = None,
        tick_data_column_mapping: Optional[CalibrationTickDataColumnMapping] = None,
    ) -> CurrentStateTrackerSnapshot:
        if observed_price is not None:
            require_positive_finite_scalar(
                observed_price,
                "observed_price",
                calibration_error,
            )

        (
            observed_timestamps_utc,
            observed_trade_prices,
            observed_trade_sides,
        ) = self._extract_update_observations(
            observed_tick_data,
            tick_data_column_mapping=tick_data_column_mapping,
        )

        resolved_state_time_utc = self._resolve_state_time_utc(
            state_time_utc,
            observed_timestamps_utc,
        )

        from .._runtime.option_pricing.support import model_reference_price
        from .._runtime.payloads import (
            hawkes_kernel_parameters_for_model,
            long_variance_parameters_for_model,
            mean_reversion_fitted_parameters_for_model,
        )

        native_payload = native_api_module.native_module().update_current_state_tracker(
            hawkes_kernel_parameters_for_model(self._model).to_payload(),
            long_variance_parameters_for_model(self._model).to_payload(),
            mean_reversion_fitted_parameters_for_model(self._model).to_payload(),
            model_reference_price(self._model),
            None if self._snapshot is None else self._snapshot.to_payload(),
            datetime_to_utc_nanos(resolved_state_time_utc),
            observed_price,
            observed_timestamps_utc,
            observed_trade_prices,
            observed_trade_sides,
        )

        self._snapshot = CurrentStateTrackerSnapshot.from_mapping(native_payload)

        return self._snapshot

    def forecast(
        self,
        *,
        interval: Optional[str | timedelta] = None,
        horizon_interval_count: Optional[int] = None,
        end_date_utc: Optional[datetime] = None,
        measure: ForecastMeasure | str = ForecastMeasure.PHYSICAL,
        risk_free_rate: float = 0.0,
        dividend_yield: float = 0.0,
        output_path: Optional[str | Path] = None,
        table_format: ForecastTableFormat | str = ForecastTableFormat.PARQUET,
        distribution_node_count: int = DEFAULT_FORECAST_DISTRIBUTION_NODE_COUNT,
        distribution_log_return_lower: Optional[float] = None,
        distribution_log_return_upper: Optional[float] = None,
        cpu_count: Optional[int] = None,
        treat_forecast_warnings_as_errors: bool = False,
        progress_callback: Optional[Callable[[Mapping[str, Any]], None]] = None,
        progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
    ) -> JaniformForecast:
        snapshot = self._require_snapshot()

        return self._model.forecast(
            start_date_utc=snapshot.state_time_utc,
            state=snapshot.state,
            interval=interval,
            horizon_interval_count=horizon_interval_count,
            end_date_utc=end_date_utc,
            measure=measure,
            risk_free_rate=risk_free_rate,
            dividend_yield=dividend_yield,
            output_path=output_path,
            table_format=table_format,
            distribution_node_count=distribution_node_count,
            distribution_log_return_lower=distribution_log_return_lower,
            distribution_log_return_upper=distribution_log_return_upper,
            cpu_count=cpu_count,
            treat_forecast_warnings_as_errors=treat_forecast_warnings_as_errors,
            progress_callback=progress_callback,
            progress_mode=progress_mode,
        )

    def _extract_update_observations(
        self,
        observed_tick_data: Optional[pandas.DataFrame | Mapping[str, Sequence[Any]]],
        *,
        tick_data_column_mapping: Optional[CalibrationTickDataColumnMapping],
    ) -> tuple[tuple[str, ...], tuple[float, ...], tuple[str, ...]]:
        if observed_tick_data is None:
            return (), (), ()

        from .._runtime.payloads import current_state_tick_data_column_mapping_for_model

        request = CurrentStateEstimationRequest(
            observed_tick_data=observed_tick_data,
            tick_data_column_mapping=tick_data_column_mapping or self._tick_data_column_mapping,
        )

        column_mapping = current_state_tick_data_column_mapping_for_model(
            self._model,
            request=request,
        )

        return _extract_observed_tick_data_columns(
            observed_tick_data,
            column_mapping=column_mapping,
            allow_empty=True,
        )

    @staticmethod
    def _resolve_state_time_utc(
        state_time_utc: Optional[datetime],
        observed_timestamps_utc: tuple[str, ...],
    ) -> datetime:
        if state_time_utc is not None:
            return require_datetime_utc(
                state_time_utc,
                "state_time_utc",
                calibration_error,
            )

        if observed_timestamps_utc:
            return require_datetime_utc(
                datetime.fromisoformat(observed_timestamps_utc[-1]),
                "observed_tick_data.timestamp[-1]",
                calibration_error,
            )

        raise calibration_error(
            "state_time_utc",
            "is required when tracker update has no observed ticks",
        )

    def _require_snapshot(self) -> CurrentStateTrackerSnapshot:
        if self._snapshot is None:
            raise calibration_error(
                "state_tracker",
                "must be updated before current state or forecast is requested",
            )

        return self._snapshot


__all__ = [
    "JaniformStateTracker",
]
