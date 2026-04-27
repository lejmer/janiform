use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::mpsc::Sender;

use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformEquationSpecification;
use janiform_affine_oracle::sufficient_statistics::{
    SufficientStatisticsIntervalFactors, SufficientStatisticsModelInputs,
};
use janiform_engine_simulation::{
    CoarseSimulationModelInputs, CoarseSimulationRunRequest, run_coarse_simulation,
};
use janiform_foundations::time::{FIVE_MINUTES_NANOS, ONE_HOUR_NANOS};
use janiform_market_time::hourly_factors::{
    HourlyFactorContractOutput, HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource,
    HourlySeasonalityFactorSource,
};
use janiform_model_kernels::hawkes::HawkesExcitationState;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde::{Deserialize, Serialize};

use crate::adapters::load_hourly_factor_contract_for_calibration;
use crate::stages::joint_metrics::{
    JointRefinementAggregatedIntervalObservation, JointRefinementReturnScaleSummary,
    finalize_sell_share_proxies, return_scale_summary, zero_aware_normalized_bucket_value,
    zeroed_interval_observation_template,
};
use crate::stages::joint_refinement::contracts::JointRefinementStageCoarseTickConsistencyMetric;

use super::super::contracts::{
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};
use super::super::mean_reversion_runtime::{
    endogenous_interval_factors_from_hourly_contract,
    materialize_joint_refinement_mean_reversion_runtime_inputs,
};
use super::super::objective::root_mean_square;
use super::super::setup::{
    build_surprise_multipliers_for_interval_grid, cached_seed_summaries_for_candidate_seeds,
};
use super::{
    JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR, JointRefinementCoarseConsistencySummary,
    JointRefinementContext, JointRefinementSeedSummary, build_exchange_calendar_registry_artifact,
    joint_refinement_has_parallel_worker_capacity, with_joint_refinement_thread_pool,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JointRefinementCoarseConsistencyHorizon {
    FiveMinute,
    FifteenMinute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JointRefinementCoarseConsistencyTask {
    seed_position: usize,
    simulation_seed: u64,
    horizon: JointRefinementCoarseConsistencyHorizon,
}

#[derive(Debug)]
struct JointRefinementCoarseConsistencyTaskOutput {
    seed_position: usize,
    simulation_seed: u64,
    horizon: JointRefinementCoarseConsistencyHorizon,
    intervals: Option<Vec<JointRefinementAggregatedIntervalObservation>>,
}

#[derive(Clone, Copy)]
pub(crate) struct JointRefinementCoarseConsistencyProgress<'a> {
    pub(crate) sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    pub(crate) phase: JointRefinementStageProgressPhase,
    pub(crate) current_item_label: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct JointRefinementCoarseConsistencyArtifacts {
    pub(crate) summary: JointRefinementCoarseConsistencySummary,
    pub(crate) metrics: Vec<JointRefinementStageCoarseTickConsistencyMetric>,
}

pub(crate) fn build_joint_coarse_tick_consistency_metrics(
    context: &JointRefinementContext,
    fitted_parameters: &janiform_calibration_artifacts::JointRefinementStageFittedParameters,
    tick_seed_summaries: &[(u64, JointRefinementSeedSummary)],
    progress: Option<JointRefinementCoarseConsistencyProgress<'_>>,
) -> Vec<JointRefinementStageCoarseTickConsistencyMetric> {
    let task_count = coarse_consistency_task_count(tick_seed_summaries);

    if joint_refinement_has_parallel_worker_capacity(context) && !tick_seed_summaries.is_empty() {
        return build_joint_coarse_tick_consistency_metrics_parallel(
            context,
            fitted_parameters,
            tick_seed_summaries,
            progress,
            task_count,
        );
    }

    let mut metrics = Vec::new();
    let mut completed_tasks = 0;

    for (simulation_seed, tick_summary) in tick_seed_summaries {
        let coarse_5m_intervals = build_joint_coarse_interval_observations(
            context,
            fitted_parameters,
            *simulation_seed,
            FIVE_MINUTES_NANOS,
            context
                .observed_data
                .simulated_5m_interval_template
                .as_slice(),
        );
        completed_tasks += 1;

        emit_coarse_consistency_metric_progress(
            progress,
            completed_tasks,
            task_count,
            *simulation_seed,
            JointRefinementCoarseConsistencyHorizon::FiveMinute,
        );

        let coarse_15m_intervals = build_joint_coarse_interval_observations(
            context,
            fitted_parameters,
            *simulation_seed,
            3 * FIVE_MINUTES_NANOS,
            context
                .observed_data
                .simulated_15m_interval_template
                .as_slice(),
        );
        completed_tasks += 1;

        emit_coarse_consistency_metric_progress(
            progress,
            completed_tasks,
            task_count,
            *simulation_seed,
            JointRefinementCoarseConsistencyHorizon::FifteenMinute,
        );

        extend_joint_coarse_tick_consistency_metrics_for_seed(
            &mut metrics,
            *simulation_seed,
            coarse_5m_intervals.as_deref(),
            coarse_15m_intervals.as_deref(),
            tick_summary,
        );
    }

    metrics
}

fn build_joint_coarse_tick_consistency_metrics_parallel(
    context: &JointRefinementContext,
    fitted_parameters: &janiform_calibration_artifacts::JointRefinementStageFittedParameters,
    tick_seed_summaries: &[(u64, JointRefinementSeedSummary)],
    progress: Option<JointRefinementCoarseConsistencyProgress<'_>>,
    task_count: usize,
) -> Vec<JointRefinementStageCoarseTickConsistencyMetric> {
    let tasks = tick_seed_summaries
        .iter()
        .enumerate()
        .flat_map(|(seed_position, (simulation_seed, _))| {
            [
                JointRefinementCoarseConsistencyTask {
                    seed_position,
                    simulation_seed: *simulation_seed,
                    horizon: JointRefinementCoarseConsistencyHorizon::FiveMinute,
                },
                JointRefinementCoarseConsistencyTask {
                    seed_position,
                    simulation_seed: *simulation_seed,
                    horizon: JointRefinementCoarseConsistencyHorizon::FifteenMinute,
                },
            ]
        })
        .collect::<Vec<_>>();
    let completed_tasks = Mutex::new(0usize);

    let task_outputs = with_joint_refinement_thread_pool(context, || {
        tasks
            .into_par_iter()
            .map(|task| JointRefinementCoarseConsistencyTaskOutput {
                seed_position: task.seed_position,
                simulation_seed: task.simulation_seed,
                horizon: task.horizon,
                intervals: build_joint_coarse_interval_observations(
                    context,
                    fitted_parameters,
                    task.simulation_seed,
                    task.horizon.interval_duration_nanos(),
                    task.horizon.template(context),
                ),
            })
            .inspect(|task_output| {
                if let Ok(mut completed_tasks_guard) = completed_tasks.lock() {
                    *completed_tasks_guard = (*completed_tasks_guard).saturating_add(1);

                    emit_coarse_consistency_metric_progress(
                        progress,
                        *completed_tasks_guard,
                        task_count,
                        task_output.simulation_seed(),
                        task_output.horizon,
                    );
                }
            })
            .collect::<Vec<_>>()
    });

    let mut intervals_by_seed: Vec<(
        Option<Vec<JointRefinementAggregatedIntervalObservation>>,
        Option<Vec<JointRefinementAggregatedIntervalObservation>>,
    )> = (0..tick_seed_summaries.len())
        .map(|_| (None, None))
        .collect();

    for task_output in task_outputs {
        let Some((coarse_5m_intervals, coarse_15m_intervals)) =
            intervals_by_seed.get_mut(task_output.seed_position)
        else {
            continue;
        };

        match task_output.horizon {
            JointRefinementCoarseConsistencyHorizon::FiveMinute => {
                *coarse_5m_intervals = task_output.intervals;
            }
            JointRefinementCoarseConsistencyHorizon::FifteenMinute => {
                *coarse_15m_intervals = task_output.intervals;
            }
        }
    }

    let mut metrics = Vec::new();

    for ((simulation_seed, tick_summary), (coarse_5m_intervals, coarse_15m_intervals)) in
        tick_seed_summaries.iter().zip(intervals_by_seed.iter())
    {
        extend_joint_coarse_tick_consistency_metrics_for_seed(
            &mut metrics,
            *simulation_seed,
            coarse_5m_intervals.as_deref(),
            coarse_15m_intervals.as_deref(),
            tick_summary,
        );
    }

    metrics
}

fn coarse_consistency_task_count(
    tick_seed_summaries: &[(u64, JointRefinementSeedSummary)],
) -> usize {
    tick_seed_summaries.len().saturating_mul(2)
}

fn emit_coarse_consistency_metric_progress(
    progress: Option<JointRefinementCoarseConsistencyProgress<'_>>,
    completed_units: usize,
    total_units: usize,
    simulation_seed: u64,
    horizon: JointRefinementCoarseConsistencyHorizon,
) {
    let Some(progress) = progress else {
        return;
    };

    let Some(progress_sender) = progress.sender else {
        return;
    };

    if total_units == 0 {
        return;
    }

    let current_item_label = match progress.current_item_label {
        Some(label) => format!("{label}, seed {simulation_seed}, {}", horizon.label()),
        None => format!("seed {simulation_seed}, {}", horizon.label()),
    };

    let _ = progress_sender.send(JointRefinementStageProgressUpdate {
        current_item_label: Some(current_item_label),
        current_evaluation_count: Some(completed_units),
        total_evaluation_budget: Some(total_units),
        ..JointRefinementStageProgressUpdate::new(progress.phase, completed_units, total_units)
    });
}

impl JointRefinementCoarseConsistencyTaskOutput {
    const fn simulation_seed(&self) -> u64 {
        self.simulation_seed
    }
}

impl JointRefinementCoarseConsistencyHorizon {
    const fn interval_duration_nanos(self) -> i64 {
        match self {
            Self::FiveMinute => FIVE_MINUTES_NANOS,
            Self::FifteenMinute => 3 * FIVE_MINUTES_NANOS,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::FiveMinute => "5m",
            Self::FifteenMinute => "15m",
        }
    }

    fn template(
        self,
        context: &JointRefinementContext,
    ) -> &[JointRefinementAggregatedIntervalObservation] {
        match self {
            Self::FiveMinute => context
                .observed_data
                .simulated_5m_interval_template
                .as_slice(),
            Self::FifteenMinute => context
                .observed_data
                .simulated_15m_interval_template
                .as_slice(),
        }
    }
}

pub(crate) fn extend_joint_coarse_tick_consistency_metrics_for_seed(
    metrics: &mut Vec<JointRefinementStageCoarseTickConsistencyMetric>,
    simulation_seed: u64,
    coarse_5m_intervals: Option<&[JointRefinementAggregatedIntervalObservation]>,
    coarse_15m_intervals: Option<&[JointRefinementAggregatedIntervalObservation]>,
    tick_summary: &JointRefinementSeedSummary,
) {
    if let Some(coarse_5m_intervals) = coarse_5m_intervals {
        metrics.extend(joint_coarse_tick_metrics_for_horizon(
            simulation_seed,
            "5m",
            sum_interval_tick_count(coarse_5m_intervals) as f64,
            return_scale_summary(coarse_5m_intervals),
            sum_interval_tick_count(tick_summary.simulated_5m_intervals.as_slice()) as f64,
            tick_summary.simulated_5m_return_scale_summary,
        ));
    }

    if let Some(coarse_15m_intervals) = coarse_15m_intervals {
        metrics.extend(joint_coarse_tick_metrics_for_horizon(
            simulation_seed,
            "15m",
            sum_interval_tick_count(coarse_15m_intervals) as f64,
            return_scale_summary(coarse_15m_intervals),
            sum_interval_tick_count(tick_summary.simulated_15m_intervals.as_slice()) as f64,
            tick_summary.simulated_15m_return_scale_summary,
        ));
    }
}

pub(crate) fn build_joint_candidate_coarse_consistency_artifacts(
    context: &JointRefinementContext,
    fitted_parameters: &janiform_calibration_artifacts::JointRefinementStageFittedParameters,
    parameter_bits: Option<&[u64]>,
    baseline_raw_fit: Option<f64>,
    progress: Option<JointRefinementCoarseConsistencyProgress<'_>>,
) -> Option<JointRefinementCoarseConsistencyArtifacts> {
    let finalist_seed_summaries = cached_seed_summaries_for_candidate_seeds(
        parameter_bits,
        fitted_parameters,
        context,
        context.final_simulation_seeds.as_slice(),
        None,
        None,
        0,
        context.final_simulation_seeds.len(),
    )
    .ok()?;

    let coarse_tick_consistency = build_joint_coarse_tick_consistency_metrics(
        context,
        fitted_parameters,
        finalist_seed_summaries.as_slice(),
        progress,
    );

    let raw_fit = joint_coarse_tick_consistency_fit(coarse_tick_consistency.as_slice())?;

    Some(JointRefinementCoarseConsistencyArtifacts {
        summary: JointRefinementCoarseConsistencySummary {
            raw_fit,
            normalized_fit: zero_aware_normalized_bucket_value(
                raw_fit,
                baseline_raw_fit.unwrap_or(raw_fit),
                JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR,
            ),
            admissibility_threshold: super::selection::admissibility_threshold_from_sample_count(
                coarse_tick_consistency.len(),
            ),
        },
        metrics: coarse_tick_consistency,
    })
}

fn joint_coarse_tick_consistency_fit(
    coarse_tick_consistency: &[JointRefinementStageCoarseTickConsistencyMetric],
) -> Option<f64> {
    if coarse_tick_consistency.is_empty() {
        return None;
    }

    Some(root_mean_square(
        coarse_tick_consistency
            .iter()
            .map(|metric| metric.normalized_delta)
            .collect::<Vec<_>>()
            .as_slice(),
    ))
}

fn joint_coarse_tick_metrics_for_horizon(
    simulation_seed: u64,
    horizon_label: &str,
    coarse_event_count: f64,
    coarse_summary: JointRefinementReturnScaleSummary,
    tick_event_count: f64,
    tick_summary: JointRefinementReturnScaleSummary,
) -> Vec<JointRefinementStageCoarseTickConsistencyMetric> {
    [
        ("event_count", coarse_event_count, tick_event_count),
        (
            "mean_absolute_return",
            coarse_summary.mean_absolute_return,
            tick_summary.mean_absolute_return,
        ),
        (
            "realized_variance",
            coarse_summary.realized_variance,
            tick_summary.realized_variance,
        ),
        (
            "absolute_return_quantile_q90",
            coarse_summary.absolute_return_quantile_q90,
            tick_summary.absolute_return_quantile_q90,
        ),
    ]
    .into_iter()
    .map(|(metric_id, coarse_value, tick_value)| {
        let absolute_delta = (coarse_value - tick_value).abs();
        let normalized_delta = absolute_delta / tick_value.abs().max(1.0e-8);
        JointRefinementStageCoarseTickConsistencyMetric {
            simulation_seed,
            horizon_label: horizon_label.to_string(),
            metric_id: metric_id.to_string(),
            coarse_value,
            tick_value,
            absolute_delta,
            normalized_delta,
        }
    })
    .collect()
}

fn build_joint_coarse_interval_observations(
    context: &JointRefinementContext,
    fitted_parameters: &janiform_calibration_artifacts::JointRefinementStageFittedParameters,
    simulation_seed: u64,
    interval_duration_nanos: i64,
    template: &[JointRefinementAggregatedIntervalObservation],
) -> Option<Vec<JointRefinementAggregatedIntervalObservation>> {
    if template.is_empty() {
        return None;
    }

    let interval_count = ((context
        .observed_data
        .hourly_simulation_end_utc
        .timestamp_nanos_opt()?
        - context
            .observed_data
            .hourly_simulation_start_utc
            .timestamp_nanos_opt()?)
    .div_euclid(interval_duration_nanos)) as usize;

    let surprise_deterministic_time_multiplier_by_interval_index =
        build_surprise_multipliers_for_interval_grid(
            context.observed_data.hourly_simulation_start_utc,
            interval_count,
            interval_duration_nanos,
            context
                .observed_data
                .surprise_weekly_hour_utc_multipliers
                .as_slice(),
        );

    let hourly_factor_contract = load_hourly_factor_contract_for_calibration(
        &context.calendar_registry,
        HourlyFactorProviderRequest {
            exchange_code: context.exchange_code.as_str(),
            range_start_utc: context.observed_data.hourly_simulation_start_utc,
            range_end_utc: context.observed_data.hourly_simulation_end_utc,
            interval_duration_nanos,
            seasonality_source: Some(HourlySeasonalityFactorSource::Explicit {
                hourly_factors: context.runtime_hourly_seasonality_factors.as_slice(),
                materialization_method:
                    "runtime_compact_seasonality_parameters_mean_renormalized.v1",
            }),
            news_intensity_source: Some(HourlyNewsIntensityFactorSource::Materialized {
                known_scheduled_event_timestamps_utc_nanos: fitted_parameters
                    .return_components_fitted_parameters
                    .news_process_configuration
                    .arrivals
                    .known
                    .scheduled_event_timestamps_utc_nanos
                    .as_slice(),
                surprise_baseline_intensity: fitted_parameters
                    .return_components_fitted_parameters
                    .news_process_configuration
                    .arrivals
                    .surprise
                    .baseline_intensity,
                surprise_deterministic_time_multiplier_by_interval_index:
                    surprise_deterministic_time_multiplier_by_interval_index.as_slice(),
            }),
        },
    )
    .ok()?;

    let interval_delta = interval_duration_nanos as f64 / ONE_HOUR_NANOS as f64;

    let interval_factors = build_coarse_interval_factors_from_hourly_contract(
        &hourly_factor_contract,
        fitted_parameters
            .return_components_fitted_parameters
            .news_process_configuration
            .arrivals
            .known
            .scheduled_event_timestamps_utc_nanos
            .as_slice(),
        interval_delta,
    )?;

    let model_inputs =
        build_joint_coarse_model_inputs(context, fitted_parameters, &hourly_factor_contract)?;

    let coarse_path = run_coarse_simulation(
        CoarseSimulationRunRequest::try_new(
            simulation_seed,
            hourly_factor_contract.request_start_utc_nanos,
            hourly_factor_contract.interval_indices.len(),
            interval_delta,
            model_inputs,
            interval_factors,
        )
        .ok()?,
    )
    .ok()?;

    let mut intervals = zeroed_interval_observation_template(template);

    let interval_index_by_bounds: BTreeMap<(i64, i64), usize> = intervals
        .iter()
        .enumerate()
        .map(|(index, interval)| {
            (
                (
                    interval.interval_start_utc_nanos,
                    interval.interval_end_utc_nanos,
                ),
                index,
            )
        })
        .collect();

    for record in coarse_path.records {
        if let Some(interval_index) = interval_index_by_bounds.get(&(
            record.interval_start_utc_nanos,
            record.interval_end_utc_nanos,
        )) {
            let Some(interval) = intervals.get_mut(*interval_index) else {
                continue;
            };

            interval.log_return += record.return_components.total_return;
            interval.tick_count += (record.sufficient_statistics.n_plus
                + record.sufficient_statistics.n_minus) as usize;
            interval.sell_share_proxy += record.sufficient_statistics.n_minus as f64;
        }
    }

    finalize_sell_share_proxies(intervals.as_mut_slice(), 0.5);

    Some(intervals)
}

fn sum_interval_tick_count(intervals: &[JointRefinementAggregatedIntervalObservation]) -> usize {
    intervals.iter().map(|interval| interval.tick_count).sum()
}

fn build_coarse_interval_factors_from_hourly_contract(
    hourly_factor_contract: &HourlyFactorContractOutput,
    known_news_timestamps_utc_nanos: &[i64],
    interval_delta: f64,
) -> Option<SufficientStatisticsIntervalFactors> {
    let nu_h_known = hourly_factor_contract
        .interval_start_utc_nanos
        .iter()
        .zip(hourly_factor_contract.interval_end_utc_nanos.iter())
        .map(|(interval_start_utc_nanos, interval_end_utc_nanos)| {
            count_known_news_events(
                known_news_timestamps_utc_nanos,
                *interval_start_utc_nanos,
                *interval_end_utc_nanos,
            ) as f64
                / interval_delta
        })
        .collect::<Vec<_>>();

    let nu_h_surprise = hourly_factor_contract
        .nu_h
        .iter()
        .zip(nu_h_known.iter())
        .map(|(combined_intensity, known_intensity)| {
            (combined_intensity - known_intensity).max(0.0)
        })
        .collect::<Vec<_>>();

    SufficientStatisticsIntervalFactors::try_new(
        hourly_factor_contract.s_h.clone(),
        nu_h_known,
        nu_h_surprise,
        Some(interval_delta),
    )
    .ok()
}

fn count_known_news_events(
    known_news_timestamps_utc_nanos: &[i64],
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
) -> usize {
    known_news_timestamps_utc_nanos
        .iter()
        .filter(|timestamp_utc_nanos| {
            **timestamp_utc_nanos >= interval_start_utc_nanos
                && **timestamp_utc_nanos < interval_end_utc_nanos
        })
        .count()
}

fn build_joint_coarse_model_inputs(
    context: &JointRefinementContext,
    fitted_parameters: &janiform_calibration_artifacts::JointRefinementStageFittedParameters,
    hourly_factor_contract: &HourlyFactorContractOutput,
) -> Option<CoarseSimulationModelInputs> {
    let initial_excitation_state = HawkesExcitationState {
        y_plus_plus: 0.0,
        y_plus_minus: 0.0,
        y_minus_plus: 0.0,
        y_minus_minus: 0.0,
    };

    let endogenous_interval_factors =
        endogenous_interval_factors_from_hourly_contract(hourly_factor_contract).ok()?;

    let (mean_reversion_runtime_interval_anchors, initial_mean_reversion_boundary_state) =
        materialize_joint_refinement_mean_reversion_runtime_inputs(
            hourly_factor_contract.metadata.interval_alignment_policy,
            fitted_parameters,
            initial_excitation_state,
            endogenous_interval_factors.as_slice(),
        )
        .ok()?;

    let sufficient_statistics_model_inputs = SufficientStatisticsModelInputs::try_new(
        ExponentialAffineTransformEquationSpecification::try_new(
            fitted_parameters.hawkes_fitted_parameters.kernel_parameters,
            fitted_parameters
                .return_components_fitted_parameters
                .news_process_configuration
                .shared_mark_law
                .gamma_severity_marks,
        )
        .ok()?,
        fitted_parameters
            .return_components_fitted_parameters
            .nig_shared_plus_shift_configuration,
        initial_excitation_state,
        fitted_parameters
            .return_components_fitted_parameters
            .mean_reversion_fitted_parameters
            .clone(),
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    )
    .ok()?;

    CoarseSimulationModelInputs::try_new(
        sufficient_statistics_model_inputs,
        fitted_parameters
            .return_components_fitted_parameters
            .news_process_configuration
            .clone(),
        build_exchange_calendar_registry_artifact(
            &context.calendar_registry,
            context.exchange_code.as_str(),
            context.observed_data.hourly_simulation_start_utc,
        )?,
        context.exchange_code.clone(),
    )
    .ok()
}
