import csv
import math
import random
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any, Optional


DEFAULT_SYNTHETIC_EURUSD_OUTPUT_PATH = Path("/tmp/janiform_synthetic_eurusd_ticks.csv")
DEFAULT_SYNTHETIC_EURUSD_ROW_COUNT = 10_000
DEFAULT_SYNTHETIC_EURUSD_DURATION_MINUTES = 70.0
DEFAULT_SYNTHETIC_EURUSD_START_TIME = "2026-01-05T00:00:00+00:00"
DEFAULT_SYNTHETIC_EURUSD_START_PRICE = 1.08500
DEFAULT_SYNTHETIC_EURUSD_SEED = 17
MINIMUM_GAP_MICROSECONDS = 1_000


@dataclass(frozen=True, slots=True)
class SyntheticEurusdDataset:
    timestamps: tuple[datetime, ...]
    prices: tuple[float, ...]
    trade_signs: tuple[str, ...]
    interval_microseconds: tuple[int, ...]


def generate_synthetic_eurusd_dataset(
    *,
    row_count: int = DEFAULT_SYNTHETIC_EURUSD_ROW_COUNT,
    duration_minutes: float = DEFAULT_SYNTHETIC_EURUSD_DURATION_MINUTES,
    start_time_utc: Optional[datetime] = None,
    start_price: float = DEFAULT_SYNTHETIC_EURUSD_START_PRICE,
    seed: int = DEFAULT_SYNTHETIC_EURUSD_SEED,
) -> SyntheticEurusdDataset:
    if row_count <= 1:
        raise ValueError("row_count must be greater than 1")
    if duration_minutes <= 0.0:
        raise ValueError("duration_minutes must be strictly positive")
    if start_price <= 0.0:
        raise ValueError("start_price must be strictly positive")

    resolved_start_time_utc = start_time_utc or datetime.fromisoformat(
        DEFAULT_SYNTHETIC_EURUSD_START_TIME
    )
    if resolved_start_time_utc.tzinfo is None:
        resolved_start_time_utc = resolved_start_time_utc.replace(tzinfo=UTC)

    resolved_start_time_utc = resolved_start_time_utc.astimezone(UTC)

    total_duration_microseconds = int(duration_minutes * 60.0 * 1_000_000.0)
    required_minimum_microseconds = (row_count - 1) * MINIMUM_GAP_MICROSECONDS
    if total_duration_microseconds <= required_minimum_microseconds:
        raise ValueError(
            "duration_minutes is too small for the requested row_count while preserving "
            "strictly increasing sub-second timestamps"
        )

    randomness = random.Random(seed)
    interval_microseconds = _build_irregular_intervals_microseconds(
        randomness=randomness,
        interval_count=row_count - 1,
        total_duration_microseconds=total_duration_microseconds,
    )
    timestamps = _build_timestamps(
        start_time_utc=resolved_start_time_utc,
        interval_microseconds=interval_microseconds,
    )
    prices = _build_prices(
        randomness=randomness,
        row_count=row_count,
        start_price=start_price,
    )
    trade_signs = _build_trade_signs(
        randomness=randomness,
        prices=prices,
    )

    return SyntheticEurusdDataset(
        timestamps=tuple(timestamps),
        prices=tuple(prices),
        trade_signs=tuple(trade_signs),
        interval_microseconds=tuple(interval_microseconds),
    )


def write_synthetic_eurusd_tick_csv(
    *,
    output_path: Path,
    row_count: int = DEFAULT_SYNTHETIC_EURUSD_ROW_COUNT,
    duration_minutes: float = DEFAULT_SYNTHETIC_EURUSD_DURATION_MINUTES,
    start_time_utc: Optional[datetime] = None,
    start_price: float = DEFAULT_SYNTHETIC_EURUSD_START_PRICE,
    seed: int = DEFAULT_SYNTHETIC_EURUSD_SEED,
    timestamp_column: str = "time",
    price_column: str = "close",
    trade_sign_column: Optional[str] = "sign",
    plot_path: Optional[Path] = None,
) -> dict[str, Any]:
    dataset = generate_synthetic_eurusd_dataset(
        row_count=row_count,
        duration_minutes=duration_minutes,
        start_time_utc=start_time_utc,
        start_price=start_price,
        seed=seed,
    )

    output_path.parent.mkdir(parents=True, exist_ok=True)
    with output_path.open("w", encoding="utf-8", newline="") as csv_file:
        field_names = [timestamp_column, price_column]
        if trade_sign_column is not None:
            field_names.append(trade_sign_column)

        writer = csv.writer(csv_file)
        writer.writerow(field_names)
        for row_index, timestamp in enumerate(dataset.timestamps):
            row: list[str] = [
                timestamp.isoformat().replace("+00:00", "Z"),
                f"{dataset.prices[row_index]:.6f}",
            ]
            if trade_sign_column is not None:
                row.append(dataset.trade_signs[row_index])
            writer.writerow(row)

    observed_duration_seconds = (
        dataset.timestamps[-1] - dataset.timestamps[0]
    ).total_seconds()
    mean_gap_milliseconds = observed_duration_seconds * 1_000.0 / (len(dataset.timestamps) - 1)

    if plot_path is not None:
        write_synthetic_eurusd_generator_plot(
            plot_path=plot_path,
            timestamps=list(dataset.timestamps),
            prices=list(dataset.prices),
            interval_microseconds=list(dataset.interval_microseconds),
        )

    return {
        "output_path": str(output_path),
        "plot_path": str(plot_path) if plot_path is not None else None,
        "row_count": len(dataset.timestamps),
        "seed": seed,
        "start_time_utc": dataset.timestamps[0].isoformat(),
        "end_time_utc": dataset.timestamps[-1].isoformat(),
        "duration_minutes": round(observed_duration_seconds / 60.0, 4),
        "mean_gap_milliseconds": round(mean_gap_milliseconds, 3),
        "first_price": round(dataset.prices[0], 6),
        "last_price": round(dataset.prices[-1], 6),
        "timestamp_column": timestamp_column,
        "price_column": price_column,
        "trade_sign_column": trade_sign_column,
    }


def write_synthetic_eurusd_generator_plot(
    *,
    plot_path: Path,
    timestamps: list[datetime],
    prices: list[float],
    interval_microseconds: list[int],
) -> None:
    import matplotlib.dates as mdates
    import matplotlib.pyplot as plt

    plot_path.parent.mkdir(parents=True, exist_ok=True)

    figure, axes = plt.subplots(2, 1, figsize=(11, 8), constrained_layout=True)
    axes[0].plot(timestamps, prices, linewidth=1.0, color="#1f4e79")
    axes[0].set_title("Synthetic EUR/USD Tick Path")
    axes[0].set_ylabel("Midprice")
    axes[0].xaxis.set_major_formatter(mdates.DateFormatter("%H:%M"))

    interval_milliseconds = [gap_microseconds / 1_000.0 for gap_microseconds in interval_microseconds]
    histogram_bins = min(60, max(10, int(math.sqrt(len(interval_milliseconds)))))
    axes[1].hist(interval_milliseconds, bins=histogram_bins, color="#d95f02", alpha=0.85)
    axes[1].set_title("Synthetic Interarrival Distribution")
    axes[1].set_xlabel("Milliseconds between ticks")
    axes[1].set_ylabel("Frequency")

    figure.savefig(plot_path, dpi=160)
    plt.close(figure)


def _build_irregular_intervals_microseconds(
    *,
    randomness: random.Random,
    interval_count: int,
    total_duration_microseconds: int,
) -> list[int]:
    minimum_total_microseconds = interval_count * MINIMUM_GAP_MICROSECONDS
    extra_microseconds = total_duration_microseconds - minimum_total_microseconds
    interval_weights = [randomness.expovariate(1.0) for _ in range(interval_count)]
    weight_sum = sum(interval_weights)
    if weight_sum <= 0.0:
        raise ValueError("failed to generate positive interval weights")

    intervals = [MINIMUM_GAP_MICROSECONDS for _ in range(interval_count)]
    allocated_extra_microseconds = 0

    for interval_index, weight in enumerate(interval_weights):
        if interval_index == interval_count - 1:
            extra_for_interval = extra_microseconds - allocated_extra_microseconds
        else:
            extra_for_interval = int(extra_microseconds * (weight / weight_sum))
            allocated_extra_microseconds += extra_for_interval

        intervals[interval_index] += extra_for_interval

    return intervals


def _build_timestamps(
    *,
    start_time_utc: datetime,
    interval_microseconds: list[int],
) -> list[datetime]:
    timestamps = [start_time_utc]
    current_timestamp = start_time_utc
    for gap_microseconds in interval_microseconds:
        current_timestamp = current_timestamp + timedelta(microseconds=gap_microseconds)
        timestamps.append(current_timestamp)

    return timestamps


def _build_prices(
    *,
    randomness: random.Random,
    row_count: int,
    start_price: float,
) -> list[float]:
    prices = [start_price]
    current_log_price = 0.0

    for row_index in range(1, row_count):
        regime_scale = 1.0 + 0.35 * (1.0 + math.sin(row_index / 650.0))
        volatility = 2.5e-5 * regime_scale

        jump = 0.0
        if randomness.random() < 0.0015:
            jump = randomness.gauss(0.0, 2.0e-4)

        current_log_price += randomness.gauss(0.0, volatility) + jump
        prices.append(start_price * math.exp(current_log_price))

    return prices


def _build_trade_signs(
    *,
    randomness: random.Random,
    prices: list[float],
) -> list[str]:
    trade_signs = ["buy"]

    for previous_price, current_price in zip(prices, prices[1:]):
        if current_price > previous_price:
            sign = "buy"
        elif current_price < previous_price:
            sign = "sell"
        else:
            sign = "buy" if randomness.random() < 0.5 else "sell"

        if randomness.random() < 0.08:
            sign = "sell" if sign == "buy" else "buy"

        trade_signs.append(sign)

    return trade_signs


__all__ = [
    "DEFAULT_SYNTHETIC_EURUSD_DURATION_MINUTES",
    "DEFAULT_SYNTHETIC_EURUSD_OUTPUT_PATH",
    "DEFAULT_SYNTHETIC_EURUSD_ROW_COUNT",
    "DEFAULT_SYNTHETIC_EURUSD_SEED",
    "DEFAULT_SYNTHETIC_EURUSD_START_PRICE",
    "DEFAULT_SYNTHETIC_EURUSD_START_TIME",
    "SyntheticEurusdDataset",
    "generate_synthetic_eurusd_dataset",
    "write_synthetic_eurusd_generator_plot",
    "write_synthetic_eurusd_tick_csv",
]
