from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from decimal import Decimal
from typing import Final

import pandas as pd

from examples._shared import (
    PersistedExampleModel,
    SYNTHETIC_EURUSD_TICK_DATA_PATH,
    load_or_fit_persisted_example_model,
)
from janiform.calibration.fit.pricing.surface import JaniformVanillaOptionSurface
from janiform.calibration.fit.state import CurrentStateEstimationRequest
from janiform.calibration.market_data import CalibrationTickDataColumnMapping
from janiform.pricing import VanillaOptionPayoffType


PRICE_COLUMN: Final[str] = "close"
INCREMENT_COLUMN: Final[str] = "increments"
DEFAULT_OBSERVED_STATE_ROW_COUNT: Final[int] = 256
DEFAULT_PIP_SIZE: Final[float] = 0.0001
DEFAULT_STRIKE_OFFSETS_IN_PIPS: Final[tuple[int, ...]] = (-50, -25, -10, 0, 10, 25, 50)
DEFAULT_MATURITY_INTERVALS_HOURS: Final[tuple[float, ...]] = (1.0, 2.0, 4.0, 8.0)
DEFAULT_DISCOUNT_RATE: Final[float] = 0.02
DEFAULT_CARRY_RATE: Final[float] = 0.01
DEFAULT_CALIBRATION_ROW_COUNT: Final[int] = 5_000


@dataclass(frozen=True, slots=True)
class SyntheticVanillaOptionSurfaceContext:
    persisted_model: PersistedExampleModel
    valuation_time_utc: datetime
    spot: float
    observed_data: CurrentStateEstimationRequest
    strikes: tuple[float, ...]
    maturity_intervals_hours: tuple[float, ...]
    discount_rate: float
    carry_rate: float
    valuation_row_offset_from_end: int
    observed_state_row_count: int
    calibration_row_count: int

    def build_surface(
        self,
        *,
        payoff_type: VanillaOptionPayoffType,
    ) -> JaniformVanillaOptionSurface:
        return JaniformVanillaOptionSurface(
            spot=self.spot,
            strikes=self.strikes,
            expiry_timestamps_utc=tuple(
                self.valuation_time_utc + timedelta(hours=maturity_interval)
                for maturity_interval in self.maturity_intervals_hours
            ),
            payoff_type=payoff_type,
            risk_free_rate=self.discount_rate,
            dividend_yield=self.carry_rate,
        )


def load_synthetic_eurusd_surface_context(
    *,
    force_refit: bool = False,
    valuation_row_offset_from_end: int = 0,
    calibration_row_count: int = DEFAULT_CALIBRATION_ROW_COUNT,
    observed_state_row_count: int = DEFAULT_OBSERVED_STATE_ROW_COUNT,
    pip_size: float = DEFAULT_PIP_SIZE,
    strike_offsets_in_pips: tuple[int, ...] = DEFAULT_STRIKE_OFFSETS_IN_PIPS,
    maturity_intervals_hours: tuple[float, ...] = DEFAULT_MATURITY_INTERVALS_HOURS,
    discount_rate: float = DEFAULT_DISCOUNT_RATE,
    carry_rate: float = DEFAULT_CARRY_RATE,
) -> SyntheticVanillaOptionSurfaceContext:
    underlying_df: pd.DataFrame = pd.read_parquet(SYNTHETIC_EURUSD_TICK_DATA_PATH)

    valuation_row_position = valuation_row_position_from_end(
        underlying_df,
        valuation_row_offset_from_end=valuation_row_offset_from_end,
    )

    valuation_time_utc = timestamp_at_row_position(
        underlying_df,
        row_position=valuation_row_position,
    )

    spot = spot_at_row_position(
        underlying_df,
        row_position=valuation_row_position,
    )

    observed_data = build_observed_state_request(
        underlying_df,
        row_position=valuation_row_position,
        observed_price=spot,
        observed_state_row_count=observed_state_row_count,
    )

    persisted_model = load_or_fit_persisted_example_model(
        tick_data_path=SYNTHETIC_EURUSD_TICK_DATA_PATH,
        tail_row_count=calibration_row_count,
        force_refit=force_refit,
        silent=True,
    )

    return SyntheticVanillaOptionSurfaceContext(
        persisted_model=persisted_model,
        valuation_time_utc=valuation_time_utc,
        spot=spot,
        observed_data=observed_data,
        strikes=fx_strikes_from_spot(
            spot,
            pip_size=pip_size,
            strike_offsets_in_pips=strike_offsets_in_pips,
        ),
        maturity_intervals_hours=maturity_intervals_hours,
        discount_rate=discount_rate,
        carry_rate=carry_rate,
        valuation_row_offset_from_end=valuation_row_offset_from_end,
        observed_state_row_count=observed_state_row_count,
        calibration_row_count=calibration_row_count,
    )


def valuation_row_position_from_end(
    underlying_df: pd.DataFrame,
    *,
    valuation_row_offset_from_end: int,
) -> int:
    if valuation_row_offset_from_end < 0:
        raise ValueError("valuation_row_offset_from_end must be non-negative")
    if len(underlying_df.index) == 0:
        raise ValueError("underlying_df must contain at least one row")

    row_position = len(underlying_df.index) - 1 - valuation_row_offset_from_end
    if row_position < 0:
        raise ValueError(
            "valuation_row_offset_from_end exceeds the number of available underlying rows"
        )

    return row_position


def timestamp_at_row_position(
    underlying_df: pd.DataFrame,
    *,
    row_position: int,
) -> datetime:
    raw_timestamp = underlying_df.index[row_position]
    if not isinstance(raw_timestamp, pd.Timestamp):
        raise TypeError("expected the underlying parquet index to contain pandas Timestamp values")

    if raw_timestamp.tzinfo is None:
        return raw_timestamp.tz_localize(UTC).to_pydatetime()

    return raw_timestamp.tz_convert(UTC).to_pydatetime()


def spot_at_row_position(
    underlying_df: pd.DataFrame,
    *,
    row_position: int,
) -> float:
    raw_price = underlying_df[PRICE_COLUMN].iloc[row_position]
    if isinstance(raw_price, Decimal):
        return float(raw_price)

    return float(raw_price)


def build_observed_state_request(
    underlying_df: pd.DataFrame,
    *,
    row_position: int,
    observed_price: float,
    observed_state_row_count: int = DEFAULT_OBSERVED_STATE_ROW_COUNT,
) -> CurrentStateEstimationRequest:
    observed_df: pd.DataFrame = (
        underlying_df.iloc[: row_position + 1]
        .tail(observed_state_row_count)
        .reset_index(names="timestamp_utc")
        .copy()
    )
    observed_df["timestamp_utc"] = pd.to_datetime(observed_df["timestamp_utc"], utc=True)
    observed_df["trade_price"] = observed_df[PRICE_COLUMN].astype(float)
    observed_df = observed_df[observed_df[INCREMENT_COLUMN].ne(0.0)].copy()
    observed_df["trade_sign"] = observed_df[INCREMENT_COLUMN].map(
        lambda increment: "buy" if increment > 0.0 else "sell"
    )

    return CurrentStateEstimationRequest(
        observed_tick_data=observed_df[["timestamp_utc", "trade_price", "trade_sign"]],
        tick_data_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        observed_price=observed_price,
    )


def fx_strikes_from_spot(
    spot: float,
    *,
    pip_size: float = DEFAULT_PIP_SIZE,
    strike_offsets_in_pips: tuple[int, ...] = DEFAULT_STRIKE_OFFSETS_IN_PIPS,
) -> tuple[float, ...]:
    return tuple(spot + pip_size * pip_offset for pip_offset in strike_offsets_in_pips)
