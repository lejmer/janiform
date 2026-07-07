from __future__ import annotations

from io import BytesIO
from typing import Any, Optional

from .artifacts import WorkbenchArtifacts
from .fit_context import (
    WorkbenchObservedSimulationComparison,
    build_observed_simulation_comparison,
)
from .plot_manifest import WorkbenchPlotManifestEntry, workbench_plot_manifest
from .plots import create_message_figure

_COMPARISON_CACHE_MAX_ENTRIES = 4
_OBSERVED_SIMULATION_COMPARISON_CACHE: dict[
    tuple[str, int],
    Optional[WorkbenchObservedSimulationComparison],
] = {}


def plot_manifest_entry_by_task_id(task_id: str) -> WorkbenchPlotManifestEntry:
    for manifest_entry in workbench_plot_manifest():
        if manifest_entry.task_id == task_id:
            return manifest_entry

    raise KeyError(f"unknown workbench plot task `{task_id}`")


def render_plot_png_bytes(
    artifacts: WorkbenchArtifacts,
    task_id: str,
) -> bytes:
    manifest_entry = plot_manifest_entry_by_task_id(task_id)
    observed_simulation_comparison: Optional[Any] = None

    try:
        if manifest_entry.uses_observed_simulation_comparison:
            observed_simulation_comparison = _cached_observed_simulation_comparison(
                artifacts
            )
            figure = manifest_entry.builder(artifacts, observed_simulation_comparison)
        else:
            figure = manifest_entry.builder(artifacts)
    except Exception as error:
        figure = create_message_figure(
            f"{manifest_entry.tab_title} Failed",
            str(error),
        )

    buffer = BytesIO()
    figure.savefig(buffer, format="png", dpi=140)
    return buffer.getvalue()


def _cached_observed_simulation_comparison(
    artifacts: WorkbenchArtifacts,
) -> Optional[WorkbenchObservedSimulationComparison]:
    cache_key = (str(artifacts.source_path), 1)
    if cache_key in _OBSERVED_SIMULATION_COMPARISON_CACHE:
        return _OBSERVED_SIMULATION_COMPARISON_CACHE[cache_key]

    comparison = build_observed_simulation_comparison(artifacts)
    if len(_OBSERVED_SIMULATION_COMPARISON_CACHE) >= _COMPARISON_CACHE_MAX_ENTRIES:
        first_cache_key = next(iter(_OBSERVED_SIMULATION_COMPARISON_CACHE))
        _OBSERVED_SIMULATION_COMPARISON_CACHE.pop(first_cache_key)

    _OBSERVED_SIMULATION_COMPARISON_CACHE[cache_key] = comparison
    return comparison


__all__ = [
    "plot_manifest_entry_by_task_id",
    "render_plot_png_bytes",
]
