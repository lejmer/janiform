"""Generate empirical, synthetic, and comparison inter-arrival intraday profiles."""

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

from shared.datasets import DatasetSpec, load_timestamp_dataset
from shared.es_continuous_inputs import (
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


FIGURE_ID: Final[str] = "inter_arrival_intraday_profile"
FIGURE_TITLE: Final[str] = "Inter-arrival intraday profile"

DEFAULT_EMPIRICAL_INPUT_PATH: Final[Path] = default_empirical_input_path(REPO_ROOT)
DEFAULT_SYNTHETIC_INPUT_PATH: Final[Path] = default_synthetic_input_path(REPO_ROOT)
DEFAULT_OUTPUT_DIR: Final[Path] = REPO_ROOT / "paper" / "assets" / "figures"
DEFAULT_OUTPUT_PREFIX: Final[str] = FIGURE_ID

DEFAULT_BIN_MINUTES: Final[int] = 60

SCIENCEPLOTS_STYLE: Final[list[str]] = ["science", "no-latex"]


@dataclass(frozen=True)
class IntradayProfile:
    label: str
    color: str
    bucket_midpoint_hours: np.ndarray
    observation_counts: np.ndarray
    median_inter_arrival_seconds: np.ndarray
    lower_quartile_inter_arrival_seconds: np.ndarray
    upper_quartile_inter_arrival_seconds: np.ndarray


@dataclass(frozen=True)
class IntradayProfileDataset:
    spec: DatasetSpec
    input_metadata: dict[str, Any]
    data_summary: dict[str, Any]
    profile: IntradayProfile


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

    empirical_dataset = load_intraday_profile_dataset(empirical_spec, bin_minutes)
    synthetic_dataset = load_intraday_profile_dataset(synthetic_spec, bin_minutes)

    print("Applying SciencePlots style: " + ", ".join(SCIENCEPLOTS_STYLE))

    plt.style.use(SCIENCEPLOTS_STYLE)

    render_single_dataset_figure(
        dataset=empirical_dataset,
        output_path=output_dir / f"{output_prefix}.pdf",
        title="Empirical ES continuous futures inter-arrival intraday profile",
        command=sys.argv,
        bin_minutes=bin_minutes,
    )

    render_single_dataset_figure(
        dataset=synthetic_dataset,
        output_path=output_dir / f"{output_prefix}_synthetic.pdf",
        title="Janiform synthetic ES inter-arrival intraday profile",
        command=sys.argv,
        bin_minutes=bin_minutes,
    )

    render_comparison_figure(
        datasets=[empirical_dataset, synthetic_dataset],
        output_path=output_dir / f"{output_prefix}_comparison.pdf",
        title="Empirical vs Janiform synthetic inter-arrival intraday profile",
        command=sys.argv,
        bin_minutes=bin_minutes,
    )

    print("Inter-arrival intraday profile figure generation complete")


def load_intraday_profile_dataset(
    spec: DatasetSpec,
    bin_minutes: int,
) -> IntradayProfileDataset:
    timestamp_dataset = load_timestamp_dataset(spec, repository_root=REPO_ROOT)
    timestamps = timestamp_dataset.timestamps

    print(f"[{spec.label}] Computing inter-arrival times")

    timestamp_ns = timestamps.astype("int64").to_numpy()
    inter_arrival_ns = np.diff(timestamp_ns)

    zero_inter_arrival_count = int(np.count_nonzero(inter_arrival_ns == 0))
    negative_inter_arrival_count = int(np.count_nonzero(inter_arrival_ns < 0))

    positive_inter_arrival_mask = inter_arrival_ns > 0
    positive_inter_arrival_ns = inter_arrival_ns[positive_inter_arrival_mask]
    if positive_inter_arrival_ns.size == 0:
        raise ValueError(
            f"{spec.label} dataset has no positive inter-arrival times for plotting"
        )

    positive_inter_arrival_seconds = positive_inter_arrival_ns.astype(np.float64)
    positive_inter_arrival_seconds /= 1_000_000_000.0

    print(f"[{spec.label}] Assigning inter-arrival times to intraday buckets")

    arrival_timestamps = timestamps.iloc[1:].reset_index(drop=True)
    positive_arrival_timestamps = arrival_timestamps.iloc[positive_inter_arrival_mask]
    bucket_indices = intraday_bucket_indices(positive_arrival_timestamps, bin_minutes)
    profile = build_intraday_profile(
        spec=spec,
        bucket_indices=bucket_indices,
        inter_arrival_seconds=positive_inter_arrival_seconds,
        bin_minutes=bin_minutes,
    )

    data_summary = timestamp_dataset.data_summary | {
        "raw_inter_arrival_count": int(inter_arrival_ns.size),
        "positive_inter_arrival_count": int(positive_inter_arrival_ns.size),
        "zero_inter_arrival_count": zero_inter_arrival_count,
        "negative_inter_arrival_count_after_sorting": negative_inter_arrival_count,
        "positive_inter_arrival_seconds": {
            "minimum": float(np.min(positive_inter_arrival_seconds)),
            "maximum": float(np.max(positive_inter_arrival_seconds)),
        },
        "intraday_profile": profile_summary(profile),
    }

    return IntradayProfileDataset(
        spec=spec,
        input_metadata=timestamp_dataset.input_metadata,
        data_summary=data_summary,
        profile=profile,
    )


def build_intraday_profile(
    *,
    spec: DatasetSpec,
    bucket_indices: np.ndarray,
    inter_arrival_seconds: np.ndarray,
    bin_minutes: int,
) -> IntradayProfile:
    print(f"[{spec.label}] Aggregating profile statistics")

    bucket_count = MINUTES_PER_DAY // bin_minutes
    profile_frame = pd.DataFrame(
        {
            "bucket_index": bucket_indices,
            "inter_arrival_seconds": inter_arrival_seconds,
        }
    )
    grouped_inter_arrival_seconds = profile_frame.groupby("bucket_index")[
        "inter_arrival_seconds"
    ]

    observation_counts = np.zeros(bucket_count, dtype=np.int64)
    median_values = np.full(bucket_count, np.nan, dtype=np.float64)
    lower_quartile_values = np.full(bucket_count, np.nan, dtype=np.float64)
    upper_quartile_values = np.full(bucket_count, np.nan, dtype=np.float64)

    assign_grouped_values(observation_counts, grouped_inter_arrival_seconds.size())
    assign_grouped_values(median_values, grouped_inter_arrival_seconds.median())
    assign_grouped_values(lower_quartile_values, grouped_inter_arrival_seconds.quantile(0.25))
    assign_grouped_values(upper_quartile_values, grouped_inter_arrival_seconds.quantile(0.75))

    return IntradayProfile(
        label=spec.label,
        color=spec.color,
        bucket_midpoint_hours=intraday_bucket_midpoint_hours(bin_minutes),
        observation_counts=observation_counts,
        median_inter_arrival_seconds=median_values,
        lower_quartile_inter_arrival_seconds=lower_quartile_values,
        upper_quartile_inter_arrival_seconds=upper_quartile_values,
    )


def render_single_dataset_figure(
    *,
    dataset: IntradayProfileDataset,
    output_path: Path,
    title: str,
    command: list[str],
    bin_minutes: int,
) -> None:
    print(f"[{dataset.spec.label}] Rendering intraday profile")

    figure, axes = plt.subplots(figsize=(6.5, 4.0))
    draw_profile(axes, dataset.profile, include_quartile_band=True)
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
    datasets: list[IntradayProfileDataset],
    output_path: Path,
    title: str,
    command: list[str],
    bin_minutes: int,
) -> None:
    print("[comparison] Rendering overlaid intraday profile")

    figure, axes = plt.subplots(figsize=(6.5, 4.0))
    for dataset in datasets:
        draw_profile(axes, dataset.profile, include_quartile_band=False)

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


def draw_profile(
    axes: Axes,
    profile: IntradayProfile,
    *,
    include_quartile_band: bool,
) -> None:
    visible_profile_mask = np.isfinite(profile.median_inter_arrival_seconds)
    visible_profile_mask &= profile.median_inter_arrival_seconds > 0.0

    x_values = profile.bucket_midpoint_hours[visible_profile_mask]
    median_values = profile.median_inter_arrival_seconds[visible_profile_mask]

    axes.plot(
        x_values,
        median_values,
        color=profile.color,
        linewidth=1.6,
        label=profile.label,
    )

    if include_quartile_band:
        lower_values = profile.lower_quartile_inter_arrival_seconds[visible_profile_mask]
        upper_values = profile.upper_quartile_inter_arrival_seconds[visible_profile_mask]
        axes.fill_between(
            x_values,
            lower_values,
            upper_values,
            color=profile.color,
            alpha=0.18,
            linewidth=0.0,
        )


def configure_profile_axes(axes: Axes) -> None:
    axes.set_xlim(0.0, 24.0)
    axes.set_yscale("log")
    axes.set_xlabel("Time of day (UTC)")
    axes.set_ylabel("Median inter-arrival time (seconds)")
    axes.set_xticks(np.arange(0.0, 25.0, 3.0))

    axes.grid(True, which="both", alpha=0.25)


def metadata_parameters(bin_minutes: int) -> dict[str, Any]:
    return {
        "bin_minutes": bin_minutes,
        "bin_count": MINUTES_PER_DAY // bin_minutes,
        "bucket_assignment_policy": "positive_inter_arrival_assigned_to_later_timestamp_utc",
        "profile_statistic": "median_positive_inter_arrival_seconds",
        "quartile_band": "p25_to_p75_positive_inter_arrival_seconds",
        "style_provider": "scienceplots",
        "style": SCIENCEPLOTS_STYLE,
        "zero_inter_arrival_policy": "excluded_from_log_profile_and_reported_in_metadata",
        "matplotlib_config_dir": path_for_metadata(
            MATPLOTLIB_CONFIG_DIR,
            repository_root=REPO_ROOT,
        ),
    }


def figure_notes() -> list[str]:
    return [
        "Only timestamp columns are loaded from parquet inputs.",
        "Inter-arrival times are assigned to the UTC time of day of the later event.",
        "Zero timestamp deltas cannot be displayed on log-y profile plots.",
    ]


def profile_summary(profile: IntradayProfile) -> dict[str, Any]:
    nonempty_bucket_mask = profile.observation_counts > 0
    median_values = profile.median_inter_arrival_seconds[nonempty_bucket_mask]

    return {
        "nonempty_bucket_count": int(np.count_nonzero(nonempty_bucket_mask)),
        "empty_bucket_count": int(np.count_nonzero(~nonempty_bucket_mask)),
        "minimum_bucket_observation_count": int(np.min(profile.observation_counts)),
        "maximum_bucket_observation_count": int(np.max(profile.observation_counts)),
        "median_inter_arrival_seconds": {
            "minimum": float(np.min(median_values)),
            "maximum": float(np.max(median_values)),
        },
    }


def plot_summary(profiles: list[IntradayProfile]) -> dict[str, Any]:
    return {
        "plot_type": "log_y_intraday_profile",
        "x_axis": "utc_time_of_day",
        "y_axis": "median_positive_inter_arrival_seconds",
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
