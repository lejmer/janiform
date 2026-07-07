"""Shared intraday time-bucket helpers for paper figure scripts."""

from typing import Final

import numpy as np
import pandas as pd


MINUTES_PER_DAY: Final[int] = 1440


def validate_bin_minutes(bin_minutes: int) -> None:
    if bin_minutes <= 0:
        raise ValueError("--bin-minutes must be positive")

    if MINUTES_PER_DAY % bin_minutes != 0:
        raise ValueError("--bin-minutes must divide evenly into 1,440 minutes")


def intraday_bucket_indices(timestamps: pd.Series, bin_minutes: int) -> np.ndarray:
    minute_of_day = timestamps.dt.hour.to_numpy(dtype=np.float64) * 60.0
    minute_of_day += timestamps.dt.minute.to_numpy(dtype=np.float64)
    minute_of_day += timestamps.dt.second.to_numpy() / 60.0
    minute_of_day += timestamps.dt.microsecond.to_numpy() / 60_000_000.0
    minute_of_day += timestamps.dt.nanosecond.to_numpy() / 60_000_000_000.0

    return np.floor(minute_of_day / bin_minutes).astype(np.int64)


def intraday_bucket_midpoint_hours(bin_minutes: int) -> np.ndarray:
    bucket_count = MINUTES_PER_DAY // bin_minutes
    bucket_start_minutes = np.arange(bucket_count, dtype=np.float64) * bin_minutes

    return (bucket_start_minutes + bin_minutes / 2.0) / 60.0


def assign_grouped_values(target: np.ndarray, grouped_values: pd.Series) -> None:
    for bucket_index, value in grouped_values.items():
        target[int(bucket_index)] = value
