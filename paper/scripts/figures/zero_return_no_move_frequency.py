"""Generate empirical, synthetic, and comparison no-move frequency profiles."""

import argparse
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Final

from shared.matplotlib_environment import configure_matplotlib_runtime


REPO_ROOT: Final[Path] = Path(__file__).resolve().parents[3]

# Configure MPLCONFIGDIR before importing Matplotlib because home may be read-only.
MATPLOTLIB_CONFIG_DIR: Final[Path] = configure_matplotlib_runtime(REPO_ROOT)

import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
import scienceplots  # noqa: F401
from matplotlib.axes import Axes
from matplotlib.ticker import PercentFormatter

from shared.datasets import DatasetSpec, load_timestamp_value_dataset
from shared.es_continuous_inputs import (
    EMPIRICAL_PRICE_COLUMN,
    SYNTHETIC_PRICE_COLUMN,
    default_empirical_input_path,
    default_synthetic_input_path,
    es_continuous_dataset_specs,
)
from shared.figure_metadata import path_for_metadata
from shared.figure_output import write_figure_and_metadata
from shared.intraday_buckets import (
    MINUTES_PER_DAY,
    assign_grouped_values,
    intraday_bucket_indices,
    intraday_bucket_midpoint_hours,
    validate_bin_minutes,
)


FIGURE_ID: Final[str] = "zero_return_no_move_frequency"
FIGURE_TITLE: Final[str] = "Zero-return no-move frequency"

DEFAULT_EMPIRICAL_INPUT_PATH: Final[Path] = default_empirical_input_path(REPO_ROOT)
DEFAULT_SYNTHETIC_INPUT_PATH: Final[Path] = default_synthetic_input_path(REPO_ROOT)
DEFAULT_OUTPUT_DIR: Final[Path] = REPO_ROOT / "paper" / "assets" / "figures"
DEFAULT_OUTPUT_PREFIX: Final[str] = FIGURE_ID

DEFAULT_BIN_MINUTES: Final[int] = 60

SCIENCEPLOTS_STYLE: Final[list[str]] = ["science", "no-latex"]


@dataclass(frozen=True)
class NoMoveProfile:
    label: str
    color: str
    bucket_midpoint_hours: np.ndarray
    observation_counts: np.ndarray
    no_move_counts: np.ndarray
    no_move_frequency: np.ndarray


@dataclass(frozen=True)
class NoMoveProfileDataset:
    spec: DatasetSpec
    input_metadata: dict[str, Any]
    data_summary: dict[str, Any]
    profile: NoMoveProfile


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
        "--bin-minutes",
        type=int,
        default=DEFAULT_BIN_MINUTES,
        help="Intraday profile bin width in minutes.",
    )

    return parser.parse_args()


def main() -> None:
    arguments = parse_args()

    output_dir: Path = arguments.output_dir

    output_prefix = str(arguments.output_prefix)
    bin_minutes = int(arguments.bin_minutes)

    validate_bin_minutes(bin_minutes)

    empirical_spec, synthetic_spec = es_continuous_dataset_specs(
        empirical_input_path=arguments.empirical_input_path,
        synthetic_input_path=arguments.synthetic_input_path,
    )

    empirical_dataset = load_no_move_profile_dataset(
        empirical_spec,
        value_column=EMPIRICAL_PRICE_COLUMN,
        bin_minutes=bin_minutes,
    )

    synthetic_dataset = load_no_move_profile_dataset(
        synthetic_spec,
        value_column=SYNTHETIC_PRICE_COLUMN,
        bin_minutes=bin_minutes,
    )

    print("Applying SciencePlots style: " + ", ".join(SCIENCEPLOTS_STYLE))

    plt.style.use(SCIENCEPLOTS_STYLE)

    render_single_dataset_figure(
        dataset=empirical_dataset,
        output_path=output_dir / f"{output_prefix}.pdf",
        title="Empirical ES continuous futures no-move frequency",
        command=sys.argv,
        bin_minutes=bin_minutes,
    )

    render_single_dataset_figure(
        dataset=synthetic_dataset,
        output_path=output_dir / f"{output_prefix}_synthetic.pdf",
        title="Janiform synthetic ES no-move frequency",
        command=sys.argv,
        bin_minutes=bin_minutes,
    )

    render_comparison_figure(
        datasets=[empirical_dataset, synthetic_dataset],
        output_path=output_dir / f"{output_prefix}_comparison.pdf",
        title="Empirical vs Janiform synthetic no-move frequency",
        command=sys.argv,
        bin_minutes=bin_minutes,
    )

    print("Zero-return no-move frequency figure generation complete")


def load_no_move_profile_dataset(
    spec: DatasetSpec,
    *,
    value_column: str,
    bin_minutes: int,
) -> NoMoveProfileDataset:
    observation_dataset = load_timestamp_value_dataset(
        spec,
        value_column=value_column,
        repository_root=REPO_ROOT,
    )

    print(f"[{spec.label}] Computing exact no-move indicators")

    timestamps = observation_dataset.timestamps
    prices = observation_dataset.values

    previous_prices = prices.iloc[:-1].reset_index(drop=True)
    current_prices = prices.iloc[1:].reset_index(drop=True)

    comparison_timestamps = timestamps.iloc[1:].reset_index(drop=True)

    valid_comparison_mask = previous_prices.notna() & current_prices.notna()
    valid_previous_prices = previous_prices[valid_comparison_mask]
    valid_current_prices = current_prices[valid_comparison_mask]
    valid_comparison_timestamps = comparison_timestamps[valid_comparison_mask]

    no_move_flags = (valid_current_prices.to_numpy() == valid_previous_prices.to_numpy())
    if no_move_flags.size == 0:
        raise ValueError(f"{spec.label} dataset has no valid price comparisons")

    print(f"[{spec.label}] Assigning no-move indicators to intraday buckets")

    bucket_indices = intraday_bucket_indices(valid_comparison_timestamps, bin_minutes)

    profile = build_no_move_profile(
        spec=spec,
        bucket_indices=bucket_indices,
        no_move_flags=no_move_flags,
        bin_minutes=bin_minutes,
    )

    data_summary = observation_dataset.data_summary | {
        "price_column": value_column,
        "price_comparison_count": int(no_move_flags.size),
        "no_move_count": int(np.count_nonzero(no_move_flags)),
        "move_count": int(np.count_nonzero(~no_move_flags)),
        "overall_no_move_frequency": float(np.mean(no_move_flags)),
        "no_move_profile": profile_summary(profile),
    }

    return NoMoveProfileDataset(
        spec=spec,
        input_metadata=observation_dataset.input_metadata,
        data_summary=data_summary,
        profile=profile,
    )


def build_no_move_profile(
    *,
    spec: DatasetSpec,
    bucket_indices: np.ndarray,
    no_move_flags: np.ndarray,
    bin_minutes: int,
) -> NoMoveProfile:
    print(f"[{spec.label}] Aggregating no-move frequency")

    bucket_count = MINUTES_PER_DAY // bin_minutes

    profile_frame = pd.DataFrame(
        {
            "bucket_index": bucket_indices,
            "no_move": no_move_flags.astype(np.float64),
        }
    )

    grouped_no_move = profile_frame.groupby("bucket_index")["no_move"]

    observation_counts = np.zeros(bucket_count, dtype=np.int64)
    no_move_counts = np.zeros(bucket_count, dtype=np.int64)
    no_move_frequency = np.full(bucket_count, np.nan, dtype=np.float64)

    assign_grouped_values(observation_counts, grouped_no_move.size())
    assign_grouped_values(no_move_counts, grouped_no_move.sum())
    assign_grouped_values(no_move_frequency, grouped_no_move.mean())

    return NoMoveProfile(
        label=spec.label,
        color=spec.color,
        bucket_midpoint_hours=intraday_bucket_midpoint_hours(bin_minutes),
        observation_counts=observation_counts,
        no_move_counts=no_move_counts,
        no_move_frequency=no_move_frequency,
    )


def render_single_dataset_figure(
    *,
    dataset: NoMoveProfileDataset,
    output_path: Path,
    title: str,
    command: list[str],
    bin_minutes: int,
) -> None:
    print(f"[{dataset.spec.label}] Rendering no-move frequency profile")

    figure, axes = plt.subplots(figsize=(6.5, 4.0))
    draw_profile(axes, dataset.profile)
    axes.set_title(title)

    configure_profile_axes(axes)

    write_figure_and_metadata(
        figure=figure,
        output_path=output_path,
        figure_id=f"{FIGURE_ID}_{dataset.spec.role}",
        title=title,
        script_path=Path(__file__).resolve(),
        repository_root=REPO_ROOT,
        inputs=[dataset.input_metadata],
        parameters=metadata_parameters(bin_minutes),
        data_summary={dataset.spec.role: dataset.data_summary},
        plot_summary=plot_summary([dataset.profile]),
        command=command,
        notes=figure_notes(),
    )


def render_comparison_figure(
    *,
    datasets: list[NoMoveProfileDataset],
    output_path: Path,
    title: str,
    command: list[str],
    bin_minutes: int,
) -> None:
    print("[comparison] Rendering overlaid no-move frequency profile")

    figure, axes = plt.subplots(figsize=(6.5, 4.0))
    for dataset in datasets:
        draw_profile(axes, dataset.profile)

    axes.set_title(title)
    axes.legend(frameon=False)

    configure_profile_axes(axes)

    write_figure_and_metadata(
        figure=figure,
        output_path=output_path,
        figure_id=f"{FIGURE_ID}_comparison",
        title=title,
        script_path=Path(__file__).resolve(),
        repository_root=REPO_ROOT,
        inputs=[dataset.input_metadata for dataset in datasets],
        parameters=metadata_parameters(bin_minutes),
        data_summary={dataset.spec.role: dataset.data_summary for dataset in datasets},
        plot_summary=plot_summary([dataset.profile for dataset in datasets]),
        command=command,
        notes=figure_notes(),
    )


def draw_profile(axes: Axes, profile: NoMoveProfile) -> None:
    visible_profile_mask = np.isfinite(profile.no_move_frequency)

    axes.plot(
        profile.bucket_midpoint_hours[visible_profile_mask],
        profile.no_move_frequency[visible_profile_mask],
        color=profile.color,
        linewidth=1.6,
        label=profile.label,
    )


def configure_profile_axes(axes: Axes) -> None:
    axes.set_xlim(0.0, 24.0)
    axes.set_ylim(0.0, 1.0)
    axes.set_xlabel("Time of day (UTC)")
    axes.set_ylabel("No-move frequency")
    axes.set_xticks(np.arange(0.0, 25.0, 3.0))
    axes.yaxis.set_major_formatter(PercentFormatter(xmax=1.0))

    axes.grid(True, which="both", alpha=0.25)


def metadata_parameters(bin_minutes: int) -> dict[str, Any]:
    return {
        "bin_minutes": bin_minutes,
        "bin_count": MINUTES_PER_DAY // bin_minutes,
        "bucket_assignment_policy": "price_comparison_assigned_to_later_timestamp_utc",
        "comparison_policy": "exact_observed_price_equality",
        "profile_statistic": "no_move_count_divided_by_valid_price_comparison_count",
        "style_provider": "scienceplots",
        "style": SCIENCEPLOTS_STYLE,
        "matplotlib_config_dir": path_for_metadata(
            MATPLOTLIB_CONFIG_DIR,
            repository_root=REPO_ROOT,
        ),
    }


def figure_notes() -> list[str]:
    return [
        "Only timestamp and observed price columns are loaded from parquet inputs.",
        "No-move indicators use exact equality of consecutive observed prices.",
        "Each price comparison is assigned to the UTC time of day of the later event.",
    ]


def profile_summary(profile: NoMoveProfile) -> dict[str, Any]:
    nonempty_bucket_mask = profile.observation_counts > 0
    frequency_values = profile.no_move_frequency[nonempty_bucket_mask]

    return {
        "nonempty_bucket_count": int(np.count_nonzero(nonempty_bucket_mask)),
        "empty_bucket_count": int(np.count_nonzero(~nonempty_bucket_mask)),
        "minimum_bucket_observation_count": int(np.min(profile.observation_counts)),
        "maximum_bucket_observation_count": int(np.max(profile.observation_counts)),
        "minimum_bucket_no_move_frequency": float(np.min(frequency_values)),
        "maximum_bucket_no_move_frequency": float(np.max(frequency_values)),
    }


def plot_summary(profiles: list[NoMoveProfile]) -> dict[str, Any]:
    return {
        "plot_type": "intraday_no_move_frequency_profile",
        "x_axis": "utc_time_of_day",
        "y_axis": "no_move_frequency",
        "curves": [
            {
                "label": profile.label,
                "nonempty_bucket_count": int(
                    np.count_nonzero(profile.observation_counts > 0)
                ),
            }
            for profile in profiles
        ],
    }


if __name__ == "__main__":
    main()
