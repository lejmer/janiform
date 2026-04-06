use std::collections::BTreeMap;

use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformEquationSpecification;
use janiform_affine_oracle::sufficient_statistics::{
    SufficientStatisticsRiskIntervalFactors, SufficientStatisticsRiskModelInputs,
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
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryState, MeanReversionFittedParameters, MeanReversionRuntimeIntervalAnchors,
};

use crate::adapters::load_hourly_factor_contract_for_calibration;
use crate::stages::joint_metrics::{
    JointRefinementAggregatedIntervalObservation, JointRefinementReturnScaleSummary,
    finalize_sell_share_proxies, return_scale_summary, zero_aware_normalized_bucket_value,
    zeroed_interval_observation_template,
};
use crate::stages::joint_refinement::contracts::JointRefinementStageCoarseTickConsistencyMetric;

use super::super::objective::root_mean_square;
use super::super::setup::{
    build_surprise_multipliers_for_interval_grid, cached_seed_summaries_for_candidate_seeds,
};
use super::{
    JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR, JointRefinementCoarseConsistencySummary,
    JointRefinementContext, JointRefinementSeedSummary, build_exchange_calendar_registry_artifact,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JointRefinementCoarseConsistencyArtifacts {
    pub(crate) summary: JointRefinementCoarseConsistencySummary,
    pub(crate) metrics: Vec<JointRefinementStageCoarseTickConsistencyMetric>,
}

pub(crate) fn build_joint_coarse_tick_consistency_metrics(
    context: &JointRefinementContext,
    fitted_parameters: &janiform_calibration_artifacts::JointRefinementStageFittedParameters,
    tick_seed_summaries: &[(u64, JointRefinementSeedSummary)],
) -> Vec<JointRefinementStageCoarseTickConsistencyMetric> {
    let mut metrics = Vec::new();

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
) -> Option<JointRefinementCoarseConsistencyArtifacts> {
    let finalist_seed_summaries = cached_seed_summaries_for_candidate_seeds(
        parameter_bits,
        fitted_parameters,
        context,
        context.final_simulation_seeds.as_slice(),
        None,
        None,
    )
    .ok()?;

    let coarse_tick_consistency = build_joint_coarse_tick_consistency_metrics(
        context,
        fitted_parameters,
        finalist_seed_summaries.as_slice(),
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

    let model_inputs = build_joint_coarse_model_inputs(
        context,
        fitted_parameters,
        hourly_factor_contract.interval_indices.len(),
        interval_delta,
    )?;

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
) -> Option<SufficientStatisticsRiskIntervalFactors> {
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

    SufficientStatisticsRiskIntervalFactors::try_new(
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
    interval_count: usize,
    interval_delta: f64,
) -> Option<CoarseSimulationModelInputs> {
    let risk_model_inputs = SufficientStatisticsRiskModelInputs::try_new(
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
        HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        },
        MeanReversionFittedParameters::zero_kappa_flat_anchor_mean_reversion(),
        MeanReversionRuntimeIntervalAnchors::try_new(
            "explicit_runtime_interval_grid_alignment.v1",
            0.0,
            0.0,
            vec![0.0; interval_count],
            vec![0.0; interval_count],
            vec![interval_delta; interval_count],
        )
        .ok()?,
        MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0, 0.0).ok()?,
    )
    .ok()?;

    CoarseSimulationModelInputs::try_new(
        risk_model_inputs,
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
