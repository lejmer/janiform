"""Generate empirical, synthetic, and comparison inter-arrival figures."""

import argparse
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Final


REPO_ROOT: Final[Path] = Path(__file__).resolve().parents[3]

from shared.matplotlib_environment import configure_matplotlib_runtime

import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
import pyarrow.parquet as pq
import scienceplots  # noqa: F401
from matplotlib.axes import Axes

from shared.figure_output import write_figure_and_metadata
from shared.figure_metadata import (
    file_artifact_metadata,
    path_for_metadata,
)


MATPLOTLIB_CONFIG_DIR: Final[Path] = configure_matplotlib_runtime(REPO_ROOT)

FIGURE_ID: Final[str] = "inter_arrival_distribution"
FIGURE_TITLE: Final[str] = "Inter-arrival distribution"

DEFAULT_EMPIRICAL_INPUT_PATH: Final[Path] = (
    REPO_ROOT
    / "data"
    / "databento-converted"
    / "ES_FUT-TBBO"
    / "continuous"
    / "es_continuous.parquet"
)

DEFAULT_SYNTHETIC_INPUT_PATH: Final[Path] = (
    REPO_ROOT
    / "paper"
    / "data"
    / "synthetic_es_continuous"
    / "synthetic_es_continuous.parquet"
)

DEFAULT_OUTPUT_DIR: Final[Path] = REPO_ROOT / "paper" / "assets" / "figures"
DEFAULT_OUTPUT_PREFIX: Final[str] = FIGURE_ID

DEFAULT_LOG_BIN_COUNT: Final[int] = 80

EMPIRICAL_TIMESTAMP_COLUMN: Final[str] = "event_timestamp_utc"
SYNTHETIC_TIMESTAMP_COLUMN: Final[str] = "timestamp_utc"

SCIENCEPLOTS_STYLE: Final[list[str]] = ["science", "no-latex"]


@dataclass(frozen=True)
class DatasetSpec:
    label: str
    role: str
    input_path: Path
    timestamp_column: str
    dataset_family: str
    instrument: str
    color: str


@dataclass(frozen=True)
class InterArrivalDataset:
    spec: DatasetSpec
    input_metadata: dict[str, Any]
    data_summary: dict[str, Any]
    positive_inter_arrival_seconds: np.ndarray


@dataclass(frozen=True)
class DensityCurve:
    label: str
    color: str
    x_values: np.ndarray
    y_values: np.ndarray
    nonzero_bin_count: int


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)

    parser.add_argument(
        "--empirical-input-path",
        type=Path,
        default=DEFAULT_EMPIRICAL_INPUT_PATH,
        help="Empirical parquet input path.",
    )

    parser.add_argument(
        "--synthetic-input-path",
        type=Path,
        default=DEFAULT_SYNTHETIC_INPUT_PATH,
        help="Synthetic parquet input path.",
    )

    parser.add_argument(
        "--output-dir",
        type=Path,
        default=DEFAULT_OUTPUT_DIR,
        help="Directory where figures and metadata sidecars are written.",
    )

    parser.add_argument(
        "--output-prefix",
        default=DEFAULT_OUTPUT_PREFIX,
        help="Output filename prefix.",
    )

    parser.add_argument(
        "--bin-count",
        type=int,
        default=DEFAULT_LOG_BIN_COUNT,
        help="Number of shared log-spaced density bins.",
    )

    return parser.parse_args()


def main() -> None:
    arguments = parse_args()

    output_dir: Path = arguments.output_dir
    output_prefix = str(arguments.output_prefix)
    bin_count = int(arguments.bin_count)

    if bin_count <= 0:
        raise ValueError("--bin-count must be positive")

    empirical_spec = DatasetSpec(
        label="Empirical",
        role="empirical_market_data",
        input_path=arguments.empirical_input_path,
        timestamp_column=EMPIRICAL_TIMESTAMP_COLUMN,
        dataset_family="databento_converted",
        instrument="ES continuous future",
        color="#2F5D8C",
    )

    synthetic_spec = DatasetSpec(
        label="Janiform synthetic",
        role="synthetic_model_output",
        input_path=arguments.synthetic_input_path,
        timestamp_column=SYNTHETIC_TIMESTAMP_COLUMN,
        dataset_family="janiform_synthetic",
        instrument="ES continuous future",
        color="#B34D4A",
    )

    empirical_dataset = load_inter_arrival_dataset(empirical_spec)
    synthetic_dataset = load_inter_arrival_dataset(synthetic_spec)

    histogram_bins = build_shared_histogram_bins(
        [empirical_dataset, synthetic_dataset],
        bin_count,
    )

    print("Applying SciencePlots style: " + ", ".join(SCIENCEPLOTS_STYLE))

    plt.style.use(SCIENCEPLOTS_STYLE)

    render_single_dataset_figure(
        dataset=empirical_dataset,
        histogram_bins=histogram_bins,
        output_path=output_dir / f"{output_prefix}.pdf",
        title="Empirical ES continuous futures inter-arrival distribution",
        command=sys.argv,
        bin_count=bin_count,
    )

    render_single_dataset_figure(
        dataset=synthetic_dataset,
        histogram_bins=histogram_bins,
        output_path=output_dir / f"{output_prefix}_synthetic.pdf",
        title="Janiform synthetic ES inter-arrival distribution",
        command=sys.argv,
        bin_count=bin_count,
    )

    render_comparison_figure(
        datasets=[empirical_dataset, synthetic_dataset],
        histogram_bins=histogram_bins,
        output_path=output_dir / f"{output_prefix}_comparison.pdf",
        title="Empirical vs Janiform synthetic inter-arrival distribution",
        command=sys.argv,
        bin_count=bin_count,
    )

    print("Inter-arrival distribution figure generation complete")


def load_inter_arrival_dataset(spec: DatasetSpec) -> InterArrivalDataset:
    print(f"[{spec.label}] Reading parquet metadata: {spec.input_path}")

    parquet_file = pq.ParquetFile(spec.input_path)
    parquet_metadata = parquet_file.metadata

    print(f"[{spec.label}] Loading timestamp column: {spec.timestamp_column}")

    timestamp_table = pq.read_table(spec.input_path, columns=[spec.timestamp_column])
    timestamp_series = timestamp_table.column(spec.timestamp_column).to_pandas()

    event_count = len(timestamp_series)

    print(f"[{spec.label}] Loaded {event_count:,} event timestamps")

    if event_count < 2:
        raise ValueError(f"{spec.label} dataset must contain at least two timestamps")

    print(f"[{spec.label}] Normalizing timestamps and checking order")

    timestamps = pd.to_datetime(timestamp_series, utc=True)
    timestamp_null_count = int(timestamps.isna().sum())
    if timestamp_null_count > 0:
        raise ValueError(
            f"{spec.label} timestamp column contains {timestamp_null_count:,} null values"
        )

    original_timestamp_order_is_monotonic = bool(timestamps.is_monotonic_increasing)
    if not original_timestamp_order_is_monotonic:
        print(f"[{spec.label}] Timestamps are not monotonic, sorting before differencing")
        timestamps = timestamps.sort_values(kind="mergesort").reset_index(drop=True)

    print(f"[{spec.label}] Computing inter-arrival times")

    timestamp_ns = timestamps.astype("int64").to_numpy()
    inter_arrival_ns = np.diff(timestamp_ns)

    zero_inter_arrival_count = int(np.count_nonzero(inter_arrival_ns == 0))
    negative_inter_arrival_count = int(np.count_nonzero(inter_arrival_ns < 0))

    positive_inter_arrival_ns = inter_arrival_ns[inter_arrival_ns > 0]
    if positive_inter_arrival_ns.size == 0:
        raise ValueError(
            f"{spec.label} dataset has no positive inter-arrival times for plotting"
        )

    positive_inter_arrival_seconds = positive_inter_arrival_ns.astype(np.float64)
    positive_inter_arrival_seconds /= 1_000_000_000.0

    minimum_inter_arrival_seconds = float(np.min(positive_inter_arrival_seconds))
    maximum_inter_arrival_seconds = float(np.max(positive_inter_arrival_seconds))
    if minimum_inter_arrival_seconds == maximum_inter_arrival_seconds:
        raise ValueError(
            f"{spec.label} positive inter-arrival times must vary for log density plotting"
        )

    input_metadata = file_artifact_metadata(
        spec.input_path,
        role=spec.role,
        artifact_type="parquet",
        media_type="application/vnd.apache.parquet",
        repository_root=REPO_ROOT,
    ) | {
        "dataset_family": spec.dataset_family,
        "instrument": spec.instrument,
        "timestamp_column": spec.timestamp_column,
        "parquet_row_count": parquet_metadata.num_rows,
        "parquet_row_group_count": parquet_metadata.num_row_groups,
        "parquet_columns": parquet_file.schema_arrow.names,
        "loaded_columns": timestamp_table.schema.names,
    }

    data_summary = {
        "label": spec.label,
        "event_count": event_count,
        "timestamp_null_count": timestamp_null_count,
        "raw_inter_arrival_count": int(inter_arrival_ns.size),
        "positive_inter_arrival_count": int(positive_inter_arrival_ns.size),
        "zero_inter_arrival_count": zero_inter_arrival_count,
        "negative_inter_arrival_count_after_sorting": negative_inter_arrival_count,
        "original_timestamp_order_is_monotonic": original_timestamp_order_is_monotonic,
        "timestamp_min_utc": timestamps.iloc[0].isoformat(),
        "timestamp_max_utc": timestamps.iloc[-1].isoformat(),
        "positive_inter_arrival_seconds": {
            "minimum": minimum_inter_arrival_seconds,
            "maximum": maximum_inter_arrival_seconds,
            "quantiles": inter_arrival_quantiles(positive_inter_arrival_seconds),
        },
    }

    return InterArrivalDataset(
        spec=spec,
        input_metadata=input_metadata,
        data_summary=data_summary,
        positive_inter_arrival_seconds=positive_inter_arrival_seconds,
    )


def build_shared_histogram_bins(
    datasets: list[InterArrivalDataset],
    bin_count: int,
) -> np.ndarray:
    print("Building shared log-spaced histogram bins")

    minimum_inter_arrival_seconds = min(
        float(np.min(dataset.positive_inter_arrival_seconds)) for dataset in datasets
    )
    maximum_inter_arrival_seconds = max(
        float(np.max(dataset.positive_inter_arrival_seconds)) for dataset in datasets
    )
    if minimum_inter_arrival_seconds == maximum_inter_arrival_seconds:
        raise ValueError("Shared positive inter-arrival range must vary")

    return np.geomspace(
        minimum_inter_arrival_seconds,
        maximum_inter_arrival_seconds,
        bin_count + 1,
    )


def render_single_dataset_figure(
    *,
    dataset: InterArrivalDataset,
    histogram_bins: np.ndarray,
    output_path: Path,
    title: str,
    command: list[str],
    bin_count: int,
) -> None:
    print(f"[{dataset.spec.label}] Rendering density plot")

    curve = density_curve(dataset, histogram_bins)

    figure, axes = plt.subplots(figsize=(6.5, 4.0))
    draw_density_curve(axes, curve)
    axes.set_title(title)

    configure_density_axes(axes)

    write_figure_and_metadata(
        figure=figure,
        output_path=output_path,
        figure_id=f"{FIGURE_ID}_{dataset.spec.role}",
        title=title,
        script_path=Path(__file__).resolve(),
        repository_root=REPO_ROOT,
        inputs=[dataset.input_metadata],
        parameters=metadata_parameters(bin_count),
        data_summary={dataset.spec.role: dataset.data_summary},
        plot_summary=plot_summary([curve]),
        command=command,
        notes=figure_notes(),
    )


def render_comparison_figure(
    *,
    datasets: list[InterArrivalDataset],
    histogram_bins: np.ndarray,
    output_path: Path,
    title: str,
    command: list[str],
    bin_count: int,
) -> None:
    print("[comparison] Rendering overlaid density plot")

    curves = [density_curve(dataset, histogram_bins) for dataset in datasets]
    figure, axes = plt.subplots(figsize=(6.5, 4.0))
    for curve in curves:
        draw_density_curve(axes, curve)

    axes.set_title(title)
    axes.legend(frameon=False)

    configure_density_axes(axes)

    write_figure_and_metadata(
        figure=figure,
        output_path=output_path,
        figure_id=f"{FIGURE_ID}_comparison",
        title=title,
        script_path=Path(__file__).resolve(),
        repository_root=REPO_ROOT,
        inputs=[dataset.input_metadata for dataset in datasets],
        parameters=metadata_parameters(bin_count),
        data_summary={dataset.spec.role: dataset.data_summary for dataset in datasets},
        plot_summary=plot_summary(curves),
        command=command,
        notes=figure_notes(),
    )


def density_curve(dataset: InterArrivalDataset, histogram_bins: np.ndarray) -> DensityCurve:
    print(f"[{dataset.spec.label}] Computing density curve")

    density_values, bin_edges = np.histogram(
        dataset.positive_inter_arrival_seconds,
        bins=histogram_bins,
        density=True,
    )

    bin_centers = np.sqrt(bin_edges[:-1] * bin_edges[1:])
    nonzero_density_mask = density_values > 0.0

    return DensityCurve(
        label=dataset.spec.label,
        color=dataset.spec.color,
        x_values=bin_centers[nonzero_density_mask],
        y_values=density_values[nonzero_density_mask],
        nonzero_bin_count=int(np.count_nonzero(nonzero_density_mask)),
    )


def draw_density_curve(axes: Axes, curve: DensityCurve) -> None:
    axes.plot(
        curve.x_values,
        curve.y_values,
        color=curve.color,
        linewidth=1.6,
        label=curve.label,
    )


def configure_density_axes(axes: Axes) -> None:
    axes.set_xscale("log")
    axes.set_yscale("log")
    axes.set_xlabel("Inter-arrival time (seconds)")
    axes.set_ylabel("Density")

    axes.grid(True, which="both", alpha=0.25)


def metadata_parameters(bin_count: int) -> dict[str, Any]:
    return {
        "bin_count": bin_count,
        "binning_policy": "shared_log_spaced_from_positive_data_minimum_to_positive_data_maximum",
        "density_policy": "numpy_histogram_density_plotted_at_geometric_bin_centers",
        "style_provider": "scienceplots",
        "style": SCIENCEPLOTS_STYLE,
        "zero_inter_arrival_policy": "excluded_from_log_density_and_reported_in_metadata",
        "matplotlib_config_dir": path_for_metadata(
            MATPLOTLIB_CONFIG_DIR,
            repository_root=REPO_ROOT,
        ),
    }


def figure_notes() -> list[str]:
    return [
        "Only timestamp columns are loaded from parquet inputs.",
        "Zero timestamp deltas cannot be displayed on log-x density plots.",
    ]


def plot_summary(curves: list[DensityCurve]) -> dict[str, Any]:
    return {
        "plot_type": "log_log_density_curve",
        "x_axis": "positive_event_inter_arrival_seconds",
        "y_axis": "density",
        "curves": [
            {
                "label": curve.label,
                "nonzero_density_bin_count": curve.nonzero_bin_count,
            }
            for curve in curves
        ],
    }


def inter_arrival_quantiles(inter_arrival_seconds: np.ndarray) -> dict[str, float]:
    quantile_levels: dict[str, float] = {
        "p50": 0.50,
        "p90": 0.90,
        "p95": 0.95,
        "p99": 0.99,
    }

    return {
        quantile_name: float(np.quantile(inter_arrival_seconds, quantile_level))
        for quantile_name, quantile_level in quantile_levels.items()
    }


if __name__ == "__main__":
    main()
