"""Generate empirical, synthetic, and comparison activity intraday profiles."""

import argparse
import sys
from dataclasses import dataclass
from enum import StrEnum
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


FIGURE_ID: Final[str] = "activity_intraday_profile"
FIGURE_TITLE: Final[str] = "Activity intraday profile"

DEFAULT_EMPIRICAL_INPUT_PATH: Final[Path] = default_empirical_input_path(REPO_ROOT)
DEFAULT_SYNTHETIC_INPUT_PATH: Final[Path] = default_synthetic_input_path(REPO_ROOT)
DEFAULT_OUTPUT_DIR: Final[Path] = REPO_ROOT / "paper" / "assets" / "figures"
DEFAULT_OUTPUT_PREFIX: Final[str] = FIGURE_ID

DEFAULT_BIN_MINUTES: Final[int] = 60
SCIENCEPLOTS_STYLE: Final[list[str]] = ["science", "no-latex"]


class ActivitySide(StrEnum):
    SELL = "sell_side"
    BUY = "buy_side"
    TOTAL = "total"


@dataclass(frozen=True)
class ActivityProfile:
    label: str
    color: str
    side: ActivitySide
    bucket_midpoint_hours: np.ndarray
    activity_counts: np.ndarray


@dataclass(frozen=True)
class ActivityProfileDataset:
    spec: DatasetSpec
    input_metadata: dict[str, Any]
    data_summary: dict[str, Any]
    profiles: dict[ActivitySide, ActivityProfile]


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

    empirical_dataset = load_activity_profile_dataset(
        empirical_spec,
        value_column=EMPIRICAL_PRICE_COLUMN,
        bin_minutes=bin_minutes,
    )

    synthetic_dataset = load_activity_profile_dataset(
        synthetic_spec,
        value_column=SYNTHETIC_PRICE_COLUMN,
        bin_minutes=bin_minutes,
    )

    print("Applying SciencePlots style: " + ", ".join(SCIENCEPLOTS_STYLE))

    plt.style.use(SCIENCEPLOTS_STYLE)

    for side in ActivitySide:
        render_single_dataset_figure(
            dataset=empirical_dataset,
            side=side,
            output_path=output_dir / f"{output_prefix}_{side.value}.pdf",
            title=figure_title(side, "Empirical ES continuous futures"),
            command=sys.argv,
            bin_minutes=bin_minutes,
        )

        render_single_dataset_figure(
            dataset=synthetic_dataset,
            side=side,
            output_path=output_dir / f"{output_prefix}_{side.value}_synthetic.pdf",
            title=figure_title(side, "Janiform synthetic ES"),
            command=sys.argv,
            bin_minutes=bin_minutes,
        )

        render_comparison_figure(
            datasets=[empirical_dataset, synthetic_dataset],
            side=side,
            output_path=output_dir / f"{output_prefix}_{side.value}_comparison.pdf",
            title=figure_title(side, "Empirical vs Janiform synthetic"),
            command=sys.argv,
            bin_minutes=bin_minutes,
        )

    print("Activity intraday profile figure generation complete")


def load_activity_profile_dataset(
    spec: DatasetSpec,
    *,
    value_column: str,
    bin_minutes: int,
) -> ActivityProfileDataset:
    observation_dataset = load_timestamp_value_dataset(
        spec,
        value_column=value_column,
        repository_root=REPO_ROOT,
    )

    print(f"[{spec.label}] Computing activity side proxy from observed price direction")

    timestamps = observation_dataset.timestamps
    prices = observation_dataset.values
    previous_prices = prices.iloc[:-1].reset_index(drop=True)
    current_prices = prices.iloc[1:].reset_index(drop=True)
    comparison_timestamps = timestamps.iloc[1:].reset_index(drop=True)

    valid_comparison_mask = previous_prices.notna() & current_prices.notna()
    valid_previous_prices = previous_prices[valid_comparison_mask]
    valid_current_prices = current_prices[valid_comparison_mask]
    valid_comparison_timestamps = comparison_timestamps[valid_comparison_mask]

    price_delta = valid_current_prices.to_numpy() - valid_previous_prices.to_numpy()

    buy_side_mask = price_delta > 0.0
    sell_side_mask = price_delta < 0.0
    no_move_mask = price_delta == 0.0

    total_bucket_indices = intraday_bucket_indices(timestamps, bin_minutes)
    side_bucket_indices = intraday_bucket_indices(valid_comparison_timestamps, bin_minutes)

    profiles = {
        ActivitySide.SELL: build_activity_profile(
            spec=spec,
            side=ActivitySide.SELL,
            bucket_indices=side_bucket_indices[sell_side_mask],
            bin_minutes=bin_minutes,
        ),
        ActivitySide.BUY: build_activity_profile(
            spec=spec,
            side=ActivitySide.BUY,
            bucket_indices=side_bucket_indices[buy_side_mask],
            bin_minutes=bin_minutes,
        ),
        ActivitySide.TOTAL: build_activity_profile(
            spec=spec,
            side=ActivitySide.TOTAL,
            bucket_indices=total_bucket_indices,
            bin_minutes=bin_minutes,
        ),
    }

    data_summary = observation_dataset.data_summary | {
        "price_column": value_column,
        "side_proxy_policy": "observed_price_direction",
        "side_comparison_count": int(price_delta.size),
        "buy_side_tick_count": int(np.count_nonzero(buy_side_mask)),
        "sell_side_tick_count": int(np.count_nonzero(sell_side_mask)),
        "no_move_tick_count": int(np.count_nonzero(no_move_mask)),
        "total_tick_count": int(timestamps.size),
        "activity_profiles": {
            side.value: profile_summary(profile) for side, profile in profiles.items()
        },
    }

    return ActivityProfileDataset(
        spec=spec,
        input_metadata=observation_dataset.input_metadata,
        data_summary=data_summary,
        profiles=profiles,
    )


def build_activity_profile(
    *,
    spec: DatasetSpec,
    side: ActivitySide,
    bucket_indices: np.ndarray,
    bin_minutes: int,
) -> ActivityProfile:
    print(f"[{spec.label}] Aggregating {side_label(side)} activity counts")

    bucket_count = MINUTES_PER_DAY // bin_minutes
    activity_counts = np.zeros(bucket_count, dtype=np.int64)

    if bucket_indices.size > 0:
        bucket_frame = pd.DataFrame({"bucket_index": bucket_indices})
        grouped_counts = bucket_frame.groupby("bucket_index").size()

        assign_grouped_values(activity_counts, grouped_counts)

    return ActivityProfile(
        label=spec.label,
        color=spec.color,
        side=side,
        bucket_midpoint_hours=intraday_bucket_midpoint_hours(bin_minutes),
        activity_counts=activity_counts,
    )


def render_single_dataset_figure(
    *,
    dataset: ActivityProfileDataset,
    side: ActivitySide,
    output_path: Path,
    title: str,
    command: list[str],
    bin_minutes: int,
) -> None:
    print(f"[{dataset.spec.label}] Rendering {side_label(side)} activity profile")

    profile = dataset.profiles[side]

    figure, axes = plt.subplots(figsize=(6.5, 4.0))
    draw_profile(axes, profile)
    axes.set_title(title)

    configure_profile_axes(axes, side)

    write_figure_and_metadata(
        figure=figure,
        output_path=output_path,
        figure_id=f"{FIGURE_ID}_{side.value}_{dataset.spec.role}",
        title=title,
        script_path=Path(__file__).resolve(),
        repository_root=REPO_ROOT,
        inputs=[dataset.input_metadata],
        parameters=metadata_parameters(bin_minutes, side),
        data_summary={dataset.spec.role: dataset.data_summary},
        plot_summary=plot_summary([profile]),
        command=command,
        notes=figure_notes(),
    )


def render_comparison_figure(
    *,
    datasets: list[ActivityProfileDataset],
    side: ActivitySide,
    output_path: Path,
    title: str,
    command: list[str],
    bin_minutes: int,
) -> None:
    print(f"[comparison] Rendering overlaid {side_label(side)} activity profile")

    profiles = [dataset.profiles[side] for dataset in datasets]
    figure, axes = plt.subplots(figsize=(6.5, 4.0))
    for profile in profiles:
        draw_profile(axes, profile)

    axes.set_title(title)
    axes.legend(frameon=False)

    configure_profile_axes(axes, side)

    write_figure_and_metadata(
        figure=figure,
        output_path=output_path,
        figure_id=f"{FIGURE_ID}_{side.value}_comparison",
        title=title,
        script_path=Path(__file__).resolve(),
        repository_root=REPO_ROOT,
        inputs=[dataset.input_metadata for dataset in datasets],
        parameters=metadata_parameters(bin_minutes, side),
        data_summary={dataset.spec.role: dataset.data_summary for dataset in datasets},
        plot_summary=plot_summary(profiles),
        command=command,
        notes=figure_notes(),
    )


def draw_profile(axes: Axes, profile: ActivityProfile) -> None:
    axes.plot(
        profile.bucket_midpoint_hours,
        profile.activity_counts,
        color=profile.color,
        linewidth=1.6,
        label=profile.label,
    )


def configure_profile_axes(axes: Axes, side: ActivitySide) -> None:
    axes.set_xlim(0.0, 24.0)
    axes.set_ylim(bottom=0.0)
    axes.set_xlabel("Time of day (UTC)")
    axes.set_ylabel(f"{side_label(side)} tick count")
    axes.set_xticks(np.arange(0.0, 25.0, 3.0))

    axes.grid(True, which="both", alpha=0.25)


def metadata_parameters(bin_minutes: int, side: ActivitySide) -> dict[str, Any]:
    return {
        "bin_minutes": bin_minutes,
        "bin_count": MINUTES_PER_DAY // bin_minutes,
        "activity_side": side.value,
        "activity_statistic": "raw_tick_count_per_utc_time_of_day_bucket",
        "side_proxy_policy": "observed_price_direction",
        "buy_side_policy": "current_observed_price_greater_than_previous_observed_price",
        "sell_side_policy": "current_observed_price_less_than_previous_observed_price",
        "total_activity_policy": "all_observed_ticks_by_timestamp_bucket",
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
        "Buy-side and sell-side activity use observed price direction as a side proxy.",
        "Unchanged observed prices contribute to total activity but not to either side.",
    ]


def plot_summary(profiles: list[ActivityProfile]) -> dict[str, Any]:
    return {
        "plot_type": "intraday_activity_count_profile",
        "x_axis": "utc_time_of_day",
        "y_axis": "tick_count",
        "curves": [
            {
                "label": profile.label,
                "activity_side": profile.side.value,
                "total_activity_count": int(np.sum(profile.activity_counts)),
                "maximum_bucket_activity_count": int(np.max(profile.activity_counts)),
            }
            for profile in profiles
        ],
    }


def profile_summary(profile: ActivityProfile) -> dict[str, Any]:
    return {
        "activity_side": profile.side.value,
        "total_activity_count": int(np.sum(profile.activity_counts)),
        "nonempty_bucket_count": int(np.count_nonzero(profile.activity_counts)),
        "empty_bucket_count": int(np.count_nonzero(profile.activity_counts == 0)),
        "minimum_bucket_activity_count": int(np.min(profile.activity_counts)),
        "maximum_bucket_activity_count": int(np.max(profile.activity_counts)),
    }


def figure_title(side: ActivitySide, dataset_label: str) -> str:
    return f"{dataset_label} {side_label(side)} activity intraday profile"


def side_label(side: ActivitySide) -> str:
    match side:
        case ActivitySide.SELL:
            return "sell-side"
        case ActivitySide.BUY:
            return "buy-side"
        case ActivitySide.TOTAL:
            return "total"


if __name__ == "__main__":
    main()
