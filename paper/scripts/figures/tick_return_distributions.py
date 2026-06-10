"""Generate tick and clock-horizon return distribution figures."""

import argparse
import sys
from pathlib import Path
from typing import Any, Final, Optional

from shared.matplotlib_environment import configure_matplotlib_runtime

REPO_ROOT: Final[Path] = Path(__file__).resolve().parents[3]

# Configure MPLCONFIGDIR before importing Matplotlib because home may be read-only.
MATPLOTLIB_CONFIG_DIR: Final[Path] = configure_matplotlib_runtime(REPO_ROOT)

import matplotlib.pyplot as plt
import scienceplots  # noqa: F401

from shared.es_continuous_inputs import (
    EMPIRICAL_PRICE_COLUMN,
    SYNTHETIC_PRICE_COLUMN,
    default_empirical_input_path,
    default_synthetic_input_path,
    es_continuous_dataset_specs,
)
from shared.figure_metadata import path_for_metadata
from shared.figure_output import script_figure_output_dir, write_figure_and_metadata
from shared.return_distribution_plotting import (
    configure_comparison_qq_axes,
    configure_histogram_axes,
    configure_normal_qq_axes,
    draw_histogram_density,
    draw_identity_line,
    draw_qq_curve,
)
from shared.return_distributions import (
    RETURN_HORIZONS,
    RETURN_SCALE_BASIS_POINTS,
    HorizonReturnDataset,
    build_horizon_return_dataset,
    comparison_qq_curve,
    data_summary,
    histogram_density,
    histogram_plot_summary,
    load_price_dataset,
    normal_qq_curve,
    pooled_plot_range,
    qq_plot_summary,
    validate_positive_count,
    validate_tail_probability,
)


FIGURE_ID: Final[str] = "tick_return_distributions"
FIGURE_TITLE: Final[str] = "Tick return distributions"

DEFAULT_EMPIRICAL_INPUT_PATH: Final[Path] = default_empirical_input_path(REPO_ROOT)
DEFAULT_SYNTHETIC_INPUT_PATH: Final[Path] = default_synthetic_input_path(REPO_ROOT)
DEFAULT_OUTPUT_DIR: Final[Path] = REPO_ROOT / "paper" / "assets" / "figures"
DEFAULT_OUTPUT_PREFIX: Final[str] = FIGURE_ID

DEFAULT_HISTOGRAM_BIN_COUNT: Final[int] = 160
DEFAULT_KDE_POINT_COUNT: Final[int] = 240
DEFAULT_QQ_POINT_COUNT: Final[int] = 401
DEFAULT_PLOT_TAIL_PROBABILITY: Final[float] = 0.001

SCIENCEPLOTS_STYLE: Final[list[str]] = ["science", "no-latex"]


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
        "--histogram-bin-count",
        type=int,
        default=DEFAULT_HISTOGRAM_BIN_COUNT,
        help="Number of histogram bins in the pooled plot range.",
    )

    parser.add_argument(
        "--kde-point-count",
        type=int,
        default=DEFAULT_KDE_POINT_COUNT,
        help="Number of points used to draw the binned KDE curve.",
    )

    parser.add_argument(
        "--qq-point-count",
        type=int,
        default=DEFAULT_QQ_POINT_COUNT,
        help="Number of quantile levels used in QQ plots.",
    )

    parser.add_argument(
        "--plot-tail-probability",
        type=float,
        default=DEFAULT_PLOT_TAIL_PROBABILITY,
        help="Two-sided tail probability excluded from histogram and QQ plot ranges.",
    )

    return parser.parse_args()


def main() -> None:
    arguments = parse_args()

    validate_positive_count(arguments.histogram_bin_count, "--histogram-bin-count")
    validate_positive_count(arguments.kde_point_count, "--kde-point-count")
    validate_positive_count(arguments.qq_point_count, "--qq-point-count")
    validate_tail_probability(arguments.plot_tail_probability)

    output_dir = script_figure_output_dir(
        output_root=arguments.output_dir,
        script_path=Path(__file__).resolve(),
    )
    output_prefix = str(arguments.output_prefix)

    empirical_spec, synthetic_spec = es_continuous_dataset_specs(
        empirical_input_path=arguments.empirical_input_path,
        synthetic_input_path=arguments.synthetic_input_path,
    )

    empirical_dataset = load_price_dataset(
        empirical_spec,
        value_column=EMPIRICAL_PRICE_COLUMN,
        repository_root=REPO_ROOT,
    )

    synthetic_dataset = load_price_dataset(
        synthetic_spec,
        value_column=SYNTHETIC_PRICE_COLUMN,
        repository_root=REPO_ROOT,
    )

    print("Applying SciencePlots style: " + ", ".join(SCIENCEPLOTS_STYLE))

    plt.style.use(SCIENCEPLOTS_STYLE)

    for horizon in RETURN_HORIZONS:
        empirical_returns = build_horizon_return_dataset(empirical_dataset, horizon)
        synthetic_returns = build_horizon_return_dataset(synthetic_dataset, horizon)
        horizon_returns = [empirical_returns, synthetic_returns]
        plot_range = pooled_plot_range(
            horizon_returns,
            arguments.plot_tail_probability,
        )

        render_histogram_outputs(
            datasets=horizon_returns,
            plot_range=plot_range,
            output_dir=output_dir,
            output_prefix=output_prefix,
            command=sys.argv,
            histogram_bin_count=arguments.histogram_bin_count,
            kde_point_count=arguments.kde_point_count,
            plot_tail_probability=arguments.plot_tail_probability,
        )

        render_qq_outputs(
            datasets=horizon_returns,
            output_dir=output_dir,
            output_prefix=output_prefix,
            command=sys.argv,
            qq_point_count=arguments.qq_point_count,
            plot_tail_probability=arguments.plot_tail_probability,
        )

    print("Tick return distribution figure generation complete")


def render_histogram_outputs(
    *,
    datasets: list[HorizonReturnDataset],
    plot_range: tuple[float, float],
    output_dir: Path,
    output_prefix: str,
    command: list[str],
    histogram_bin_count: int,
    kde_point_count: int,
    plot_tail_probability: float,
) -> None:
    for dataset, output_suffix in zip(datasets, ("", "_synthetic"), strict=True):
        render_histogram_figure(
            datasets=[dataset],
            plot_range=plot_range,
            output_path=output_dir
            / f"{output_prefix}_{dataset.horizon.output_label}_histogram{output_suffix}.pdf",
            figure_id=f"{FIGURE_ID}_{dataset.horizon.output_label}_histogram_{dataset.price_dataset.spec.role}",
            title=f"{dataset.price_dataset.spec.label} ES {dataset.horizon.label} log-return distribution",
            command=command,
            histogram_bin_count=histogram_bin_count,
            kde_point_count=kde_point_count,
            plot_tail_probability=plot_tail_probability,
        )

    render_histogram_figure(
        datasets=datasets,
        plot_range=plot_range,
        output_path=output_dir
        / f"{output_prefix}_{datasets[0].horizon.output_label}_histogram_comparison.pdf",
        figure_id=f"{FIGURE_ID}_{datasets[0].horizon.output_label}_histogram_comparison",
        title=f"Empirical vs Janiform synthetic ES {datasets[0].horizon.label} log-return distribution",
        command=command,
        histogram_bin_count=histogram_bin_count,
        kde_point_count=kde_point_count,
        plot_tail_probability=plot_tail_probability,
    )


def render_histogram_figure(
    *,
    datasets: list[HorizonReturnDataset],
    plot_range: tuple[float, float],
    output_path: Path,
    figure_id: str,
    title: str,
    command: list[str],
    histogram_bin_count: int,
    kde_point_count: int,
    plot_tail_probability: float,
) -> None:
    print(f"[{figure_id}] Rendering histogram and KDE figure")

    densities = [
        histogram_density(dataset, plot_range, histogram_bin_count, kde_point_count)
        for dataset in datasets
    ]

    figure, axes = plt.subplots(figsize=(6.5, 4.0))
    for density in densities:
        draw_histogram_density(axes, density, comparison=len(densities) > 1)

    axes.set_title(title)
    if len(densities) > 1:
        axes.legend(frameon=False)

    configure_histogram_axes(axes)

    write_figure_and_metadata(
        figure=figure,
        output_path=output_path,
        figure_id=figure_id,
        title=title,
        script_path=Path(__file__).resolve(),
        repository_root=REPO_ROOT,
        inputs=[dataset.price_dataset.input_metadata for dataset in datasets],
        parameters=metadata_parameters(
            histogram_bin_count=histogram_bin_count,
            kde_point_count=kde_point_count,
            qq_point_count=None,
            plot_tail_probability=plot_tail_probability,
        ),
        data_summary=data_summary(datasets),
        plot_summary=histogram_plot_summary(densities, plot_range),
        command=command,
        notes=figure_notes(),
    )


def render_qq_outputs(
    *,
    datasets: list[HorizonReturnDataset],
    output_dir: Path,
    output_prefix: str,
    command: list[str],
    qq_point_count: int,
    plot_tail_probability: float,
) -> None:
    for dataset, output_suffix in zip(datasets, ("", "_synthetic"), strict=True):
        render_normal_qq_figure(
            dataset=dataset,
            output_path=output_dir
            / f"{output_prefix}_{dataset.horizon.output_label}_qq{output_suffix}.pdf",
            figure_id=f"{FIGURE_ID}_{dataset.horizon.output_label}_qq_{dataset.price_dataset.spec.role}",
            title=f"{dataset.price_dataset.spec.label} ES {dataset.horizon.label} normal QQ plot",
            command=command,
            qq_point_count=qq_point_count,
            plot_tail_probability=plot_tail_probability,
        )

    render_comparison_qq_figure(
        datasets=datasets,
        output_path=output_dir
        / f"{output_prefix}_{datasets[0].horizon.output_label}_qq_comparison.pdf",
        figure_id=f"{FIGURE_ID}_{datasets[0].horizon.output_label}_qq_comparison",
        title=f"Empirical vs Janiform synthetic ES {datasets[0].horizon.label} QQ plot",
        command=command,
        qq_point_count=qq_point_count,
        plot_tail_probability=plot_tail_probability,
    )


def render_normal_qq_figure(
    *,
    dataset: HorizonReturnDataset,
    output_path: Path,
    figure_id: str,
    title: str,
    command: list[str],
    qq_point_count: int,
    plot_tail_probability: float,
) -> None:
    print(f"[{figure_id}] Rendering normal QQ figure")

    curve = normal_qq_curve(dataset, qq_point_count, plot_tail_probability)
    figure, axes = plt.subplots(figsize=(5.2, 5.2))

    draw_qq_curve(axes, curve)
    draw_identity_line(axes, curve.x_values, curve.y_values)

    axes.set_title(title)

    configure_normal_qq_axes(axes)

    write_figure_and_metadata(
        figure=figure,
        output_path=output_path,
        figure_id=figure_id,
        title=title,
        script_path=Path(__file__).resolve(),
        repository_root=REPO_ROOT,
        inputs=[dataset.price_dataset.input_metadata],
        parameters=metadata_parameters(
            histogram_bin_count=None,
            kde_point_count=None,
            qq_point_count=qq_point_count,
            plot_tail_probability=plot_tail_probability,
        ),
        data_summary=data_summary([dataset]),
        plot_summary=qq_plot_summary(
            [curve],
            x_axis="normal_reference_quantile_basis_points",
            y_axis="empirical_quantile_basis_points",
        ),
        command=command,
        notes=figure_notes(),
    )


def render_comparison_qq_figure(
    *,
    datasets: list[HorizonReturnDataset],
    output_path: Path,
    figure_id: str,
    title: str,
    command: list[str],
    qq_point_count: int,
    plot_tail_probability: float,
) -> None:
    print(f"[{figure_id}] Rendering empirical-synthetic QQ figure")

    curve = comparison_qq_curve(datasets, qq_point_count, plot_tail_probability)
    figure, axes = plt.subplots(figsize=(5.2, 5.2))

    draw_qq_curve(axes, curve)
    draw_identity_line(axes, curve.x_values, curve.y_values)

    axes.set_title(title)

    configure_comparison_qq_axes(axes)

    write_figure_and_metadata(
        figure=figure,
        output_path=output_path,
        figure_id=figure_id,
        title=title,
        script_path=Path(__file__).resolve(),
        repository_root=REPO_ROOT,
        inputs=[dataset.price_dataset.input_metadata for dataset in datasets],
        parameters=metadata_parameters(
            histogram_bin_count=None,
            kde_point_count=None,
            qq_point_count=qq_point_count,
            plot_tail_probability=plot_tail_probability,
        ),
        data_summary=data_summary(datasets),
        plot_summary=qq_plot_summary(
            [curve],
            x_axis="empirical_quantile_basis_points",
            y_axis="synthetic_quantile_basis_points",
        ),
        command=command,
        notes=figure_notes(),
    )


def metadata_parameters(
    *,
    histogram_bin_count: Optional[int],
    kde_point_count: Optional[int],
    qq_point_count: Optional[int],
    plot_tail_probability: float,
) -> dict[str, Any]:
    parameters: dict[str, Any] = {
        "return_unit": "log_return_basis_points",
        "return_scale": RETURN_SCALE_BASIS_POINTS,
        "horizons": [
            {
                "label": horizon.label,
                "duration_nanos": horizon.duration_nanos,
            }
            for horizon in RETURN_HORIZONS
        ],
        "clock_horizon_return_policy": "last_observed_price_at_or_before_start_plus_horizon",
        "plot_tail_probability": plot_tail_probability,
        "style_provider": "scienceplots",
        "style": SCIENCEPLOTS_STYLE,
        "matplotlib_config_dir": path_for_metadata(
            MATPLOTLIB_CONFIG_DIR,
            repository_root=REPO_ROOT,
        ),
    }

    if histogram_bin_count is not None:
        parameters["histogram_bin_count"] = histogram_bin_count
        parameters["kde_point_count"] = kde_point_count
        parameters["kde_policy"] = "gaussian_kde_approximated_from_histogram_bins"
        parameters["kde_bandwidth_policy"] = "silverman_rule_1_06_sigma_n_to_negative_one_fifth"

    if qq_point_count is not None:
        parameters["qq_point_count"] = qq_point_count

    return parameters


def figure_notes() -> list[str]:
    return [
        "Tick returns are adjacent event-to-event log returns.",
        "Clock-horizon returns use the last observed price at or before the horizon endpoint.",
        "Histogram and KDE x-ranges use pooled empirical and synthetic return quantiles.",
        "Single-dataset QQ plots compare against a fitted normal distribution.",
    ]


if __name__ == "__main__":
    main()
