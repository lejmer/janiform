import os
import tempfile

from pathlib import Path
from typing import Any

if "MPLCONFIGDIR" not in os.environ:
    matplotlib_cache_directory = Path(tempfile.gettempdir()) / "janiform-mpl"
    matplotlib_cache_directory.mkdir(parents=True, exist_ok=True)
    os.environ["MPLCONFIGDIR"] = str(matplotlib_cache_directory)

from matplotlib.figure import Figure

from .artifacts import WorkbenchArtifacts, _artifact_list


_STAGE_COLORS: dict[str, str] = {
    "hawkes": "#0f766e",
    "return_components": "#b45309",
    "joint_refinement": "#1d4ed8",
}


def create_frontier_figure(artifacts: WorkbenchArtifacts) -> Figure:
    figure = Figure(figsize=(12, 5), constrained_layout=True)
    scalar_axis = figure.add_subplot(1, 2, 1)
    counts_axis = figure.add_subplot(1, 2, 2)
    frontier_entries = _artifact_list(artifacts.candidate_frontier, "frontier_entries")

    grouped_entries: dict[str, list[dict[str, Any]]] = {}
    for entry in frontier_entries:
        stage = entry.get("stage", "unknown")
        if isinstance(stage, str):
            grouped_entries.setdefault(stage, []).append(entry)

    for stage, stage_entries in sorted(grouped_entries.items()):
        scalar_axis.scatter(
            [
                float(entry.get("lexicographic_rank", 0))
                for entry in stage_entries
            ],
            [
                float(entry.get("scalar_tie_break", 0.0))
                for entry in stage_entries
            ],
            color=_STAGE_COLORS.get(stage, "#475569"),
            alpha=0.9,
            label=stage,
            marker="o",
        )

    scalar_axis.set_title("Candidate Frontier")
    scalar_axis.set_xlabel("Lexicographic Rank")
    scalar_axis.set_ylabel("Scalar Tie Break")
    scalar_axis.legend(loc="best")
    scalar_axis.grid(alpha=0.3)

    stages = sorted(grouped_entries)
    counts_axis.bar(
        stages,
        [len(grouped_entries[stage]) for stage in stages],
        color=[_STAGE_COLORS.get(stage, "#475569") for stage in stages],
    )
    counts_axis.set_title("Candidates By Stage")
    counts_axis.set_ylabel("Count")
    counts_axis.tick_params(axis="x", rotation=20)
    counts_axis.grid(axis="y", alpha=0.3)

    return figure


def create_runtime_figure(artifacts: WorkbenchArtifacts) -> Figure:
    figure = Figure(figsize=(10, 4), constrained_layout=True)
    axis = figure.add_subplot(1, 1, 1)
    entries = _artifact_list(artifacts.runtime_profile, "entries")
    component_ids = [
        str(entry.get("component_id", "unknown"))
        for entry in entries
    ]
    elapsed_seconds = [
        float(entry.get("elapsed_seconds", 0.0))
        for entry in entries
    ]

    axis.bar(component_ids, elapsed_seconds, color="#1d4ed8")
    axis.set_title("Runtime Profile")
    axis.set_ylabel("Elapsed Seconds")
    axis.tick_params(axis="x", rotation=20)
    axis.grid(axis="y", alpha=0.3)

    return figure


def create_consistency_figure(artifacts: WorkbenchArtifacts) -> Figure:
    figure = Figure(figsize=(15, 5), constrained_layout=True)
    delta_axis = figure.add_subplot(1, 3, 1)
    scatter_axis = figure.add_subplot(1, 3, 2)
    event_axis = figure.add_subplot(1, 3, 3)
    metrics = sorted(
        _artifact_list(artifacts.coarse_tick_consistency, "metrics"),
        key=lambda metric: float(metric.get("normalized_delta", 0.0)),
        reverse=True,
    )
    return_metrics = [
        metric
        for metric in metrics
        if not str(metric.get("metric_id", "")).endswith("event_count")
    ]
    event_count_metrics = [
        metric
        for metric in metrics
        if str(metric.get("metric_id", "")).endswith("event_count")
    ]
    top_return_metrics = return_metrics[:12]

    metric_labels = [
        str(metric.get("metric_id", "unknown"))
        for metric in top_return_metrics
    ]
    normalized_deltas = [
        float(metric.get("normalized_delta", 0.0))
        for metric in top_return_metrics
    ]
    delta_axis.barh(metric_labels, normalized_deltas, color="#b91c1c")
    delta_axis.set_title("Largest Return-Metric Deltas")
    delta_axis.set_xlabel("Normalized Delta")
    delta_axis.invert_yaxis()
    delta_axis.grid(axis="x", alpha=0.3)
    if not top_return_metrics:
        delta_axis.text(
            0.5,
            0.5,
            "No return metrics available",
            ha="center",
            va="center",
            transform=delta_axis.transAxes,
        )

    scatter_axis.scatter(
        [float(metric.get("coarse_value", 0.0)) for metric in return_metrics],
        [float(metric.get("tick_value", 0.0)) for metric in return_metrics],
        color="#0f766e",
        alpha=0.8,
    )
    if return_metrics:
        scatter_axis.set_xscale("log")
        scatter_axis.set_yscale("log")
    scatter_axis.set_title("Coarse Versus Tick Returns")
    scatter_axis.set_xlabel("Coarse Value")
    scatter_axis.set_ylabel("Tick Value")
    scatter_axis.grid(alpha=0.3)

    event_labels = [
        str(metric.get("metric_id", "unknown"))
        for metric in event_count_metrics
    ]
    event_deltas = [
        float(metric.get("normalized_delta", 0.0))
        for metric in event_count_metrics
    ]
    event_axis.barh(event_labels, event_deltas, color="#1d4ed8")
    event_axis.set_title("Event Count Deltas")
    event_axis.set_xlabel("Normalized Delta")
    event_axis.invert_yaxis()
    event_axis.grid(axis="x", alpha=0.3)
    if not event_count_metrics:
        event_axis.text(
            0.5,
            0.5,
            "No event-count metrics available",
            ha="center",
            va="center",
            transform=event_axis.transAxes,
        )

    return figure


def create_search_surface_figure(artifacts: WorkbenchArtifacts) -> Figure:
    figure = Figure(figsize=(12, 5), constrained_layout=True)
    surface_axis = figure.add_subplot(1, 2, 1)
    objective_axis = figure.add_subplot(1, 2, 2)
    surfaces = _artifact_list(artifacts.search_surface_manifest, "surfaces")

    grouped_surfaces: dict[str, list[dict[str, Any]]] = {}
    for surface in surfaces:
        surface_id = surface.get("surface_id", "unknown")
        if isinstance(surface_id, str):
            grouped_surfaces.setdefault(surface_id, []).append(surface)

    for surface_id, surface_points in sorted(grouped_surfaces.items()):
        x_values = [float(point.get("x_value", 0.0)) for point in surface_points]
        y_values = [
            float(point.get("y_value", 0.0))
            for point in surface_points
            if isinstance(point.get("y_value"), int | float)
        ]
        objective_values = [
            float(point.get("objective_value", 0.0)) for point in surface_points
        ]
        if y_values:
            surface_axis.scatter(
                x_values,
                y_values,
                alpha=0.7,
                label=surface_id,
            )
        objective_axis.plot(
            x_values,
            objective_values,
            marker="o",
            alpha=0.8,
            label=surface_id,
        )

    surface_axis.set_title("Search Surface Coordinates")
    surface_axis.set_xlabel("X")
    surface_axis.set_ylabel("Y")
    surface_axis.grid(alpha=0.3)
    if grouped_surfaces:
        surface_axis.legend(loc="best", fontsize="small")

    objective_axis.set_title("Search Surface Objective Values")
    objective_axis.set_xlabel("X")
    objective_axis.set_ylabel("Objective")
    objective_axis.grid(alpha=0.3)
    if grouped_surfaces:
        objective_axis.legend(loc="best", fontsize="small")

    return figure


def save_all_figures(artifacts: WorkbenchArtifacts, output_directory: Path) -> dict[str, str]:
    output_directory.mkdir(parents=True, exist_ok=True)

    figures = {
        "candidate_frontier": create_frontier_figure(artifacts),
        "runtime_profile": create_runtime_figure(artifacts),
        "coarse_tick_consistency": create_consistency_figure(artifacts),
        "search_surface_manifest": create_search_surface_figure(artifacts),
    }
    written_paths: dict[str, str] = {}
    for figure_name, figure in figures.items():
        output_path = output_directory / f"{figure_name}.png"
        figure.savefig(output_path, dpi=160)
        written_paths[figure_name] = str(output_path)

    return written_paths
