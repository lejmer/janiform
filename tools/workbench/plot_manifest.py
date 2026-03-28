from collections.abc import Callable
from dataclasses import dataclass

from matplotlib.figure import Figure

from .plots import (
    create_candidate_compare_figure,
    create_consistency_figure,
    create_frontier_figure,
    create_observed_simulated_figure,
    create_observed_targets_figure,
    create_parameter_objective_figure,
    create_parameter_search_figure,
    create_runtime_figure,
    create_run_progress_figure,
    create_search_surface_figure,
    create_selected_candidate_diagnostics_figure,
    create_simulation_summary_figure,
    create_td_sd_autocorrelation_figure,
    create_td_sd_mean_reversion_future_change_figure,
    create_td_sd_mean_reversion_variance_growth_figure,
    create_td_sd_hourly_profile_figure,
    create_td_sd_horizon_profile_figure,
    create_td_sd_return_diagnostics_figure,
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
    how_to_read: tuple[str, ...]
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
            how_to_read=(
                "Use the family bar chart to see which target families the selected fit misses most.",
                "In the observed-versus-simulated scatter, points near the diagonal are matched well.",
                "The largest-misses panel ranks the worst individual metric gaps by normalized distance.",
                "Lower normalized distances are better.",
            ),
            builder=create_selected_candidate_diagnostics_figure,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="observed_targets",
            task_id="plot_observed_targets",
            family_id="td_sd_profiles",
            tab_title="Observed Targets",
            description="Rendering observed target family coverage and target stability.",
            how_to_read=(
                "The family count chart shows how much target coverage each family contributes.",
                "The stability scatter shows effective sample size against stability score for observed targets.",
                "Targets with low sample size or low stability are weaker calibration anchors.",
            ),
            builder=create_observed_targets_figure,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="frontier",
            task_id="plot_frontier",
            family_id="search_frontier",
            tab_title="Frontier",
            description="Rendering candidate frontier structure and admissibility state.",
            how_to_read=(
                "The frontier scatter shows candidate rank structure by stage. The selected candidate is highlighted.",
                "Lower scalar tie-break values are better.",
                "The stage bars show how many candidates were admissible versus rejected in each stage.",
                "The best-by-stage panel shows the best admissible tie-break each stage managed to reach.",
            ),
            builder=create_frontier_figure,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="run_progress",
            task_id="plot_run_progress",
            family_id="run_progress",
            tab_title="Run Progress",
            description="Rendering best-objective progress, admissible progress, stage counts, and rejection counts.",
            how_to_read=(
                "Objective panels are ordered by candidate evaluation order, not wall-clock time.",
                "The running-best curves should trend downward if the search is improving.",
                "Flat running-best curves suggest stagnation.",
                "The lower panels show how fast each stage is generating and rejecting candidates.",
            ),
            builder=create_run_progress_figure,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="runtime",
            task_id="plot_runtime",
            family_id="run_progress",
            tab_title="Runtime",
            description="Rendering runtime and budget breakdown.",
            how_to_read=(
                "The left panel shows elapsed time by runtime component.",
                "The right panel accumulates those times to show where total runtime is going.",
                "Use this to spot preparation, simulation, or diagnostics phases that dominate runtime.",
            ),
            builder=create_runtime_figure,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="parameter_objective",
            task_id="plot_parameter_objective",
            family_id="parameter_objective",
            tab_title="Param Objective",
            description="Rendering searched values versus objective for each scalar parameter.",
            how_to_read=(
                "Each subplot shows one scalar parameter against the candidate objective.",
                "Lower objective values are better.",
                "Blue points are admissible candidates. Red points are rejected candidates.",
                "Look for low-objective regions, whether the search visited them often, and whether good regions are narrow or broad.",
            ),
            builder=create_parameter_objective_figure,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="parameter_search",
            task_id="plot_parameter_search",
            family_id="parameter_search",
            tab_title="Param Search",
            description="Rendering parameter admissibility, pairwise search geometry, and coverage density.",
            how_to_read=(
                "The admissibility panel shows how admissible share changes across parameter bins.",
                "The density panel shows where search effort was concentrated.",
                "The pairwise panel shows the geometry of the explored region for the selected parameter pair.",
                "The summary panel gives coverage and admissibility snapshots for the most active parameters.",
            ),
            builder=create_parameter_search_figure,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="consistency",
            task_id="plot_consistency",
            family_id="consistency",
            tab_title="Coarse/Tick",
            description="Rendering coarse-versus-tick disagreement diagnostics.",
            how_to_read=(
                "The mismatch ranking shows the largest coarse-versus-tick disagreements.",
                "In the coarse-versus-tick scatter, points near the diagonal are consistent.",
                "The heatmap shows how mismatch changes by metric and horizon.",
                "Large normalized deltas indicate disagreement between evaluation tiers.",
            ),
            builder=create_consistency_figure,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="simulation_summary",
            task_id="plot_simulation_summary",
            family_id="td_sd_returns",
            tab_title="Simulation Summary",
            description="Rendering seed dispersion and horizon-level simulation summaries.",
            how_to_read=(
                "These panels summarize simulation diagnostics across horizons and compare them to observed reference values.",
                "Use them to see whether simulated event count, mean absolute return, and realized variance line up with observed targets.",
                "Large seed dispersion suggests noisy simulation-backed scoring.",
            ),
            builder=create_simulation_summary_figure,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="search_surfaces",
            task_id="plot_search_surfaces",
            family_id="parameter_search",
            tab_title="Search Surfaces",
            description="Rendering search surfaces and admissibility overlays.",
            how_to_read=(
                "The coordinate panel shows the explored search surface geometry.",
                "Admissible and rejected regions are separated so you can see where constraints cut off the search.",
                "The objective panel shows how objective changes along each recorded surface slice.",
                "Flat or noisy slices can indicate weak search guidance.",
            ),
            builder=create_search_surface_figure,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="observed_simulated",
            task_id="plot_observed_simulated",
            family_id="td_sd_activity",
            tab_title="Paths & Activity",
            description="Rendering training-data versus simulated path and activity comparisons.",
            how_to_read=(
                "The top row compares raw and rebased price paths.",
                "The lower-left panel compares return distributions.",
                "The lower-right panel compares hourly event activity.",
                "For a good fit, shapes should broadly match across all four views.",
            ),
            builder=create_observed_simulated_figure,
            uses_observed_simulation_comparison=True,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="td_sd_return_distributions",
            task_id="plot_td_sd_return_distributions",
            family_id="td_sd_returns",
            tab_title="Return Distributions",
            description="Rendering observed and simulated tick, aggregated 15-minute, and coarse 15-minute return histograms.",
            how_to_read=(
                "This compares return histograms at tick and 15-minute scales.",
                "Look for mismatches in spread, skew, and tail width.",
                "If tick-level returns look right but aggregated or coarse returns do not, the problem may be in time aggregation or coarse simulation behavior.",
            ),
            builder=create_td_sd_return_distribution_figure,
            uses_observed_simulation_comparison=True,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="td_sd_return_diagnostics",
            task_id="plot_td_sd_return_diagnostics",
            family_id="td_sd_returns",
            tab_title="Distribution Diagnostics",
            description="Rendering QQ, empirical CDF, and tail exceedance diagnostics for observed versus simulated returns.",
            how_to_read=(
                "QQ plots show whether simulated quantiles line up with observed quantiles.",
                "The ECDF panel compares the full cumulative distributions.",
                "The tail panel focuses on exceedance behavior in the extremes.",
                "Curvature away from the QQ diagonal usually means scale, skew, or tail mismatch.",
            ),
            builder=create_td_sd_return_diagnostics_figure,
            uses_observed_simulation_comparison=True,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="td_sd_hourly_profiles",
            task_id="plot_td_sd_hourly_profiles",
            family_id="td_sd_activity",
            tab_title="Hourly Profiles",
            description="Rendering hourly activity, hourly absolute-return profiles, and rolling volatility-versus-activity diagnostics.",
            how_to_read=(
                "The top row compares average intraday shape by hour bin.",
                "The lower panels compare rolling realized-volatility versus activity clouds for observed and simulated data separately.",
                "Use these to judge whether the model reproduces both intraday structure and the volatility-activity relationship.",
            ),
            builder=create_td_sd_hourly_profile_figure,
            uses_observed_simulation_comparison=True,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="td_sd_horizon_profiles",
            task_id="plot_td_sd_horizon_profiles",
            family_id="td_sd_profiles",
            tab_title="Horizon Profiles",
            description="Rendering rebased paths plus realized-variance, absolute-return, and event-count-by-horizon diagnostics.",
            how_to_read=(
                "The rebased-path panel compares cumulative path shape.",
                "The remaining panels compare how variance, absolute returns, and event counts scale with horizon.",
                "Use this to see whether the fit preserves behavior only at one horizon or across multiple horizons.",
            ),
            builder=create_td_sd_horizon_profile_figure,
            uses_observed_simulation_comparison=True,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="td_sd_mean_reversion_variance_growth",
            task_id="plot_td_sd_mean_reversion_variance_growth",
            family_id="td_sd_profiles",
            tab_title="Deviation Variance",
            description="Rendering variance growth of deviation from the fitted anchor for observed and simulated paths.",
            how_to_read=(
                "This compares how quickly deviation-from-anchor increments spread out as horizon increases.",
                "A curve that bends and flattens indicates stronger mean reversion than a curve that keeps growing linearly.",
                "Observed and simulated curves should agree on both the short-horizon slope and the long-horizon flattening pattern.",
            ),
            builder=create_td_sd_mean_reversion_variance_growth_figure,
            uses_observed_simulation_comparison=True,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="td_sd_mean_reversion_future_change",
            task_id="plot_td_sd_mean_reversion_future_change",
            family_id="td_sd_profiles",
            tab_title="Restoring Force",
            description="Rendering conditional future change against current deviation from the fitted anchor.",
            how_to_read=(
                "This is the direct restoring-force view.",
                "Positive deviations should map to negative future changes and negative deviations should map to positive future changes.",
                "Observed and simulated curves should both slope downward around zero if the anchor pull is modeled correctly.",
            ),
            builder=create_td_sd_mean_reversion_future_change_figure,
            uses_observed_simulation_comparison=True,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="td_sd_autocorrelation",
            task_id="plot_td_sd_autocorrelation",
            family_id="td_sd_returns",
            tab_title="Autocorrelation",
            description="Rendering 50-lag squared-return autocorrelograms for observed 15-minute, simulated tick-aggregated 15-minute, and simulated coarse 15-minute returns.",
            how_to_read=(
                "These panels compare volatility clustering through squared-return autocorrelation.",
                "Higher and slower-decaying autocorrelation indicates stronger clustering.",
                "Compare observed, simulated tick-aggregated, and simulated coarse panels to see whether clustering is preserved consistently across representations.",
            ),
            builder=create_td_sd_autocorrelation_figure,
            uses_observed_simulation_comparison=True,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="td_sd_summary_statistics",
            task_id="plot_td_sd_summary_statistics",
            family_id="td_sd_profiles",
            tab_title="Summary Statistics",
            description="Rendering summary statistics across observed ticks, observed 15-minute returns, simulated ticks, simulated tick-aggregated 15-minute returns, and simulated coarse 15-minute returns.",
            how_to_read=(
                "This is a compact numerical summary across the main observed and simulated return views.",
                "Use it as a quick scan for obvious scale, dispersion, or tail mismatches before digging into the richer plots.",
            ),
            builder=create_td_sd_summary_statistics_figure,
            uses_observed_simulation_comparison=True,
        ),
        WorkbenchPlotManifestEntry(
            plot_id="candidate_compare",
            task_id="plot_candidate_compare",
            family_id="candidate_compare",
            tab_title="Compare Plot",
            description="Rendering selected-versus-frontier candidate diagnostics and parameter deltas.",
            how_to_read=(
                "This compares the selected candidate against a nearby frontier alternative.",
                "The objective panel shows which candidate scores better.",
                "The coordinate and family-distance panels show where the candidates differ in parameters and target fit.",
                "Large family-distance deltas reveal which target families drove the selection decision.",
            ),
            builder=create_candidate_compare_figure,
        ),
    )


__all__ = [
    "WorkbenchPlotManifestEntry",
    "workbench_plot_manifest",
]
