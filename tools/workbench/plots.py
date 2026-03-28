from __future__ import annotations

import math
import os
import tempfile
from collections import defaultdict
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Optional, cast

import numpy as np
from numpy.typing import NDArray

if "MPLCONFIGDIR" not in os.environ:
    matplotlib_cache_directory = Path(tempfile.gettempdir()) / "janiform-mpl"
    matplotlib_cache_directory.mkdir(parents=True, exist_ok=True)
    os.environ["MPLCONFIGDIR"] = str(matplotlib_cache_directory)

from matplotlib.figure import Figure

from janiform._timestamps import utc_microseconds_to_datetime
from janiform.calibration.fit.model import (
    _materialize_mean_reversion_anchor_value,
    _mean_reversion_fitted_parameters,
)
from janiform.contracts import MeanReversionFittedParameters

from .artifacts import WorkbenchArtifacts, _artifact_list
from .fit_context import (
    WorkbenchObservedSimulationComparison,
    WorkbenchPriceSeries,
    build_observed_simulation_comparison,
)


_STAGE_COLORS: dict[str, str] = {
    "hawkes": "#0f766e",
    "return_components": "#b45309",
    "joint_refinement": "#1d4ed8",
}

_FAMILY_COLORS: dict[str, str] = {
    "diagnostic": "#475569",
    "activity_level": "#1d4ed8",
    "activity_shape": "#0f766e",
    "activity_clustering": "#0d9488",
    "activity_vs_volatility_coupling": "#0891b2",
    "return_center_mass": "#b45309",
    "return_scale": "#ea580c",
    "return_asymmetry": "#dc2626",
    "return_tails": "#7c3aed",
    "event_conditioned_activity_response": "#65a30d",
    "event_conditioned_return_behavior": "#16a34a",
}

_SUPPORTED_ANALYSIS_HORIZONS_MINUTES: tuple[int, ...] = (15, 30, 60, 240, 1440)
_OBSERVED_SIMULATION_ANALYSIS_CACHE: dict[int, WorkbenchObservedSimulationAnalysis] = {}
_OBSERVED_SIMULATION_ANALYSIS_CACHE_MAX_ENTRIES = 8
_MAX_PATH_PLOT_POINTS = 4_000
_MAX_SCATTER_POINTS = 8_000
_MEAN_REVERSION_BIN_COUNT_LOWER_BOUND = 5
_MEAN_REVERSION_BIN_COUNT_UPPER_BOUND = 10
_MEAN_REVERSION_BIN_MINIMUM_SAMPLE_COUNT = 10
_MEAN_REVERSION_TAIL_QUANTILE_LOWER_BOUND = 0.02
_MEAN_REVERSION_TAIL_QUANTILE_UPPER_BOUND = 0.98


@dataclass(frozen=True, slots=True)
class WorkbenchMeanReversionSeriesAnalysis:
    timestamp_microseconds_utc: NDArray[np.int64]
    log_prices: NDArray[np.float64]
    anchor_values: NDArray[np.float64]
    deviations_from_anchor: NDArray[np.float64]


@dataclass(frozen=True, slots=True)
class WorkbenchBucketSeriesAnalysis:
    bucket_timestamps_utc: tuple[datetime, ...]
    close_prices: NDArray[np.float64]
    returns: NDArray[np.float64]
    event_counts: NDArray[np.int64]


@dataclass(frozen=True, slots=True)
class WorkbenchSeriesAnalysis:
    timestamp_microseconds_utc: NDArray[np.int64]
    prices: NDArray[np.float64]
    normalized_prices: NDArray[np.float64]
    returns: NDArray[np.float64]
    hourly_event_counts: dict[datetime, int]
    hourly_activity_profile: NDArray[np.float64]
    hourly_absolute_return_profile: NDArray[np.float64]
    bucket_series_by_horizon: dict[int, WorkbenchBucketSeriesAnalysis]
    rolling_activity_points: NDArray[np.float64]


@dataclass(frozen=True, slots=True)
class WorkbenchObservedSimulationAnalysis:
    observed: WorkbenchSeriesAnalysis
    simulated: WorkbenchSeriesAnalysis
    simulated_seeded_15m: WorkbenchSeriesAnalysis
    simulated_coarse_15m: WorkbenchSeriesAnalysis
    available_horizon_minutes: tuple[int, ...]
    observed_mean_reversion: Optional[WorkbenchMeanReversionSeriesAnalysis] = None
    simulated_mean_reversion: Optional[WorkbenchMeanReversionSeriesAnalysis] = None


def create_selected_candidate_diagnostics_figure(artifacts: WorkbenchArtifacts) -> Figure:
    figure = Figure(figsize=(14, 8), constrained_layout=True)
    family_axis = figure.add_subplot(2, 2, 1)
    scatter_axis = figure.add_subplot(2, 2, 2)
    metric_axis = figure.add_subplot(2, 2, 3)
    summary_axis = figure.add_subplot(2, 2, 4)

    selected_candidate_entry = _selected_candidate_entry(artifacts)
    if selected_candidate_entry is None:
        _render_unavailable_axes(
            (family_axis, scatter_axis, metric_axis, summary_axis),
            "Selected candidate diagnostics are unavailable for this run.",
        )
        return figure

    family_metrics = [
        metric
        for metric in selected_candidate_entry.get("family_metrics", [])
        if isinstance(metric, dict)
    ]

    family_distances = _aggregate_family_distances(family_metrics)

    family_names = sorted(
        family_distances,
        key=lambda family_name: family_distances[family_name],
        reverse=True,
    )

    family_axis.barh(
        family_names,
        [family_distances[family_name] for family_name in family_names],
        color=[_FAMILY_COLORS.get(family_name, "#475569") for family_name in family_names],
    )
    family_axis.set_title("Selected Candidate By Target Family")
    family_axis.set_xlabel("Mean Normalized Distance")
    family_axis.invert_yaxis()
    family_axis.grid(axis="x", alpha=0.3)

    observed_values: list[float] = []
    simulated_values: list[float] = []
    decisive_metric_ids = set(_decisive_metric_ids(artifacts))
    decisive_points_x: list[float] = []
    decisive_points_y: list[float] = []

    for metric in family_metrics:
        if not _metric_has_comparable_observed_simulated_values(metric):
            continue

        observed_value = metric.get("observed_value")
        simulated_value = metric.get("simulated_value")
        metric_id = metric.get("metric_id")

        if not isinstance(observed_value, int | float) or not isinstance(simulated_value, int | float):
            continue

        observed_values.append(float(observed_value))
        simulated_values.append(float(simulated_value))

        if isinstance(metric_id, str) and metric_id in decisive_metric_ids:
            decisive_points_x.append(float(observed_value))
            decisive_points_y.append(float(simulated_value))

    if observed_values and simulated_values:
        scatter_axis.scatter(
            observed_values,
            simulated_values,
            color="#334155",
            alpha=0.8,
            label="all metrics",
        )

        if decisive_points_x and decisive_points_y:
            scatter_axis.scatter(
                decisive_points_x,
                decisive_points_y,
                color="#dc2626",
                alpha=0.95,
                label="decisive metrics",
                marker="x",
                s=80,
            )

        identity_lower_bound = min([*observed_values, *simulated_values])
        identity_upper_bound = max([*observed_values, *simulated_values])

        scatter_axis.plot(
            [identity_lower_bound, identity_upper_bound],
            [identity_lower_bound, identity_upper_bound],
            color="#94a3b8",
            linestyle="--",
            linewidth=1.0,
        )

        _configure_metric_scatter_axis_scaling(
            scatter_axis,
            observed_values,
            simulated_values,
        )
    else:
        message = (
            "This selected stage exposes family-distance scores rather than direct "
            "observed-versus-simulated value pairs."
        )
        if _selected_candidate_entry_uses_legacy_joint_distance_only_metrics(
            selected_candidate_entry
        ):
            message = (
                "This run stores legacy joint-refinement family-distance scores only. "
                "Re-run calibration with the updated diagnostics contract to see "
                "observed-versus-simulated pairs."
            )

        scatter_axis.text(
            0.5,
            0.5,
            message,
            ha="center",
            va="center",
            wrap=True,
            transform=scatter_axis.transAxes,
        )
        scatter_axis.set_xticks([])
        scatter_axis.set_yticks([])

    scatter_axis.set_title("Observed Versus Simulated Metrics")
    scatter_axis.set_xlabel("Observed")
    scatter_axis.set_ylabel("Simulated")
    scatter_axis.grid(alpha=0.3)

    if observed_values and simulated_values:
        scatter_axis.legend(loc="best")

    top_metric_misses = sorted(
        (
            metric
            for metric in family_metrics
            if isinstance(metric.get("normalized_distance"), int | float)
        ),
        key=lambda metric: float(metric.get("normalized_distance", 0.0)),
        reverse=True,
    )[:10]

    metric_axis.barh(
        [
            _metric_label(metric)
            for metric in top_metric_misses
        ],
        [
            float(metric.get("normalized_distance", 0.0))
            for metric in top_metric_misses
        ],
        color=[
            "#dc2626"
            if isinstance(metric.get("metric_id"), str) and metric.get("metric_id") in decisive_metric_ids
            else _FAMILY_COLORS.get(cast(str, metric.get("target_family", "diagnostic")), "#475569")
            for metric in top_metric_misses
        ],
    )
    metric_axis.set_title("Largest Selected-Candidate Misses")
    metric_axis.set_xlabel("Normalized Distance")
    metric_axis.invert_yaxis()
    metric_axis.grid(axis="x", alpha=0.3)

    summary_axis.set_title("Selection Summary")
    summary_axis.axis("off")

    decision = artifacts.selection_rationale.get("decision", {})

    summary_axis.text(
        0.0,
        1.0,
        "\n".join(
            [
                f"candidate: {selected_candidate_entry.get('candidate_id', 'unknown')}",
                f"stage: {selected_candidate_entry.get('stage', 'unknown')}",
                f"tier: {selected_candidate_entry.get('evaluation_tier', 'unknown')}",
                f"admissible: {selected_candidate_entry.get('admissible', False)}",
                f"valid: {selected_candidate_entry.get('valid', False)}",
                f"selection stage: {decision.get('selection_stage', 'unknown')}",
                f"selection policy: {decision.get('selection_policy', 'unknown')}",
                f"decisive metrics: {', '.join(_decisive_metric_ids(artifacts)) or 'none'}",
                f"pareto frontier count: {decision.get('pareto_frontier_count', 'unknown')}",
                f"admissible count: {decision.get('admissible_candidate_count', 'unknown')}",
            ]
        ),
        va="top",
        ha="left",
        family="monospace",
    )

    return figure


def create_observed_targets_figure(artifacts: WorkbenchArtifacts) -> Figure:
    figure = Figure(figsize=(12, 5), constrained_layout=True)

    counts_axis = figure.add_subplot(1, 2, 1)
    sample_axis = figure.add_subplot(1, 2, 2)

    targets = _artifact_list(artifacts.observed_targets, "targets")
    if not targets:
        _render_unavailable_axes(
            (counts_axis, sample_axis),
            "Observed targets are unavailable for this run.",
        )

        return figure

    targets_by_family: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for target in targets:
        family_name = str(target.get("target_family", "unknown"))
        targets_by_family[family_name].append(target)

    family_names = sorted(targets_by_family)

    counts_axis.bar(
        family_names,
        [len(targets_by_family[family_name]) for family_name in family_names],
        color=[_FAMILY_COLORS.get(family_name, "#475569") for family_name in family_names],
    )
    counts_axis.set_title("Observed Targets By Family")
    counts_axis.set_ylabel("Target Count")
    counts_axis.tick_params(axis="x", rotation=25)
    counts_axis.grid(axis="y", alpha=0.3)

    for family_name in family_names:
        family_targets = targets_by_family[family_name]

        effective_sample_sizes = [
            float(target.get("effective_sample_size", 0.0))
            for target in family_targets
            if isinstance(target.get("effective_sample_size"), int | float)
        ]

        stability_scores = [
            float(target.get("stability_score", 0.0))
            for target in family_targets
            if isinstance(target.get("stability_score"), int | float)
        ]

        sample_axis.scatter(
            effective_sample_sizes,
            stability_scores,
            color=_FAMILY_COLORS.get(family_name, "#475569"),
            alpha=0.8,
            label=family_name,
        )

    sample_axis.set_title("Observed Target Stability")
    sample_axis.set_xlabel("Effective Sample Size")
    sample_axis.set_ylabel("Stability Score")
    sample_axis.grid(alpha=0.3)

    if family_names:
        sample_axis.legend(loc="best", fontsize="small")

    return figure


def create_frontier_figure(artifacts: WorkbenchArtifacts) -> Figure:
    figure = Figure(figsize=(15, 5), constrained_layout=True)

    scatter_axis = figure.add_subplot(1, 3, 1)
    counts_axis = figure.add_subplot(1, 3, 2)
    best_axis = figure.add_subplot(1, 3, 3)

    frontier_entries = _artifact_list(artifacts.candidate_frontier, "frontier_entries")
    if not frontier_entries:
        _render_unavailable_axes(
            (scatter_axis, counts_axis, best_axis),
            "Candidate frontier diagnostics are unavailable for this run.",
        )

        return figure

    selected_candidate_id = artifacts.selected_candidate_id

    stages = sorted({str(entry.get("stage", "unknown")) for entry in frontier_entries})
    for stage in stages:
        stage_entries = [
            entry for entry in frontier_entries if str(entry.get("stage", "unknown")) == stage
        ]

        rejected_entries = [entry for entry in stage_entries if not bool(entry.get("admissible"))]
        admissible_entries = [entry for entry in stage_entries if bool(entry.get("admissible"))]

        if rejected_entries:
            scatter_axis.scatter(
                [float(entry.get("lexicographic_rank", 0.0)) for entry in rejected_entries],
                [float(entry.get("scalar_tie_break", 0.0)) for entry in rejected_entries],
                color="#cbd5e1",
                alpha=0.65,
                label=f"{stage} rejected",
                marker="o",
            )

        if admissible_entries:
            scatter_axis.scatter(
                [float(entry.get("lexicographic_rank", 0.0)) for entry in admissible_entries],
                [float(entry.get("scalar_tie_break", 0.0)) for entry in admissible_entries],
                color=_STAGE_COLORS.get(stage, "#475569"),
                alpha=0.9,
                label=f"{stage} admissible",
                marker="o",
            )

    selected_candidate_entry = _selected_candidate_entry(artifacts)
    if selected_candidate_entry is not None:
        scatter_axis.scatter(
            [float(selected_candidate_entry.get("lexicographic_rank", 0.0))],
            [float(selected_candidate_entry.get("scalar_tie_break", 0.0))],
            color="#f59e0b",
            edgecolors="#111827",
            linewidths=1.0,
            s=140,
            label=f"selected {selected_candidate_id}",
            marker="*",
        )

    scatter_axis.set_title("Candidate Frontier")
    scatter_axis.set_xlabel("Lexicographic Rank")
    scatter_axis.set_ylabel("Scalar Tie Break")
    scatter_axis.grid(alpha=0.3)
    scatter_axis.legend(loc="best", fontsize="small")

    counts_axis.bar(
        stages,
        [
            sum(
                1
                for entry in frontier_entries
                if str(entry.get("stage", "unknown")) == stage and bool(entry.get("admissible"))
            )
            for stage in stages
        ],
        color=[_STAGE_COLORS.get(stage, "#475569") for stage in stages],
        label="admissible",
    )

    counts_axis.bar(
        stages,
        [
            sum(
                1
                for entry in frontier_entries
                if str(entry.get("stage", "unknown")) == stage and not bool(entry.get("admissible"))
            )
            for stage in stages
        ],
        bottom=[
            sum(
                1
                for entry in frontier_entries
                if str(entry.get("stage", "unknown")) == stage and bool(entry.get("admissible"))
            )
            for stage in stages
        ],
        color="#cbd5e1",
        label="rejected",
    )

    counts_axis.set_title("Admissibility By Stage")
    counts_axis.set_ylabel("Candidate Count")
    counts_axis.tick_params(axis="x", rotation=20)
    counts_axis.grid(axis="y", alpha=0.3)
    counts_axis.legend(loc="best")

    best_scalar_tie_break_by_stage: dict[str, float] = {}
    for stage in stages:
        stage_entries = [
            entry
            for entry in frontier_entries
            if str(entry.get("stage", "unknown")) == stage and bool(entry.get("admissible"))
        ]
        if not stage_entries:
            continue

        best_scalar_tie_break_by_stage[stage] = min(
            float(entry.get("scalar_tie_break", 0.0)) for entry in stage_entries
        )

    best_axis.bar(
        list(best_scalar_tie_break_by_stage.keys()),
        list(best_scalar_tie_break_by_stage.values()),
        color=[_STAGE_COLORS.get(stage, "#475569") for stage in best_scalar_tie_break_by_stage],
    )
    best_axis.set_title("Best Admissible Tie Break By Stage")
    best_axis.set_ylabel("Scalar Tie Break")
    best_axis.tick_params(axis="x", rotation=20)
    best_axis.grid(axis="y", alpha=0.3)

    return figure


def create_runtime_figure(artifacts: WorkbenchArtifacts) -> Figure:
    figure = Figure(figsize=(12, 4), constrained_layout=True)

    bar_axis = figure.add_subplot(1, 2, 1)
    cumulative_axis = figure.add_subplot(1, 2, 2)

    entries = _artifact_list(artifacts.runtime_profile, "entries")

    component_ids = [str(entry.get("component_id", "unknown")) for entry in entries]

    elapsed_seconds = [
        float(entry.get("elapsed_seconds", 0.0))
        for entry in entries
        if isinstance(entry.get("elapsed_seconds"), int | float)
    ]

    bar_axis.bar(component_ids, elapsed_seconds, color="#1d4ed8")
    bar_axis.set_title("Runtime Profile")
    bar_axis.set_ylabel("Elapsed Seconds")
    bar_axis.tick_params(axis="x", rotation=20)
    bar_axis.grid(axis="y", alpha=0.3)

    cumulative_totals: list[float] = []
    running_total = 0.0
    for elapsed_seconds_value in elapsed_seconds:
        running_total += elapsed_seconds_value
        cumulative_totals.append(running_total)

    cumulative_axis.plot(component_ids, cumulative_totals, color="#0f766e", marker="o")
    cumulative_axis.set_title("Cumulative Runtime")
    cumulative_axis.set_ylabel("Elapsed Seconds")
    cumulative_axis.tick_params(axis="x", rotation=20)
    cumulative_axis.grid(alpha=0.3)

    cpu_count = artifacts.runtime_profile.get("cpu_count")
    if isinstance(cpu_count, int):
        cumulative_axis.text(
            0.99,
            0.02,
            f"cpu count: {cpu_count}",
            ha="right",
            va="bottom",
            transform=cumulative_axis.transAxes,
        )

    return figure


def create_run_progress_figure(artifacts: WorkbenchArtifacts) -> Figure:
    figure = Figure(figsize=(15, 9), constrained_layout=True)

    objective_axis = figure.add_subplot(2, 2, 1)
    admissible_axis = figure.add_subplot(2, 2, 2)
    stage_axis = figure.add_subplot(2, 2, 3)
    rejection_axis = figure.add_subplot(2, 2, 4)

    frontier_entries = _artifact_list(artifacts.candidate_frontier, "frontier_entries")
    if not frontier_entries:
        _render_unavailable_axes(
            (objective_axis, admissible_axis, stage_axis, rejection_axis),
            "Run-progress diagnostics are unavailable for this run.",
        )

        return figure

    candidate_order = list(range(1, len(frontier_entries) + 1))
    objective_values = [_candidate_objective_value(entry) for entry in frontier_entries]
    admissible_flags = [bool(entry.get("admissible")) for entry in frontier_entries]

    stage_names = sorted(
        {
            str(entry.get("stage", "unknown"))
            for entry in frontier_entries
        }
    )

    valid_objective_points = [
        (position, objective_value)
        for position, objective_value in zip(candidate_order, objective_values, strict=False)
        if objective_value is not None
    ]

    if valid_objective_points:
        objective_axis.scatter(
            [position for position, _ in valid_objective_points],
            [objective_value for _, objective_value in valid_objective_points],
            c=[
                "#1d4ed8" if admissible_flags[position - 1] else "#dc2626"
                for position, _ in valid_objective_points
            ],
            alpha=0.75,
        )

        objective_axis.plot(
            [position for position, _ in valid_objective_points],
            _running_best(
                [objective_value for _, objective_value in valid_objective_points]
            ),
            color="#111827",
            linewidth=1.5,
            label="running best",
        )

        objective_axis.legend(loc="best")

    objective_axis.set_title("Objective Over Candidate Order")
    objective_axis.set_xlabel("Candidate order")
    objective_axis.set_ylabel("Objective")
    objective_axis.grid(alpha=0.3)

    admissible_objective_values = [
        objective_value
        for objective_value, admissible in zip(objective_values, admissible_flags, strict=False)
        if objective_value is not None and admissible
    ]
    admissible_candidate_order = [
        position
        for position, objective_value, admissible in zip(
            candidate_order,
            objective_values,
            admissible_flags,
            strict=False,
        )
        if objective_value is not None and admissible
    ]
    if admissible_objective_values:
        admissible_axis.plot(
            admissible_candidate_order,
            _running_best(admissible_objective_values),
            color="#0f766e",
            linewidth=1.8,
            marker="o",
        )

    admissible_axis.set_title("Best Admissible Objective")
    admissible_axis.set_xlabel("Candidate order")
    admissible_axis.set_ylabel("Best admissible objective")
    admissible_axis.grid(alpha=0.3)

    for stage_name in stage_names:
        cumulative_counts: list[int] = []
        running_count = 0

        for entry in frontier_entries:
            if str(entry.get("stage", "unknown")) == stage_name:
                running_count += 1

            cumulative_counts.append(running_count)

        stage_axis.plot(
            candidate_order,
            cumulative_counts,
            label=stage_name,
            color=_STAGE_COLORS.get(stage_name, "#475569"),
        )

    stage_axis.set_title("Candidate Counts By Stage")
    stage_axis.set_xlabel("Candidate order")
    stage_axis.set_ylabel("Cumulative candidates")
    stage_axis.grid(alpha=0.3)

    if stage_names:
        stage_axis.legend(loc="best", fontsize="small")

    for stage_name in stage_names:
        cumulative_rejections: list[int] = []
        running_count = 0

        for entry in frontier_entries:
            if (
                str(entry.get("stage", "unknown")) == stage_name
                and not bool(entry.get("admissible"))
            ):
                running_count += 1

            cumulative_rejections.append(running_count)

        rejection_axis.plot(
            candidate_order,
            cumulative_rejections,
            label=stage_name,
            color=_STAGE_COLORS.get(stage_name, "#475569"),
        )

    rejection_axis.set_title("Rejection Counts Over Candidate Order")
    rejection_axis.set_xlabel("Candidate order")
    rejection_axis.set_ylabel("Cumulative rejected candidates")
    rejection_axis.grid(alpha=0.3)

    if stage_names:
        rejection_axis.legend(loc="best", fontsize="small")

    return figure


def create_consistency_figure(artifacts: WorkbenchArtifacts) -> Figure:
    figure = Figure(figsize=(15, 8), constrained_layout=True)

    delta_axis = figure.add_subplot(2, 2, 1)
    scatter_axis = figure.add_subplot(2, 2, 2)
    heatmap_axis = figure.add_subplot(2, 2, 3)
    summary_axis = figure.add_subplot(2, 2, 4)

    metrics = sorted(
        _artifact_list(artifacts.coarse_tick_consistency, "metrics"),
        key=lambda metric: float(metric.get("normalized_delta", 0.0)),
        reverse=True,
    )
    if not metrics:
        _render_unavailable_axes(
            (delta_axis, scatter_axis, heatmap_axis, summary_axis),
            "Coarse-versus-tick consistency diagnostics are unavailable for this run.",
        )

        return figure

    top_metrics = metrics[:12]

    delta_axis.barh(
        [_metric_label(metric) for metric in top_metrics],
        [float(metric.get("normalized_delta", 0.0)) for metric in top_metrics],
        color="#dc2626",
    )
    delta_axis.set_title("Largest Coarse/Tick Mismatches")
    delta_axis.set_xlabel("Normalized Delta")
    delta_axis.invert_yaxis()
    delta_axis.grid(axis="x", alpha=0.3)

    comparable_metrics = [
        metric
        for metric in metrics
        if isinstance(metric.get("coarse_value"), int | float)
        and isinstance(metric.get("tick_value"), int | float)
    ]

    coarse_values = [float(metric.get("coarse_value", 0.0)) for metric in comparable_metrics]
    tick_values = [float(metric.get("tick_value", 0.0)) for metric in comparable_metrics]

    scatter_axis.scatter(coarse_values, tick_values, color="#0f766e", alpha=0.8)

    if coarse_values and tick_values:
        identity_lower_bound = min([*coarse_values, *tick_values])
        identity_upper_bound = max([*coarse_values, *tick_values])

        scatter_axis.plot(
            [identity_lower_bound, identity_upper_bound],
            [identity_lower_bound, identity_upper_bound],
            color="#94a3b8",
            linestyle="--",
            linewidth=1.0,
        )

        if _all_positive(coarse_values) and _all_positive(tick_values):
            scatter_axis.set_xscale("log")
            scatter_axis.set_yscale("log")

    scatter_axis.set_title("Coarse Versus Tick")
    scatter_axis.set_xlabel("Coarse")
    scatter_axis.set_ylabel("Tick")
    scatter_axis.grid(alpha=0.3)

    metric_names, horizon_labels, matrix = _consistency_heatmap_matrix(metrics)
    if metric_names and horizon_labels:
        heatmap = heatmap_axis.imshow(matrix, aspect="auto", cmap="magma")

        figure.colorbar(heatmap, ax=heatmap_axis, fraction=0.046, pad=0.04)

        heatmap_axis.set_title("Mismatch Heatmap")
        heatmap_axis.set_xticks(range(len(horizon_labels)))
        heatmap_axis.set_xticklabels(horizon_labels)
        heatmap_axis.set_yticks(range(len(metric_names)))
        heatmap_axis.set_yticklabels(metric_names)
    else:
        heatmap_axis.text(
            0.5,
            0.5,
            "No heatmap-ready metrics available",
            ha="center",
            va="center",
            transform=heatmap_axis.transAxes,
        )

    heatmap_axis.grid(False)

    summary_axis.set_title("Consistency Summary")
    summary_axis.axis("off")

    worst_metric = metrics[0]

    mean_delta_by_horizon = _mean_delta_by_horizon(metrics)

    summary_axis.text(
        0.0,
        1.0,
        "\n".join(
            [
                f"worst metric: {_metric_label(worst_metric)}",
                f"worst delta: {float(worst_metric.get('normalized_delta', 0.0)):.3f}",
                f"metric count: {len(metrics)}",
                f"mean delta by horizon: {json_like(mean_delta_by_horizon)}",
            ]
        ),
        va="top",
        ha="left",
        family="monospace",
    )

    return figure


def create_simulation_summary_figure(artifacts: WorkbenchArtifacts) -> Figure:
    figure = Figure(figsize=(15, 4), constrained_layout=True)

    event_axis = figure.add_subplot(1, 3, 1)
    return_axis = figure.add_subplot(1, 3, 2)
    variance_axis = figure.add_subplot(1, 3, 3)

    summaries = _artifact_list(artifacts.simulation_summary, "summaries")
    if not summaries:
        _render_unavailable_axes(
            (event_axis, return_axis, variance_axis),
            "Simulation summaries are unavailable for this run.",
        )

        return figure

    _render_simulation_summary_metric(
        axis=event_axis,
        summaries=summaries,
        metric_key="event_count",
        metric_title="Event Count By Horizon",
        observed_lookup=_observed_tick_metric_lookup(artifacts, "event_count"),
    )
    _render_simulation_summary_metric(
        axis=return_axis,
        summaries=summaries,
        metric_key="mean_absolute_return",
        metric_title="Mean Absolute Return By Horizon",
        observed_lookup=_observed_tick_metric_lookup(artifacts, "mean_absolute_return"),
    )
    _render_simulation_summary_metric(
        axis=variance_axis,
        summaries=summaries,
        metric_key="realized_variance",
        metric_title="Realized Variance By Horizon",
        observed_lookup=_observed_tick_metric_lookup(artifacts, "realized_variance"),
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
        two_dimensional_admissible_points = [
            point
            for point in surface_points
            if bool(point.get("admissible"))
            and isinstance(point.get("x_value"), int | float)
            and isinstance(point.get("y_value"), int | float)
        ]
        two_dimensional_rejected_points = [
            point
            for point in surface_points
            if not bool(point.get("admissible"))
            and isinstance(point.get("x_value"), int | float)
            and isinstance(point.get("y_value"), int | float)
        ]
        one_dimensional_points = [
            point
            for point in surface_points
            if isinstance(point.get("x_value"), int | float)
            and not isinstance(point.get("y_value"), int | float)
        ]

        if two_dimensional_admissible_points:
            surface_axis.scatter(
                [float(point.get("x_value", 0.0)) for point in two_dimensional_admissible_points],
                [
                    float(point.get("y_value", 0.0))
                    for point in two_dimensional_admissible_points
                ],
                alpha=0.8,
                label=f"{surface_id} admissible",
            )

        if two_dimensional_rejected_points:
            surface_axis.scatter(
                [float(point.get("x_value", 0.0)) for point in two_dimensional_rejected_points],
                [
                    float(point.get("y_value", 0.0))
                    for point in two_dimensional_rejected_points
                ],
                alpha=0.5,
                color="#cbd5e1",
                label=f"{surface_id} rejected",
            )

        if one_dimensional_points:
            x_values = [float(point.get("x_value", 0.0)) for point in one_dimensional_points]
            y_position = 0.0
            surface_axis.scatter(
                x_values,
                [y_position] * len(x_values),
                alpha=0.75,
                color="#7c3aed",
                label=f"{surface_id} 1d",
                marker="|",
                s=220,
            )

        objective_axis.plot(
            [
                float(point.get("x_value", 0.0))
                for point in surface_points
                if isinstance(point.get("x_value"), int | float)
                and isinstance(point.get("objective_value"), int | float)
            ],
            [
                float(point.get("objective_value", 0.0))
                for point in surface_points
                if isinstance(point.get("x_value"), int | float)
                and isinstance(point.get("objective_value"), int | float)
            ],
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


def create_parameter_objective_figure(artifacts: WorkbenchArtifacts) -> Figure:
    parameter_points = _parameter_search_points(artifacts)

    parameter_names = sorted(parameter_points)
    if not parameter_names:
        return create_message_figure(
            "Parameter Objective",
            "No scalar parameter search points are available for this run.",
        )

    return create_parameter_objective_figure_with_columns(
        artifacts,
        column_count=2,
    )


def create_parameter_objective_figure_with_columns(
    artifacts: WorkbenchArtifacts,
    *,
    column_count: int,
) -> Figure:
    parameter_points = _parameter_search_points(artifacts)

    parameter_names = sorted(parameter_points)
    if not parameter_names:
        return create_message_figure(
            "Parameter Objective",
            "No scalar parameter search points are available for this run.",
        )

    column_count = max(1, min(column_count, len(parameter_names)))
    row_count = math.ceil(len(parameter_names) / column_count)

    figure = Figure(
        figsize=(max(6, column_count * 5.2), max(4, row_count * 3.1)),
        constrained_layout=True,
    )
    axes = figure.subplots(row_count, column_count, squeeze=False)

    for axis, parameter_name in zip(axes.flat, parameter_names, strict=False):
        points = parameter_points[parameter_name]
        admissible_points = [point for point in points if point["admissible"]]
        rejected_points = [point for point in points if not point["admissible"]]
        y_focus_limits = _parameter_objective_focus_limits(
            [float(point["objective"]) for point in points]
        )

        if admissible_points:
            _render_parameter_objective_points(
                axis,
                admissible_points,
                color="#1d4ed8",
                label="admissible",
                y_focus_limits=y_focus_limits,
            )

        if rejected_points:
            _render_parameter_objective_points(
                axis,
                rejected_points,
                color="#dc2626",
                label="rejected",
                y_focus_limits=y_focus_limits,
            )

        axis.set_title(f"{parameter_name} vs objective")
        axis.set_xlabel(parameter_name)
        axis.set_ylabel("Objective")
        axis.grid(alpha=0.3)

        if y_focus_limits is not None:
            focus_lower_bound, focus_upper_bound = y_focus_limits
            axis.set_ylim(
                _expanded_axis_bound(
                    focus_lower_bound,
                    focus_upper_bound,
                    prefer_upper=False,
                ),
                _expanded_axis_bound(
                    focus_lower_bound,
                    focus_upper_bound,
                    prefer_upper=True,
                ),
            )

        if admissible_points or rejected_points:
            axis.legend(loc="best", fontsize="small")

    for axis in list(axes.flat)[len(parameter_names):]:
        axis.set_axis_off()

    return figure


def _render_parameter_objective_points(
    axis: Any,
    points: list[dict[str, float | bool]],
    *,
    color: str,
    label: str,
    y_focus_limits: Optional[tuple[float, float]],
) -> None:
    in_range_points = points
    lower_outlier_count = 0
    upper_outlier_count = 0

    if y_focus_limits is not None:
        focus_lower_bound, focus_upper_bound = y_focus_limits
        in_range_points = [
            point
            for point in points
            if focus_lower_bound <= float(point["objective"]) <= focus_upper_bound
        ]
        lower_outlier_points = [
            point for point in points if float(point["objective"]) < focus_lower_bound
        ]
        upper_outlier_points = [
            point for point in points if float(point["objective"]) > focus_upper_bound
        ]
        lower_outlier_count = len(lower_outlier_points)
        upper_outlier_count = len(upper_outlier_points)

        if lower_outlier_points:
            axis.scatter(
                [float(point["value"]) for point in lower_outlier_points],
                [focus_lower_bound for _ in lower_outlier_points],
                color=color,
                alpha=0.8,
                marker="v",
            )

        if upper_outlier_points:
            axis.scatter(
                [float(point["value"]) for point in upper_outlier_points],
                [focus_upper_bound for _ in upper_outlier_points],
                color=color,
                alpha=0.8,
                marker="^",
            )

    if in_range_points:
        axis.scatter(
            [float(point["value"]) for point in in_range_points],
            [float(point["objective"]) for point in in_range_points],
            color=color,
            alpha=0.75 if label == "admissible" else 0.55,
            label=label,
        )

    if lower_outlier_count > 0 or upper_outlier_count > 0:
        clipped_parts: list[str] = []
        if upper_outlier_count > 0:
            clipped_parts.append(f"{upper_outlier_count} high")
        if lower_outlier_count > 0:
            clipped_parts.append(f"{lower_outlier_count} low")

        axis.text(
            0.99,
            0.02 if label == "admissible" else 0.10,
            f"{label} clipped: {', '.join(clipped_parts)}",
            ha="right",
            va="bottom",
            color=color,
            fontsize="x-small",
            transform=axis.transAxes,
        )


def _parameter_objective_focus_limits(
    objective_values: list[float],
) -> Optional[tuple[float, float]]:
    if len(objective_values) < 4:
        return None

    objective_array = np.asarray(sorted(objective_values), dtype=np.float64)
    lower_bound = float(objective_array[0])
    upper_bound = float(objective_array[-1])
    if not math.isfinite(lower_bound) or not math.isfinite(upper_bound):
        return None
    if lower_bound == upper_bound:
        return None

    focus_values = objective_array.copy()
    removed_outlier_count = 0
    while focus_values.size >= 4:
        first_quartile, third_quartile = np.quantile(
            focus_values,
            np.asarray([0.25, 0.75], dtype=np.float64),
        )
        interquartile_range = float(third_quartile - first_quartile)
        if interquartile_range <= 0.0:
            break

        focus_upper_bound = float(third_quartile + 1.5 * interquartile_range)
        if float(focus_values[-1]) <= focus_upper_bound:
            break

        focus_values = focus_values[:-1]
        removed_outlier_count += 1

    if removed_outlier_count == 0 or focus_values.size == 0:
        return None

    return lower_bound, float(focus_values[-1])


def _expanded_axis_bound(
    lower_bound: float,
    upper_bound: float,
    *,
    prefer_upper: bool,
) -> float:
    span = upper_bound - lower_bound
    padding = max(span * 0.08, 0.05)
    if prefer_upper:
        return upper_bound + padding

    return lower_bound - padding


def create_parameter_search_figure(
    artifacts: WorkbenchArtifacts,
    *,
    left_parameter: Optional[str] = None,
    right_parameter: Optional[str] = None,
) -> Figure:
    figure = Figure(figsize=(15, 9), constrained_layout=True)

    admissibility_axis = figure.add_subplot(2, 2, 1)
    pairwise_axis = figure.add_subplot(2, 2, 2)
    coverage_axis = figure.add_subplot(2, 2, 3)
    summary_axis = figure.add_subplot(2, 2, 4)

    parameter_points = _parameter_search_points(artifacts)
    parameter_names = sorted(
        parameter_points,
        key=lambda parameter_name: len(parameter_points[parameter_name]),
        reverse=True,
    )
    if not parameter_names:
        _render_unavailable_axes(
            (admissibility_axis, pairwise_axis, coverage_axis, summary_axis),
            "Parameter-search diagnostics are unavailable for this run.",
        )

        return figure

    for parameter_name in parameter_names[:3]:
        points = parameter_points[parameter_name]
        binned_points = _binned_admissibility(points, bin_count=8)
        if not binned_points:
            continue

        admissibility_axis.plot(
            [point[0] for point in binned_points],
            [point[1] for point in binned_points],
            marker="o",
            label=parameter_name,
        )

    admissibility_axis.set_title("Searched Values vs Admissibility")
    admissibility_axis.set_xlabel("Parameter bin center")
    admissibility_axis.set_ylabel("Admissible share")
    admissibility_axis.set_ylim(-0.05, 1.05)
    admissibility_axis.grid(alpha=0.3)

    if admissibility_axis.lines:
        admissibility_axis.legend(loc="best", fontsize="small")

    for parameter_name in parameter_names[:4]:
        values = [point["value"] for point in parameter_points[parameter_name]]
        if not values:
            continue

        coverage_axis.hist(
            values,
            bins=_bin_count_for_histogram(values),
            alpha=0.4,
            density=True,
            label=parameter_name,
        )

    coverage_axis.set_title("Parameter Coverage Density")
    coverage_axis.set_xlabel("Parameter value")
    coverage_axis.set_ylabel("Density")
    coverage_axis.grid(alpha=0.3)

    if parameter_names:
        coverage_axis.legend(loc="best", fontsize="small")

    if len(parameter_names) >= 2:
        selected_left_parameter = (
            left_parameter if left_parameter in parameter_names else parameter_names[0]
        )
        selected_right_parameter = (
            right_parameter if right_parameter in parameter_names else parameter_names[1]
        )

        if selected_left_parameter == selected_right_parameter:
            selected_right_parameter = next(
                (
                    parameter_name
                    for parameter_name in parameter_names
                    if parameter_name != selected_left_parameter
                ),
                selected_right_parameter,
            )

        pairwise_points = _pairwise_parameter_points(
            artifacts,
            left_parameter=selected_left_parameter,
            right_parameter=selected_right_parameter,
        )

        if pairwise_points:
            color_values = [point[2] for point in pairwise_points]
            pairwise_plot = pairwise_axis.scatter(
                [point[0] for point in pairwise_points],
                [point[1] for point in pairwise_points],
                c=color_values,
                cmap="viridis_r",
                alpha=0.75,
            )
            figure.colorbar(pairwise_plot, ax=pairwise_axis, fraction=0.046, pad=0.04)
        else:
            pairwise_axis.text(
                0.5,
                0.5,
                "No pairwise parameter points available",
                ha="center",
                va="center",
                transform=pairwise_axis.transAxes,
            )

        pairwise_axis.set_title(f"{selected_left_parameter} vs {selected_right_parameter}")
        pairwise_axis.set_xlabel(selected_left_parameter)
        pairwise_axis.set_ylabel(selected_right_parameter)
        pairwise_axis.grid(alpha=0.3)
    else:
        pairwise_axis.text(
            0.5,
            0.5,
            "At least two scalar parameters are needed for pairwise search diagnostics.",
            ha="center",
            va="center",
            transform=pairwise_axis.transAxes,
        )
        pairwise_axis.set_xticks([])
        pairwise_axis.set_yticks([])

    summary_axis.axis("off")
    summary_axis.set_title("Parameter Search Summary")
    summary_axis.text(
        0.0,
        1.0,
        "\n".join(
            [
                _format_parameter_search_summary(parameter_name, parameter_points[parameter_name])
                for parameter_name in parameter_names[:8]
            ]
        ),
        va="top",
        ha="left",
        family="monospace",
        transform=summary_axis.transAxes,
    )

    return figure


def _observed_simulation_analysis_for_plot(
    artifacts: WorkbenchArtifacts,
    comparison: Optional[Any],
) -> Optional[WorkbenchObservedSimulationAnalysis]:
    resolved_comparison = _observed_simulation_comparison_for_plot(artifacts, comparison)
    if resolved_comparison is None:
        return None

    return _analysis_for_observed_simulation_comparison(resolved_comparison)


def _analysis_for_observed_simulation_comparison(
    comparison: WorkbenchObservedSimulationComparison,
) -> WorkbenchObservedSimulationAnalysis:
    cache_key = id(comparison)
    cached_analysis = _OBSERVED_SIMULATION_ANALYSIS_CACHE.get(cache_key)
    if cached_analysis is not None:
        return cached_analysis

    observed_analysis = _build_series_analysis(comparison.observed_series)
    simulated_analysis = _build_series_analysis(comparison.simulated_series)
    simulated_seeded_15m_analysis = _build_series_analysis(comparison.simulated_seeded_series_15m)
    simulated_coarse_analysis = _build_series_analysis(comparison.simulated_coarse_series_15m)

    available_horizon_minutes = tuple(
        horizon_minutes
        for horizon_minutes in _SUPPORTED_ANALYSIS_HORIZONS_MINUTES
        if len(observed_analysis.bucket_series_by_horizon[horizon_minutes].close_prices) >= 3
        and len(simulated_seeded_15m_analysis.bucket_series_by_horizon[horizon_minutes].close_prices) >= 3
    )
    if len(available_horizon_minutes) == 0:
        available_horizon_minutes = (15,)

    analysis = WorkbenchObservedSimulationAnalysis(
        observed=observed_analysis,
        simulated=simulated_analysis,
        simulated_seeded_15m=simulated_seeded_15m_analysis,
        simulated_coarse_15m=simulated_coarse_analysis,
        available_horizon_minutes=available_horizon_minutes,
        observed_mean_reversion=_build_mean_reversion_series_analysis(
            _comparison_model(comparison),
            comparison.observed_series,
        ),
        simulated_mean_reversion=_build_mean_reversion_series_analysis(
            _comparison_model(comparison),
            comparison.simulated_series,
        ),
    )
    if len(_OBSERVED_SIMULATION_ANALYSIS_CACHE) >= _OBSERVED_SIMULATION_ANALYSIS_CACHE_MAX_ENTRIES:
        first_cache_key = next(iter(_OBSERVED_SIMULATION_ANALYSIS_CACHE))
        _OBSERVED_SIMULATION_ANALYSIS_CACHE.pop(first_cache_key)

    _OBSERVED_SIMULATION_ANALYSIS_CACHE[cache_key] = analysis
    return analysis


def _build_series_analysis(series: WorkbenchPriceSeries) -> WorkbenchSeriesAnalysis:
    timestamp_microseconds_utc = series.timestamp_microseconds_utc
    if timestamp_microseconds_utc.size == 0 or series.prices.size == 0:
        empty_prices = np.asarray([], dtype=np.float64)
        empty_counts = np.asarray([], dtype=np.int64)
        empty_points = np.empty((0, 2), dtype=np.float64)
        empty_bucket_series = {
            horizon_minutes: WorkbenchBucketSeriesAnalysis(
                bucket_timestamps_utc=(),
                close_prices=empty_prices,
                returns=empty_prices,
                event_counts=empty_counts,
            )
            for horizon_minutes in _SUPPORTED_ANALYSIS_HORIZONS_MINUTES
        }
        return WorkbenchSeriesAnalysis(
            timestamp_microseconds_utc=np.asarray([], dtype=np.int64),
            prices=empty_prices,
            normalized_prices=empty_prices,
            returns=empty_prices,
            hourly_event_counts={},
            hourly_activity_profile=np.zeros(24, dtype=np.float64),
            hourly_absolute_return_profile=np.zeros(24, dtype=np.float64),
            bucket_series_by_horizon=empty_bucket_series,
            rolling_activity_points=empty_points,
        )

    prices = series.prices
    hourly_event_counts = _hourly_event_counts_from_microseconds(timestamp_microseconds_utc)
    hourly_activity_profile = _hourly_activity_profile_from_microseconds(timestamp_microseconds_utc)
    direct_return_timestamps_microseconds_utc = series.direct_return_timestamps_microseconds_utc
    direct_returns = series.direct_returns

    if (
        direct_return_timestamps_microseconds_utc is not None
        and direct_returns is not None
        and direct_return_timestamps_microseconds_utc.size == direct_returns.size
        and direct_returns.size > 0
    ):
        returns = direct_returns
        hourly_absolute_return_profile = _hourly_absolute_return_profile_from_return_timestamps(
            direct_return_timestamps_microseconds_utc,
            direct_returns,
        )
    else:
        returns = _log_returns_array(prices)
        hourly_absolute_return_profile = _hourly_absolute_return_profile_from_microseconds(
            timestamp_microseconds_utc,
            returns,
        )
    bucket_series_by_horizon = {
        horizon_minutes: _bucket_series_analysis(
            timestamp_microseconds_utc=timestamp_microseconds_utc,
            prices=prices,
            interval_minutes=horizon_minutes,
        )
        for horizon_minutes in _SUPPORTED_ANALYSIS_HORIZONS_MINUTES
    }

    rolling_activity_points = _rolling_activity_points_from_bucket_series(
        bucket_series_by_horizon[15]
    )

    return WorkbenchSeriesAnalysis(
        timestamp_microseconds_utc=timestamp_microseconds_utc,
        prices=prices,
        normalized_prices=_normalize_series_array(prices),
        returns=returns,
        hourly_event_counts=hourly_event_counts,
        hourly_activity_profile=hourly_activity_profile,
        hourly_absolute_return_profile=hourly_absolute_return_profile,
        bucket_series_by_horizon=bucket_series_by_horizon,
        rolling_activity_points=rolling_activity_points,
    )


def _build_mean_reversion_series_analysis(
    model: Any,
    series: WorkbenchPriceSeries,
) -> Optional[WorkbenchMeanReversionSeriesAnalysis]:
    if (
        series.timestamp_microseconds_utc.size == 0
        or series.prices.size == 0
        or np.any(series.prices <= 0.0)
    ):
        return None

    fitted_parameters = _resolve_mean_reversion_fitted_parameters(model)
    if fitted_parameters is None or not fitted_parameters.is_enabled:
        return None

    log_prices = np.log(series.prices)
    anchor_values = np.asarray(
        [
            _materialize_mean_reversion_anchor_value(
                fitted_parameters=fitted_parameters,
                boundary_time_utc=utc_microseconds_to_datetime(int(timestamp_microseconds_utc)),
            )
            for timestamp_microseconds_utc in series.timestamp_microseconds_utc
        ],
        dtype=np.float64,
    )

    return WorkbenchMeanReversionSeriesAnalysis(
        timestamp_microseconds_utc=series.timestamp_microseconds_utc,
        log_prices=log_prices,
        anchor_values=anchor_values,
        deviations_from_anchor=log_prices - anchor_values,
    )


def _resolve_mean_reversion_fitted_parameters(
    model: Any,
) -> Optional[MeanReversionFittedParameters]:
    if model is None:
        return None

    try:
        return _mean_reversion_fitted_parameters(model)
    except (AttributeError, RuntimeError, TypeError, ValueError):
        return None


def _comparison_model(comparison: WorkbenchObservedSimulationComparison) -> Any:
    try:
        return comparison.fit_context.model
    except AttributeError:
        return None


def _hourly_event_counts_from_microseconds(
    timestamp_microseconds_utc: NDArray[np.int64],
) -> dict[datetime, int]:
    if timestamp_microseconds_utc.size == 0:
        return {}

    hour_bucket_index = np.floor_divide(timestamp_microseconds_utc, 3_600_000_000)
    unique_hour_bucket_index, event_counts = np.unique(
        hour_bucket_index,
        return_counts=True,
    )
    return {
        datetime.fromtimestamp(int(hour_bucket) * 3600, tz=UTC): int(event_count)
        for hour_bucket, event_count in zip(
            unique_hour_bucket_index,
            event_counts,
            strict=False,
        )
    }


def _hourly_activity_profile_from_microseconds(
    timestamp_microseconds_utc: NDArray[np.int64],
) -> NDArray[np.float64]:
    if timestamp_microseconds_utc.size == 0:
        return np.zeros(24, dtype=np.float64)

    hourly_bucket_index = np.floor_divide(timestamp_microseconds_utc, 3_600_000_000)
    unique_hourly_bucket_index, event_counts = np.unique(
        hourly_bucket_index,
        return_counts=True,
    )
    hour_of_day = np.mod(unique_hourly_bucket_index, 24).astype(np.int64)
    summed_event_counts = np.bincount(
        hour_of_day,
        weights=event_counts.astype(np.float64),
        minlength=24,
    )
    hourly_bucket_counts = np.bincount(hour_of_day, minlength=24)

    hourly_activity_profile = np.zeros(24, dtype=np.float64)
    non_zero_mask = hourly_bucket_counts > 0
    hourly_activity_profile[non_zero_mask] = (
        summed_event_counts[non_zero_mask] / hourly_bucket_counts[non_zero_mask]
    )
    return hourly_activity_profile


def _hourly_absolute_return_profile_from_microseconds(
    timestamp_microseconds_utc: NDArray[np.int64],
    returns: NDArray[np.float64],
) -> NDArray[np.float64]:
    if returns.size == 0 or timestamp_microseconds_utc.size < 2:
        return np.zeros(24, dtype=np.float64)

    hour_of_day = np.mod(
        np.floor_divide(timestamp_microseconds_utc[1:], 3_600_000_000),
        24,
    ).astype(np.int64)
    absolute_returns = np.abs(returns)
    summed_absolute_returns = np.bincount(
        hour_of_day,
        weights=absolute_returns,
        minlength=24,
    )
    counts = np.bincount(hour_of_day, minlength=24)
    hourly_profile = np.zeros(24, dtype=np.float64)
    non_zero_mask = counts > 0
    hourly_profile[non_zero_mask] = (
        summed_absolute_returns[non_zero_mask] / counts[non_zero_mask]
    )
    return hourly_profile


def _hourly_absolute_return_profile_from_return_timestamps(
    return_timestamp_microseconds_utc: NDArray[np.int64],
    returns: NDArray[np.float64],
) -> NDArray[np.float64]:
    if returns.size == 0 or return_timestamp_microseconds_utc.size != returns.size:
        return np.zeros(24, dtype=np.float64)

    hour_of_day = np.mod(
        np.floor_divide(return_timestamp_microseconds_utc, 3_600_000_000),
        24,
    ).astype(np.int64)
    absolute_returns = np.abs(returns)
    summed_absolute_returns = np.bincount(
        hour_of_day,
        weights=absolute_returns,
        minlength=24,
    )
    counts = np.bincount(hour_of_day, minlength=24)
    hourly_profile = np.zeros(24, dtype=np.float64)
    non_zero_mask = counts > 0
    hourly_profile[non_zero_mask] = (
        summed_absolute_returns[non_zero_mask] / counts[non_zero_mask]
    )
    return hourly_profile


def _bucket_series_analysis(
    *,
    timestamp_microseconds_utc: NDArray[np.int64],
    prices: NDArray[np.float64],
    interval_minutes: int,
) -> WorkbenchBucketSeriesAnalysis:
    if timestamp_microseconds_utc.size == 0 or prices.size == 0:
        return WorkbenchBucketSeriesAnalysis(
            bucket_timestamps_utc=(),
            close_prices=np.asarray([], dtype=np.float64),
            returns=np.asarray([], dtype=np.float64),
            event_counts=np.asarray([], dtype=np.int64),
        )

    interval_seconds = interval_minutes * 60
    bucket_index = np.floor_divide(timestamp_microseconds_utc, interval_seconds * 1_000_000)
    bucket_change_positions = np.flatnonzero(bucket_index[1:] != bucket_index[:-1]) + 1
    bucket_start_positions = np.concatenate(
        (
            np.asarray([0], dtype=np.int64),
            bucket_change_positions.astype(np.int64),
        )
    )
    bucket_end_positions = np.concatenate(
        (
            bucket_change_positions.astype(np.int64) - 1,
            np.asarray([len(bucket_index) - 1], dtype=np.int64),
        )
    )
    bucket_ids = bucket_index[bucket_start_positions]
    close_prices = prices[bucket_end_positions]
    event_counts = (bucket_end_positions - bucket_start_positions + 1).astype(np.int64)
    bucket_timestamps_utc = tuple(
        datetime.fromtimestamp(int(bucket_id) * interval_seconds, tz=UTC)
        for bucket_id in bucket_ids
    )

    return WorkbenchBucketSeriesAnalysis(
        bucket_timestamps_utc=bucket_timestamps_utc,
        close_prices=close_prices,
        returns=_log_returns_array(close_prices),
        event_counts=event_counts,
    )


def _rolling_activity_points_from_bucket_series(
    bucket_series: WorkbenchBucketSeriesAnalysis,
) -> NDArray[np.float64]:
    bucket_returns = bucket_series.returns
    event_counts = bucket_series.event_counts
    if event_counts.size < 25 or bucket_returns.size < 24:
        return np.empty((0, 2), dtype=np.float64)

    point_count = event_counts.size - 24
    rolling_points = np.empty((point_count, 2), dtype=np.float64)
    for point_index, end_index in enumerate(range(24, event_counts.size)):
        returns_window = bucket_returns[end_index - 24:end_index]
        activity_window = event_counts[end_index - 24:end_index]
        rolling_points[point_index, 0] = float(np.sum(activity_window))
        rolling_points[point_index, 1] = float(np.sqrt(np.sum(returns_window * returns_window)))

    return rolling_points


def _normalize_series_array(values: NDArray[np.float64]) -> NDArray[np.float64]:
    if values.size == 0:
        return values.copy()

    base_value = float(values[0])
    if base_value == 0.0:
        return values.copy()

    return 100.0 * values / base_value


def _log_returns_array(values: NDArray[np.float64]) -> NDArray[np.float64]:
    if values.size < 2 or np.any(values <= 0.0):
        return np.asarray([], dtype=np.float64)

    return np.diff(np.log(values))


def _downsample_time_series(
    timestamp_microseconds_utc: NDArray[np.int64],
    values: NDArray[np.float64],
    *,
    max_points: int,
) -> tuple[tuple[datetime, ...], NDArray[np.float64]]:
    if timestamp_microseconds_utc.size <= max_points or values.size <= max_points:
        return (
            tuple(
                utc_microseconds_to_datetime(int(timestamp_microseconds))
                for timestamp_microseconds in timestamp_microseconds_utc
            ),
            values,
        )

    sample_indexes = np.linspace(
        0,
        values.size - 1,
        num=max_points,
        dtype=np.int64,
    )
    return (
        tuple(
            utc_microseconds_to_datetime(int(timestamp_microseconds_utc[int(sample_index)]))
            for sample_index in sample_indexes
        ),
        values[sample_indexes],
    )


def _downsample_point_cloud(
    points: NDArray[np.float64],
    *,
    max_points: int,
) -> NDArray[np.float64]:
    if points.shape[0] <= max_points:
        return points

    sample_indexes = np.linspace(
        0,
        points.shape[0] - 1,
        num=max_points,
        dtype=np.int64,
    )
    return points[sample_indexes]


def create_message_figure(title: str, message: str) -> Figure:
    figure = Figure(figsize=(8, 4), constrained_layout=True)

    axis = figure.add_subplot(1, 1, 1)

    axis.set_title(title)
    axis.text(
        0.5,
        0.5,
        message,
        ha="center",
        va="center",
        transform=axis.transAxes,
    )
    axis.set_xticks([])
    axis.set_yticks([])

    return figure


def create_td_sd_mean_reversion_variance_growth_figure(
    artifacts: WorkbenchArtifacts,
    comparison: Optional[Any] = None,
) -> Figure:
    figure = Figure(figsize=(14, 5), constrained_layout=True)

    variance_axis = figure.add_subplot(1, 2, 1)
    summary_axis = figure.add_subplot(1, 2, 2)

    analysis = _observed_simulation_analysis_for_plot(artifacts, comparison)
    if analysis is None:
        _render_unavailable_axes(
            (variance_axis, summary_axis),
            "Observed-versus-simulated comparison is unavailable for this input.",
        )
        return figure

    observed_mean_reversion = analysis.observed_mean_reversion
    simulated_mean_reversion = analysis.simulated_mean_reversion
    if observed_mean_reversion is None or simulated_mean_reversion is None:
        _render_unavailable_axes(
            (variance_axis, summary_axis),
            "Mean-reversion anchor diagnostics are unavailable because the loaded model does not expose an enabled anchor path for both observed and simulated series.",
        )
        return figure

    max_shared_horizon = min(
        observed_mean_reversion.deviations_from_anchor.size,
        simulated_mean_reversion.deviations_from_anchor.size,
    ) - 1
    horizons = _mean_reversion_log_spaced_horizons(max_shared_horizon)
    if horizons.size == 0:
        _render_unavailable_axes(
            (variance_axis, summary_axis),
            "Mean-reversion variance growth requires at least two aligned observations in both series.",
        )
        return figure

    observed_variance_growth = np.asarray(
        [
            _variance_of_deviation_difference(
                observed_mean_reversion.deviations_from_anchor,
                int(horizon),
            )
            for horizon in horizons
        ],
        dtype=np.float64,
    )
    simulated_variance_growth = np.asarray(
        [
            _variance_of_deviation_difference(
                simulated_mean_reversion.deviations_from_anchor,
                int(horizon),
            )
            for horizon in horizons
        ],
        dtype=np.float64,
    )

    variance_axis.plot(
        horizons,
        observed_variance_growth,
        color="#111827",
        marker="o",
        label="Observed",
    )
    variance_axis.plot(
        horizons,
        simulated_variance_growth,
        color="#dc2626",
        marker="o",
        label="Simulated",
    )
    variance_axis.set_xscale("log")
    variance_axis.set_title("Variance Growth of Deviation from Anchor")
    variance_axis.set_xlabel("Horizon (steps)")
    variance_axis.set_ylabel("Var(D[t+h] - D[t])")
    variance_axis.grid(alpha=0.3)
    variance_axis.legend(loc="best")

    summary_axis.axis("off")
    summary_axis.set_title("Diagnostic Summary")
    summary_axis.text(
        0.0,
        1.0,
        "\n".join(
            [
                f"horizon count: {horizons.size}",
                f"max shared horizon: {int(horizons[-1])}",
                f"observed deviation rows: {observed_mean_reversion.deviations_from_anchor.size}",
                f"simulated deviation rows: {simulated_mean_reversion.deviations_from_anchor.size}",
                f"observed terminal variance: {observed_variance_growth[-1]:.6g}",
                f"simulated terminal variance: {simulated_variance_growth[-1]:.6g}",
            ]
        ),
        va="top",
        family="monospace",
        transform=summary_axis.transAxes,
    )

    return figure


def create_td_sd_mean_reversion_future_change_figure(
    artifacts: WorkbenchArtifacts,
    comparison: Optional[Any] = None,
) -> Figure:
    figure = Figure(figsize=(14, 5), constrained_layout=True)

    conditional_axis = figure.add_subplot(1, 2, 1)
    summary_axis = figure.add_subplot(1, 2, 2)

    analysis = _observed_simulation_analysis_for_plot(artifacts, comparison)
    if analysis is None:
        _render_unavailable_axes(
            (conditional_axis, summary_axis),
            "Observed-versus-simulated comparison is unavailable for this input.",
        )
        return figure

    observed_mean_reversion = analysis.observed_mean_reversion
    simulated_mean_reversion = analysis.simulated_mean_reversion
    if observed_mean_reversion is None or simulated_mean_reversion is None:
        _render_unavailable_axes(
            (conditional_axis, summary_axis),
            "Mean-reversion anchor diagnostics are unavailable because the loaded model does not expose an enabled anchor path for both observed and simulated series.",
        )
        return figure

    max_shared_horizon = min(
        observed_mean_reversion.deviations_from_anchor.size,
        simulated_mean_reversion.deviations_from_anchor.size,
    ) - 1
    horizons = _mean_reversion_log_spaced_horizons(max_shared_horizon)
    if horizons.size == 0:
        _render_unavailable_axes(
            (conditional_axis, summary_axis),
            "Conditional future-change diagnostics require at least two aligned observations in both series.",
        )
        return figure

    selected_horizon = int(horizons[len(horizons) // 2])
    shared_bin_edges = _shared_mean_reversion_bin_edges(
        observed_mean_reversion,
        simulated_mean_reversion,
        horizon=selected_horizon,
    )
    observed_conditional = _conditional_future_change_profile(
        observed_mean_reversion,
        horizon=selected_horizon,
        shared_bin_edges=shared_bin_edges,
    )
    simulated_conditional = _conditional_future_change_profile(
        simulated_mean_reversion,
        horizon=selected_horizon,
        shared_bin_edges=shared_bin_edges,
    )

    if (
        observed_conditional.mean_deviation.size == 0
        and simulated_conditional.mean_deviation.size == 0
    ):
        _render_unavailable_axes(
            (conditional_axis, summary_axis),
            "Conditional future-change diagnostics could not form any shared deviation bins with enough samples.",
        )
        return figure

    if observed_conditional.mean_deviation.size > 0:
        conditional_axis.plot(
            observed_conditional.mean_deviation,
            observed_conditional.mean_future_change,
            color="#111827",
            marker="o",
            label="Observed",
        )
    if simulated_conditional.mean_deviation.size > 0:
        conditional_axis.plot(
            simulated_conditional.mean_deviation,
            simulated_conditional.mean_future_change,
            color="#dc2626",
            marker="o",
            label="Simulated",
        )

    conditional_axis.axhline(0.0, color="#94a3b8", linestyle="--", linewidth=1.0)
    conditional_axis.axvline(0.0, color="#94a3b8", linestyle="--", linewidth=1.0)
    conditional_axis.set_title("Conditional Future Change vs Deviation")
    conditional_axis.set_xlabel("Deviation from anchor")
    conditional_axis.set_ylabel(f"Mean future log-price change ({selected_horizon} steps)")
    conditional_axis.grid(alpha=0.3)
    conditional_axis.legend(loc="best")

    summary_axis.axis("off")
    summary_axis.set_title("Diagnostic Summary")
    summary_axis.text(
        0.0,
        1.0,
        "\n".join(
            [
                f"selected horizon: {selected_horizon} steps",
                f"shared bin count: {max(observed_conditional.shared_bin_edges.size - 1, 0)}",
                f"minimum samples per bin: {max(observed_conditional.minimum_sample_count, simulated_conditional.minimum_sample_count)}",
                f"observed plotted bins: {observed_conditional.mean_deviation.size}",
                f"simulated plotted bins: {simulated_conditional.mean_deviation.size}",
                f"deviation clip quantiles: [{_MEAN_REVERSION_TAIL_QUANTILE_LOWER_BOUND:.0%}, {_MEAN_REVERSION_TAIL_QUANTILE_UPPER_BOUND:.0%}]",
            ]
        ),
        va="top",
        family="monospace",
        transform=summary_axis.transAxes,
    )

    return figure


def create_observed_simulated_figure(
    artifacts: WorkbenchArtifacts,
    comparison: Optional[Any] = None,
) -> Figure:
    figure = Figure(figsize=(14, 8), constrained_layout=True)

    price_axis = figure.add_subplot(2, 2, 1)
    normalized_axis = figure.add_subplot(2, 2, 2)
    return_axis = figure.add_subplot(2, 2, 3)
    activity_axis = figure.add_subplot(2, 2, 4)

    resolved_comparison = _observed_simulation_comparison_for_plot(artifacts, comparison)
    analysis = _observed_simulation_analysis_for_plot(artifacts, comparison)
    if analysis is None or resolved_comparison is None:
        _render_unavailable_axes(
            (price_axis, normalized_axis, return_axis, activity_axis),
            "Observed-versus-simulated comparison is unavailable for this input.",
        )

        return figure

    if analysis.observed.prices.size == 0 or analysis.simulated.prices.size == 0:
        _render_unavailable_axes(
            (price_axis, normalized_axis, return_axis, activity_axis),
            "Observed-versus-simulated rows could not be normalized into a price series.",
        )

        return figure

    observed_timestamps, observed_prices = _downsample_time_series(
        analysis.observed.timestamp_microseconds_utc,
        analysis.observed.prices,
        max_points=_MAX_PATH_PLOT_POINTS,
    )
    simulated_timestamps, simulated_prices = _downsample_time_series(
        analysis.simulated.timestamp_microseconds_utc,
        analysis.simulated.prices,
        max_points=_MAX_PATH_PLOT_POINTS,
    )

    price_axis.plot(
        observed_timestamps,
        observed_prices,
        color="#111827",
        alpha=0.9,
        label="Observed",
    )

    price_axis.plot(
        simulated_timestamps,
        simulated_prices,
        color="#dc2626",
        alpha=0.75,
        label=f"Simulated (seed={resolved_comparison.simulation_seed})",
    )

    price_axis.set_title("Observed Versus Simulated Price Path")
    price_axis.set_ylabel("Price")
    price_axis.grid(alpha=0.3)
    price_axis.legend(loc="best")

    normalized_axis.plot(
        observed_timestamps,
        _downsample_time_series(
            analysis.observed.timestamp_microseconds_utc,
            analysis.observed.normalized_prices,
            max_points=_MAX_PATH_PLOT_POINTS,
        )[1],
        color="#111827",
        alpha=0.9,
        label="observed rebased",
    )

    normalized_axis.plot(
        simulated_timestamps,
        _downsample_time_series(
            analysis.simulated.timestamp_microseconds_utc,
            analysis.simulated.normalized_prices,
            max_points=_MAX_PATH_PLOT_POINTS,
        )[1],
        color="#dc2626",
        alpha=0.75,
        label="simulated rebased",
    )

    normalized_axis.set_title("Rebased Path Comparison")
    normalized_axis.set_ylabel("Index Level")
    normalized_axis.grid(alpha=0.3)
    normalized_axis.legend(loc="best")

    _render_return_histogram(
        return_axis,
        analysis.observed.returns,
        analysis.simulated.returns,
        title="Return Distribution",
        observed_label="Observed log returns",
        simulated_label="Simulated log returns",
    )
    observed_hourly_counts = analysis.observed.hourly_event_counts
    simulated_hourly_counts = analysis.simulated.hourly_event_counts

    observed_hours = sorted(observed_hourly_counts)
    simulated_hours = sorted(simulated_hourly_counts)

    activity_axis.plot(
        observed_hours,
        [observed_hourly_counts[hour] for hour in observed_hours],
        color="#111827",
        alpha=0.9,
        label="observed hourly events",
        marker="o",
    )

    activity_axis.plot(
        simulated_hours,
        [simulated_hourly_counts[hour] for hour in simulated_hours],
        color="#dc2626",
        alpha=0.75,
        label="simulated hourly events",
        marker="o",
    )

    activity_axis.set_title("Hourly Activity Comparison")
    activity_axis.set_ylabel("Event Count")
    activity_axis.grid(alpha=0.3)
    activity_axis.legend(loc="best")

    return figure


def create_td_sd_return_distribution_figure(
    artifacts: WorkbenchArtifacts,
    comparison: Optional[Any] = None,
) -> Figure:
    figure = Figure(figsize=(14, 10), constrained_layout=True)

    tick_axis = figure.add_subplot(2, 2, 1)
    aggregated_axis = figure.add_subplot(2, 2, 2)
    coarse_axis = figure.add_subplot(2, 2, 3)
    summary_axis = figure.add_subplot(2, 2, 4)

    analysis = _observed_simulation_analysis_for_plot(artifacts, comparison)
    if analysis is None:
        _render_unavailable_axes(
            (tick_axis, aggregated_axis, coarse_axis, summary_axis),
            "Observed-versus-simulated comparison is unavailable for this input.",
        )
        return figure

    observed_tick_returns = analysis.observed.returns
    simulated_tick_returns = analysis.simulated.returns
    observed_aggregated_returns = analysis.observed.bucket_series_by_horizon[15].returns
    simulated_aggregated_returns = analysis.simulated_seeded_15m.returns
    coarse_returns = analysis.simulated_coarse_15m.returns

    _render_return_histogram(
        tick_axis,
        observed_tick_returns,
        simulated_tick_returns,
        title="Tick Return Histogram",
        observed_label="Observed tick returns",
        simulated_label="Simulated tick returns",
    )
    _render_return_histogram(
        aggregated_axis,
        observed_aggregated_returns,
        simulated_aggregated_returns,
        title="Aggregated 15m Return Histogram",
        observed_label="Observed 15m returns",
        simulated_label="Simulated tick aggregated to 15m",
    )
    _render_single_return_histogram(
        coarse_axis,
        coarse_returns,
        title="Simulated Coarse 15m Return Histogram",
        series_label="Simulated coarse 15m returns",
        color="#2563eb",
    )

    summary_axis.axis("off")
    summary_axis.set_title("Distribution Summary")
    summary_axis.text(
        0.0,
        1.0,
        "\n".join(
            [
                _format_distribution_summary("Observed tick", observed_tick_returns),
                _format_distribution_summary("Simulated tick", simulated_tick_returns),
                _format_distribution_summary("Observed 15m", observed_aggregated_returns),
                _format_distribution_summary("Simulated tick 15m", simulated_aggregated_returns),
                _format_distribution_summary("Simulated coarse 15m", coarse_returns),
            ]
        ),
        va="top",
        family="monospace",
        transform=summary_axis.transAxes,
    )

    return figure


def create_td_sd_return_diagnostics_figure(
    artifacts: WorkbenchArtifacts,
    comparison: Optional[Any] = None,
) -> Figure:
    figure = Figure(figsize=(15, 9), constrained_layout=True)

    tick_qq_axis = figure.add_subplot(2, 2, 1)
    aggregated_qq_axis = figure.add_subplot(2, 2, 2)
    ecdf_axis = figure.add_subplot(2, 2, 3)
    tail_axis = figure.add_subplot(2, 2, 4)

    analysis = _observed_simulation_analysis_for_plot(artifacts, comparison)
    if analysis is None:
        _render_unavailable_axes(
            (tick_qq_axis, aggregated_qq_axis, ecdf_axis, tail_axis),
            "Observed-versus-simulated comparison is unavailable for this input.",
        )

        return figure

    observed_tick_returns = analysis.observed.returns
    simulated_tick_returns = analysis.simulated.returns
    observed_aggregated_returns = analysis.observed.bucket_series_by_horizon[15].returns
    simulated_aggregated_returns = analysis.simulated_seeded_15m.returns

    _render_qq_plot(
        tick_qq_axis,
        observed_tick_returns,
        simulated_tick_returns,
        title="Observed vs Simulated Tick QQ",
    )
    _render_qq_plot(
        aggregated_qq_axis,
        observed_aggregated_returns,
        simulated_aggregated_returns,
        title="Observed vs Simulated 15m QQ",
    )
    _render_ecdf_overlay(
        ecdf_axis,
        (
            ("Observed tick", observed_tick_returns, "#111827"),
            ("Simulated tick", simulated_tick_returns, "#dc2626"),
            ("Observed 15m", observed_aggregated_returns, "#0f766e"),
            ("Simulated 15m", simulated_aggregated_returns, "#2563eb"),
        ),
    )
    _render_tail_exceedance(
        tail_axis,
        (
            ("Observed tick", observed_tick_returns, "#111827"),
            ("Simulated tick", simulated_tick_returns, "#dc2626"),
            ("Observed 15m", observed_aggregated_returns, "#0f766e"),
            ("Simulated 15m", simulated_aggregated_returns, "#2563eb"),
        ),
    )

    return figure


def create_td_sd_hourly_profile_figure(
    artifacts: WorkbenchArtifacts,
    comparison: Optional[Any] = None,
) -> Figure:
    figure = Figure(figsize=(14, 10), constrained_layout=True)

    activity_axis = figure.add_subplot(2, 2, 1)
    absolute_return_axis = figure.add_subplot(2, 2, 2)
    rolling_td_axis = figure.add_subplot(2, 2, 3)
    rolling_sd_axis = figure.add_subplot(2, 2, 4)

    analysis = _observed_simulation_analysis_for_plot(artifacts, comparison)
    if analysis is None:
        _render_unavailable_axes(
            (activity_axis, absolute_return_axis, rolling_td_axis, rolling_sd_axis),
            "Observed-versus-simulated comparison is unavailable for this input.",
        )
        return figure

    _render_hourly_profile(
        activity_axis,
        analysis.observed.hourly_activity_profile,
        analysis.simulated.hourly_activity_profile,
        title="Hourly Activity Profile",
        y_label="Mean events per hour bin",
    )
    _render_hourly_profile(
        absolute_return_axis,
        analysis.observed.hourly_absolute_return_profile,
        analysis.simulated.hourly_absolute_return_profile,
        title="Hourly Absolute Return Profile",
        y_label="Mean absolute return",
    )
    _render_rolling_activity_plot(
        rolling_td_axis,
        analysis.observed.rolling_activity_points,
        title="Observed Rolling Realized Volatility vs Activity",
        color="#111827",
    )
    _render_rolling_activity_plot(
        rolling_sd_axis,
        analysis.simulated.rolling_activity_points,
        title="Simulated Rolling Realized Volatility vs Activity",
        color="#dc2626",
    )

    return figure


def create_td_sd_horizon_profile_figure(
    artifacts: WorkbenchArtifacts,
    comparison: Optional[Any] = None,
) -> Figure:
    figure = Figure(figsize=(15, 10), constrained_layout=True)

    path_axis = figure.add_subplot(2, 2, 1)
    variance_axis = figure.add_subplot(2, 2, 2)
    absolute_return_axis = figure.add_subplot(2, 2, 3)
    event_count_axis = figure.add_subplot(2, 2, 4)

    analysis = _observed_simulation_analysis_for_plot(artifacts, comparison)
    if analysis is None:
        _render_unavailable_axes(
            (path_axis, variance_axis, absolute_return_axis, event_count_axis),
            "Observed-versus-simulated comparison is unavailable for this input.",
        )

        return figure

    if analysis.observed.prices.size == 0 or analysis.simulated.prices.size == 0:
        _render_unavailable_axes(
            (path_axis, variance_axis, absolute_return_axis, event_count_axis),
            "Observed-versus-simulated rows could not be normalized into a price series.",
        )

        return figure

    observed_timestamps, observed_normalized_prices = _downsample_time_series(
        analysis.observed.timestamp_microseconds_utc,
        analysis.observed.normalized_prices,
        max_points=_MAX_PATH_PLOT_POINTS,
    )
    simulated_timestamps, simulated_normalized_prices = _downsample_time_series(
        analysis.simulated.timestamp_microseconds_utc,
        analysis.simulated.normalized_prices,
        max_points=_MAX_PATH_PLOT_POINTS,
    )

    path_axis.plot(
        observed_timestamps,
        observed_normalized_prices,
        color="#111827",
        alpha=0.9,
        label="Observed rebased",
    )
    path_axis.plot(
        simulated_timestamps,
        simulated_normalized_prices,
        color="#dc2626",
        alpha=0.8,
        label="Simulated rebased",
    )

    path_axis.set_title("Observed vs Simulated Rebased Path")
    path_axis.set_ylabel("Index level")
    path_axis.grid(alpha=0.3)
    path_axis.legend(loc="best")

    horizon_minutes = analysis.available_horizon_minutes
    horizon_labels = [_horizon_label(interval_minutes) for interval_minutes in horizon_minutes]

    td_realized_variance = [
        _realized_variance(analysis.observed.bucket_series_by_horizon[interval_minutes].returns)
        for interval_minutes in horizon_minutes
    ]
    sd_realized_variance = [
        _realized_variance(analysis.simulated_seeded_15m.bucket_series_by_horizon[interval_minutes].returns)
        for interval_minutes in horizon_minutes
    ]
    td_absolute_returns = [
        _mean_absolute_value(analysis.observed.bucket_series_by_horizon[interval_minutes].returns)
        for interval_minutes in horizon_minutes
    ]
    sd_absolute_returns = [
        _mean_absolute_value(
            analysis.simulated_seeded_15m.bucket_series_by_horizon[interval_minutes].returns
        )
        for interval_minutes in horizon_minutes
    ]

    _render_grouped_bars(
        variance_axis,
        horizon_labels,
        td_realized_variance,
        sd_realized_variance,
        left_label="Observed",
        right_label="Simulated",
        title="Realized Variance By Horizon",
        y_label="Realized variance",
    )
    _render_grouped_bars(
        absolute_return_axis,
        horizon_labels,
        td_absolute_returns,
        sd_absolute_returns,
        left_label="Observed",
        right_label="Simulated",
        title="Mean Absolute Return By Horizon",
        y_label="Mean absolute return",
    )
    _render_event_count_distribution(
        event_count_axis,
        analysis,
        horizon_minutes=horizon_minutes,
    )

    return figure


def create_td_sd_autocorrelation_figure(
    artifacts: WorkbenchArtifacts,
    comparison: Optional[Any] = None,
) -> Figure:
    figure = Figure(figsize=(14, 10), constrained_layout=True)

    td_axis = figure.add_subplot(2, 2, 1)
    sd_tick_axis = figure.add_subplot(2, 2, 2)
    sd_coarse_axis = figure.add_subplot(2, 2, 3)
    text_axis = figure.add_subplot(2, 2, 4)

    analysis = _observed_simulation_analysis_for_plot(artifacts, comparison)
    if analysis is None:
        _render_unavailable_axes(
            (td_axis, sd_tick_axis, sd_coarse_axis, text_axis),
            "Observed-versus-simulated comparison is unavailable for this input.",
        )

        return figure

    td_squared_returns = np.square(analysis.observed.bucket_series_by_horizon[15].returns)
    sd_tick_squared_returns = np.square(analysis.simulated_seeded_15m.returns)
    sd_coarse_squared_returns = np.square(analysis.simulated_coarse_15m.returns)

    td_autocorrelation = _autocorrelation(td_squared_returns, lag_count=50)
    sd_tick_autocorrelation = _autocorrelation(sd_tick_squared_returns, lag_count=50)
    sd_coarse_autocorrelation = _autocorrelation(sd_coarse_squared_returns, lag_count=50)

    _render_autocorrelation(td_axis, td_autocorrelation, title="Observed 15m Squared Return Autocorrelogram")
    _render_autocorrelation(sd_tick_axis, sd_tick_autocorrelation, title="Simulated Tick->15m Squared Return Autocorrelogram")
    _render_autocorrelation(sd_coarse_axis, sd_coarse_autocorrelation, title="Simulated Coarse 15m Squared Return Autocorrelogram")

    text_axis.axis("off")
    text_axis.set_title("Autocorrelation Summary")
    text_axis.text(
        0.0,
        1.0,
        "\n".join(
            [
                _format_autocorrelation_summary("Observed 15m", td_autocorrelation),
                _format_autocorrelation_summary("Simulated tick->15m", sd_tick_autocorrelation),
                _format_autocorrelation_summary("Simulated coarse 15m", sd_coarse_autocorrelation),
            ]
        ),
        va="top",
        family="monospace",
        transform=text_axis.transAxes,
    )

    return figure


def create_td_sd_summary_statistics_figure(
    artifacts: WorkbenchArtifacts,
    comparison: Optional[Any] = None,
) -> Figure:
    figure = Figure(figsize=(14, 8), constrained_layout=True)

    axis = figure.add_subplot(1, 1, 1)
    axis.axis("off")

    analysis = _observed_simulation_analysis_for_plot(artifacts, comparison)
    if analysis is None:
        axis.text(
            0.5,
            0.5,
            "Observed-versus-simulated comparison is unavailable for this input.",
            ha="center",
            va="center",
            transform=axis.transAxes,
        )

        return figure

    summary_lines = [
        _summary_statistics_line("Observed ticks", analysis.observed.prices),
        _summary_statistics_line("Observed 15m", analysis.observed.bucket_series_by_horizon[15].returns),
        _summary_statistics_line("Simulated ticks", analysis.simulated.returns),
        _summary_statistics_line("Simulated tick 15m", analysis.simulated_seeded_15m.returns),
        _summary_statistics_line("Simulated coarse 15m", analysis.simulated_coarse_15m.returns),
    ]

    axis.set_title("Summary Statistics")
    axis.text(
        0.0,
        1.0,
        "\n".join(summary_lines),
        va="top",
        family="monospace",
        transform=axis.transAxes,
    )

    return figure


def create_candidate_compare_figure(artifacts: WorkbenchArtifacts) -> Figure:
    figure = Figure(figsize=(15, 8), constrained_layout=True)

    objective_axis = figure.add_subplot(2, 2, 1)
    coordinate_axis = figure.add_subplot(2, 2, 2)
    metric_axis = figure.add_subplot(2, 2, 3)
    summary_axis = figure.add_subplot(2, 2, 4)

    selected_candidate = _selected_candidate_entry(artifacts)
    frontier_entries = _artifact_list(artifacts.candidate_frontier, "frontier_entries")

    alternative_candidates = sorted(
        [
            entry
            for entry in frontier_entries
            if entry.get("candidate_id") != artifacts.selected_candidate_id
        ],
        key=lambda entry: (
            _candidate_objective_value(entry) if _candidate_objective_value(entry) is not None else float("inf"),
            str(entry.get("candidate_id", "")),
        ),
    )
    comparison_candidate = alternative_candidates[0] if alternative_candidates else None

    if selected_candidate is None or comparison_candidate is None:
        _render_unavailable_axes(
            (objective_axis, coordinate_axis, metric_axis, summary_axis),
            "Candidate-compare diagnostics require a selected candidate and at least one comparison candidate.",
        )

        return figure

    objective_axis.bar(
        ["selected", "comparison"],
        [
            _candidate_objective_value(selected_candidate) or 0.0,
            _candidate_objective_value(comparison_candidate) or 0.0,
        ],
        color=["#f59e0b", "#2563eb"],
    )

    objective_axis.set_title("Objective Comparison")
    objective_axis.set_ylabel("Objective")
    objective_axis.grid(axis="y", alpha=0.3)

    selected_coordinates = _coordinate_mapping_from_raw(selected_candidate.get("canonical_coordinates"))
    comparison_coordinates = _coordinate_mapping_from_raw(comparison_candidate.get("canonical_coordinates"))

    common_coordinate_names = sorted(set(selected_coordinates) | set(comparison_coordinates))
    if common_coordinate_names:
        coordinate_axis.barh(
            common_coordinate_names,
            [
                selected_coordinates.get(coordinate_name, 0.0) - comparison_coordinates.get(coordinate_name, 0.0)
                for coordinate_name in common_coordinate_names
            ],
            color="#0f766e",
        )

        coordinate_axis.invert_yaxis()
    else:
        coordinate_axis.text(
            0.5,
            0.5,
            "No canonical coordinates available",
            ha="center",
            va="center",
            transform=coordinate_axis.transAxes,
        )

    coordinate_axis.set_title("Selected Minus Comparison Coordinates")
    coordinate_axis.set_xlabel("Delta")
    coordinate_axis.grid(axis="x", alpha=0.3)

    selected_metrics = _family_metric_mapping(selected_candidate)
    comparison_metrics = _family_metric_mapping(comparison_candidate)

    common_metric_names = sorted(set(selected_metrics) | set(comparison_metrics))

    metric_axis.barh(
        common_metric_names,
        [
            selected_metrics.get(metric_name, 0.0) - comparison_metrics.get(metric_name, 0.0)
            for metric_name in common_metric_names
        ],
        color="#dc2626",
    )
    metric_axis.set_title("Selected Minus Comparison Family Distances")
    metric_axis.set_xlabel("Normalized-distance delta")
    metric_axis.invert_yaxis()
    metric_axis.grid(axis="x", alpha=0.3)

    summary_axis.axis("off")
    summary_axis.set_title("Candidate Compare Summary")
    summary_axis.text(
        0.0,
        1.0,
        "\n".join(
            [
                f"selected: {selected_candidate.get('candidate_id', 'unknown')}",
                f"comparison: {comparison_candidate.get('candidate_id', 'unknown')}",
                f"selected admissible: {bool(selected_candidate.get('admissible'))}",
                f"comparison admissible: {bool(comparison_candidate.get('admissible'))}",
                f"selected stage: {selected_candidate.get('stage', 'unknown')}",
                f"comparison stage: {comparison_candidate.get('stage', 'unknown')}",
            ]
        ),
        va="top",
        ha="left",
        family="monospace",
        transform=summary_axis.transAxes,
    )

    return figure


def save_all_figures(artifacts: WorkbenchArtifacts, output_directory: Path) -> dict[str, str]:
    output_directory.mkdir(parents=True, exist_ok=True)
    observed_simulation_comparison = build_observed_simulation_comparison(artifacts)

    figures = {
        "selected_candidate_diagnostics": create_selected_candidate_diagnostics_figure(artifacts),
        "observed_targets": create_observed_targets_figure(artifacts),
        "candidate_frontier": create_frontier_figure(artifacts),
        "run_progress": create_run_progress_figure(artifacts),
        "runtime_profile": create_runtime_figure(artifacts),
        "parameter_objective": create_parameter_objective_figure(artifacts),
        "parameter_search": create_parameter_search_figure(artifacts),
        "coarse_tick_consistency": create_consistency_figure(artifacts),
        "simulation_summary": create_simulation_summary_figure(artifacts),
        "search_surface_manifest": create_search_surface_figure(artifacts),
        "observed_simulated_comparison": create_observed_simulated_figure(
            artifacts,
            observed_simulation_comparison,
        ),
        "td_sd_return_distributions": create_td_sd_return_distribution_figure(
            artifacts,
            observed_simulation_comparison,
        ),
        "td_sd_return_diagnostics": create_td_sd_return_diagnostics_figure(
            artifacts,
            observed_simulation_comparison,
        ),
        "td_sd_hourly_profiles": create_td_sd_hourly_profile_figure(
            artifacts,
            observed_simulation_comparison,
        ),
        "td_sd_horizon_profiles": create_td_sd_horizon_profile_figure(
            artifacts,
            observed_simulation_comparison,
        ),
        "td_sd_mean_reversion_variance_growth": create_td_sd_mean_reversion_variance_growth_figure(
            artifacts,
            observed_simulation_comparison,
        ),
        "td_sd_mean_reversion_future_change": create_td_sd_mean_reversion_future_change_figure(
            artifacts,
            observed_simulation_comparison,
        ),
        "td_sd_autocorrelation": create_td_sd_autocorrelation_figure(
            artifacts,
            observed_simulation_comparison,
        ),
        "td_sd_summary_statistics": create_td_sd_summary_statistics_figure(
            artifacts,
            observed_simulation_comparison,
        ),
        "candidate_compare": create_candidate_compare_figure(artifacts),
    }

    written_paths: dict[str, str] = {}
    for figure_name, figure in figures.items():
        output_path = output_directory / f"{figure_name}.png"
        figure.savefig(output_path, dpi=160)
        written_paths[figure_name] = str(output_path)

    return written_paths


def _observed_simulation_comparison_for_plot(
    artifacts: WorkbenchArtifacts,
    comparison: Optional[Any],
) -> Optional[Any]:
    if comparison is not None:
        return comparison

    return build_observed_simulation_comparison(artifacts)


def _render_return_histogram(
    axis: Any,
    observed_returns: NDArray[np.float64],
    simulated_returns: NDArray[np.float64],
    *,
    title: str,
    observed_label: str,
    simulated_label: str,
) -> None:
    bins = _bin_count_for_histogram(
        _combine_histogram_samples(observed_returns, simulated_returns)
    )

    if observed_returns.size > 0:
        observed_histogram, observed_edges = np.histogram(observed_returns, bins=bins)
        observed_widths = np.diff(observed_edges)
        axis.bar(
            observed_edges[:-1],
            observed_histogram,
            width=observed_widths,
            align="edge",
            alpha=0.55,
            color="#111827",
            label=observed_label,
        )

    if simulated_returns.size > 0:
        simulated_histogram, simulated_edges = np.histogram(simulated_returns, bins=bins)
        simulated_widths = np.diff(simulated_edges)
        axis.bar(
            simulated_edges[:-1],
            simulated_histogram,
            width=simulated_widths,
            align="edge",
            alpha=0.45,
            color="#dc2626",
            label=simulated_label,
        )

    axis.set_title(title)
    axis.set_xlabel("Log return")
    axis.set_ylabel("Count")
    axis.grid(alpha=0.3)

    if observed_returns.size > 0 or simulated_returns.size > 0:
        axis.legend(loc="best")


def _combine_histogram_samples(
    left_values: NDArray[np.float64],
    right_values: NDArray[np.float64],
) -> NDArray[np.float64]:
    if left_values.size == 0:
        return right_values
    if right_values.size == 0:
        return left_values

    return np.concatenate((left_values, right_values))


def _render_single_return_histogram(
    axis: Any,
    returns: NDArray[np.float64],
    *,
    title: str,
    series_label: str,
    color: str,
) -> None:
    if returns.size > 0:
        histogram, edges = np.histogram(returns, bins=_bin_count_for_histogram(returns))
        axis.bar(
            edges[:-1],
            histogram,
            width=np.diff(edges),
            align="edge",
            alpha=0.6,
            color=color,
            label=series_label,
        )
        axis.legend(loc="best")

    axis.set_title(title)
    axis.set_xlabel("Log return")
    axis.set_ylabel("Count")
    axis.grid(alpha=0.3)


def _bin_count_for_histogram(values: Any) -> int:
    normalized_values = np.asarray(values, dtype=np.float64)
    if normalized_values.size < 2:
        return 10

    sorted_values = np.sort(normalized_values)
    q1 = float(np.quantile(sorted_values, 0.25))
    q3 = float(np.quantile(sorted_values, 0.75))

    interquartile_range = q3 - q1
    if interquartile_range <= 0.0:
        return max(10, min(60, int(math.sqrt(normalized_values.size))))

    bin_width = 2.0 * interquartile_range / (normalized_values.size ** (1.0 / 3.0))
    if bin_width <= 0.0:
        return max(10, min(60, int(math.sqrt(normalized_values.size))))

    bin_count = math.ceil((float(sorted_values[-1]) - float(sorted_values[0])) / bin_width)

    return max(10, min(80, bin_count))


def _format_distribution_summary(label: str, values: NDArray[np.float64]) -> str:
    if values.size == 0:
        return f"{label:<16} count=0"

    mean_value = float(np.mean(values))
    variance = float(np.var(values))

    return (
        f"{label:<16} count={values.size:>6} mean={mean_value:>10.4g} "
        f"stdev={math.sqrt(max(variance, 0.0)):>10.4g} "
        f"min={float(np.min(values)):>10.4g} max={float(np.max(values)):>10.4g}"
    )


def _render_hourly_profile(
    axis: Any,
    observed_profile: NDArray[np.float64],
    simulated_profile: NDArray[np.float64],
    *,
    title: str,
    y_label: str,
) -> None:
    hours = list(range(24))

    axis.plot(hours, observed_profile, label="Observed", color="#111827")
    axis.plot(hours, simulated_profile, label="Simulated", color="#dc2626")

    axis.set_title(title)
    axis.set_xlabel("Hour of day (UTC)")
    axis.set_ylabel(y_label)
    axis.set_xticks([0, 4, 8, 12, 16, 20, 23])
    axis.grid(alpha=0.3)
    axis.legend(loc="best")


@dataclass(frozen=True, slots=True)
class WorkbenchConditionalFutureChangeProfile:
    mean_deviation: NDArray[np.float64]
    mean_future_change: NDArray[np.float64]
    shared_bin_edges: NDArray[np.float64]
    minimum_sample_count: int


def _mean_reversion_log_spaced_horizons(max_horizon: int) -> NDArray[np.int64]:
    if max_horizon < 1:
        return np.asarray([], dtype=np.int64)

    horizon_count = min(
        max_horizon,
        max(2, int(math.ceil(math.log2(max_horizon))) + 1),
    )
    return np.unique(
        np.rint(
            np.geomspace(
                1,
                max_horizon,
                num=horizon_count,
            )
        ).astype(np.int64)
    )


def _variance_of_deviation_difference(
    deviations_from_anchor: NDArray[np.float64],
    horizon: int,
) -> float:
    deviation_differences = deviations_from_anchor[horizon:] - deviations_from_anchor[:-horizon]
    return float(np.var(deviation_differences))


def _conditional_future_change_profile(
    series: WorkbenchMeanReversionSeriesAnalysis,
    *,
    horizon: int,
    shared_bin_edges: Optional[NDArray[np.float64]] = None,
) -> WorkbenchConditionalFutureChangeProfile:
    if series.deviations_from_anchor.size <= horizon or series.log_prices.size <= horizon:
        return WorkbenchConditionalFutureChangeProfile(
            mean_deviation=np.asarray([], dtype=np.float64),
            mean_future_change=np.asarray([], dtype=np.float64),
            shared_bin_edges=(
                np.asarray([], dtype=np.float64)
                if shared_bin_edges is None
                else shared_bin_edges
            ),
            minimum_sample_count=0,
        )

    deviation = series.deviations_from_anchor[:-horizon]
    future_change = series.log_prices[horizon:] - series.log_prices[:-horizon]
    if shared_bin_edges is None:
        lower_quantile, upper_quantile = np.quantile(
            deviation,
            (
                _MEAN_REVERSION_TAIL_QUANTILE_LOWER_BOUND,
                _MEAN_REVERSION_TAIL_QUANTILE_UPPER_BOUND,
            ),
        )
        if not np.isfinite(lower_quantile) or not np.isfinite(upper_quantile) or lower_quantile >= upper_quantile:
            return WorkbenchConditionalFutureChangeProfile(
                mean_deviation=np.asarray([], dtype=np.float64),
                mean_future_change=np.asarray([], dtype=np.float64),
                shared_bin_edges=np.asarray([], dtype=np.float64),
                minimum_sample_count=0,
            )

        bin_count = min(
            _MEAN_REVERSION_BIN_COUNT_UPPER_BOUND,
            max(
                _MEAN_REVERSION_BIN_COUNT_LOWER_BOUND,
                int(math.floor(math.sqrt(deviation.size))),
            ),
        )
        shared_bin_edges = np.linspace(
            float(lower_quantile),
            float(upper_quantile),
            num=bin_count + 1,
            dtype=np.float64,
        )

    if shared_bin_edges.size < 2:
        return WorkbenchConditionalFutureChangeProfile(
            mean_deviation=np.asarray([], dtype=np.float64),
            mean_future_change=np.asarray([], dtype=np.float64),
            shared_bin_edges=shared_bin_edges,
            minimum_sample_count=0,
        )

    inlier_mask = (
        np.isfinite(deviation)
        & np.isfinite(future_change)
        & (deviation >= shared_bin_edges[0])
        & (deviation <= shared_bin_edges[-1])
    )
    clipped_deviation = deviation[inlier_mask]
    clipped_future_change = future_change[inlier_mask]
    if clipped_deviation.size == 0:
        return WorkbenchConditionalFutureChangeProfile(
            mean_deviation=np.asarray([], dtype=np.float64),
            mean_future_change=np.asarray([], dtype=np.float64),
            shared_bin_edges=shared_bin_edges,
            minimum_sample_count=0,
        )

    raw_bin_indexes = np.digitize(clipped_deviation, shared_bin_edges, right=False) - 1
    raw_bin_indexes = np.clip(raw_bin_indexes, 0, shared_bin_edges.size - 2)
    effective_minimum_sample_count = min(
        _MEAN_REVERSION_BIN_MINIMUM_SAMPLE_COUNT,
        max(1, int(clipped_deviation.size // max(shared_bin_edges.size - 1, 1))),
    )

    mean_deviation_by_bin: list[float] = []
    mean_future_change_by_bin: list[float] = []
    for bin_index in range(shared_bin_edges.size - 1):
        bin_mask = raw_bin_indexes == bin_index
        if int(np.count_nonzero(bin_mask)) < effective_minimum_sample_count:
            continue

        mean_deviation_by_bin.append(float(np.mean(clipped_deviation[bin_mask])))
        mean_future_change_by_bin.append(float(np.mean(clipped_future_change[bin_mask])))

    return WorkbenchConditionalFutureChangeProfile(
        mean_deviation=np.asarray(mean_deviation_by_bin, dtype=np.float64),
        mean_future_change=np.asarray(mean_future_change_by_bin, dtype=np.float64),
        shared_bin_edges=shared_bin_edges,
        minimum_sample_count=effective_minimum_sample_count,
    )


def _shared_mean_reversion_bin_edges(
    observed_series: WorkbenchMeanReversionSeriesAnalysis,
    simulated_series: WorkbenchMeanReversionSeriesAnalysis,
    *,
    horizon: int,
) -> NDArray[np.float64]:
    observed_deviation = observed_series.deviations_from_anchor[:-horizon]
    simulated_deviation = simulated_series.deviations_from_anchor[:-horizon]
    combined_deviation = np.concatenate((observed_deviation, simulated_deviation))
    if combined_deviation.size == 0:
        return np.asarray([], dtype=np.float64)

    lower_quantile, upper_quantile = np.quantile(
        combined_deviation,
        (
            _MEAN_REVERSION_TAIL_QUANTILE_LOWER_BOUND,
            _MEAN_REVERSION_TAIL_QUANTILE_UPPER_BOUND,
        ),
    )
    if not np.isfinite(lower_quantile) or not np.isfinite(upper_quantile) or lower_quantile >= upper_quantile:
        return np.asarray([], dtype=np.float64)

    target_bin_count = min(
        _MEAN_REVERSION_BIN_COUNT_UPPER_BOUND,
        max(
            _MEAN_REVERSION_BIN_COUNT_LOWER_BOUND,
            int(math.floor(math.sqrt(combined_deviation.size))),
        ),
    )
    admissible_bin_count = max(
        1,
        int(combined_deviation.size // _MEAN_REVERSION_BIN_MINIMUM_SAMPLE_COUNT),
    )
    bin_count = min(target_bin_count, admissible_bin_count)
    return np.linspace(
        float(lower_quantile),
        float(upper_quantile),
        num=bin_count + 1,
        dtype=np.float64,
    )


def _render_rolling_activity_plot(
    axis: Any,
    points: NDArray[np.float64],
    *,
    title: str,
    color: str,
) -> None:
    if points.shape[0] > 0:
        rendered_points = _downsample_point_cloud(points, max_points=_MAX_SCATTER_POINTS)
        axis.scatter(
            rendered_points[:, 0],
            rendered_points[:, 1],
            alpha=0.6,
            color=color,
        )

    axis.set_title(title)
    axis.set_xlabel("Rolling activity over 24 x 15m bins")
    axis.set_ylabel("Rolling realized volatility")
    axis.grid(alpha=0.3)


def _autocorrelation(values: NDArray[np.float64], *, lag_count: int) -> list[float]:
    if values.size < 2:
        return []

    mean_value = float(np.mean(values))
    centered_values = values - mean_value
    denominator = float(np.dot(centered_values, centered_values))
    if denominator <= 0.0:
        return [0.0 for _ in range(lag_count)]

    correlations: list[float] = []
    for lag in range(1, lag_count + 1):
        if lag >= values.size:
            correlations.append(0.0)
            continue

        numerator = float(np.dot(centered_values[lag:], centered_values[:-lag]))
        correlations.append(numerator / denominator)

    return correlations


def _render_autocorrelation(axis: Any, values: list[float], *, title: str) -> None:
    lags = list(range(1, len(values) + 1))

    if values:
        axis.bar(lags, values, color="#2563eb", alpha=0.75)

    axis.set_title(title)
    axis.set_xlabel("Lag")
    axis.set_ylabel("Autocorrelation")
    axis.grid(alpha=0.3)


def _format_autocorrelation_summary(label: str, values: list[float]) -> str:
    if len(values) == 0:
        return f"{label:<16} no data"

    return (
        f"{label:<16} lag1={values[0]:>8.4f} "
        f"lag5={values[4] if len(values) >= 5 else 0.0:>8.4f} "
        f"lag10={values[9] if len(values) >= 10 else 0.0:>8.4f}"
    )


def _summary_statistics_line(label: str, values: NDArray[np.float64]) -> str:
    if values.size == 0:
        return f"{label:<14} count=0"

    mean_value = float(np.mean(values))
    variance = float(np.var(values))
    quantiles = np.quantile(values, np.asarray([0.05, 0.50, 0.95], dtype=np.float64))

    return (
        f"{label:<14} count={values.size:>6} mean={mean_value:>10.4g} "
        f"stdev={math.sqrt(max(variance, 0.0)):>10.4g} "
        f"p05={float(quantiles[0]):>10.4g} "
        f"p50={float(quantiles[1]):>10.4g} "
        f"p95={float(quantiles[2]):>10.4g}"
    )


def _candidate_objective_value(entry: dict[str, Any]) -> Optional[float]:
    for field_name in ("scalar_tie_break", "penalized_objective_value", "objective_value"):
        value = entry.get(field_name)
        if isinstance(value, int | float):
            return float(value)

    return None


def _coordinate_mapping_from_raw(raw_coordinates: Any) -> dict[str, float]:
    if isinstance(raw_coordinates, dict):
        return {
            key: float(value)
            for key, value in raw_coordinates.items()
            if isinstance(key, str) and isinstance(value, int | float)
        }

    if not isinstance(raw_coordinates, list):
        return {}

    mapping: dict[str, float] = {}
    for raw_entry in raw_coordinates:
        if (
            isinstance(raw_entry, list | tuple)
            and len(raw_entry) == 2
            and isinstance(raw_entry[0], str)
            and isinstance(raw_entry[1], int | float)
        ):
            mapping[raw_entry[0]] = float(raw_entry[1])

    return mapping


def _parameter_search_points(
    artifacts: WorkbenchArtifacts,
) -> dict[str, list[dict[str, float | bool]]]:
    parameter_points: dict[str, list[dict[str, float | bool]]] = defaultdict(list)
    frontier_entries = _artifact_list(artifacts.candidate_frontier, "frontier_entries")

    for entry in frontier_entries:
        objective_value = _candidate_objective_value(entry)
        if objective_value is None:
            continue

        coordinate_mapping = _coordinate_mapping_from_raw(entry.get("canonical_coordinates"))
        for parameter_name, parameter_value in coordinate_mapping.items():
            parameter_points[parameter_name].append(
                {
                    "value": parameter_value,
                    "objective": objective_value,
                    "admissible": bool(entry.get("admissible")),
                }
            )

    if parameter_points:
        return parameter_points

    for surface_point in _artifact_list(artifacts.search_surface_manifest, "surfaces"):
        surface_id = surface_point.get("surface_id")
        x_value = surface_point.get("x_value")
        objective_value = surface_point.get("objective_value")

        if (
            not isinstance(surface_id, str)
            or not isinstance(x_value, int | float)
            or not isinstance(objective_value, int | float)
            or isinstance(surface_point.get("y_value"), int | float)
        ):
            continue

        parameter_points[surface_id].append(
            {
                "value": float(x_value),
                "objective": float(objective_value),
                "admissible": bool(surface_point.get("admissible")),
            }
        )

    return parameter_points


def _pairwise_parameter_points(
    artifacts: WorkbenchArtifacts,
    *,
    left_parameter: str,
    right_parameter: str,
) -> list[tuple[float, float, float]]:
    pairwise_points: list[tuple[float, float, float]] = []
    for entry in _artifact_list(artifacts.candidate_frontier, "frontier_entries"):
        objective_value = _candidate_objective_value(entry)
        coordinate_mapping = _coordinate_mapping_from_raw(entry.get("canonical_coordinates"))

        if (
            objective_value is None
            or left_parameter not in coordinate_mapping
            or right_parameter not in coordinate_mapping
        ):
            continue

        pairwise_points.append(
            (
                coordinate_mapping[left_parameter],
                coordinate_mapping[right_parameter],
                objective_value,
            )
        )

    return pairwise_points


def _binned_admissibility(
    points: list[dict[str, float | bool]],
    *,
    bin_count: int,
) -> list[tuple[float, float]]:
    values = [float(point["value"]) for point in points]
    if len(values) < 2:
        return []

    lower_bound = min(values)
    upper_bound = max(values)
    if lower_bound == upper_bound:
        admissible_share = sum(1 for point in points if bool(point["admissible"])) / len(points)
        return [(lower_bound, admissible_share)]

    width = (upper_bound - lower_bound) / max(bin_count, 1)
    bins: list[list[bool]] = [[] for _ in range(bin_count)]

    for point in points:
        value = float(point["value"])
        bin_index = min(int((value - lower_bound) / width), bin_count - 1)
        bins[bin_index].append(bool(point["admissible"]))

    binned_points: list[tuple[float, float]] = []
    for index, admissibility_values in enumerate(bins):
        if not admissibility_values:
            continue

        center = lower_bound + (index + 0.5) * width
        admissible_share = sum(admissibility_values) / len(admissibility_values)

        binned_points.append((center, admissible_share))

    return binned_points


def _format_parameter_search_summary(
    parameter_name: str,
    points: list[dict[str, float | bool]],
) -> str:
    values = [float(point["value"]) for point in points]
    objectives = [float(point["objective"]) for point in points]
    admissible_count = sum(1 for point in points if bool(point["admissible"]))

    if not values or not objectives:
        return f"{parameter_name:<24} no data"

    return (
        f"{parameter_name:<24} count={len(points):>5} "
        f"admissible={admissible_count:>5} "
        f"min={min(values):>10.4g} max={max(values):>10.4g} "
        f"best_obj={min(objectives):>10.4g}"
    )


def _render_qq_plot(
    axis: Any,
    observed_values: NDArray[np.float64],
    simulated_values: NDArray[np.float64],
    *,
    title: str,
) -> None:
    quantile_pairs = _qq_pairs(observed_values, simulated_values)
    if quantile_pairs:
        axis.scatter(
            [pair[0] for pair in quantile_pairs],
            [pair[1] for pair in quantile_pairs],
            color="#1d4ed8",
            alpha=0.75,
        )

        lower_bound = min([value for pair in quantile_pairs for value in pair])
        upper_bound = max([value for pair in quantile_pairs for value in pair])

        axis.plot(
            [lower_bound, upper_bound],
            [lower_bound, upper_bound],
            color="#94a3b8",
            linestyle="--",
            linewidth=1.0,
        )
    else:
        axis.text(
            0.5,
            0.5,
            "Insufficient data for QQ plot",
            ha="center",
            va="center",
            transform=axis.transAxes,
        )

    axis.set_title(title)
    axis.set_xlabel("Observed quantiles")
    axis.set_ylabel("Simulated quantiles")
    axis.grid(alpha=0.3)


def _qq_pairs(
    observed_values: NDArray[np.float64],
    simulated_values: NDArray[np.float64],
) -> list[tuple[float, float]]:
    sample_size = min(observed_values.size, simulated_values.size, 100)
    if sample_size < 2:
        return []

    observed_quantiles = np.quantile(
        observed_values,
        np.linspace(0.0, 1.0, num=sample_size),
    )
    simulated_quantiles = np.quantile(
        simulated_values,
        np.linspace(0.0, 1.0, num=sample_size),
    )

    return [
        (float(observed_quantile), float(simulated_quantile))
        for observed_quantile, simulated_quantile in zip(
            observed_quantiles,
            simulated_quantiles,
            strict=False,
        )
    ]


def _render_ecdf_overlay(
    axis: Any,
    series: tuple[tuple[str, NDArray[np.float64], str], ...],
) -> None:
    for label, values, color in series:
        if values.size == 0:
            continue

        sample_size = min(values.size, 2_000)
        quantile_grid = np.linspace(0.0, 1.0, num=sample_size)
        quantiles = np.quantile(values, quantile_grid)
        axis.plot(quantiles, quantile_grid, label=label, color=color)

    axis.set_title("Empirical CDF Overlay")
    axis.set_xlabel("Return")
    axis.set_ylabel("Cumulative probability")
    axis.grid(alpha=0.3)
    axis.legend(loc="best", fontsize="small")


def _render_tail_exceedance(
    axis: Any,
    series: tuple[tuple[str, NDArray[np.float64], str], ...],
) -> None:
    non_empty_series = [values for _, values, _ in series if values.size > 0]
    if len(non_empty_series) == 0:
        axis.text(
            0.5,
            0.5,
            "Insufficient data for tail exceedance",
            ha="center",
            va="center",
            transform=axis.transAxes,
        )
        axis.set_xticks([])
        axis.set_yticks([])

        return

    absolute_values = np.abs(np.concatenate(non_empty_series))
    if absolute_values.size < 2:
        axis.text(
            0.5,
            0.5,
            "Insufficient data for tail exceedance",
            ha="center",
            va="center",
            transform=axis.transAxes,
        )
        axis.set_xticks([])
        axis.set_yticks([])
        return

    thresholds = np.unique(
        np.quantile(
            absolute_values,
            np.asarray([0.50, 0.70, 0.80, 0.90, 0.95, 0.99], dtype=np.float64),
        )
    )

    for label, values, color in series:
        if values.size == 0:
            continue

        exceedance = np.asarray(
            [
                float(np.mean(np.abs(values) >= threshold))
                for threshold in thresholds
            ],
            dtype=np.float64,
        )

        axis.plot(thresholds, exceedance, label=label, color=color, marker="o")

    axis.set_title("Tail Exceedance Comparison")
    axis.set_xlabel("Absolute-return threshold")
    axis.set_ylabel("Exceedance probability")
    axis.grid(alpha=0.3)
    axis.legend(loc="best", fontsize="small")


def _horizon_label(interval_minutes: int) -> str:
    if interval_minutes < 60:
        return f"{interval_minutes}m"
    if interval_minutes < 1440:
        return f"{interval_minutes // 60}h"

    return f"{interval_minutes // 1440}d"


def _realized_variance(values: NDArray[np.float64]) -> float:
    return float(np.sum(values * values))


def _mean_absolute_value(values: NDArray[np.float64]) -> float:
    if values.size == 0:
        return 0.0

    return float(np.mean(np.abs(values)))


def _render_grouped_bars(
    axis: Any,
    labels: list[str],
    left_values: list[float],
    right_values: list[float],
    *,
    left_label: str,
    right_label: str,
    title: str,
    y_label: str,
) -> None:
    positions = list(range(len(labels)))
    width = 0.38

    axis.bar(
        [position - width / 2 for position in positions],
        left_values,
        width=width,
        label=left_label,
        color="#111827",
        alpha=0.85,
    )
    axis.bar(
        [position + width / 2 for position in positions],
        right_values,
        width=width,
        label=right_label,
        color="#dc2626",
        alpha=0.75,
    )

    axis.set_title(title)
    axis.set_xticks(positions)
    axis.set_xticklabels(labels)
    axis.set_ylabel(y_label)
    axis.grid(axis="y", alpha=0.3)
    axis.legend(loc="best")


def _render_event_count_distribution(
    axis: Any,
    analysis: WorkbenchObservedSimulationAnalysis,
    *,
    horizon_minutes: tuple[int, ...],
) -> None:
    data: list[list[int]] = []
    labels: list[str] = []
    positions: list[float] = []
    current_position = 1.0

    for interval_minutes in horizon_minutes:
        observed_counts = analysis.observed.bucket_series_by_horizon[interval_minutes].event_counts
        simulated_counts = analysis.simulated.bucket_series_by_horizon[interval_minutes].event_counts

        if observed_counts.size > 0:
            data.append(observed_counts.tolist())
            positions.append(current_position)
            labels.append(f"Observed {_horizon_label(interval_minutes)}")
            current_position += 0.4

        if simulated_counts.size > 0:
            data.append(simulated_counts.tolist())
            positions.append(current_position)
            labels.append(f"Simulated {_horizon_label(interval_minutes)}")
            current_position += 0.7

    if data:
        axis.boxplot(
            data,
            positions=positions,
            widths=0.28,
            orientation="vertical",
            patch_artist=False,
        )
        axis.set_xticks(positions)
        axis.set_xticklabels(labels, rotation=20)
    else:
        axis.text(
            0.5,
            0.5,
            "No event-count distributions available",
            ha="center",
            va="center",
            transform=axis.transAxes,
        )
        axis.set_xticks([])
        axis.set_yticks([])

    axis.set_title("Event Count Distribution By Horizon")
    axis.set_ylabel("Bucket event count")
    axis.grid(axis="y", alpha=0.3)


def _family_metric_mapping(candidate_entry: dict[str, Any]) -> dict[str, float]:
    metrics = candidate_entry.get("family_metrics")
    if not isinstance(metrics, list):
        return {}

    family_metric_mapping: dict[str, float] = {}
    for metric in metrics:
        if not isinstance(metric, dict):
            continue

        family_name = metric.get("target_family")
        normalized_distance = metric.get("normalized_distance")
        if isinstance(family_name, str) and isinstance(normalized_distance, int | float):
            family_metric_mapping[family_name] = float(normalized_distance)

    return family_metric_mapping


def _running_best(values: list[float]) -> list[float]:
    running_values: list[float] = []
    best_value = float("inf")

    for value in values:
        best_value = min(best_value, value)
        running_values.append(best_value)

    return running_values


def _selected_candidate_entry(artifacts: WorkbenchArtifacts) -> Optional[dict[str, Any]]:
    selected_candidate_id = artifacts.selected_candidate_id
    if selected_candidate_id is None:
        return None

    selected_candidate_entry = next(
        (
            entry
            for entry in _artifact_list(artifacts.candidate_frontier, "frontier_entries")
            if entry.get("candidate_id") == selected_candidate_id
        ),
        None,
    )
    if not isinstance(selected_candidate_entry, dict):
        return None

    return _selected_candidate_entry_with_stage_report_fallback(
        artifacts,
        selected_candidate_entry,
        selected_candidate_id,
    )


def _selected_candidate_entry_with_stage_report_fallback(
    artifacts: WorkbenchArtifacts,
    selected_candidate_entry: dict[str, Any],
    selected_candidate_id: str,
) -> dict[str, Any]:
    if selected_candidate_entry.get("stage") != "joint_refinement":
        return selected_candidate_entry

    family_metrics = selected_candidate_entry.get("family_metrics")
    if not isinstance(family_metrics, list):
        family_metrics = []

    if any(
        isinstance(metric, dict) and _metric_has_comparable_observed_simulated_values(metric)
        for metric in family_metrics
    ):
        return selected_candidate_entry

    stage_report_candidate = _selected_joint_stage_report_candidate(
        artifacts,
        selected_candidate_id,
    )
    if stage_report_candidate is None:
        return selected_candidate_entry

    objective_terms = stage_report_candidate.get("objective_terms")
    stage_report_family_metrics = _family_metrics_from_objective_terms(
        objective_terms,
        admissible=bool(stage_report_candidate.get("admissible")),
    )
    if not any(
        _metric_has_comparable_observed_simulated_values(metric)
        for metric in stage_report_family_metrics
    ):
        return selected_candidate_entry

    enriched_entry = dict(selected_candidate_entry)
    enriched_entry["family_metrics"] = stage_report_family_metrics
    return enriched_entry


def _selected_joint_stage_report_candidate(
    artifacts: WorkbenchArtifacts,
    selected_candidate_id: str,
) -> Optional[dict[str, Any]]:
    stage_reports = _stage_report_payloads(artifacts)
    for stage_report in stage_reports:
        if stage_report.get("stage_report_kind") != "joint_refinement":
            continue

        fitted_stage_payload = stage_report.get("fitted_stage_payload")
        if not isinstance(fitted_stage_payload, dict):
            continue

        candidate_frontier = fitted_stage_payload.get("candidate_frontier")
        if not isinstance(candidate_frontier, list):
            continue

        for candidate in candidate_frontier:
            if (
                isinstance(candidate, dict)
                and candidate.get("candidate_id") == selected_candidate_id
            ):
                return candidate

    return None


def _stage_report_payloads(artifacts: WorkbenchArtifacts) -> list[dict[str, Any]]:
    fit_report_payload = artifacts.fit_report_payload
    if not isinstance(fit_report_payload, dict):
        return []

    stage_reports = fit_report_payload.get("stage_reports")
    if isinstance(stage_reports, list):
        return [payload for payload in stage_reports if isinstance(payload, dict)]

    staged_response = fit_report_payload.get("staged_response")
    if not isinstance(staged_response, dict):
        return []

    nested_stage_reports = staged_response.get("stage_reports")
    if not isinstance(nested_stage_reports, list):
        return []

    return [payload for payload in nested_stage_reports if isinstance(payload, dict)]


def _family_metrics_from_objective_terms(
    raw_terms: Any,
    *,
    admissible: bool,
) -> list[dict[str, Any]]:
    if not isinstance(raw_terms, list):
        return []

    family_metrics: list[dict[str, Any]] = []
    for raw_term in raw_terms:
        if not isinstance(raw_term, dict):
            continue

        metric_id = raw_term.get("metric_id")
        target_id = raw_term.get("target_id", metric_id)
        target_family = raw_term.get("target_family")
        normalized_distance = raw_term.get("normalized_error")
        observed_value = raw_term.get("observed_value")
        simulated_value = raw_term.get("simulated_value")
        uncertainty_scale = raw_term.get("uncertainty_scale")

        if not isinstance(metric_id, str) or not isinstance(target_id, str):
            continue
        if not isinstance(target_family, str):
            target_family = "diagnostic"
        if not isinstance(normalized_distance, int | float):
            continue
        if not isinstance(observed_value, int | float) or not isinstance(
            simulated_value, int | float
        ):
            continue
        if not isinstance(uncertainty_scale, int | float):
            continue

        family_metrics.append(
            {
                "metric_id": metric_id,
                "target_id": target_id,
                "target_family": target_family,
                "normalized_distance": float(normalized_distance),
                "admissible": admissible,
                "simulated_value": float(simulated_value),
                "observed_value": float(observed_value),
                "uncertainty_scale": float(uncertainty_scale),
            }
        )

    return family_metrics


def _decisive_metric_ids(artifacts: WorkbenchArtifacts) -> tuple[str, ...]:
    decision = artifacts.selection_rationale.get("decision", {})
    if not isinstance(decision, dict):
        return ()

    raw_metric_ids = decision.get("decisive_metric_ids", [])
    if not isinstance(raw_metric_ids, list):
        return ()

    return tuple(metric_id for metric_id in raw_metric_ids if isinstance(metric_id, str))


def _aggregate_family_distances(family_metrics: list[dict[str, Any]]) -> dict[str, float]:
    grouped_values: dict[str, list[float]] = defaultdict(list)
    for metric in family_metrics:
        family_name = metric.get("target_family")
        normalized_distance = metric.get("normalized_distance")
        if isinstance(family_name, str) and isinstance(normalized_distance, int | float):
            grouped_values[family_name].append(float(normalized_distance))

    return {
        family_name: sum(values) / len(values)
        for family_name, values in grouped_values.items()
        if values
    }


def _metric_has_comparable_observed_simulated_values(metric: dict[str, Any]) -> bool:
    observed_value = metric.get("observed_value")
    simulated_value = metric.get("simulated_value")
    if not isinstance(observed_value, int | float) or not isinstance(simulated_value, int | float):
        return False

    if _is_legacy_joint_distance_only_metric(metric):
        return False

    return True


def _configure_metric_scatter_axis_scaling(
    scatter_axis: Any,
    observed_values: list[float],
    simulated_values: list[float],
) -> None:
    if _all_positive(observed_values) and _all_positive(simulated_values):
        scatter_axis.set_xscale("log")
        scatter_axis.set_yscale("log")
        return

    positive_absolute_values = [
        abs(value)
        for value in [*observed_values, *simulated_values]
        if math.isfinite(value) and not math.isclose(value, 0.0, abs_tol=1.0e-12)
    ]
    if len(positive_absolute_values) < 2:
        return

    smallest_positive_value = min(positive_absolute_values)
    largest_positive_value = max(positive_absolute_values)
    if largest_positive_value / smallest_positive_value < 1.0e3:
        return

    linear_threshold = max(
        float(np.median(np.asarray(positive_absolute_values))),
        smallest_positive_value * 10.0,
        1.0e-6,
    )
    scatter_axis.set_xscale("symlog", linthresh=linear_threshold)
    scatter_axis.set_yscale("symlog", linthresh=linear_threshold)


def _selected_candidate_entry_uses_legacy_joint_distance_only_metrics(
    selected_candidate_entry: dict[str, Any],
) -> bool:
    family_metrics = selected_candidate_entry.get("family_metrics")
    if not isinstance(family_metrics, list):
        return False

    legacy_joint_metric_count = sum(
        1
        for metric in family_metrics
        if isinstance(metric, dict) and _is_legacy_joint_distance_only_metric(metric)
    )
    return legacy_joint_metric_count > 0 and legacy_joint_metric_count == len(family_metrics)


def _is_legacy_joint_distance_only_metric(metric: dict[str, Any]) -> bool:
    target_id = metric.get("target_id")
    normalized_distance = metric.get("normalized_distance")
    uncertainty_scale = metric.get("uncertainty_scale")
    observed_value = metric.get("observed_value")
    simulated_value = metric.get("simulated_value")

    return (
        isinstance(target_id, str)
        and target_id.startswith("joint.")
        and isinstance(normalized_distance, int | float)
        and isinstance(uncertainty_scale, int | float)
        and isinstance(observed_value, int | float)
        and isinstance(simulated_value, int | float)
        and math.isclose(float(observed_value), 0.0, abs_tol=1.0e-12)
        and math.isclose(
            float(simulated_value),
            float(normalized_distance),
            rel_tol=1.0e-12,
            abs_tol=1.0e-12,
        )
        and math.isclose(float(uncertainty_scale), 1.0, rel_tol=1.0e-12, abs_tol=1.0e-12)
    )


def _render_simulation_summary_metric(
    *,
    axis: Any,
    summaries: list[dict[str, Any]],
    metric_key: str,
    metric_title: str,
    observed_lookup: dict[str, float],
) -> None:
    grouped_values: dict[str, list[float]] = defaultdict(list)
    for summary in summaries:
        horizon_label = summary.get("horizon_label")
        metric_value = summary.get(metric_key)
        if isinstance(horizon_label, str) and isinstance(metric_value, int | float):
            grouped_values[horizon_label].append(float(metric_value))

    horizon_labels = sorted(grouped_values)
    for horizon_index, horizon_label in enumerate(horizon_labels):
        values = grouped_values[horizon_label]

        axis.scatter(
            [horizon_index] * len(values),
            values,
            color="#1d4ed8",
            alpha=0.8,
        )

        if horizon_label in observed_lookup:
            axis.scatter(
                [horizon_index],
                [observed_lookup[horizon_label]],
                color="#dc2626",
                marker="_",
                s=600,
                linewidths=2.5,
            )

    if horizon_labels:
        all_values = [value for values in grouped_values.values() for value in values]
        observed_values = list(observed_lookup.values())
        if _all_positive([*all_values, *observed_values]):
            axis.set_yscale("log")

    axis.set_title(metric_title)
    axis.set_xticks(range(len(horizon_labels)))
    axis.set_xticklabels(horizon_labels)
    axis.set_ylabel(metric_key)
    axis.grid(alpha=0.3)


def _observed_tick_metric_lookup(
    artifacts: WorkbenchArtifacts,
    metric_suffix: str,
) -> dict[str, float]:
    lookup: dict[str, float] = {}
    for metric in _artifact_list(artifacts.coarse_tick_consistency, "metrics"):
        horizon_label = metric.get("horizon_label")
        metric_id = metric.get("metric_id")
        tick_value = metric.get("tick_value")

        if (
            isinstance(horizon_label, str)
            and isinstance(metric_id, str)
            and metric_id.endswith(metric_suffix)
            and isinstance(tick_value, int | float)
        ):
            lookup[horizon_label] = float(tick_value)

    return lookup


def _consistency_heatmap_matrix(
    metrics: list[dict[str, Any]],
) -> tuple[list[str], list[str], list[list[float]]]:
    metric_names = sorted(
        {
            _metric_short_name(cast(str, metric.get("metric_id", "unknown")))
            for metric in metrics
        }
    )
    horizon_labels = sorted(
        {
            str(metric.get("horizon_label", "unknown"))
            for metric in metrics
        }
    )
    if not metric_names or not horizon_labels:
        return [], [], []

    matrix = [
        [0.0 for _ in horizon_labels]
        for _ in metric_names
    ]
    metric_name_to_index = {
        metric_name: metric_index
        for metric_index, metric_name in enumerate(metric_names)
    }
    horizon_to_index = {
        horizon_label: horizon_index
        for horizon_index, horizon_label in enumerate(horizon_labels)
    }

    for metric in metrics:
        metric_name = _metric_short_name(str(metric.get("metric_id", "unknown")))
        horizon_label = str(metric.get("horizon_label", "unknown"))
        normalized_delta = metric.get("normalized_delta")

        if not isinstance(normalized_delta, int | float):
            continue

        matrix[metric_name_to_index[metric_name]][horizon_to_index[horizon_label]] = float(
            normalized_delta
        )

    return metric_names, horizon_labels, matrix


def _mean_delta_by_horizon(metrics: list[dict[str, Any]]) -> dict[str, float]:
    grouped_values: dict[str, list[float]] = defaultdict(list)
    for metric in metrics:
        horizon_label = metric.get("horizon_label")
        normalized_delta = metric.get("normalized_delta")
        if isinstance(horizon_label, str) and isinstance(normalized_delta, int | float):
            grouped_values[horizon_label].append(float(normalized_delta))

    return {
        horizon_label: sum(values) / len(values)
        for horizon_label, values in grouped_values.items()
        if values
    }


def _metric_short_name(metric_id: str) -> str:
    return metric_id.split(".", maxsplit=1)[-1]


def _metric_label(metric: dict[str, Any]) -> str:
    metric_id = str(metric.get("metric_id", "unknown"))
    horizon_label = metric.get("horizon_label")
    if isinstance(horizon_label, str):
        return f"{horizon_label} {_metric_short_name(metric_id)}"

    return metric_id


def _render_unavailable_axes(axes: tuple[Any, ...], message: str) -> None:
    for axis in axes:
        axis.text(
            0.5,
            0.5,
            message,
            ha="center",
            va="center",
            transform=axis.transAxes,
        )
        axis.set_xticks([])
        axis.set_yticks([])


def _all_positive(values: list[float]) -> bool:
    return bool(values) and all(value > 0.0 for value in values)


def json_like(value: dict[str, float]) -> str:
    return "{" + ", ".join(
        f"{key}: {float(metric_value):.3f}"
        for key, metric_value in sorted(value.items())
    ) + "}"


__all__ = [
    "create_candidate_compare_figure",
    "create_message_figure",
    "create_consistency_figure",
    "create_frontier_figure",
    "create_observed_simulated_figure",
    "create_observed_targets_figure",
    "create_parameter_objective_figure",
    "create_parameter_objective_figure_with_columns",
    "create_parameter_search_figure",
    "create_runtime_figure",
    "create_run_progress_figure",
    "create_search_surface_figure",
    "create_selected_candidate_diagnostics_figure",
    "create_simulation_summary_figure",
    "create_td_sd_autocorrelation_figure",
    "create_td_sd_horizon_profile_figure",
    "create_td_sd_hourly_profile_figure",
    "create_td_sd_mean_reversion_future_change_figure",
    "create_td_sd_mean_reversion_variance_growth_figure",
    "create_td_sd_return_diagnostics_figure",
    "create_td_sd_return_distribution_figure",
    "create_td_sd_summary_statistics_figure",
    "save_all_figures",
]
