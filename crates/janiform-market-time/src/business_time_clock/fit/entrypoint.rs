use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono_tz::Tz;
use janiform_foundations::cancellation::CancellationToken;
use janiform_foundations::linear_algebra::weighted_ridge_least_squares_from_normal_equations;
use janiform_foundations::time::{NANOS_PER_MINUTE, checked_floor_to_interval_utc_nanos};
use janiform_work_scheduler::{
    StreamingWorkContext, StreamingWorkOutput, StreamingWorkPoolInput, StreamingWorkProgress,
    run_streaming_work_pool,
};

use super::super::coordinates::{
    FOURIER_BUSINESS_TIME_CLOCK_AXES, FourierBusinessTimeClockAxis, FourierCoefficientSlices,
    FourierHarmonicOrders,
};
use super::super::coordinates::{feature_vector, split_coefficients_by_axis};
use super::super::exposure::{
    BusinessTimeClockExposureBuildInput, BusinessTimeClockExposureError,
    BusinessTimeClockExposureRow, build_business_time_clock_exposure_rows,
};
use super::super::{
    FOURIER_BUSINESS_TIME_CLOCK_BASIS, FOURIER_BUSINESS_TIME_CLOCK_COORDINATE_SYSTEM,
    FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION,
    FOURIER_BUSINESS_TIME_CLOCK_NORMALIZATION_POLICY, FOURIER_BUSINESS_TIME_CLOCK_SELECTION_POLICY,
    FourierBusinessTimeClockModel, log_rate_envelope_fits_representable_positive_f64_domain,
};
use super::candidate::{CandidateOrder, candidate_orders};
use crate::calendar::ExchangeCalendarDefinition;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FourierBusinessTimeClockFitSettings {
    pub maximum_harmonic_orders: BTreeMap<FourierBusinessTimeClockAxis, usize>,
    pub maximum_parameter_count: usize,
    pub worker_count: usize,
}

#[derive(Debug, Clone)]
pub struct FourierBusinessTimeClockFitInput<'a> {
    pub calendar_definition: &'a ExchangeCalendarDefinition,
    pub timezone: Tz,
    pub timestamp_utc_nanos: &'a [i64],
    pub settings: FourierBusinessTimeClockFitSettings,
    pub cancellation_token: CancellationToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FourierBusinessTimeClockFitProgressPhase {
    ExposureAssembly,
    CandidateEvaluation,
    ModelFinalization,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FourierBusinessTimeClockFitProgress {
    pub phase: FourierBusinessTimeClockFitProgressPhase,
    pub completed_units: usize,
    pub total_units: usize,
    pub current_item_label: Option<String>,
    pub current_objective_value: Option<f64>,
    pub best_objective_value: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FourierBusinessTimeClockFitSummary {
    pub model: FourierBusinessTimeClockModel,
    pub evaluated_candidate_count: usize,
    pub candidate_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FourierBusinessTimeClockFitError {
    EmptyTimestampEvents,
    InvalidSettings {
        reason: String,
    },
    TimestampGridOverflow,
    ExposureAssemblyFailed {
        source: BusinessTimeClockExposureError,
    },
    NoOpenExposureRows,
    NoOpenEventsInExposureRows,
    InvalidExposure,
    NoCandidateModels,
    CandidateEvaluationFailed {
        reason: String,
    },
}

impl Display for FourierBusinessTimeClockFitError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyTimestampEvents => {
                formatter.write_str("business-time clock fitting requires timestamp events")
            }
            Self::InvalidSettings { reason } => write!(
                formatter,
                "business-time clock fitting received invalid settings: {reason}"
            ),
            Self::TimestampGridOverflow => {
                formatter.write_str("business-time clock fitting overflowed the UTC minute grid")
            }
            Self::ExposureAssemblyFailed { source } => {
                write!(
                    formatter,
                    "business-time clock exposure assembly failed: {source}"
                )
            }
            Self::NoOpenExposureRows => {
                formatter.write_str("business-time clock fitting found no open exposure rows")
            }
            Self::NoOpenEventsInExposureRows => formatter
                .write_str("business-time clock fitting found no events in open exposure rows"),
            Self::InvalidExposure => {
                formatter.write_str("business-time clock fitting found invalid open exposure")
            }
            Self::NoCandidateModels => {
                formatter.write_str("business-time clock fitting produced no candidate models")
            }
            Self::CandidateEvaluationFailed { reason } => write!(
                formatter,
                "business-time clock candidate evaluation failed: {reason}"
            ),
        }
    }
}

impl Error for FourierBusinessTimeClockFitError {}

pub fn fit_fourier_business_time_clock_model(
    input: FourierBusinessTimeClockFitInput<'_>,
    mut progress_observer: Option<&mut dyn FnMut(FourierBusinessTimeClockFitProgress)>,
) -> Result<FourierBusinessTimeClockFitSummary, FourierBusinessTimeClockFitError> {
    validate_settings(&input.settings)?;

    emit_fit_progress(
        &mut progress_observer,
        FourierBusinessTimeClockFitProgress {
            phase: FourierBusinessTimeClockFitProgressPhase::ExposureAssembly,
            completed_units: 0,
            total_units: 1,
            current_item_label: None,
            current_objective_value: None,
            best_objective_value: None,
        },
    );

    let design = build_fit_design(&input)?;

    emit_fit_progress(
        &mut progress_observer,
        FourierBusinessTimeClockFitProgress {
            phase: FourierBusinessTimeClockFitProgressPhase::ExposureAssembly,
            completed_units: 1,
            total_units: 1,
            current_item_label: None,
            current_objective_value: None,
            best_objective_value: None,
        },
    );

    let candidates = candidate_orders(&input.settings);
    if candidates.is_empty() {
        return Err(FourierBusinessTimeClockFitError::NoCandidateModels);
    }

    let selected_candidate = evaluate_candidate_models(
        &design,
        &candidates,
        &input.settings,
        input.cancellation_token.shared_handle(),
        &mut progress_observer,
    )?;

    emit_fit_progress(
        &mut progress_observer,
        FourierBusinessTimeClockFitProgress {
            phase: FourierBusinessTimeClockFitProgressPhase::ModelFinalization,
            completed_units: 0,
            total_units: 1,
            current_item_label: None,
            current_objective_value: Some(selected_candidate.objective_value),
            best_objective_value: Some(selected_candidate.objective_value),
        },
    );

    let model = selected_candidate.into_model(&design).ok_or_else(|| {
        FourierBusinessTimeClockFitError::CandidateEvaluationFailed {
            reason: "selected candidate coefficients do not match harmonic orders".to_string(),
        }
    })?;

    model
        .validate()
        .map_err(|reason| FourierBusinessTimeClockFitError::CandidateEvaluationFailed { reason })?;

    emit_fit_progress(
        &mut progress_observer,
        FourierBusinessTimeClockFitProgress {
            phase: FourierBusinessTimeClockFitProgressPhase::ModelFinalization,
            completed_units: 1,
            total_units: 1,
            current_item_label: None,
            current_objective_value: Some(model.selected_objective_value),
            best_objective_value: Some(model.selected_objective_value),
        },
    );

    Ok(FourierBusinessTimeClockFitSummary {
        model,
        evaluated_candidate_count: candidates.len(),
        candidate_count: candidates.len(),
    })
}

fn validate_settings(
    settings: &FourierBusinessTimeClockFitSettings,
) -> Result<(), FourierBusinessTimeClockFitError> {
    let maximum_orders = FourierHarmonicOrders::from_order_map(&settings.maximum_harmonic_orders)
        .ok_or_else(|| FourierBusinessTimeClockFitError::InvalidSettings {
        reason: "maximum_harmonic_orders must provide one order for every Fourier clock axis"
            .to_string(),
    })?;

    if settings.maximum_harmonic_orders.len() != FOURIER_BUSINESS_TIME_CLOCK_AXES.len() {
        return Err(FourierBusinessTimeClockFitError::InvalidSettings {
            reason: "maximum_harmonic_orders must provide one order for every Fourier clock axis"
                .to_string(),
        });
    }

    for (axis, harmonic_order) in maximum_orders.axis_orders() {
        if let Some(nyquist_limit) = axis.nyquist_harmonic_limit()
            && harmonic_order > nyquist_limit
        {
            return Err(FourierBusinessTimeClockFitError::InvalidSettings {
                reason: format!(
                    "{} exceeds the {} Nyquist limit",
                    axis.maximum_harmonic_order_setting_name(),
                    axis.coordinate_name()
                ),
            });
        }
    }

    if settings.maximum_parameter_count == 0 {
        return Err(FourierBusinessTimeClockFitError::InvalidSettings {
            reason: "maximum_parameter_count must be positive".to_string(),
        });
    }

    if settings.worker_count == 0 {
        return Err(FourierBusinessTimeClockFitError::InvalidSettings {
            reason: "worker_count must be positive".to_string(),
        });
    }

    Ok(())
}

fn build_fit_design(
    input: &FourierBusinessTimeClockFitInput<'_>,
) -> Result<FourierBusinessTimeClockFitDesign, FourierBusinessTimeClockFitError> {
    let Some(first_timestamp_utc_nanos) = input.timestamp_utc_nanos.iter().min().copied() else {
        return Err(FourierBusinessTimeClockFitError::EmptyTimestampEvents);
    };

    let Some(last_timestamp_utc_nanos) = input.timestamp_utc_nanos.iter().max().copied() else {
        return Err(FourierBusinessTimeClockFitError::EmptyTimestampEvents);
    };

    let range_start_utc_nanos =
        checked_floor_to_interval_utc_nanos(first_timestamp_utc_nanos, NANOS_PER_MINUTE)
            .ok_or(FourierBusinessTimeClockFitError::TimestampGridOverflow)?;

    let last_minute_start_utc_nanos =
        checked_floor_to_interval_utc_nanos(last_timestamp_utc_nanos, NANOS_PER_MINUTE)
            .ok_or(FourierBusinessTimeClockFitError::TimestampGridOverflow)?;

    let range_end_utc_nanos = last_minute_start_utc_nanos
        .checked_add(NANOS_PER_MINUTE)
        .ok_or(FourierBusinessTimeClockFitError::TimestampGridOverflow)?;

    let event_counts_by_minute_start_utc_nanos =
        event_counts_by_minute_start_utc_nanos(input.timestamp_utc_nanos)?;

    let exposure_rows =
        build_business_time_clock_exposure_rows(BusinessTimeClockExposureBuildInput {
            calendar_definition: input.calendar_definition,
            timezone: input.timezone,
            range_start_utc_nanos,
            range_end_utc_nanos,
            event_counts_by_minute_start_utc_nanos: &event_counts_by_minute_start_utc_nanos,
        })
        .map_err(|source| FourierBusinessTimeClockFitError::ExposureAssemblyFailed { source })?;

    if exposure_rows.is_empty() {
        return Err(FourierBusinessTimeClockFitError::NoOpenExposureRows);
    }

    let fit_open_event_count = exposure_rows
        .iter()
        .map(|row| row.event_count)
        .sum::<usize>();
    if fit_open_event_count == 0 {
        return Err(FourierBusinessTimeClockFitError::NoOpenEventsInExposureRows);
    }

    let fit_open_exposure_seconds = exposure_rows
        .iter()
        .map(|row| row.exposure_seconds)
        .sum::<f64>();
    if !fit_open_exposure_seconds.is_finite() || fit_open_exposure_seconds <= 0.0 {
        return Err(FourierBusinessTimeClockFitError::InvalidExposure);
    }

    Ok(FourierBusinessTimeClockFitDesign {
        exposure_rows,
        fit_open_event_count,
        fit_open_exposure_seconds,
    })
}

fn event_counts_by_minute_start_utc_nanos(
    timestamp_utc_nanos: &[i64],
) -> Result<BTreeMap<i64, usize>, FourierBusinessTimeClockFitError> {
    let mut counts = BTreeMap::new();

    for &timestamp_utc_nanos in timestamp_utc_nanos {
        let minute_start_utc_nanos =
            checked_floor_to_interval_utc_nanos(timestamp_utc_nanos, NANOS_PER_MINUTE)
                .ok_or(FourierBusinessTimeClockFitError::TimestampGridOverflow)?;

        let current_count = counts.entry(minute_start_utc_nanos).or_insert(0_usize);

        *current_count = current_count
            .checked_add(1)
            .ok_or(FourierBusinessTimeClockFitError::TimestampGridOverflow)?;
    }

    Ok(counts)
}

fn evaluate_candidate_models(
    design: &FourierBusinessTimeClockFitDesign,
    candidates: &[CandidateOrder],
    settings: &FourierBusinessTimeClockFitSettings,
    cancellation_token: CancellationToken,
    progress_observer: &mut Option<&mut dyn FnMut(FourierBusinessTimeClockFitProgress)>,
) -> Result<CandidateFit, FourierBusinessTimeClockFitError> {
    let mut state = CandidateSearchState {
        completed_candidate_count: 0,
        total_candidate_count: candidates.len(),
        best_candidate: None,
        progress_observer,
    };

    run_streaming_work_pool(StreamingWorkPoolInput::new(
        &mut state,
        cancellation_token,
        candidates.to_vec(),
        settings.worker_count,
        |candidate: CandidateOrder,
         context: StreamingWorkContext<CandidateProgress, Option<CandidateFit>>| {
            let _ = context.emit_progress(CandidateProgress {
                label: candidate.label(),
            });

            fit_candidate(design, candidate)
        },
        record_candidate_progress,
        record_candidate_output,
    ))
    .map_err(|reason: FourierBusinessTimeClockFitError| reason)?;

    let mut best_candidate = state
        .best_candidate
        .ok_or(FourierBusinessTimeClockFitError::NoCandidateModels)?;

    best_candidate.evaluated_candidate_count = state.completed_candidate_count;

    Ok(best_candidate)
}

fn record_candidate_progress(
    state: &mut CandidateSearchState<'_, '_>,
    progress: StreamingWorkProgress<CandidateProgress>,
) -> Result<(), FourierBusinessTimeClockFitError> {
    emit_fit_progress(
        &mut *state.progress_observer,
        FourierBusinessTimeClockFitProgress {
            phase: FourierBusinessTimeClockFitProgressPhase::CandidateEvaluation,
            completed_units: state.completed_candidate_count,
            total_units: state.total_candidate_count,
            current_item_label: Some(progress.progress.label),
            current_objective_value: None,
            best_objective_value: state
                .best_candidate
                .as_ref()
                .map(|candidate| candidate.objective_value),
        },
    );

    Ok(())
}

fn record_candidate_output(
    state: &mut CandidateSearchState<'_, '_>,
    output: StreamingWorkOutput<Option<CandidateFit>>,
) -> Result<(), FourierBusinessTimeClockFitError> {
    state.completed_candidate_count = state.completed_candidate_count.checked_add(1).ok_or(
        FourierBusinessTimeClockFitError::CandidateEvaluationFailed {
            reason: "candidate progress counter overflowed".to_string(),
        },
    )?;

    if let Some(candidate_fit) = output.output
        && candidate_is_better(state.best_candidate.as_ref(), &candidate_fit)
    {
        state.best_candidate = Some(candidate_fit);
    }

    let current_objective_value = state
        .best_candidate
        .as_ref()
        .map(|candidate| candidate.objective_value);

    emit_fit_progress(
        &mut *state.progress_observer,
        FourierBusinessTimeClockFitProgress {
            phase: FourierBusinessTimeClockFitProgressPhase::CandidateEvaluation,
            completed_units: state.completed_candidate_count,
            total_units: state.total_candidate_count,
            current_item_label: Some(candidates_progress_label(
                output.job_position,
                state.total_candidate_count,
            )),
            current_objective_value,
            best_objective_value: current_objective_value,
        },
    );

    Ok(())
}

fn fit_candidate(
    design: &FourierBusinessTimeClockFitDesign,
    candidate: CandidateOrder,
) -> Option<CandidateFit> {
    let coefficients = fit_candidate_coefficients(design, candidate)?;
    let normalization = candidate_normalization(design, candidate, coefficients.as_slice())?;

    candidate_log_rate_envelope_fits_representable_positive_f64_domain(
        candidate,
        coefficients.as_slice(),
        normalization,
    )?;

    let negative_log_likelihood = candidate_negative_log_likelihood(
        design,
        candidate,
        coefficients.as_slice(),
        normalization,
    )?;

    let objective_value = aicc_objective_value(
        negative_log_likelihood,
        candidate.parameter_count,
        design.exposure_rows.len(),
    )?;

    Some(CandidateFit {
        order: candidate,
        coefficients,
        normalization,
        negative_log_likelihood,
        objective_value,
        fit_open_minute_count: design.exposure_rows.len(),
        fit_open_event_count: design.fit_open_event_count,
        fit_open_exposure_seconds: design.fit_open_exposure_seconds,
        evaluated_candidate_count: 0,
    })
}

fn fit_candidate_coefficients(
    design: &FourierBusinessTimeClockFitDesign,
    candidate: CandidateOrder,
) -> Option<Vec<f64>> {
    let mut normal_matrix_rows =
        vec![vec![0.0; candidate.parameter_count]; candidate.parameter_count];

    let mut normal_rhs = vec![0.0; candidate.parameter_count];

    let diagonal_penalties = vec![0.0; candidate.parameter_count];

    let mut positive_count_row_count = 0_usize;

    for row in &design.exposure_rows {
        if row.event_count == 0 {
            continue;
        }

        let response = (row.event_count as f64 / row.exposure_seconds).ln();
        if !response.is_finite() {
            return None;
        }

        let weight = row.event_count as f64;

        let features = feature_vector(candidate.orders, row.phases)?;
        if features.len() != candidate.parameter_count {
            return None;
        }

        positive_count_row_count = positive_count_row_count.checked_add(1)?;

        for left_position in 0..candidate.parameter_count {
            normal_rhs[left_position] += weight * features[left_position] * response;

            for right_position in 0..candidate.parameter_count {
                normal_matrix_rows[left_position][right_position] +=
                    weight * features[left_position] * features[right_position];
            }
        }
    }

    if positive_count_row_count < candidate.parameter_count {
        return None;
    }

    weighted_ridge_least_squares_from_normal_equations(
        normal_matrix_rows.as_slice(),
        normal_rhs.as_slice(),
        diagonal_penalties.as_slice(),
    )
}

fn candidate_normalization(
    design: &FourierBusinessTimeClockFitDesign,
    candidate: CandidateOrder,
    coefficients: &[f64],
) -> Option<f64> {
    let mut weighted_rate_sum = 0.0;

    for row in &design.exposure_rows {
        let raw_rate = candidate_raw_rate(candidate, coefficients, row)?;

        weighted_rate_sum += row.exposure_seconds * raw_rate;
        if !weighted_rate_sum.is_finite() {
            return None;
        }
    }

    let normalization = weighted_rate_sum / design.fit_open_exposure_seconds;

    (normalization.is_finite() && normalization > 0.0).then_some(normalization)
}

fn candidate_negative_log_likelihood(
    design: &FourierBusinessTimeClockFitDesign,
    candidate: CandidateOrder,
    coefficients: &[f64],
    normalization: f64,
) -> Option<f64> {
    let base_rate = design.fit_open_event_count as f64 / design.fit_open_exposure_seconds;
    if !base_rate.is_finite()
        || base_rate <= 0.0
        || !normalization.is_finite()
        || normalization <= 0.0
    {
        return None;
    }

    let mut negative_log_likelihood = 0.0;

    for row in &design.exposure_rows {
        let raw_rate = candidate_raw_rate(candidate, coefficients, row)?;

        let expected_count = base_rate * row.exposure_seconds * raw_rate / normalization;
        if !expected_count.is_finite() || expected_count <= 0.0 {
            return None;
        }

        negative_log_likelihood += expected_count;

        if row.event_count > 0 {
            negative_log_likelihood -= row.event_count as f64 * expected_count.ln();
        }

        if !negative_log_likelihood.is_finite() {
            return None;
        }
    }

    Some(negative_log_likelihood)
}

fn candidate_raw_rate(
    candidate: CandidateOrder,
    coefficients: &[f64],
    row: &BusinessTimeClockExposureRow,
) -> Option<f64> {
    let features = feature_vector(candidate.orders, row.phases)?;
    if features.len() != coefficients.len() {
        return None;
    }

    let log_rate = features
        .iter()
        .zip(coefficients.iter())
        .map(|(feature, coefficient)| feature * coefficient)
        .sum::<f64>();
    if !log_rate.is_finite() {
        return None;
    }

    let raw_rate = log_rate.exp();

    (raw_rate.is_finite() && raw_rate > 0.0).then_some(raw_rate)
}

fn candidate_log_rate_envelope_fits_representable_positive_f64_domain(
    candidate: CandidateOrder,
    coefficients: &[f64],
    normalization: f64,
) -> Option<()> {
    let intercept = normalized_candidate_intercept(coefficients, normalization)?;
    let harmonic_series = split_coefficients_by_axis(coefficients, candidate.orders)?;

    log_rate_envelope_fits_representable_positive_f64_domain(FourierCoefficientSlices {
        intercept,
        harmonic_series: &harmonic_series,
    })
    .then_some(())
}

fn normalized_candidate_intercept(coefficients: &[f64], normalization: f64) -> Option<f64> {
    if !normalization.is_finite() || normalization <= 0.0 {
        return None;
    }

    let intercept = *coefficients.first()? - normalization.ln();

    intercept.is_finite().then_some(intercept)
}

fn aicc_objective_value(
    negative_log_likelihood: f64,
    parameter_count: usize,
    observation_count: usize,
) -> Option<f64> {
    if !negative_log_likelihood.is_finite()
        || observation_count == 0
        || parameter_count == 0
        || observation_count <= parameter_count + 1
    {
        return None;
    }

    let parameter_count = parameter_count as f64;
    let observation_count = observation_count as f64;

    let small_sample_correction = 2.0 * parameter_count * (parameter_count + 1.0)
        / (observation_count - parameter_count - 1.0);

    let objective = 2.0 * negative_log_likelihood + 2.0 * parameter_count + small_sample_correction;

    objective.is_finite().then_some(objective)
}

fn candidate_is_better(current_best: Option<&CandidateFit>, candidate: &CandidateFit) -> bool {
    let Some(current_best) = current_best else {
        return true;
    };

    let mut ordering = candidate
        .objective_value
        .total_cmp(&current_best.objective_value)
        .then(
            candidate
                .order
                .parameter_count
                .cmp(&current_best.order.parameter_count),
        );

    for axis in FOURIER_BUSINESS_TIME_CLOCK_AXES {
        ordering = ordering.then(
            candidate
                .order
                .orders
                .order_for_axis(axis)
                .cmp(&current_best.order.orders.order_for_axis(axis)),
        );

        if ordering != Ordering::Equal {
            break;
        }
    }

    ordering.is_lt()
}

fn candidates_progress_label(job_position: usize, total_candidate_count: usize) -> String {
    format!(
        "candidate {}/{}",
        job_position.saturating_add(1),
        total_candidate_count
    )
}

fn emit_fit_progress(
    progress_observer: &mut Option<&mut dyn FnMut(FourierBusinessTimeClockFitProgress)>,
    progress: FourierBusinessTimeClockFitProgress,
) {
    let Some(observer) = progress_observer else {
        return;
    };

    observer(progress);
}

#[derive(Debug)]
struct FourierBusinessTimeClockFitDesign {
    exposure_rows: Vec<BusinessTimeClockExposureRow>,
    fit_open_event_count: usize,
    fit_open_exposure_seconds: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct CandidateFit {
    order: CandidateOrder,
    coefficients: Vec<f64>,
    normalization: f64,
    negative_log_likelihood: f64,
    objective_value: f64,
    fit_open_minute_count: usize,
    fit_open_event_count: usize,
    fit_open_exposure_seconds: f64,
    evaluated_candidate_count: usize,
}

impl CandidateFit {
    fn into_model(
        self,
        design: &FourierBusinessTimeClockFitDesign,
    ) -> Option<FourierBusinessTimeClockModel> {
        if self.coefficients.len() != self.order.parameter_count {
            return None;
        }

        let intercept =
            normalized_candidate_intercept(self.coefficients.as_slice(), self.normalization)?;

        let coefficient_vectors =
            split_coefficients_by_axis(self.coefficients.as_slice(), self.order.orders)?;

        Some(FourierBusinessTimeClockModel {
            schema_version: FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION.to_string(),
            coordinate_system: FOURIER_BUSINESS_TIME_CLOCK_COORDINATE_SYSTEM.to_string(),
            coefficient_basis: FOURIER_BUSINESS_TIME_CLOCK_BASIS.to_string(),
            normalization_policy: FOURIER_BUSINESS_TIME_CLOCK_NORMALIZATION_POLICY.to_string(),
            selection_policy: FOURIER_BUSINESS_TIME_CLOCK_SELECTION_POLICY.to_string(),
            intercept,
            harmonic_series: coefficient_vectors,
            selected_objective_value: self.objective_value,
            selected_negative_log_likelihood: self.negative_log_likelihood,
            evaluated_candidate_count: self.evaluated_candidate_count.max(1),
            fit_open_minute_count: design.exposure_rows.len(),
            fit_open_event_count: self.fit_open_event_count,
            fit_open_exposure_seconds: self.fit_open_exposure_seconds,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CandidateProgress {
    label: String,
}

struct CandidateSearchState<'a, 'b> {
    completed_candidate_count: usize,
    total_candidate_count: usize,
    best_candidate: Option<CandidateFit>,
    progress_observer: &'a mut Option<&'b mut dyn FnMut(FourierBusinessTimeClockFitProgress)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_rate_domain_validation_rejects_overflowing_envelope() {
        let mut orders = FourierHarmonicOrders::zeros();

        orders.set_order_for_axis(FourierBusinessTimeClockAxis::Intraday, 1);

        let candidate = CandidateOrder {
            orders,
            parameter_count: orders
                .parameter_count()
                .expect("test candidate parameter count should fit"),
        };

        assert_eq!(
            candidate_log_rate_envelope_fits_representable_positive_f64_domain(
                candidate,
                &[0.0, 1.0, 0.0],
                1.0,
            ),
            Some(())
        );
        assert_eq!(
            candidate_log_rate_envelope_fits_representable_positive_f64_domain(
                candidate,
                &[f64::MAX.ln(), f64::MAX.ln(), 0.0],
                1.0,
            ),
            None
        );
    }
}
