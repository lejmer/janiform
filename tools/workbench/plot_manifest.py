from collections.abc import Callable
from dataclasses import dataclass

from matplotlib.figure import Figure

from .plots import (
    create_consistency_figure,
    create_frontier_figure,
    create_observed_simulated_figure,
    create_observed_targets_figure,
    create_runtime_figure,
    create_search_surface_figure,
    create_selected_candidate_diagnostics_figure,
    create_simulation_summary_figure,
    create_td_sd_autocorrelation_figure,
    create_td_sd_hourly_profile_figure,
    create_td_sd_return_distribution_figure,
    create_td_sd_summary_statistics_figure,
)


@dataclass(frozen=True, slots=True)
class WorkbenchPlotManifestEntry:
    plot_id: str
    task_id: str
    family_id: str
    tab_title: str
    description: str
    builder: Callable[..., Figure]
    uses_observed_simulation_comparison: bool = False


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
            uses_observed_simulation_comparison=True,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="td_sd_return_distributions",
            task_id="plot_td_sd_return_distributions",
            family_id="td_sd_returns",
            tab_title="TD/SD Returns",
            description="Rendering TD and SD tick, aggregated 15-minute, and coarse 15-minute return histograms.",
            builder=create_td_sd_return_distribution_figure,
            uses_observed_simulation_comparison=True,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="td_sd_hourly_profiles",
            task_id="plot_td_sd_hourly_profiles",
            family_id="td_sd_activity",
            tab_title="TD/SD Hourly",
            description="Rendering hourly activity, hourly absolute-return profiles, and rolling volatility-versus-activity diagnostics.",
            builder=create_td_sd_hourly_profile_figure,
            uses_observed_simulation_comparison=True,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="td_sd_autocorrelation",
            task_id="plot_td_sd_autocorrelation",
            family_id="td_sd_returns",
            tab_title="TD/SD Autocorr",
            description="Rendering 50-lag squared-return autocorrelograms for TD, SD tick, and SD coarse 15-minute returns.",
            builder=create_td_sd_autocorrelation_figure,
            uses_observed_simulation_comparison=True,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="td_sd_summary_statistics",
            task_id="plot_td_sd_summary_statistics",
            family_id="td_sd_profiles",
            tab_title="TD/SD Stats",
            description="Rendering summary statistics across TD ticks, TD 15-minute returns, SD ticks, SD tick 15-minute returns, and SD coarse 15-minute returns.",
            builder=create_td_sd_summary_statistics_figure,
            uses_observed_simulation_comparison=True,
        ),
    )


__all__ = [
    "WorkbenchPlotManifestEntry",
    "workbench_plot_manifest",
]
