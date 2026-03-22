from collections import defaultdict
from datetime import UTC, datetime
import math
import os
from pathlib import Path
import tempfile
from typing import Any, Optional, cast

if "MPLCONFIGDIR" not in os.environ:
    matplotlib_cache_directory = Path(tempfile.gettempdir()) / "janiform-mpl"
    matplotlib_cache_directory.mkdir(parents=True, exist_ok=True)
    os.environ["MPLCONFIGDIR"] = str(matplotlib_cache_directory)

from matplotlib.figure import Figure

from .artifacts import WorkbenchArtifacts, _artifact_list
from .fit_context import build_observed_simulation_comparison


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
_TIME_KEY_PREFERENCES: tuple[str, ...] = ("timestamp_utc", "timestamp", "time", "time_microseconds", "datetime")
_PRICE_KEY_PREFERENCES: tuple[str, ...] = ("price", "mid_price", "close", "trade_price", "last_price", "mark_price")


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
        if _all_positive(observed_values) and _all_positive(simulated_values):
            scatter_axis.set_xscale("log")
            scatter_axis.set_yscale("log")
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


def create_observed_simulated_figure(artifacts: WorkbenchArtifacts) -> Figure:
    figure = Figure(figsize=(14, 8), constrained_layout=True)
    price_axis = figure.add_subplot(2, 2, 1)
    normalized_axis = figure.add_subplot(2, 2, 2)
    return_axis = figure.add_subplot(2, 2, 3)
    activity_axis = figure.add_subplot(2, 2, 4)
    comparison = build_observed_simulation_comparison(artifacts)
    if comparison is None:
        _render_unavailable_axes(
            (price_axis, normalized_axis, return_axis, activity_axis),
            "Observed-versus-simulated comparison is unavailable for this input.",
        )
        return figure

    observed_series = _extract_timestamped_price_series(comparison.observed_rows)
    simulated_series = _extract_timestamped_price_series(comparison.simulated_rows)
    if observed_series is None or simulated_series is None:
        _render_unavailable_axes(
            (price_axis, normalized_axis, return_axis, activity_axis),
            "Observed-versus-simulated rows could not be normalized into a price series.",
        )
        return figure

    observed_timestamps, observed_prices, observed_label = observed_series
    simulated_timestamps, simulated_prices, simulated_label = simulated_series

    price_axis.plot(
        observed_timestamps,
        observed_prices,
        color="#111827",
        alpha=0.9,
        label=observed_label,
    )
    price_axis.plot(
        simulated_timestamps,
        simulated_prices,
        color="#dc2626",
        alpha=0.75,
        label=f"{simulated_label} (seed={comparison.simulation_seed})",
    )
    price_axis.set_title("Observed Versus Simulated Price Path")
    price_axis.set_ylabel("Price")
    price_axis.grid(alpha=0.3)
    price_axis.legend(loc="best")

    normalized_axis.plot(
        observed_timestamps,
        _normalize_series(observed_prices),
        color="#111827",
        alpha=0.9,
        label="observed rebased",
    )
    normalized_axis.plot(
        simulated_timestamps,
        _normalize_series(simulated_prices),
        color="#dc2626",
        alpha=0.75,
        label="simulated rebased",
    )
    normalized_axis.set_title("Rebased Path Comparison")
    normalized_axis.set_ylabel("Index Level")
    normalized_axis.grid(alpha=0.3)
    normalized_axis.legend(loc="best")

    observed_returns = _log_returns(observed_prices)
    simulated_returns = _log_returns(simulated_prices)
    histogram_count = max(10, min(50, len(observed_returns) + len(simulated_returns)))
    if observed_returns:
        return_axis.hist(
            observed_returns,
            bins=histogram_count,
            alpha=0.55,
            color="#111827",
            label="observed log returns",
        )
    if simulated_returns:
        return_axis.hist(
            simulated_returns,
            bins=histogram_count,
            alpha=0.45,
            color="#dc2626",
            label="simulated log returns",
        )
    return_axis.set_title("Return Distribution")
    return_axis.set_xlabel("Log Return")
    return_axis.set_ylabel("Count")
    return_axis.grid(alpha=0.3)
    if observed_returns or simulated_returns:
        return_axis.legend(loc="best")

    observed_hourly_counts = _hourly_event_counts(comparison.observed_rows)
    simulated_hourly_counts = _hourly_event_counts(comparison.simulated_rows)
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


def save_all_figures(artifacts: WorkbenchArtifacts, output_directory: Path) -> dict[str, str]:
    output_directory.mkdir(parents=True, exist_ok=True)

    figures = {
        "selected_candidate_diagnostics": create_selected_candidate_diagnostics_figure(artifacts),
        "observed_targets": create_observed_targets_figure(artifacts),
        "candidate_frontier": create_frontier_figure(artifacts),
        "runtime_profile": create_runtime_figure(artifacts),
        "coarse_tick_consistency": create_consistency_figure(artifacts),
        "simulation_summary": create_simulation_summary_figure(artifacts),
        "search_surface_manifest": create_search_surface_figure(artifacts),
        "observed_simulated_comparison": create_observed_simulated_figure(artifacts),
    }
    written_paths: dict[str, str] = {}
    for figure_name, figure in figures.items():
        output_path = output_directory / f"{figure_name}.png"
        figure.savefig(output_path, dpi=160)
        written_paths[figure_name] = str(output_path)

    return written_paths


def _selected_candidate_entry(artifacts: WorkbenchArtifacts) -> Optional[dict[str, Any]]:
    selected_candidate_id = artifacts.selected_candidate_id
    if selected_candidate_id is None:
        return None

    return next(
        (
            entry
            for entry in _artifact_list(artifacts.candidate_frontier, "frontier_entries")
            if entry.get("candidate_id") == selected_candidate_id
        ),
        None,
    )


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


def _extract_timestamped_price_series(
    rows: tuple[dict[str, Any], ...],
) -> tuple[list[object], list[float], str] | None:
    if len(rows) == 0:
        return None

    timestamp_key = _find_time_key(rows[0])
    if timestamp_key is None:
        return None

    price_key = _find_price_key(rows[0], timestamp_key=timestamp_key)
    if price_key is None:
        return None

    filtered_rows = [row for row in rows if isinstance(row.get(price_key), int | float)]
    return (
        [row[timestamp_key] for row in filtered_rows],
        [float(cast(Any, row[price_key])) for row in filtered_rows],
        price_key,
    )


def _hourly_event_counts(rows: tuple[dict[str, Any], ...]) -> dict[datetime, int]:
    if len(rows) == 0:
        return {}

    timestamp_key = _find_time_key(rows[0])
    if timestamp_key is None:
        return {}

    counts_by_hour: dict[datetime, int] = defaultdict(int)
    for row in rows:
        timestamp_value = row.get(timestamp_key)
        if timestamp_value is None:
            continue
        normalized_timestamp = parse_datetime(timestamp_value)
        hour_timestamp = normalized_timestamp.replace(minute=0, second=0, microsecond=0)
        counts_by_hour[hour_timestamp] += 1

    return dict(counts_by_hour)


def _find_time_key(row: dict[str, Any]) -> Optional[str]:
    for preferred_key in _TIME_KEY_PREFERENCES:
        if preferred_key in row:
            return preferred_key

    for key in row:
        normalized_key = key.lower()
        if "timestamp" in normalized_key:
            return key

    for key in row:
        normalized_key = key.lower()
        if (
            normalized_key == "time"
            or "time_microseconds" in normalized_key
            or normalized_key.endswith("_time")
            or "datetime" in normalized_key
        ):
            return key

    return None


def _find_price_key(row: dict[str, Any], *, timestamp_key: str) -> Optional[str]:
    for preferred_key in _PRICE_KEY_PREFERENCES:
        if preferred_key == timestamp_key:
            continue
        if preferred_key in row and isinstance(row.get(preferred_key), int | float):
            return preferred_key

    for key, value in row.items():
        if key == timestamp_key or _find_time_key({key: value}) == key:
            continue
        if isinstance(value, int | float):
            return key

    return None


def _normalize_series(values: list[float]) -> list[float]:
    if not values:
        return []
    base_value = values[0]
    if base_value == 0.0:
        return values
    return [100.0 * value / base_value for value in values]


def _log_returns(prices: list[float]) -> list[float]:
    if len(prices) < 2:
        return []

    log_returns: list[float] = []
    previous_price = prices[0]
    for current_price in prices[1:]:
        if previous_price > 0.0 and current_price > 0.0:
            log_returns.append(math.log(current_price / previous_price))
        previous_price = current_price

    return log_returns


def _all_positive(values: list[float]) -> bool:
    return bool(values) and all(value > 0.0 for value in values)


def parse_datetime(value: Any) -> datetime:
    if isinstance(value, datetime):
        return value.astimezone(UTC)

    text_value = str(value)
    if text_value.endswith("Z"):
        return datetime.fromisoformat(text_value.replace("Z", "+00:00")).astimezone(UTC)

    return datetime.fromisoformat(text_value).astimezone(UTC)


def json_like(value: dict[str, float]) -> str:
    return "{" + ", ".join(
        f"{key}: {float(metric_value):.3f}"
        for key, metric_value in sorted(value.items())
    ) + "}"


__all__ = [
    "create_message_figure",
    "create_consistency_figure",
    "create_frontier_figure",
    "create_observed_simulated_figure",
    "create_observed_targets_figure",
    "create_runtime_figure",
    "create_search_surface_figure",
    "create_selected_candidate_diagnostics_figure",
    "create_simulation_summary_figure",
    "save_all_figures",
]
