"""Shared observed-option market-data loading helpers."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from typing import TYPE_CHECKING, Optional

import pandas

from janiform.pricing import VanillaOptionPayoffType
from janiform.calibration.market_data.contracts import CalibrationOptionSurfaceColumnMapping, CalibrationTickDataColumnMapping
from janiform.calibration.fit._runtime.payloads import current_state_tick_data_column_mapping_for_model
from janiform.calibration.fit.state import CurrentStateEstimationRequest
from .loading import (
    ResolvedOptionSnapshotColumns,
    filter_observed_tick_data,
    load_option_surface_frame,
    load_tick_data_frame,
    normalize_requested_option_type,
    observed_spot_from_tick_data,
    resolve_snapshot_valuation_time_utc,
)

if TYPE_CHECKING:
    from .....model import JaniformModel
    from ..inputs import TabularFrameInput


@dataclass(frozen=True, slots=True)
class PreparedObservedOptionMarketContext:
    tick_data_frame: pandas.DataFrame
    resolved_tick_column_mapping: CalibrationTickDataColumnMapping
    option_surface_frame: pandas.DataFrame
    resolved_option_columns: ResolvedOptionSnapshotColumns
    valuation_time_utc: datetime
    option_type: Optional[VanillaOptionPayoffType]
    filtered_tick_data: pandas.DataFrame
    spot: float


def prepare_observed_option_market_context(
    model: JaniformModel,
    *,
    tick_data: TabularFrameInput,
    option_surface: TabularFrameInput,
    valuation_time_utc: Optional[datetime],
    option_type: Optional[VanillaOptionPayoffType | str],
    option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping],
) -> PreparedObservedOptionMarketContext:
    tick_column_mapping = current_state_tick_data_column_mapping_for_model(
        model,
        request=CurrentStateEstimationRequest(observed_tick_data=pandas.DataFrame()),
    )

    tick_data_frame, resolved_tick_column_mapping = load_tick_data_frame(
        tick_data,
        tick_column_mapping=tick_column_mapping,
    )

    option_surface_frame, resolved_option_columns = load_option_surface_frame(
        option_surface,
        option_surface_column_mapping=option_surface_column_mapping,
    )

    effective_valuation_time_utc = resolve_snapshot_valuation_time_utc(
        option_surface_frame=option_surface_frame,
        quote_timestamp_column=resolved_option_columns.quote_timestamp_column,
        valuation_time_utc=valuation_time_utc,
    )

    normalized_option_type = normalize_requested_option_type(option_type)

    filtered_tick_data = filter_observed_tick_data(
        tick_data_frame=tick_data_frame,
        timestamp_column=resolved_tick_column_mapping.timestamp_column,
        valuation_time_utc=effective_valuation_time_utc,
    )

    spot = observed_spot_from_tick_data(
        filtered_tick_data,
        price_column=resolved_tick_column_mapping.price_column,
    )

    return PreparedObservedOptionMarketContext(
        tick_data_frame=tick_data_frame,
        resolved_tick_column_mapping=resolved_tick_column_mapping,
        option_surface_frame=option_surface_frame,
        resolved_option_columns=resolved_option_columns,
        valuation_time_utc=effective_valuation_time_utc,
        option_type=normalized_option_type,
        filtered_tick_data=filtered_tick_data,
        spot=spot,
    )


__all__ = [
    "PreparedObservedOptionMarketContext",
    "prepare_observed_option_market_context",
]
