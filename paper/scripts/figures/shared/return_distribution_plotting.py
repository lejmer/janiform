"""Shared Matplotlib drawing helpers for return-distribution figures."""

import numpy as np
from matplotlib.axes import Axes

from .return_distributions import HistogramDensity, QqCurve


def draw_histogram_density(
    axes: Axes,
    density: HistogramDensity,
    *,
    comparison: bool,
) -> None:
    if comparison:
        axes.plot(
            density.bin_centers,
            density.histogram_density,
            color=density.color,
            linewidth=0.8,
            alpha=0.55,
            label=f"{density.label} histogram",
        )
    else:
        axes.bar(
            density.bin_centers,
            density.histogram_density,
            width=density.bin_width,
            color=density.color,
            alpha=0.28,
            linewidth=0.0,
            label=f"{density.label} histogram",
        )

    axes.plot(
        density.kde_x_values,
        density.kde_density,
        color=density.color,
        linewidth=1.6,
        label=f"{density.label} KDE",
    )


def draw_qq_curve(axes: Axes, curve: QqCurve) -> None:
    axes.scatter(
        curve.x_values,
        curve.y_values,
        color=curve.color,
        s=8.0,
        alpha=0.75,
        linewidths=0.0,
        label=curve.label,
    )


def draw_identity_line(axes: Axes, x_values: np.ndarray, y_values: np.ndarray) -> None:
    line_minimum = float(min(np.min(x_values), np.min(y_values)))
    line_maximum = float(max(np.max(x_values), np.max(y_values)))

    axes.plot(
        [line_minimum, line_maximum],
        [line_minimum, line_maximum],
        color="#555555",
        linewidth=0.9,
        linestyle="--",
    )


def configure_histogram_axes(axes: Axes) -> None:
    axes.set_xlabel("Log return (basis points)")
    axes.set_ylabel("Density")
    axes.grid(True, which="both", alpha=0.25)


def configure_normal_qq_axes(axes: Axes) -> None:
    axes.set_xlabel("Fitted normal quantile (basis points)")
    axes.set_ylabel("Empirical quantile (basis points)")
    axes.grid(True, which="both", alpha=0.25)


def configure_comparison_qq_axes(axes: Axes) -> None:
    axes.set_xlabel("Empirical quantile (basis points)")
    axes.set_ylabel("Janiform synthetic quantile (basis points)")
    axes.grid(True, which="both", alpha=0.25)
