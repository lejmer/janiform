"""Shared return-distribution helpers for paper figure scripts."""

from dataclasses import dataclass
from pathlib import Path
from statistics import NormalDist
from typing import Any, Final, Optional

import numpy as np

from .datasets import DatasetSpec, load_timestamp_value_dataset


NANOS_PER_SECOND: Final[int] = 1_000_000_000
RETURN_SCALE_BASIS_POINTS: Final[float] = 10_000.0


@dataclass(frozen=True)
class ReturnHorizon:
    label: str
    output_label: str
    duration_nanos: Optional[int]


@dataclass(frozen=True)
class PriceDataset:
    spec: DatasetSpec
    input_metadata: dict[str, Any]
    data_summary: dict[str, Any]
    timestamp_ns: np.ndarray
    log_prices: np.ndarray


@dataclass(frozen=True)
class HorizonReturnDataset:
    price_dataset: PriceDataset
    horizon: ReturnHorizon
    return_basis_points: np.ndarray
    data_summary: dict[str, Any]


@dataclass(frozen=True)
class HistogramDensity:
    label: str
    color: str
    bin_centers: np.ndarray
    bin_width: float
    histogram_density: np.ndarray
    kde_x_values: np.ndarray
    kde_density: np.ndarray
    plotted_return_count: int
    lower_tail_excluded_count: int
    upper_tail_excluded_count: int


@dataclass(frozen=True)
class QqCurve:
    label: str
    color: str
    x_values: np.ndarray
    y_values: np.ndarray


RETURN_HORIZONS: Final[tuple[ReturnHorizon, ...]] = (
    ReturnHorizon(label="tick", output_label="tick", duration_nanos=None),
    ReturnHorizon(label="1s", output_label="1s", duration_nanos=NANOS_PER_SECOND),
    ReturnHorizon(label="1m", output_label="1m", duration_nanos=60 * NANOS_PER_SECOND),
    ReturnHorizon(label="5m", output_label="5m", duration_nanos=5 * 60 * NANOS_PER_SECOND),
    ReturnHorizon(label="1h", output_label="1h", duration_nanos=60 * 60 * NANOS_PER_SECOND),
    ReturnHorizon(
        label="daily",
        output_label="daily",
        duration_nanos=24 * 60 * 60 * NANOS_PER_SECOND,
    ),
)


def load_price_dataset(
    spec: DatasetSpec,
    *,
    value_column: str,
    repository_root: Path,
) -> PriceDataset:
    loaded_dataset = load_timestamp_value_dataset(
        spec,
        value_column=value_column,
        repository_root=repository_root,
    )

    print(f"[{spec.label}] Preparing positive observed prices")

    prices = loaded_dataset.values.to_numpy(dtype=np.float64)
    timestamps = loaded_dataset.timestamps

    finite_positive_price_mask = np.isfinite(prices) & (prices > 0.0)
    if not np.all(finite_positive_price_mask):
        removed_count = int(prices.size - np.count_nonzero(finite_positive_price_mask))
        print(f"[{spec.label}] Removing {removed_count:,} non-finite or non-positive prices")

        prices = prices[finite_positive_price_mask]
        timestamps = timestamps.iloc[finite_positive_price_mask].reset_index(drop=True)

    if prices.size < 2:
        raise ValueError(f"{spec.label} dataset must contain at least two positive prices")

    return PriceDataset(
        spec=spec,
        input_metadata=loaded_dataset.input_metadata,
        data_summary=loaded_dataset.data_summary
        | {
            "price_column": value_column,
            "positive_price_count": int(prices.size),
        },
        timestamp_ns=timestamps.astype("int64").to_numpy(),
        log_prices=np.log(prices),
    )


def build_horizon_return_dataset(
    dataset: PriceDataset,
    horizon: ReturnHorizon,
) -> HorizonReturnDataset:
    print(f"[{dataset.spec.label}] Computing {horizon.label} log returns")

    if horizon.duration_nanos is None:
        raw_returns = np.diff(dataset.log_prices)
        source_observation_count = dataset.log_prices.size - 1
    else:
        raw_returns, source_observation_count = clock_horizon_returns(
            dataset,
            horizon.duration_nanos,
        )

    finite_returns = raw_returns[np.isfinite(raw_returns)] * RETURN_SCALE_BASIS_POINTS
    if finite_returns.size < 2:
        raise ValueError(
            f"{dataset.spec.label} {horizon.label} horizon has fewer than two finite returns"
        )

    summary = dataset.data_summary | {
        "horizon": horizon.label,
        "horizon_duration_nanos": horizon.duration_nanos,
        "source_observation_count": int(source_observation_count),
        "finite_return_count": int(finite_returns.size),
        "return_unit": "log_return_basis_points",
        "return_summary": return_summary(finite_returns),
    }

    return HorizonReturnDataset(
        price_dataset=dataset,
        horizon=horizon,
        return_basis_points=finite_returns,
        data_summary=summary,
    )


def clock_horizon_returns(
    dataset: PriceDataset,
    horizon_duration_nanos: int,
) -> tuple[np.ndarray, int]:
    latest_start_ns = dataset.timestamp_ns[-1] - horizon_duration_nanos
    source_positions = np.flatnonzero(dataset.timestamp_ns <= latest_start_ns)
    if source_positions.size == 0:
        raise ValueError(
            f"{dataset.spec.label} dataset does not cover the requested clock horizon"
        )

    target_timestamps = dataset.timestamp_ns[source_positions] + horizon_duration_nanos
    target_positions = (
        np.searchsorted(dataset.timestamp_ns, target_timestamps, side="right") - 1
    )
    returns = dataset.log_prices[target_positions] - dataset.log_prices[source_positions]

    return returns, int(source_positions.size)


def pooled_plot_range(
    datasets: list[HorizonReturnDataset],
    plot_tail_probability: float,
) -> tuple[float, float]:
    print(f"[{datasets[0].horizon.label}] Computing pooled plot range")

    pooled_values = np.concatenate([dataset.return_basis_points for dataset in datasets])
    lower_bound, upper_bound = np.quantile(
        pooled_values,
        [plot_tail_probability, 1.0 - plot_tail_probability],
    )
    if lower_bound == upper_bound:
        lower_bound = float(np.min(pooled_values))
        upper_bound = float(np.max(pooled_values))

    if lower_bound == upper_bound:
        raise ValueError(f"{datasets[0].horizon.label} return plot range has zero width")

    return float(lower_bound), float(upper_bound)


def histogram_density(
    dataset: HorizonReturnDataset,
    plot_range: tuple[float, float],
    histogram_bin_count: int,
    kde_point_count: int,
) -> HistogramDensity:
    values = dataset.return_basis_points
    lower_bound, upper_bound = plot_range
    in_range_mask = (values >= lower_bound) & (values <= upper_bound)
    plotted_values = values[in_range_mask]

    counts, bin_edges = np.histogram(
        plotted_values,
        bins=histogram_bin_count,
        range=plot_range,
        density=False,
    )
    bin_width = float(bin_edges[1] - bin_edges[0])
    histogram_values = counts.astype(np.float64) / (float(plotted_values.size) * bin_width)
    bin_centers = (bin_edges[:-1] + bin_edges[1:]) / 2.0
    kde_x_values = np.linspace(lower_bound, upper_bound, kde_point_count)
    kde_values = binned_gaussian_kde(
        bin_centers=bin_centers,
        counts=counts,
        kde_x_values=kde_x_values,
        plotted_values=plotted_values,
    )

    return HistogramDensity(
        label=dataset.price_dataset.spec.label,
        color=dataset.price_dataset.spec.color,
        bin_centers=bin_centers,
        bin_width=bin_width,
        histogram_density=histogram_values,
        kde_x_values=kde_x_values,
        kde_density=kde_values,
        plotted_return_count=int(plotted_values.size),
        lower_tail_excluded_count=int(np.count_nonzero(values < lower_bound)),
        upper_tail_excluded_count=int(np.count_nonzero(values > upper_bound)),
    )


def binned_gaussian_kde(
    *,
    bin_centers: np.ndarray,
    counts: np.ndarray,
    kde_x_values: np.ndarray,
    plotted_values: np.ndarray,
) -> np.ndarray:
    standard_deviation = float(np.std(plotted_values, ddof=1))
    if standard_deviation == 0.0:
        return np.full(kde_x_values.size, np.nan, dtype=np.float64)

    bandwidth = 1.06 * standard_deviation * plotted_values.size ** (-1.0 / 5.0)
    scaled_distance = (kde_x_values[:, None] - bin_centers[None, :]) / bandwidth
    kernel_values = np.exp(-0.5 * scaled_distance * scaled_distance)
    normalizing_constant = float(plotted_values.size) * bandwidth * np.sqrt(2.0 * np.pi)

    return kernel_values @ counts.astype(np.float64) / normalizing_constant


def normal_qq_curve(
    dataset: HorizonReturnDataset,
    qq_point_count: int,
    plot_tail_probability: float,
) -> QqCurve:
    quantile_levels = qq_levels(qq_point_count, plot_tail_probability)
    values = dataset.return_basis_points
    standard_deviation = float(np.std(values, ddof=1))
    if standard_deviation == 0.0:
        raise ValueError(f"{dataset.price_dataset.spec.label} returns have zero standard deviation")

    normal_distribution = NormalDist(
        mu=float(np.mean(values)),
        sigma=standard_deviation,
    )

    return QqCurve(
        label=dataset.price_dataset.spec.label,
        color=dataset.price_dataset.spec.color,
        x_values=np.array(
            [normal_distribution.inv_cdf(float(level)) for level in quantile_levels],
            dtype=np.float64,
        ),
        y_values=np.quantile(values, quantile_levels),
    )


def comparison_qq_curve(
    datasets: list[HorizonReturnDataset],
    qq_point_count: int,
    plot_tail_probability: float,
) -> QqCurve:
    quantile_levels = qq_levels(qq_point_count, plot_tail_probability)
    empirical_dataset, synthetic_dataset = datasets

    return QqCurve(
        label="Janiform synthetic vs empirical",
        color=synthetic_dataset.price_dataset.spec.color,
        x_values=np.quantile(empirical_dataset.return_basis_points, quantile_levels),
        y_values=np.quantile(synthetic_dataset.return_basis_points, quantile_levels),
    )


def qq_levels(point_count: int, plot_tail_probability: float) -> np.ndarray:
    return np.linspace(
        plot_tail_probability,
        1.0 - plot_tail_probability,
        point_count,
    )


def validate_positive_count(value: int, argument_name: str) -> None:
    if value <= 0:
        raise ValueError(f"{argument_name} must be positive")


def validate_tail_probability(value: float) -> None:
    if not np.isfinite(value) or value <= 0.0 or value >= 0.5:
        raise ValueError("--plot-tail-probability must be finite and between 0 and 0.5")


def data_summary(datasets: list[HorizonReturnDataset]) -> dict[str, Any]:
    return {
        dataset.price_dataset.spec.role: dataset.data_summary for dataset in datasets
    }


def return_summary(values: np.ndarray) -> dict[str, Any]:
    quantile_levels: dict[str, float] = {
        "p001": 0.001,
        "p01": 0.01,
        "p05": 0.05,
        "p50": 0.50,
        "p95": 0.95,
        "p99": 0.99,
        "p999": 0.999,
    }

    return {
        "minimum": float(np.min(values)),
        "maximum": float(np.max(values)),
        "mean": float(np.mean(values)),
        "standard_deviation": float(np.std(values, ddof=1)),
        "quantiles": {
            quantile_name: float(np.quantile(values, quantile_level))
            for quantile_name, quantile_level in quantile_levels.items()
        },
    }


def histogram_plot_summary(
    densities: list[HistogramDensity],
    plot_range: tuple[float, float],
) -> dict[str, Any]:
    return {
        "plot_type": "histogram_with_binned_gaussian_kde",
        "x_axis": "log_return_basis_points",
        "y_axis": "density",
        "plot_range_basis_points": {
            "minimum": plot_range[0],
            "maximum": plot_range[1],
        },
        "series": [
            {
                "label": density.label,
                "plotted_return_count": density.plotted_return_count,
                "lower_tail_excluded_count": density.lower_tail_excluded_count,
                "upper_tail_excluded_count": density.upper_tail_excluded_count,
            }
            for density in densities
        ],
    }


def qq_plot_summary(
    curves: list[QqCurve],
    *,
    x_axis: str,
    y_axis: str,
) -> dict[str, Any]:
    return {
        "plot_type": "qq_plot",
        "x_axis": x_axis,
        "y_axis": y_axis,
        "curves": [
            {
                "label": curve.label,
                "point_count": int(curve.x_values.size),
                "x_minimum": float(np.min(curve.x_values)),
                "x_maximum": float(np.max(curve.x_values)),
                "y_minimum": float(np.min(curve.y_values)),
                "y_maximum": float(np.max(curve.y_values)),
            }
            for curve in curves
        ],
    }
