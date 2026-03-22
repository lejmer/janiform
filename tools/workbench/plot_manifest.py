from dataclasses import dataclass
from typing import Callable

from matplotlib.figure import Figure

from .artifacts import WorkbenchArtifacts
from .plots import (
    create_consistency_figure,
    create_frontier_figure,
    create_observed_simulated_figure,
    create_observed_targets_figure,
    create_runtime_figure,
    create_search_surface_figure,
    create_selected_candidate_diagnostics_figure,
    create_simulation_summary_figure,
)


@dataclass(frozen=True, slots=True)
class WorkbenchPlotManifestEntry:
    plot_id: str
    task_id: str
    family_id: str
    tab_title: str
    description: str
    builder: Callable[[WorkbenchArtifacts], Figure]


def workbench_plot_manifest() -> tuple[WorkbenchPlotManifestEntry, ...]:
    return (
        WorkbenchPlotManifestEntry(
            plot_id="selected_candidate_diagnostics",
            task_id="plot_selected_candidate",
            family_id="candidate_diagnostics",
            tab_title="Selected Candidate",
            description="Rendering selected-candidate fit diagnostics and top misses.",
            builder=create_selected_candidate_diagnostics_figure,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="observed_targets",
            task_id="plot_observed_targets",
            family_id="td_sd_profiles",
            tab_title="Observed Targets",
            description="Rendering observed target family coverage and target stability.",
            builder=create_observed_targets_figure,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="frontier",
            task_id="plot_frontier",
            family_id="search_frontier",
            tab_title="Frontier",
            description="Rendering candidate frontier structure and admissibility state.",
            builder=create_frontier_figure,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="runtime",
            task_id="plot_runtime",
            family_id="run_progress",
            tab_title="Runtime",
            description="Rendering runtime and budget breakdown.",
            builder=create_runtime_figure,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="consistency",
            task_id="plot_consistency",
            family_id="consistency",
            tab_title="Coarse/Tick",
            description="Rendering coarse-versus-tick disagreement diagnostics.",
            builder=create_consistency_figure,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="simulation_summary",
            task_id="plot_simulation_summary",
            family_id="td_sd_returns",
            tab_title="Simulation Summary",
            description="Rendering seed dispersion and horizon-level simulation summaries.",
            builder=create_simulation_summary_figure,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="search_surfaces",
            task_id="plot_search_surfaces",
            family_id="parameter_search",
            tab_title="Search Surfaces",
            description="Rendering search surfaces and admissibility overlays.",
            builder=create_search_surface_figure,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="observed_simulated",
            task_id="plot_observed_simulated",
            family_id="td_sd_activity",
            tab_title="Observed/Simulated",
            description="Rendering training-data versus simulated path and activity comparisons.",
            builder=create_observed_simulated_figure,
        ),
    )


__all__ = [
    "WorkbenchPlotManifestEntry",
    "workbench_plot_manifest",
]
