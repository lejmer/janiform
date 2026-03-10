import argparse
import csv
import math
import random
from datetime import UTC, datetime, timedelta
from pathlib import Path


DEFAULT_OUTPUT_PATH = Path("/tmp/janiform_synthetic_eurusd_ticks.csv")
DEFAULT_ROW_COUNT = 10_000
DEFAULT_DURATION_MINUTES = 70.0
DEFAULT_START_TIME = "2026-01-05T00:00:00+00:00"
DEFAULT_START_PRICE = 1.08500
DEFAULT_SEED = 17
MINIMUM_GAP_MICROSECONDS = 1_000


def run_example(
    *,
    output_path: Path,
    row_count: int = DEFAULT_ROW_COUNT,
    duration_minutes: float = DEFAULT_DURATION_MINUTES,
    start_time_utc: datetime | None = None,
    start_price: float = DEFAULT_START_PRICE,
    seed: int = DEFAULT_SEED,
    plot_path: Path | None = None,
) -> dict[str, object]:
    if row_count <= 1:
        raise ValueError("row_count must be greater than 1")
    if duration_minutes <= 0.0:
        raise ValueError("duration_minutes must be strictly positive")
    if start_price <= 0.0:
        raise ValueError("start_price must be strictly positive")

    resolved_start_time_utc = start_time_utc or datetime.fromisoformat(DEFAULT_START_TIME)
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

    output_path.parent.mkdir(parents=True, exist_ok=True)
    with output_path.open("w", encoding="utf-8", newline="") as csv_file:
        writer = csv.writer(csv_file)
        writer.writerow(["time", "close", "sign"])
        for timestamp, price, trade_sign in zip(timestamps, prices, trade_signs):
            writer.writerow(
                [
                    timestamp.isoformat().replace("+00:00", "Z"),
                    f"{price:.6f}",
                    trade_sign,
                ]
            )

    observed_duration_seconds = (timestamps[-1] - timestamps[0]).total_seconds()
    mean_gap_milliseconds = observed_duration_seconds * 1_000.0 / (row_count - 1)

    if plot_path is not None:
        _write_generator_plot(
            plot_path=plot_path,
            timestamps=timestamps,
            prices=prices,
            interval_microseconds=interval_microseconds,
        )

    return {
        "output_path": str(output_path),
        "plot_path": str(plot_path) if plot_path is not None else None,
        "row_count": row_count,
        "seed": seed,
        "start_time_utc": timestamps[0].isoformat(),
        "end_time_utc": timestamps[-1].isoformat(),
        "duration_minutes": round(observed_duration_seconds / 60.0, 4),
        "mean_gap_milliseconds": round(mean_gap_milliseconds, 3),
        "first_price": round(prices[0], 6),
        "last_price": round(prices[-1], 6),
    }


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


def _write_generator_plot(
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


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Generate a synthetic EUR/USD tick CSV with non-grid timestamps suitable for "
            "smoke-testing the end-to-end Janiform example."
        )
    )
    parser.add_argument(
        "--output-path",
        type=Path,
        default=DEFAULT_OUTPUT_PATH,
        help="Where to write the generated CSV.",
    )
    parser.add_argument(
        "--row-count",
        type=int,
        default=DEFAULT_ROW_COUNT,
        help="Number of synthetic tick rows to generate.",
    )
    parser.add_argument(
        "--duration-minutes",
        type=float,
        default=DEFAULT_DURATION_MINUTES,
        help="Target span of the synthetic sample in minutes.",
    )
    parser.add_argument(
        "--start-time-utc",
        default=DEFAULT_START_TIME,
        help="UTC start time in ISO-8601 format.",
    )
    parser.add_argument(
        "--start-price",
        type=float,
        default=DEFAULT_START_PRICE,
        help="Starting EUR/USD midprice.",
    )
    parser.add_argument(
        "--seed",
        type=int,
        default=DEFAULT_SEED,
        help="Deterministic random seed.",
    )
    parser.add_argument(
        "--plot-path",
        type=Path,
        default=None,
        help="Optional output path for a PNG showing the synthetic path and interarrival distribution.",
    )
    return parser.parse_args()


def main() -> None:
    args = _parse_args()
    summary = run_example(
        output_path=args.output_path,
        row_count=args.row_count,
        duration_minutes=args.duration_minutes,
        start_time_utc=datetime.fromisoformat(args.start_time_utc),
        start_price=args.start_price,
        seed=args.seed,
        plot_path=args.plot_path,
    )
    for key, value in summary.items():
        print(f"{key}: {value}")


if __name__ == "__main__":
    main()
