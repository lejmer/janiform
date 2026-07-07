use std::sync::mpsc::Sender;

use janiform_foundations::cancellation::CancellationToken;
use janiform_model_kernels::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesKernelLoadings, HawkesKernelValidationStatus,
};
use janiform_work_scheduler::{
    StreamingWorkContext, StreamingWorkOutput, StreamingWorkPoolInput, StreamingWorkProgress,
    run_streaming_work_pool,
};

use super::bit_flags::HawkesBitFlags;
use super::clock::hawkes_adjacent_open_gap;
use super::contracts::{
    HAWKES_EVENT_SIDE_CODE_BUY, HAWKES_EVENT_SIDE_CODE_SELL, HawkesStageCandidateDiagnostic,
    HawkesStageProgressPhase, HawkesStageProgressUpdate,
};
use super::execution::HawkesPreparedCandidate;
use super::likelihood_objective::{
    HawkesSignedLikelihoodDiagnostic, append_signed_likelihood_objective_term,
};
use super::progress::{
    HawkesConvergenceProgress, emit_hawkes_convergence_progress, emit_hawkes_progress,
};
use super::seasonality_input::validate_hawkes_seasonality_event_vectors;
use super::selection::ranked_hawkes_candidate_indices;
use crate::stages::seasonality::SeasonalityStageNormalizedPayload;
use janiform_market_time::calendar::OPEN_MARKET_STATE_CODE;

const PROGRESS_PERCENT_BUCKET_COUNT: usize = 100;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct HawkesLikelihoodTape {
    delta_exposure_hours: Vec<f64>,
    buy_side_flags: HawkesBitFlags,
    reset_flags: HawkesBitFlags,
    event_count: usize,
    baseline_negative_log_likelihood_per_event: f64,
}

pub(super) fn build_hawkes_likelihood_tape(
    seasonality_payload: &SeasonalityStageNormalizedPayload,
    event_side_codes: &[u8],
    progress_sender: Option<&Sender<HawkesStageProgressUpdate>>,
) -> Result<Option<HawkesLikelihoodTape>, String> {
    if event_side_codes.is_empty() {
        emit_hawkes_progress(
            progress_sender,
            HawkesStageProgressPhase::EventTapePreparation,
            1,
            1,
            Some("side tape unavailable"),
        );

        return Ok(None);
    }

    validate_likelihood_tape_inputs(seasonality_payload, event_side_codes)?;

    let source_event_count = seasonality_payload.timestamp_utc_nanos.len();
    let progress_stride = progress_stride(source_event_count);

    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::EventTapePreparation,
        0,
        source_event_count,
        Some("signed event tape"),
    );

    let mut tape_builder = HawkesLikelihoodTapeBuilder::new(source_event_count);

    for (
        event_position,
        (((timestamp_utc_nanos, delta_exposure_hours), market_state_code), event_side_code),
    ) in seasonality_payload
        .timestamp_utc_nanos
        .iter()
        .copied()
        .zip(
            seasonality_payload
                .open_event_business_time_delta_exposure_hours_by_event
                .iter()
                .copied(),
        )
        .zip(seasonality_payload.market_state_codes.iter().copied())
        .zip(event_side_codes.iter().copied())
        .enumerate()
    {
        validate_event_side_code(event_position, event_side_code)?;

        if market_state_code == OPEN_MARKET_STATE_CODE {
            tape_builder.push_open_event(
                timestamp_utc_nanos,
                delta_exposure_hours,
                event_side_code,
            )?;
        }

        if event_position % progress_stride == 0 {
            emit_hawkes_progress(
                progress_sender,
                HawkesStageProgressPhase::EventTapePreparation,
                event_position,
                source_event_count,
                Some("signed event tape"),
            );
        }
    }

    let Some(tape) = tape_builder.finish()? else {
        return Ok(None);
    };

    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::EventTapePreparation,
        source_event_count,
        source_event_count,
        Some("signed event tape"),
    );

    Ok(Some(tape))
}

pub(super) fn score_hawkes_candidates_with_signed_likelihood(
    prepared_candidates: &[HawkesPreparedCandidate],
    candidate_diagnostics: &[HawkesStageCandidateDiagnostic],
    likelihood_tape: &HawkesLikelihoodTape,
    max_scored_candidate_count: usize,
    cpu_count: usize,
    progress_sender: Option<&Sender<HawkesStageProgressUpdate>>,
) -> Result<Vec<HawkesPreparedCandidate>, String> {
    if prepared_candidates.len() != candidate_diagnostics.len() {
        return Err(format!(
            "prepared candidate count {} does not match diagnostic count {}",
            prepared_candidates.len(),
            candidate_diagnostics.len()
        ));
    }

    let candidate_indices = shortlist_candidate_indices(
        prepared_candidates,
        candidate_diagnostics,
        max_scored_candidate_count,
    );
    if candidate_indices.is_empty() {
        return Ok(Vec::new());
    }

    let candidate_labels = candidate_indices
        .iter()
        .map(|candidate_index| {
            prepared_candidates
                .get(*candidate_index)
                .map(|candidate| candidate.adjusted_candidate_input.candidate_id.as_str())
                .unwrap_or("candidate")
        })
        .collect::<Vec<_>>();

    emit_hawkes_progress(
        progress_sender,
        HawkesStageProgressPhase::LikelihoodShortlistEvaluation,
        0,
        candidate_indices.len(),
        Some("signed likelihood"),
    );

    let jobs = candidate_indices.to_vec();
    let mut state = HawkesLikelihoodScoringState {
        candidate_labels,
        completed_candidate_count: 0,
        total_candidate_count: candidate_indices.len(),
        total_event_count: likelihood_tape.event_count,
        best_relative_negative_log_likelihood_per_event: None,
        outputs: empty_likelihood_outputs(candidate_indices.len()),
        progress_sender,
    };

    let summary = run_streaming_work_pool(StreamingWorkPoolInput::new(
        &mut state,
        CancellationToken::uncancelled(),
        jobs,
        cpu_count,
        |candidate_index, context| {
            let Some(prepared_candidate) = prepared_candidates.get(candidate_index) else {
                return HawkesLikelihoodCandidateOutput {
                    candidate_index,
                    diagnostic: None,
                    warning: Some(format!(
                        "candidate index {candidate_index} is outside the prepared candidate set"
                    )),
                };
            };

            evaluate_candidate_signed_likelihood(
                candidate_index,
                prepared_candidate,
                likelihood_tape,
                context,
            )
        },
        record_likelihood_worker_progress,
        record_likelihood_worker_output,
    ))
    .map_err(|error_message: String| error_message)?;

    if summary.completed_work_count != summary.scheduled_work_count {
        return Err(format!(
            "signed Hawkes likelihood completed {} of {} scheduled candidate evaluations",
            summary.completed_work_count, summary.scheduled_work_count
        ));
    }

    build_likelihood_scored_candidates(prepared_candidates, state.outputs)
}

fn validate_likelihood_tape_inputs(
    seasonality_payload: &SeasonalityStageNormalizedPayload,
    event_side_codes: &[u8],
) -> Result<(), String> {
    validate_hawkes_seasonality_event_vectors(seasonality_payload)?;

    let expected_event_count = seasonality_payload.timestamp_utc_nanos.len();

    if event_side_codes.len() != expected_event_count {
        return Err(format!(
            "event_side_codes length {} must match seasonality event count {}",
            event_side_codes.len(),
            expected_event_count
        ));
    }

    Ok(())
}

fn validate_event_side_code(event_position: usize, event_side_code: u8) -> Result<(), String> {
    match event_side_code {
        HAWKES_EVENT_SIDE_CODE_BUY | HAWKES_EVENT_SIDE_CODE_SELL => Ok(()),
        _ => Err(format!(
            "event_side_codes[{event_position}] must be {} for buy or {} for sell, found {}",
            HAWKES_EVENT_SIDE_CODE_BUY, HAWKES_EVENT_SIDE_CODE_SELL, event_side_code
        )),
    }
}

#[derive(Debug, Clone, PartialEq)]
struct HawkesLikelihoodTapeBuilder {
    delta_exposure_hours: Vec<f64>,
    buy_side_flags: HawkesBitFlags,
    reset_flags: HawkesBitFlags,
    event_count: usize,
    buy_event_count: usize,
    sell_event_count: usize,
    total_exposure_hours: f64,
    previous_open_timestamp_utc_nanos: Option<i64>,
}

impl HawkesLikelihoodTapeBuilder {
    fn new(source_event_count: usize) -> Self {
        Self {
            delta_exposure_hours: Vec::with_capacity(source_event_count),
            buy_side_flags: HawkesBitFlags::new(),
            reset_flags: HawkesBitFlags::new(),
            event_count: 0,
            buy_event_count: 0,
            sell_event_count: 0,
            total_exposure_hours: 0.0,
            previous_open_timestamp_utc_nanos: None,
        }
    }

    fn push_open_event(
        &mut self,
        timestamp_utc_nanos: i64,
        delta_exposure_hours: f64,
        event_side_code: u8,
    ) -> Result<(), String> {
        if !delta_exposure_hours.is_finite() || delta_exposure_hours < 0.0 {
            return Err(format!(
                "seasonality open_event_business_time_delta_exposure_hours_by_event produced invalid exposure {delta_exposure_hours}"
            ));
        }

        let (applied_delta_exposure_hours, reset_state) = self
            .previous_open_timestamp_utc_nanos
            .map(|previous_open_timestamp_utc_nanos| {
                match hawkes_adjacent_open_gap(
                    previous_open_timestamp_utc_nanos,
                    timestamp_utc_nanos,
                ) {
                    Some(_) => (delta_exposure_hours, false),
                    None => (0.0, true),
                }
            })
            .unwrap_or((0.0, true));

        self.delta_exposure_hours.push(applied_delta_exposure_hours);

        self.total_exposure_hours += applied_delta_exposure_hours;

        self.buy_side_flags.push(
            self.event_count,
            event_side_code == HAWKES_EVENT_SIDE_CODE_BUY,
        );

        self.reset_flags.push(self.event_count, reset_state);

        if event_side_code == HAWKES_EVENT_SIDE_CODE_BUY {
            self.buy_event_count = self.buy_event_count.saturating_add(1);
        } else {
            self.sell_event_count = self.sell_event_count.saturating_add(1);
        }

        self.event_count = self.event_count.saturating_add(1);
        self.previous_open_timestamp_utc_nanos = Some(timestamp_utc_nanos);

        Ok(())
    }

    fn finish(self) -> Result<Option<HawkesLikelihoodTape>, String> {
        if self.event_count == 0 {
            return Ok(None);
        }

        if self.total_exposure_hours <= 0.0 {
            return Err("signed Hawkes likelihood tape has zero open-session exposure".to_string());
        }

        let baseline_negative_log_likelihood_per_event =
            homogeneous_signed_poisson_negative_log_likelihood_per_event(
                self.buy_event_count,
                self.sell_event_count,
                self.total_exposure_hours,
            )?;

        Ok(Some(HawkesLikelihoodTape {
            delta_exposure_hours: self.delta_exposure_hours,
            buy_side_flags: self.buy_side_flags,
            reset_flags: self.reset_flags,
            event_count: self.event_count,
            baseline_negative_log_likelihood_per_event,
        }))
    }
}

fn homogeneous_signed_poisson_negative_log_likelihood_per_event(
    buy_event_count: usize,
    sell_event_count: usize,
    total_exposure_hours: f64,
) -> Result<f64, String> {
    if !total_exposure_hours.is_finite() || total_exposure_hours <= 0.0 {
        return Err(format!(
            "homogeneous signed Poisson baseline exposure must be finite and positive, found {total_exposure_hours}"
        ));
    }

    let event_count = buy_event_count.saturating_add(sell_event_count);
    if event_count == 0 {
        return Err("homogeneous signed Poisson baseline requires at least one event".to_string());
    }

    let buy_intensity = buy_event_count as f64 / total_exposure_hours;
    let sell_intensity = sell_event_count as f64 / total_exposure_hours;

    let mut log_intensity_sum = 0.0;
    if buy_event_count > 0 {
        log_intensity_sum += buy_event_count as f64 * buy_intensity.ln();
    }
    if sell_event_count > 0 {
        log_intensity_sum += sell_event_count as f64 * sell_intensity.ln();
    }

    let compensator = (buy_intensity + sell_intensity) * total_exposure_hours;
    let negative_log_likelihood_per_event = (compensator - log_intensity_sum) / event_count as f64;

    if !negative_log_likelihood_per_event.is_finite() {
        return Err(format!(
            "homogeneous signed Poisson baseline likelihood is non-finite, found {negative_log_likelihood_per_event}"
        ));
    }

    Ok(negative_log_likelihood_per_event)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct HawkesLikelihoodWorkerProgress {
    processed_event_count: usize,
    current_relative_negative_log_likelihood_per_event: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
struct HawkesLikelihoodCandidateOutput {
    candidate_index: usize,
    diagnostic: Option<HawkesSignedLikelihoodDiagnostic>,
    warning: Option<String>,
}

struct HawkesLikelihoodScoringState<'a> {
    candidate_labels: Vec<&'a str>,
    completed_candidate_count: usize,
    total_candidate_count: usize,
    total_event_count: usize,
    best_relative_negative_log_likelihood_per_event: Option<f64>,
    outputs: Vec<Option<HawkesLikelihoodCandidateOutput>>,
    progress_sender: Option<&'a Sender<HawkesStageProgressUpdate>>,
}

fn evaluate_candidate_signed_likelihood(
    candidate_index: usize,
    prepared_candidate: &HawkesPreparedCandidate,
    likelihood_tape: &HawkesLikelihoodTape,
    context: StreamingWorkContext<HawkesLikelihoodWorkerProgress, HawkesLikelihoodCandidateOutput>,
) -> HawkesLikelihoodCandidateOutput {
    let diagnostic = match signed_likelihood_derived_alpha(prepared_candidate) {
        Some(derived_alpha) => evaluate_signed_hawkes_likelihood(
            likelihood_tape,
            prepared_candidate.adjusted_candidate_input.lambda_bar,
            prepared_candidate.adjusted_candidate_input.beta,
            derived_alpha,
            context,
        ),
        None => Err("candidate does not provide a valid derived alpha loading".to_string()),
    };

    match diagnostic {
        Ok(diagnostic) => HawkesLikelihoodCandidateOutput {
            candidate_index,
            diagnostic: Some(diagnostic),
            warning: None,
        },
        Err(error_message) => HawkesLikelihoodCandidateOutput {
            candidate_index,
            diagnostic: None,
            warning: Some(error_message),
        },
    }
}

fn signed_likelihood_derived_alpha(
    prepared_candidate: &HawkesPreparedCandidate,
) -> Option<HawkesKernelLoadings> {
    if prepared_candidate.validation_report.status != HawkesKernelValidationStatus::Pass {
        return None;
    }

    prepared_candidate.validation_report.derived_alpha
}

fn evaluate_signed_hawkes_likelihood(
    likelihood_tape: &HawkesLikelihoodTape,
    lambda_bar: f64,
    beta: f64,
    derived_alpha: HawkesKernelLoadings,
    context: StreamingWorkContext<HawkesLikelihoodWorkerProgress, HawkesLikelihoodCandidateOutput>,
) -> Result<HawkesSignedLikelihoodDiagnostic, String> {
    if !lambda_bar.is_finite() || lambda_bar <= 0.0 {
        return Err(format!(
            "candidate lambda_bar must be finite and positive for signed likelihood, found {lambda_bar}"
        ));
    }
    if !beta.is_finite() || beta <= 0.0 {
        return Err(format!(
            "candidate beta must be finite and positive for signed likelihood, found {beta}"
        ));
    }

    let progress_stride = progress_stride(likelihood_tape.event_count);
    let cancellation_token = context.cancellation_token();
    let mut state = HawkesExcitationState::from_target_sums(0.0, 0.0);
    let mut compensator = 0.0;
    let mut log_intensity_sum = 0.0;

    for event_position in 0..likelihood_tape.event_count {
        cancellation_token
            .check_cancelled()
            .map_err(|source| source.to_string())?;

        if likelihood_tape.reset_state_before_event(event_position) {
            state = HawkesExcitationState::from_target_sums(0.0, 0.0);
        } else {
            let Some(delta_exposure_hours) = likelihood_tape
                .delta_exposure_hours
                .get(event_position)
                .copied()
            else {
                return Err(format!(
                    "signed Hawkes likelihood tape is missing delta exposure for event {event_position}"
                ));
            };
            let decay_factor = (-beta * delta_exposure_hours).exp();
            let decay_integral = (1.0 - decay_factor) / beta;

            compensator += 2.0 * lambda_bar * delta_exposure_hours
                + state.target_plus_sum() * decay_integral
                + state.target_minus_sum() * decay_integral;

            state = state.apply_shared_decay_drift(beta, delta_exposure_hours);
        }

        let event_side = likelihood_tape.event_side(event_position);
        let projected_intensities = state.project_deseasonalized_intensities(lambda_bar);
        let event_intensity = match event_side {
            HawkesEventSide::Buy => projected_intensities.lambda_plus,
            HawkesEventSide::Sell => projected_intensities.lambda_minus,
        };

        if !event_intensity.is_finite() || event_intensity <= 0.0 {
            return Err(format!(
                "candidate produced non-positive signed Hawkes event intensity {event_intensity}"
            ));
        }

        log_intensity_sum += event_intensity.ln();
        state = state.apply_endogenous_event_jump(event_side, derived_alpha);

        let processed_event_count = event_position.saturating_add(1);
        if processed_event_count % progress_stride == 0
            || processed_event_count == likelihood_tape.event_count
        {
            let current_relative_negative_log_likelihood_per_event =
                current_relative_negative_log_likelihood_per_event(
                    compensator,
                    log_intensity_sum,
                    processed_event_count,
                    likelihood_tape.baseline_negative_log_likelihood_per_event,
                );

            let _ = context.emit_progress(HawkesLikelihoodWorkerProgress {
                processed_event_count,
                current_relative_negative_log_likelihood_per_event,
            });
        }
    }

    let hawkes_negative_log_likelihood_per_event =
        (compensator - log_intensity_sum) / likelihood_tape.event_count as f64;
    if !hawkes_negative_log_likelihood_per_event.is_finite() {
        return Err(format!(
            "candidate signed Hawkes likelihood is non-finite, found {hawkes_negative_log_likelihood_per_event}"
        ));
    }

    let relative_negative_log_likelihood_per_event = hawkes_negative_log_likelihood_per_event
        - likelihood_tape.baseline_negative_log_likelihood_per_event;

    if !relative_negative_log_likelihood_per_event.is_finite() {
        return Err(format!(
            "candidate signed Hawkes likelihood delta is non-finite, found {relative_negative_log_likelihood_per_event}"
        ));
    }

    Ok(HawkesSignedLikelihoodDiagnostic {
        hawkes_negative_log_likelihood_per_event,
        baseline_negative_log_likelihood_per_event: likelihood_tape
            .baseline_negative_log_likelihood_per_event,
        relative_negative_log_likelihood_per_event,
    })
}

fn current_relative_negative_log_likelihood_per_event(
    compensator: f64,
    log_intensity_sum: f64,
    processed_event_count: usize,
    baseline_negative_log_likelihood_per_event: f64,
) -> Option<f64> {
    if processed_event_count == 0 {
        return None;
    }

    let current_negative_log_likelihood_per_event =
        (compensator - log_intensity_sum) / processed_event_count as f64;
    let current_relative_negative_log_likelihood_per_event =
        current_negative_log_likelihood_per_event - baseline_negative_log_likelihood_per_event;

    current_relative_negative_log_likelihood_per_event
        .is_finite()
        .then_some(current_relative_negative_log_likelihood_per_event)
}

fn record_likelihood_worker_progress(
    state: &mut HawkesLikelihoodScoringState<'_>,
    progress: StreamingWorkProgress<HawkesLikelihoodWorkerProgress>,
) -> Result<(), String> {
    let candidate_label = state
        .candidate_labels
        .get(progress.job_position)
        .copied()
        .unwrap_or("candidate");

    emit_hawkes_convergence_progress(
        state.progress_sender,
        HawkesConvergenceProgress {
            phase: HawkesStageProgressPhase::LikelihoodShortlistEvaluation,
            completed_units: state.completed_candidate_count,
            total_units: state.total_candidate_count,
            current_item_label: Some(candidate_label),
            current_evaluation_count: progress.progress.processed_event_count,
            total_evaluation_budget: state.total_event_count,
            current_weighted_objective: progress
                .progress
                .current_relative_negative_log_likelihood_per_event,
            best_weighted_objective: state.best_relative_negative_log_likelihood_per_event,
            current_convergence_ratio: None,
        },
    );

    Ok(())
}

fn record_likelihood_worker_output(
    state: &mut HawkesLikelihoodScoringState<'_>,
    output: StreamingWorkOutput<HawkesLikelihoodCandidateOutput>,
) -> Result<(), String> {
    let candidate_label = state
        .candidate_labels
        .get(output.job_position)
        .copied()
        .unwrap_or("candidate");

    let current_relative_negative_log_likelihood_per_event = output
        .output
        .diagnostic
        .map(|diagnostic| diagnostic.relative_negative_log_likelihood_per_event);

    if let Some(current_loss) = current_relative_negative_log_likelihood_per_event {
        state.best_relative_negative_log_likelihood_per_event =
            match state.best_relative_negative_log_likelihood_per_event {
                Some(best_loss) if best_loss <= current_loss => Some(best_loss),
                _ => Some(current_loss),
            };
    }

    let Some(output_slot) = state.outputs.get_mut(output.job_position) else {
        return Err(format!(
            "signed Hawkes likelihood output position {} is outside the scheduled candidate set",
            output.job_position
        ));
    };

    *output_slot = Some(output.output);
    state.completed_candidate_count = state.completed_candidate_count.saturating_add(1);

    emit_hawkes_convergence_progress(
        state.progress_sender,
        HawkesConvergenceProgress {
            phase: HawkesStageProgressPhase::LikelihoodShortlistEvaluation,
            completed_units: state.completed_candidate_count,
            total_units: state.total_candidate_count,
            current_item_label: Some(candidate_label),
            current_evaluation_count: state.total_event_count,
            total_evaluation_budget: state.total_event_count,
            current_weighted_objective: current_relative_negative_log_likelihood_per_event,
            best_weighted_objective: state.best_relative_negative_log_likelihood_per_event,
            current_convergence_ratio: None,
        },
    );

    Ok(())
}

fn build_likelihood_scored_candidates(
    prepared_candidates: &[HawkesPreparedCandidate],
    outputs: Vec<Option<HawkesLikelihoodCandidateOutput>>,
) -> Result<Vec<HawkesPreparedCandidate>, String> {
    let mut scored_candidates = Vec::with_capacity(outputs.len());

    for (output_position, output) in outputs.into_iter().enumerate() {
        let Some(output) = output else {
            return Err(format!(
                "missing signed Hawkes likelihood output for shortlisted candidate position {output_position}"
            ));
        };

        let Some(prepared_candidate) = prepared_candidates.get(output.candidate_index) else {
            return Err(format!(
                "signed Hawkes likelihood output references missing candidate index {}",
                output.candidate_index
            ));
        };

        // Intentional clone because the likelihood shortlist owns augmented
        // objective terms while the cheap full-grid candidate set remains unchanged.
        let mut scored_candidate = prepared_candidate.clone();

        if let Some(diagnostic) = output.diagnostic {
            append_signed_likelihood_objective_term(&mut scored_candidate, diagnostic);
        } else {
            scored_candidate.external_constraint_violation_count = scored_candidate
                .external_constraint_violation_count
                .saturating_add(1);
            if let Some(warning) = output.warning {
                scored_candidate.warnings.push(format!(
                    "signed Hawkes likelihood evaluation failed: {warning}"
                ));
            }
        }

        scored_candidates.push(scored_candidate);
    }

    Ok(scored_candidates)
}

fn shortlist_candidate_indices(
    prepared_candidates: &[HawkesPreparedCandidate],
    candidate_diagnostics: &[HawkesStageCandidateDiagnostic],
    max_scored_candidate_count: usize,
) -> Vec<usize> {
    let scoring_limit = max_scored_candidate_count
        .max(1)
        .min(prepared_candidates.len());

    let eligible_candidate_indices = (0..prepared_candidates.len()).collect::<Vec<_>>();

    ranked_hawkes_candidate_indices(candidate_diagnostics, eligible_candidate_indices.as_slice())
        .into_iter()
        .take(scoring_limit)
        .collect()
}

impl HawkesLikelihoodTape {
    fn reset_state_before_event(&self, event_position: usize) -> bool {
        self.reset_flags.is_set(event_position)
    }

    fn event_side(&self, event_position: usize) -> HawkesEventSide {
        if self.buy_side_flags.is_set(event_position) {
            HawkesEventSide::Buy
        } else {
            HawkesEventSide::Sell
        }
    }
}

fn progress_stride(total_unit_count: usize) -> usize {
    total_unit_count
        .checked_div(PROGRESS_PERCENT_BUCKET_COUNT)
        .filter(|stride| *stride > 0)
        .unwrap_or(1)
}

fn empty_likelihood_outputs(
    candidate_count: usize,
) -> Vec<Option<HawkesLikelihoodCandidateOutput>> {
    let mut outputs = Vec::with_capacity(candidate_count);
    outputs.resize_with(candidate_count, || None);
    outputs
}
