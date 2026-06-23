mod aggregation;
mod dynamic_clock;
mod scoring;

use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::SurpriseNewsDeterministicIntensityProfile;
use janiform_model_kernels::long_variance::LongVarianceParameters;
use janiform_model_kernels::news::NewsProcessConfiguration;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use janiform_model_kernels::return_clock::ReturnClockExposureParameters;

use super::progress::{
    RETURN_BACKFIT_STAGE_PROGRESS_UNITS, ReturnBackfitStageProgressPhase,
    ReturnBackfitStageProgressUpdate, emit_return_backfit_progress,
    emit_return_backfit_subphase_progress,
};
use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::return_components::{
    ReturnComponentsObservedOpenSessionReturn, ReturnComponentsStageCandidateGenerationInput,
    prepare_continuous_return_calibration_inputs,
    resolve_activity_calibration_context_from_hawkes_payload,
};
use aggregation::select_quantile_shape_aggregation_sample;
use dynamic_clock::{DynamicClockReplayInput, replay_dynamic_clock_for_continuous_returns};
use scoring::{
    QuantileShapeEvaluation, data_derived_delta_multipliers,
    evaluate_quantile_shape_delta_candidate, max_delta_multiplier_candidate_count,
};

pub(super) const RETURN_BACKFIT_QUANTILE_LEVELS: [f64; 6] = [0.01, 0.05, 0.25, 0.75, 0.95, 0.99];
pub(super) const RETURN_BACKFIT_QUANTILE_STABILITY_ORDER_MULTIPLIER: usize = 10;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ReturnBackfitDeltaQuantileShapeFit {
    pub(super) long_variance_clock_scale_multiplier: f64,
    pub(super) delta_backfit_multiplier: f64,
    pub(super) selected_nig_delta: f64,
    pub(super) post_backfit_nig_delta: f64,
    pub(super) selected_aggregation_horizon: String,
    pub(super) selected_aggregation_interval_nanos: Option<i64>,
    pub(super) selected_aggregation_sample_count: usize,
    pub(super) minimum_aggregation_sample_count: usize,
    pub(super) quantile_count: usize,
    pub(super) quantile_stability_order_multiplier: usize,
    pub(super) quantile_levels: Vec<f64>,
    pub(super) observed_quantiles: Vec<f64>,
    pub(super) before_fitted_quantiles: Vec<f64>,
    pub(super) after_fitted_quantiles: Vec<f64>,
    pub(super) before_quantile_shape_score: f64,
    pub(super) after_quantile_shape_score: f64,
    pub(super) candidate_delta_multiplier_count: usize,
    pub(super) post_backfit_nig_configuration: NigSharedPlusShiftConfiguration,
    pub(super) warnings: Vec<String>,
}

pub(super) struct ReturnBackfitDeltaQuantileShapeInput<'a> {
    pub(super) hawkes_stage_payload: &'a HawkesStageFittedPayload,
    pub(super) candidate_generation_input: &'a ReturnComponentsStageCandidateGenerationInput,
    pub(super) nig_configuration: NigSharedPlusShiftConfiguration,
    pub(super) return_clock_exposure_parameters: ReturnClockExposureParameters,
    pub(super) long_variance_parameters: LongVarianceParameters,
    pub(super) news_process_configuration: &'a NewsProcessConfiguration,
    pub(super) surprise_deterministic_intensity_profile:
        &'a SurpriseNewsDeterministicIntensityProfile,
    pub(super) stage_seed: u64,
    pub(super) progress_sender: Option<Sender<ReturnBackfitStageProgressUpdate>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct DynamicContinuousReturnObservation {
    pub(super) observation: ReturnComponentsObservedOpenSessionReturn,
    pub(super) original_business_time_scale: f64,
    pub(super) dynamic_business_time_scale: f64,
}

struct DeltaQuantileShapeSelection {
    baseline_evaluation: QuantileShapeEvaluation,
    best_evaluation: QuantileShapeEvaluation,
    candidate_delta_multiplier_count: usize,
    warnings: Vec<String>,
}

pub(super) fn return_backfit_delta_quantile_shape_backfit(
    input: ReturnBackfitDeltaQuantileShapeInput<'_>,
) -> Result<ReturnBackfitDeltaQuantileShapeFit, String> {
    let ReturnBackfitDeltaQuantileShapeInput {
        hawkes_stage_payload,
        candidate_generation_input,
        nig_configuration,
        return_clock_exposure_parameters,
        long_variance_parameters,
        news_process_configuration,
        surprise_deterministic_intensity_profile,
        stage_seed,
        progress_sender,
    } = input;

    validate_nig_configuration_for_backfit(nig_configuration)?;

    emit_return_backfit_progress(
        progress_sender.as_ref(),
        ReturnBackfitStageProgressPhase::ContinuousSamplePreparation,
        1,
        RETURN_BACKFIT_STAGE_PROGRESS_UNITS,
        Some("prepare continuous return sample"),
    );

    let activity_calibration_context =
        resolve_activity_calibration_context_from_hawkes_payload(hawkes_stage_payload);

    let continuous_preparation = prepare_continuous_return_calibration_inputs(
        candidate_generation_input,
        activity_calibration_context,
    )
    .map_err(|source| format!("return-backfit continuous sample preparation failed: {source}"))?;

    emit_return_backfit_progress(
        progress_sender.as_ref(),
        ReturnBackfitStageProgressPhase::DynamicClockReplay,
        2,
        RETURN_BACKFIT_STAGE_PROGRESS_UNITS,
        Some("replay dynamic return clock"),
    );

    let dynamic_clock_replay =
        replay_dynamic_clock_for_continuous_returns(DynamicClockReplayInput {
            hawkes_stage_payload,
            candidate_generation_input,
            news_process_configuration,
            surprise_deterministic_intensity_profile,
            nig_configuration,
            return_clock_exposure_parameters,
            long_variance_parameters,
            sorted_signals: &continuous_preparation.sorted_signals,
            progress_sender: progress_sender.clone(),
        })?;

    let minimum_aggregation_sample_count =
        quantile_shape_minimum_sample_count(RETURN_BACKFIT_QUANTILE_LEVELS.len());

    let mut warnings = Vec::new();
    let aggregation_sample = select_quantile_shape_aggregation_sample(
        dynamic_clock_replay.continuous_returns.as_slice(),
        minimum_aggregation_sample_count,
        &mut warnings,
    )?;

    let selection = select_delta_quantile_shape_candidate(
        nig_configuration,
        &aggregation_sample,
        dynamic_clock_replay.long_variance_clock_scale_multiplier,
        stage_seed,
        progress_sender.as_ref(),
    )?;

    warnings.extend(selection.warnings);

    Ok(ReturnBackfitDeltaQuantileShapeFit {
        long_variance_clock_scale_multiplier: dynamic_clock_replay
            .long_variance_clock_scale_multiplier,
        delta_backfit_multiplier: selection.best_evaluation.delta_multiplier,
        selected_nig_delta: nig_configuration.delta,
        post_backfit_nig_delta: selection.best_evaluation.nig_configuration.delta,
        selected_aggregation_horizon: aggregation_sample.horizon.label(),
        selected_aggregation_interval_nanos: aggregation_sample.horizon.interval_nanos,
        selected_aggregation_sample_count: aggregation_sample.sample_count(),
        minimum_aggregation_sample_count,
        quantile_count: RETURN_BACKFIT_QUANTILE_LEVELS.len(),
        quantile_stability_order_multiplier: RETURN_BACKFIT_QUANTILE_STABILITY_ORDER_MULTIPLIER,
        quantile_levels: RETURN_BACKFIT_QUANTILE_LEVELS.to_vec(),
        observed_quantiles: selection.baseline_evaluation.observed_quantiles,
        before_fitted_quantiles: selection.baseline_evaluation.fitted_quantiles,
        after_fitted_quantiles: selection.best_evaluation.fitted_quantiles,
        before_quantile_shape_score: selection.baseline_evaluation.score,
        after_quantile_shape_score: selection.best_evaluation.score,
        candidate_delta_multiplier_count: selection.candidate_delta_multiplier_count,
        post_backfit_nig_configuration: selection.best_evaluation.nig_configuration,
        warnings,
    })
}

fn select_delta_quantile_shape_candidate(
    nig_configuration: NigSharedPlusShiftConfiguration,
    aggregation_sample: &aggregation::AggregatedQuantileCalibrationSample,
    long_variance_clock_scale_multiplier: f64,
    stage_seed: u64,
    progress_sender: Option<&Sender<ReturnBackfitStageProgressUpdate>>,
) -> Result<DeltaQuantileShapeSelection, String> {
    let baseline_evaluation = evaluate_quantile_shape_delta_candidate(
        nig_configuration,
        1.0,
        aggregation_sample,
        stage_seed,
    )?;

    let candidate_delta_multipliers =
        data_derived_delta_multipliers(&baseline_evaluation, long_variance_clock_scale_multiplier)?;

    let mut best_evaluation = baseline_evaluation.clone();
    let mut failed_candidate_count = 0usize;
    let total_progress_units =
        max_delta_multiplier_candidate_count(RETURN_BACKFIT_QUANTILE_LEVELS.len());

    for (candidate_position, delta_multiplier) in candidate_delta_multipliers.iter().enumerate() {
        let completed_candidate_count = candidate_position.saturating_add(1);

        emit_delta_quantile_shape_progress(
            progress_sender,
            completed_candidate_count,
            total_progress_units,
            Some("evaluate delta quantile candidate"),
        );

        let candidate_evaluation = evaluate_quantile_shape_delta_candidate(
            nig_configuration,
            *delta_multiplier,
            aggregation_sample,
            stage_seed,
        );

        let Ok(candidate_evaluation) = candidate_evaluation else {
            failed_candidate_count = failed_candidate_count.saturating_add(1);
            continue;
        };

        if candidate_evaluation
            .score
            .total_cmp(&best_evaluation.score)
            .is_lt()
        {
            best_evaluation = candidate_evaluation;
        }
    }

    let warnings = if failed_candidate_count > 0 {
        vec![format!(
            "return-backfit skipped {failed_candidate_count} invalid delta quantile-shape candidates"
        )]
    } else {
        Vec::new()
    };

    Ok(DeltaQuantileShapeSelection {
        baseline_evaluation,
        best_evaluation,
        candidate_delta_multiplier_count: candidate_delta_multipliers.len(),
        warnings,
    })
}

fn validate_nig_configuration_for_backfit(
    nig_configuration: NigSharedPlusShiftConfiguration,
) -> Result<(), String> {
    nig_configuration
        .derive_driver_parameters()
        .map(|_| ())
        .map_err(|source| format!("selected NIG configuration is invalid: {source}"))
}

fn emit_delta_quantile_shape_progress(
    progress_sender: Option<&Sender<ReturnBackfitStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
) {
    emit_return_backfit_subphase_progress(
        progress_sender,
        ReturnBackfitStageProgressPhase::DeltaQuantileShapeBackfit,
        3,
        completed_units,
        total_units,
        current_item_label,
    );
}

fn quantile_shape_minimum_sample_count(quantile_count: usize) -> usize {
    RETURN_BACKFIT_QUANTILE_STABILITY_ORDER_MULTIPLIER
        .saturating_mul(quantile_count)
        .saturating_mul(quantile_count)
}

#[cfg(test)]
mod tests {
    use super::quantile_shape_minimum_sample_count;

    #[test]
    fn quantile_shape_minimum_sample_count_uses_order_magnitude_square_policy() {
        assert_eq!(quantile_shape_minimum_sample_count(6), 360);
    }
}
