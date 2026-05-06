use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Serialize;

use super::calendar_buckets::{
    DayOfWeekSeasonalityEstimateOutput, DayOfYearSeasonalityEstimateOutput,
};
use super::composition::DeterministicSeasonalityCompositionOutput;
use super::intraday::IntradaySeasonalityEstimateOutput;
use super::{
    INPUT_SNAPSHOT_FINGERPRINT_ALGORITHM, SEASONALITY_DIAGNOSTICS_BATCH_REPORT_SCHEMA_VERSION,
    SEASONALITY_DIAGNOSTICS_REPORT_SCHEMA_VERSION,
};

#[derive(Debug, Clone, Copy)]
pub struct SeasonalityDiagnosticsReportInput<'a> {
    pub dataset_id: &'a str,
    pub input_schema_version: &'a str,
    pub preprocessed_schema_version: &'a str,
    pub exchange_code: &'a str,
    pub exchange_timezone_name: &'a str,
    pub source_record_positions: &'a [u64],
    pub timestamp_utc_nanos: &'a [i64],
    pub exchange_local_minute_of_day: &'a [u16],
    pub exchange_local_day_of_week: &'a [u8],
    pub exchange_local_day_of_year: &'a [u16],
    pub market_state_codes: &'a [u8],
    pub intraday_estimate: &'a IntradaySeasonalityEstimateOutput,
    pub day_of_week_estimate: &'a DayOfWeekSeasonalityEstimateOutput,
    pub day_of_year_estimate: &'a DayOfYearSeasonalityEstimateOutput,
    pub composition: &'a DeterministicSeasonalityCompositionOutput,
    pub estimator_version: &'a str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityComponentDiagnosticsSummary {
    pub component_name: &'static str,
    pub component_schema_version: String,
    pub input_event_count: usize,
    pub bucket_count: usize,
    pub zero_bucket_count: usize,
    pub sparse_bucket_count: usize,
    pub fallback_bucket_count: usize,
    pub zero_bucket_ratio: f64,
    pub sparse_bucket_ratio: f64,
    pub fallback_bucket_ratio: f64,
    pub mean_factor: f64,
    pub min_factor: f64,
    pub max_factor: f64,
    pub mean_absolute_factor_error: f64,
    pub root_mean_squared_factor_error: f64,
}

#[derive(Debug, Clone, Copy)]
struct SeasonalityComponentSummaryInput<'a> {
    component_name: &'static str,
    component_schema_version: &'a str,
    component_input_event_count: usize,
    expected_input_event_count: usize,
    event_count_by_bucket: &'a [u64],
    factor_by_bucket: &'a [f64],
    sparse_bucket_count: usize,
    fallback_bucket_count: usize,
    pseudocount: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityOverlayComponentDiagnosticsSummary {
    pub configured_overlay_count: usize,
    pub enabled_overlay_count: usize,
    pub active_overlay_event_count: usize,
    pub active_overlay_event_ratio: f64,
    pub active_event_mean_log_adjustment: f64,
    pub active_event_min_log_adjustment: f64,
    pub active_event_max_log_adjustment: f64,
    pub precedence_policy: String,
    pub merge_policy: String,
    pub window_boundary_policy: String,
    pub configured_overlay_ids_in_order: Vec<String>,
    pub applied_overlay_ids_in_order: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityFitQualityMetrics {
    pub weighted_mean_absolute_factor_error: f64,
    pub weighted_root_mean_squared_factor_error: f64,
    pub normalization_anchor_error: f64,
    pub combined_factor_mean: f64,
    pub combined_factor_standard_deviation: f64,
    pub combined_factor_min: f64,
    pub combined_factor_max: f64,
    pub overlay_active_event_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityInputSnapshotMetadata {
    pub input_schema_version: String,
    pub preprocessed_schema_version: String,
    pub exchange_code: String,
    pub exchange_timezone_name: String,
    pub input_event_count: usize,
    pub first_event_timestamp_utc_nanos: i64,
    pub last_event_timestamp_utc_nanos: i64,
    pub snapshot_fingerprint_algorithm: &'static str,
    pub snapshot_fingerprint_hex: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityEstimatorConfigurationMetadata {
    pub intraday_schema_version: String,
    pub day_of_week_schema_version: String,
    pub day_of_year_schema_version: String,
    pub composition_schema_version: String,
    pub intraday_minutes_per_bin: u16,
    pub intraday_smoothing_kernel: String,
    pub intraday_smoothing_bandwidth_bins: f64,
    pub intraday_minimum_bin_events: usize,
    pub intraday_pseudocount: f64,
    pub day_of_week_minimum_bucket_events: usize,
    pub day_of_week_pseudocount: f64,
    pub day_of_year_minimum_bucket_events: usize,
    pub day_of_year_pseudocount: f64,
    pub normalization_policy: String,
    pub overlay_precedence_policy: String,
    pub overlay_merge_policy: String,
    pub overlay_window_boundary_policy: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityReproducibilityMetadata {
    pub estimator_version: String,
    pub input_snapshot: SeasonalityInputSnapshotMetadata,
    pub estimator_configuration: SeasonalityEstimatorConfigurationMetadata,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityDiagnosticsReportOutput {
    pub schema_version: &'static str,
    pub dataset_id: String,
    pub intraday_component_summary: SeasonalityComponentDiagnosticsSummary,
    pub day_of_week_component_summary: SeasonalityComponentDiagnosticsSummary,
    pub day_of_year_component_summary: SeasonalityComponentDiagnosticsSummary,
    pub overlay_component_summary: Option<SeasonalityOverlayComponentDiagnosticsSummary>,
    pub fit_quality_metrics: SeasonalityFitQualityMetrics,
    pub reproducibility_metadata: SeasonalityReproducibilityMetadata,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityDiagnosticsBatchReportOutput {
    pub schema_version: &'static str,
    pub report_count: usize,
    pub dataset_ids_in_order: Vec<String>,
    pub reports: Vec<SeasonalityDiagnosticsReportOutput>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SeasonalityDiagnosticsReportError {
    EmptyDatasetId,
    EmptyEstimatorVersion,
    EmptyInputSchemaVersion,
    EmptyPreprocessedSchemaVersion,
    EmptyExchangeCode,
    EmptyExchangeTimezoneName,
    EmptyInputEvents,
    MismatchedInputLengths {
        timestamp_utc_nanos_count: usize,
        source_record_position_count: usize,
        minute_of_day_count: usize,
        day_of_week_count: usize,
        day_of_year_count: usize,
        market_state_code_count: usize,
        combined_log_component_count: usize,
        combined_factor_count: usize,
        special_log_component_count: usize,
        special_overlay_flag_count: usize,
    },
    InputEventCountMismatch {
        component_name: &'static str,
        expected_input_event_count: usize,
        actual_input_event_count: usize,
    },
    ComponentLengthMismatch {
        component_name: &'static str,
        expected_bucket_count: usize,
        actual_bucket_count: usize,
    },
    InvalidComponentPseudocount {
        component_name: &'static str,
        pseudocount: f64,
    },
    InvalidComponentFactor {
        component_name: &'static str,
        bucket_position: usize,
    },
    InvalidCombinedFactor {
        record_position: usize,
    },
    InvalidCombinedLogComponent {
        record_position: usize,
    },
    InvalidSpecialOverlayFlag {
        record_position: usize,
        overlay_active_flag: u8,
    },
    EmptyBatchInput,
    DuplicateDatasetId {
        dataset_id: String,
    },
}

impl Display for SeasonalityDiagnosticsReportError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyDatasetId => write!(
                formatter,
                "seasonality diagnostics report requires a non-empty dataset_id"
            ),
            Self::EmptyEstimatorVersion => write!(
                formatter,
                "seasonality diagnostics report requires a non-empty estimator_version"
            ),
            Self::EmptyInputSchemaVersion => write!(
                formatter,
                "seasonality diagnostics report requires a non-empty input_schema_version"
            ),
            Self::EmptyPreprocessedSchemaVersion => write!(
                formatter,
                "seasonality diagnostics report requires a non-empty preprocessed_schema_version"
            ),
            Self::EmptyExchangeCode => write!(
                formatter,
                "seasonality diagnostics report requires a non-empty exchange_code"
            ),
            Self::EmptyExchangeTimezoneName => write!(
                formatter,
                "seasonality diagnostics report requires a non-empty exchange_timezone_name"
            ),
            Self::EmptyInputEvents => write!(
                formatter,
                "seasonality diagnostics report requires at least one accepted input event"
            ),
            Self::MismatchedInputLengths {
                timestamp_utc_nanos_count,
                source_record_position_count,
                minute_of_day_count,
                day_of_week_count,
                day_of_year_count,
                market_state_code_count,
                combined_log_component_count,
                combined_factor_count,
                special_log_component_count,
                special_overlay_flag_count,
            } => write!(
                formatter,
                "seasonality diagnostics report inputs must have matching lengths, received timestamp_utc_nanos={timestamp_utc_nanos_count}, source_record_positions={source_record_position_count}, minute_of_day={minute_of_day_count}, day_of_week={day_of_week_count}, day_of_year={day_of_year_count}, market_state_codes={market_state_code_count}, combined_log_component={combined_log_component_count}, combined_factor={combined_factor_count}, special_log_component={special_log_component_count}, special_overlay_flags={special_overlay_flag_count}"
            ),
            Self::InputEventCountMismatch {
                component_name,
                expected_input_event_count,
                actual_input_event_count,
            } => write!(
                formatter,
                "seasonality diagnostics report component '{component_name}' input_event_count mismatch, expected {expected_input_event_count} and received {actual_input_event_count}"
            ),
            Self::ComponentLengthMismatch {
                component_name,
                expected_bucket_count,
                actual_bucket_count,
            } => write!(
                formatter,
                "seasonality diagnostics report component '{component_name}' length mismatch, expected {expected_bucket_count} buckets and received {actual_bucket_count}"
            ),
            Self::InvalidComponentPseudocount {
                component_name,
                pseudocount,
            } => write!(
                formatter,
                "seasonality diagnostics report component '{component_name}' has invalid pseudocount value {pseudocount}"
            ),
            Self::InvalidComponentFactor {
                component_name,
                bucket_position,
            } => write!(
                formatter,
                "seasonality diagnostics report component '{component_name}' has non-finite or non-positive factor at bucket position {bucket_position}"
            ),
            Self::InvalidCombinedFactor { record_position } => write!(
                formatter,
                "seasonality diagnostics report received non-finite or non-positive combined factor at record position {record_position}"
            ),
            Self::InvalidCombinedLogComponent { record_position } => write!(
                formatter,
                "seasonality diagnostics report received non-finite combined log component at record position {record_position}"
            ),
            Self::InvalidSpecialOverlayFlag {
                record_position,
                overlay_active_flag,
            } => write!(
                formatter,
                "seasonality diagnostics report received invalid special overlay active flag {overlay_active_flag} at record position {record_position}"
            ),
            Self::EmptyBatchInput => write!(
                formatter,
                "seasonality diagnostics batch report requires at least one dataset input"
            ),
            Self::DuplicateDatasetId { dataset_id } => write!(
                formatter,
                "seasonality diagnostics batch report received duplicate dataset id '{dataset_id}'"
            ),
        }
    }
}

impl Error for SeasonalityDiagnosticsReportError {}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ComponentFitErrorMetrics {
    mean_absolute_factor_error: f64,
    root_mean_squared_factor_error: f64,
}

pub fn generate_seasonality_diagnostics_report(
    report_input: SeasonalityDiagnosticsReportInput<'_>,
) -> Result<SeasonalityDiagnosticsReportOutput, SeasonalityDiagnosticsReportError> {
    if report_input.dataset_id.trim().is_empty() {
        return Err(SeasonalityDiagnosticsReportError::EmptyDatasetId);
    }
    if report_input.estimator_version.trim().is_empty() {
        return Err(SeasonalityDiagnosticsReportError::EmptyEstimatorVersion);
    }
    if report_input.input_schema_version.trim().is_empty() {
        return Err(SeasonalityDiagnosticsReportError::EmptyInputSchemaVersion);
    }
    if report_input.preprocessed_schema_version.trim().is_empty() {
        return Err(SeasonalityDiagnosticsReportError::EmptyPreprocessedSchemaVersion);
    }
    if report_input.exchange_code.trim().is_empty() {
        return Err(SeasonalityDiagnosticsReportError::EmptyExchangeCode);
    }
    if report_input.exchange_timezone_name.trim().is_empty() {
        return Err(SeasonalityDiagnosticsReportError::EmptyExchangeTimezoneName);
    }
    if report_input.timestamp_utc_nanos.is_empty() {
        return Err(SeasonalityDiagnosticsReportError::EmptyInputEvents);
    }

    let input_event_count = report_input.timestamp_utc_nanos.len();
    let composed_input_event_count = report_input.composition.diagnostics.input_event_count;

    if report_input.source_record_positions.len() != input_event_count
        || report_input.exchange_local_minute_of_day.len() != input_event_count
        || report_input.exchange_local_day_of_week.len() != input_event_count
        || report_input.exchange_local_day_of_year.len() != input_event_count
        || report_input.market_state_codes.len() != input_event_count
        || report_input
            .composition
            .combined_log_component_by_event
            .len()
            != input_event_count
        || report_input.composition.combined_factor_by_event.len() != input_event_count
        || report_input
            .composition
            .special_log_component_by_event
            .len()
            != input_event_count
        || report_input
            .composition
            .special_overlay_active_flags_by_event
            .len()
            != input_event_count
        || composed_input_event_count != input_event_count
    {
        return Err(SeasonalityDiagnosticsReportError::MismatchedInputLengths {
            timestamp_utc_nanos_count: input_event_count,
            source_record_position_count: report_input.source_record_positions.len(),
            minute_of_day_count: report_input.exchange_local_minute_of_day.len(),
            day_of_week_count: report_input.exchange_local_day_of_week.len(),
            day_of_year_count: report_input.exchange_local_day_of_year.len(),
            market_state_code_count: report_input.market_state_codes.len(),
            combined_log_component_count: report_input
                .composition
                .combined_log_component_by_event
                .len(),
            combined_factor_count: report_input.composition.combined_factor_by_event.len(),
            special_log_component_count: report_input
                .composition
                .special_log_component_by_event
                .len(),
            special_overlay_flag_count: report_input
                .composition
                .special_overlay_active_flags_by_event
                .len(),
        });
    }

    let intraday_summary = build_component_summary(SeasonalityComponentSummaryInput {
        component_name: "intraday",
        component_schema_version: report_input.intraday_estimate.schema_version,
        component_input_event_count: report_input.intraday_estimate.diagnostics.input_event_count,
        expected_input_event_count: input_event_count,
        event_count_by_bucket: &report_input.intraday_estimate.event_count_by_bin,
        factor_by_bucket: &report_input.intraday_estimate.intraday_factor_by_bin,
        sparse_bucket_count: report_input.intraday_estimate.diagnostics.sparse_bin_count,
        fallback_bucket_count: 0,
        pseudocount: report_input.intraday_estimate.pseudocount,
    })?;

    let day_of_week_summary = build_component_summary(SeasonalityComponentSummaryInput {
        component_name: "day_of_week",
        component_schema_version: report_input.day_of_week_estimate.schema_version,
        component_input_event_count: report_input
            .day_of_week_estimate
            .diagnostics
            .input_event_count,
        expected_input_event_count: input_event_count,
        event_count_by_bucket: &report_input.day_of_week_estimate.event_count_by_day_of_week,
        factor_by_bucket: &report_input.day_of_week_estimate.day_of_week_factor,
        sparse_bucket_count: report_input
            .day_of_week_estimate
            .diagnostics
            .sparse_bucket_count,
        fallback_bucket_count: report_input
            .day_of_week_estimate
            .diagnostics
            .fallback_bucket_count,
        pseudocount: report_input.day_of_week_estimate.pseudocount,
    })?;

    let day_of_year_summary = build_component_summary(SeasonalityComponentSummaryInput {
        component_name: "day_of_year",
        component_schema_version: report_input.day_of_year_estimate.schema_version,
        component_input_event_count: report_input
            .day_of_year_estimate
            .diagnostics
            .input_event_count,
        expected_input_event_count: input_event_count,
        event_count_by_bucket: &report_input.day_of_year_estimate.event_count_by_day_of_year,
        factor_by_bucket: &report_input.day_of_year_estimate.day_of_year_factor,
        sparse_bucket_count: report_input
            .day_of_year_estimate
            .diagnostics
            .sparse_bucket_count,
        fallback_bucket_count: report_input
            .day_of_year_estimate
            .diagnostics
            .fallback_bucket_count,
        pseudocount: report_input.day_of_year_estimate.pseudocount,
    })?;

    let overlay_summary =
        build_overlay_component_summary(input_event_count, report_input.composition)?;

    let fit_quality_metrics = build_fit_quality_metrics(
        report_input.composition,
        &intraday_summary,
        &day_of_week_summary,
        &day_of_year_summary,
        overlay_summary.as_ref(),
    )?;

    let reproducibility_metadata = build_reproducibility_metadata(&report_input);

    Ok(SeasonalityDiagnosticsReportOutput {
        schema_version: SEASONALITY_DIAGNOSTICS_REPORT_SCHEMA_VERSION,
        dataset_id: report_input.dataset_id.to_string(),
        intraday_component_summary: intraday_summary,
        day_of_week_component_summary: day_of_week_summary,
        day_of_year_component_summary: day_of_year_summary,
        overlay_component_summary: overlay_summary,
        fit_quality_metrics,
        reproducibility_metadata,
    })
}

pub fn generate_seasonality_diagnostics_batch_report(
    report_inputs: &[SeasonalityDiagnosticsReportInput<'_>],
) -> Result<SeasonalityDiagnosticsBatchReportOutput, SeasonalityDiagnosticsReportError> {
    if report_inputs.is_empty() {
        return Err(SeasonalityDiagnosticsReportError::EmptyBatchInput);
    }

    let mut seen_dataset_ids: BTreeSet<String> = BTreeSet::new();
    let mut dataset_ids_in_order: Vec<String> = Vec::with_capacity(report_inputs.len());
    let mut reports: Vec<SeasonalityDiagnosticsReportOutput> =
        Vec::with_capacity(report_inputs.len());

    for report_input in report_inputs {
        if !seen_dataset_ids.insert(report_input.dataset_id.to_string()) {
            return Err(SeasonalityDiagnosticsReportError::DuplicateDatasetId {
                dataset_id: report_input.dataset_id.to_string(),
            });
        }

        let report = generate_seasonality_diagnostics_report(*report_input)?;

        dataset_ids_in_order.push(report.dataset_id.clone());
        reports.push(report);
    }

    Ok(SeasonalityDiagnosticsBatchReportOutput {
        schema_version: SEASONALITY_DIAGNOSTICS_BATCH_REPORT_SCHEMA_VERSION,
        report_count: reports.len(),
        dataset_ids_in_order,
        reports,
    })
}

fn build_component_summary(
    input: SeasonalityComponentSummaryInput<'_>,
) -> Result<SeasonalityComponentDiagnosticsSummary, SeasonalityDiagnosticsReportError> {
    let SeasonalityComponentSummaryInput {
        component_name,
        component_schema_version,
        component_input_event_count,
        expected_input_event_count,
        event_count_by_bucket,
        factor_by_bucket,
        sparse_bucket_count,
        fallback_bucket_count,
        pseudocount,
    } = input;

    if component_input_event_count != expected_input_event_count {
        return Err(SeasonalityDiagnosticsReportError::InputEventCountMismatch {
            component_name,
            expected_input_event_count,
            actual_input_event_count: component_input_event_count,
        });
    }

    if event_count_by_bucket.len() != factor_by_bucket.len() {
        return Err(SeasonalityDiagnosticsReportError::ComponentLengthMismatch {
            component_name,
            expected_bucket_count: event_count_by_bucket.len(),
            actual_bucket_count: factor_by_bucket.len(),
        });
    }

    if !pseudocount.is_finite() || pseudocount <= 0.0 {
        return Err(
            SeasonalityDiagnosticsReportError::InvalidComponentPseudocount {
                component_name,
                pseudocount,
            },
        );
    }

    let bucket_count = event_count_by_bucket.len();
    if bucket_count == 0 {
        return Err(SeasonalityDiagnosticsReportError::ComponentLengthMismatch {
            component_name,
            expected_bucket_count: 1,
            actual_bucket_count: 0,
        });
    }

    let observed_factor_by_bucket =
        build_observed_factor_by_bucket(event_count_by_bucket, pseudocount, bucket_count);

    let fit_error_metrics = calculate_component_fit_error_metrics(
        component_name,
        factor_by_bucket,
        &observed_factor_by_bucket,
    )?;

    let zero_bucket_count = event_count_by_bucket
        .iter()
        .copied()
        .filter(|event_count| *event_count == 0)
        .count();

    let bucket_count_float = bucket_count as f64;

    let mean_factor = factor_by_bucket.iter().sum::<f64>() / bucket_count_float;

    let min_factor = factor_by_bucket
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);

    let max_factor = factor_by_bucket
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);

    Ok(SeasonalityComponentDiagnosticsSummary {
        component_name,
        component_schema_version: component_schema_version.to_string(),
        input_event_count: component_input_event_count,
        bucket_count,
        zero_bucket_count,
        sparse_bucket_count,
        fallback_bucket_count,
        zero_bucket_ratio: zero_bucket_count as f64 / bucket_count_float,
        sparse_bucket_ratio: sparse_bucket_count as f64 / bucket_count_float,
        fallback_bucket_ratio: fallback_bucket_count as f64 / bucket_count_float,
        mean_factor,
        min_factor,
        max_factor,
        mean_absolute_factor_error: fit_error_metrics.mean_absolute_factor_error,
        root_mean_squared_factor_error: fit_error_metrics.root_mean_squared_factor_error,
    })
}

fn build_observed_factor_by_bucket(
    event_count_by_bucket: &[u64],
    pseudocount: f64,
    bucket_count: usize,
) -> Vec<f64> {
    let adjusted_event_count_by_bucket: Vec<f64> = event_count_by_bucket
        .iter()
        .copied()
        .map(|event_count| event_count as f64 + pseudocount)
        .collect();

    let adjusted_event_count_total: f64 = adjusted_event_count_by_bucket.iter().sum();

    let bucket_count_float = bucket_count as f64;

    adjusted_event_count_by_bucket
        .iter()
        .map(|adjusted_event_count| *adjusted_event_count / adjusted_event_count_total)
        .map(|density| density * bucket_count_float)
        .collect()
}

fn calculate_component_fit_error_metrics(
    component_name: &'static str,
    factor_by_bucket: &[f64],
    observed_factor_by_bucket: &[f64],
) -> Result<ComponentFitErrorMetrics, SeasonalityDiagnosticsReportError> {
    let mut absolute_error_sum = 0.0;
    let mut squared_error_sum = 0.0;

    let bucket_count = factor_by_bucket.len();
    for bucket_position in 0..bucket_count {
        let factor = factor_by_bucket[bucket_position];
        let observed_factor = observed_factor_by_bucket[bucket_position];

        if !factor.is_finite()
            || factor <= 0.0
            || !observed_factor.is_finite()
            || observed_factor <= 0.0
        {
            return Err(SeasonalityDiagnosticsReportError::InvalidComponentFactor {
                component_name,
                bucket_position,
            });
        }

        let error = factor - observed_factor;

        absolute_error_sum += error.abs();
        squared_error_sum += error.powi(2);
    }

    let bucket_count_float = bucket_count as f64;

    Ok(ComponentFitErrorMetrics {
        mean_absolute_factor_error: absolute_error_sum / bucket_count_float,
        root_mean_squared_factor_error: (squared_error_sum / bucket_count_float).sqrt(),
    })
}

fn build_overlay_component_summary(
    input_event_count: usize,
    composition_output: &DeterministicSeasonalityCompositionOutput,
) -> Result<Option<SeasonalityOverlayComponentDiagnosticsSummary>, SeasonalityDiagnosticsReportError>
{
    let overlay_metadata = &composition_output.diagnostics.overlay_provenance_metadata;
    if overlay_metadata.configured_overlay_count == 0 {
        return Ok(None);
    }

    let mut active_overlay_log_adjustments: Vec<f64> = Vec::new();

    for (record_position, overlay_active_flag) in composition_output
        .special_overlay_active_flags_by_event
        .iter()
        .copied()
        .enumerate()
    {
        match overlay_active_flag {
            0 => {}
            1 => {
                active_overlay_log_adjustments
                    .push(composition_output.special_log_component_by_event[record_position]);
            }
            _ => {
                return Err(
                    SeasonalityDiagnosticsReportError::InvalidSpecialOverlayFlag {
                        record_position,
                        overlay_active_flag,
                    },
                );
            }
        }
    }

    let (
        active_event_mean_log_adjustment,
        active_event_min_log_adjustment,
        active_event_max_log_adjustment,
    ) = if active_overlay_log_adjustments.is_empty() {
        (0.0, 0.0, 0.0)
    } else {
        let event_count_float = active_overlay_log_adjustments.len() as f64;
        let mean_value = active_overlay_log_adjustments.iter().sum::<f64>() / event_count_float;

        let min_value = active_overlay_log_adjustments
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);

        let max_value = active_overlay_log_adjustments
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);

        (mean_value, min_value, max_value)
    };

    Ok(Some(SeasonalityOverlayComponentDiagnosticsSummary {
        configured_overlay_count: overlay_metadata.configured_overlay_count,
        enabled_overlay_count: overlay_metadata.enabled_overlay_count,
        active_overlay_event_count: overlay_metadata.active_overlay_event_count,
        active_overlay_event_ratio: overlay_metadata.active_overlay_event_count as f64
            / input_event_count as f64,
        active_event_mean_log_adjustment,
        active_event_min_log_adjustment,
        active_event_max_log_adjustment,
        precedence_policy: overlay_metadata.precedence_policy.to_string(),
        merge_policy: overlay_metadata.merge_policy.to_string(),
        window_boundary_policy: overlay_metadata.window_boundary_policy.to_string(),
        configured_overlay_ids_in_order: overlay_metadata.configured_overlay_ids_in_order.clone(),
        applied_overlay_ids_in_order: overlay_metadata.applied_overlay_ids_in_order.clone(),
    }))
}

fn build_fit_quality_metrics(
    composition_output: &DeterministicSeasonalityCompositionOutput,
    intraday_summary: &SeasonalityComponentDiagnosticsSummary,
    day_of_week_summary: &SeasonalityComponentDiagnosticsSummary,
    day_of_year_summary: &SeasonalityComponentDiagnosticsSummary,
    overlay_summary: Option<&SeasonalityOverlayComponentDiagnosticsSummary>,
) -> Result<SeasonalityFitQualityMetrics, SeasonalityDiagnosticsReportError> {
    let component_summaries = [intraday_summary, day_of_week_summary, day_of_year_summary];

    let total_bucket_count: usize = component_summaries
        .iter()
        .map(|summary| summary.bucket_count)
        .sum();

    let total_bucket_count_float = total_bucket_count as f64;

    let weighted_mean_absolute_factor_error = component_summaries
        .iter()
        .map(|summary| summary.mean_absolute_factor_error * summary.bucket_count as f64)
        .sum::<f64>()
        / total_bucket_count_float;

    let weighted_root_mean_squared_factor_error = component_summaries
        .iter()
        .map(|summary| summary.root_mean_squared_factor_error * summary.bucket_count as f64)
        .sum::<f64>()
        / total_bucket_count_float;

    let mut combined_factor_sum = 0.0;
    let mut combined_factor_min = f64::INFINITY;
    let mut combined_factor_max = f64::NEG_INFINITY;

    let mut combined_log_component_sum = 0.0;

    for record_position in 0..composition_output.combined_factor_by_event.len() {
        let combined_factor = composition_output.combined_factor_by_event[record_position];

        let combined_log_component =
            composition_output.combined_log_component_by_event[record_position];

        if !combined_factor.is_finite() || combined_factor <= 0.0 {
            return Err(SeasonalityDiagnosticsReportError::InvalidCombinedFactor {
                record_position,
            });
        }

        if !combined_log_component.is_finite() {
            return Err(
                SeasonalityDiagnosticsReportError::InvalidCombinedLogComponent { record_position },
            );
        }

        combined_factor_sum += combined_factor;
        combined_log_component_sum += combined_log_component;

        combined_factor_min = combined_factor_min.min(combined_factor);
        combined_factor_max = combined_factor_max.max(combined_factor);
    }

    let input_event_count = composition_output.combined_factor_by_event.len();
    let input_event_count_float = input_event_count as f64;

    let combined_factor_mean = combined_factor_sum / input_event_count_float;
    let _combined_log_component_mean = combined_log_component_sum / input_event_count_float;

    let combined_factor_variance = composition_output
        .combined_factor_by_event
        .iter()
        .copied()
        .map(|combined_factor| (combined_factor - combined_factor_mean).powi(2))
        .sum::<f64>()
        / input_event_count_float;

    let overlay_active_event_ratio = match overlay_summary {
        Some(summary) => summary.active_overlay_event_ratio,
        None => 0.0,
    };

    let normalization_metadata = &composition_output.diagnostics.normalization_metadata;

    let normalization_anchor_error = (normalization_metadata.post_normalization_mean_factor
        - normalization_metadata.target_mean_factor)
        .abs();

    Ok(SeasonalityFitQualityMetrics {
        weighted_mean_absolute_factor_error,
        weighted_root_mean_squared_factor_error,
        normalization_anchor_error,
        combined_factor_mean,
        combined_factor_standard_deviation: combined_factor_variance.sqrt(),
        combined_factor_min,
        combined_factor_max,
        overlay_active_event_ratio,
    })
}

fn build_reproducibility_metadata(
    report_input: &SeasonalityDiagnosticsReportInput<'_>,
) -> SeasonalityReproducibilityMetadata {
    let input_snapshot_fingerprint_hex = build_input_snapshot_fingerprint(report_input);

    let first_event_timestamp_utc_nanos = report_input.timestamp_utc_nanos[0];
    let last_event_timestamp_utc_nanos =
        report_input.timestamp_utc_nanos[report_input.timestamp_utc_nanos.len() - 1];

    let overlay_metadata = &report_input
        .composition
        .diagnostics
        .overlay_provenance_metadata;

    let normalization_metadata = &report_input.composition.diagnostics.normalization_metadata;

    SeasonalityReproducibilityMetadata {
        estimator_version: report_input.estimator_version.to_string(),
        input_snapshot: SeasonalityInputSnapshotMetadata {
            input_schema_version: report_input.input_schema_version.to_string(),
            preprocessed_schema_version: report_input.preprocessed_schema_version.to_string(),
            exchange_code: report_input.exchange_code.to_string(),
            exchange_timezone_name: report_input.exchange_timezone_name.to_string(),
            input_event_count: report_input.timestamp_utc_nanos.len(),
            first_event_timestamp_utc_nanos,
            last_event_timestamp_utc_nanos,
            snapshot_fingerprint_algorithm: INPUT_SNAPSHOT_FINGERPRINT_ALGORITHM,
            snapshot_fingerprint_hex: input_snapshot_fingerprint_hex,
        },
        estimator_configuration: SeasonalityEstimatorConfigurationMetadata {
            intraday_schema_version: report_input.intraday_estimate.schema_version.to_string(),
            day_of_week_schema_version: report_input
                .day_of_week_estimate
                .schema_version
                .to_string(),
            day_of_year_schema_version: report_input
                .day_of_year_estimate
                .schema_version
                .to_string(),
            composition_schema_version: report_input.composition.schema_version.to_string(),
            intraday_minutes_per_bin: report_input.intraday_estimate.minutes_per_bin,
            intraday_smoothing_kernel: report_input
                .intraday_estimate
                .smoothing_kernel
                .as_str()
                .to_string(),
            intraday_smoothing_bandwidth_bins: report_input
                .intraday_estimate
                .smoothing_bandwidth_bins,
            intraday_minimum_bin_events: report_input.intraday_estimate.minimum_bin_events,
            intraday_pseudocount: report_input.intraday_estimate.pseudocount,
            day_of_week_minimum_bucket_events: report_input
                .day_of_week_estimate
                .minimum_bucket_events,
            day_of_week_pseudocount: report_input.day_of_week_estimate.pseudocount,
            day_of_year_minimum_bucket_events: report_input
                .day_of_year_estimate
                .minimum_bucket_events,
            day_of_year_pseudocount: report_input.day_of_year_estimate.pseudocount,
            normalization_policy: normalization_metadata.normalization_policy.to_string(),
            overlay_precedence_policy: overlay_metadata.precedence_policy.to_string(),
            overlay_merge_policy: overlay_metadata.merge_policy.to_string(),
            overlay_window_boundary_policy: overlay_metadata.window_boundary_policy.to_string(),
        },
    }
}

fn build_input_snapshot_fingerprint(
    report_input: &SeasonalityDiagnosticsReportInput<'_>,
) -> String {
    let mut hash = 14_695_981_039_346_656_037_u64;

    hash_with_text(&mut hash, report_input.dataset_id);
    hash_with_text(&mut hash, report_input.input_schema_version);
    hash_with_text(&mut hash, report_input.preprocessed_schema_version);
    hash_with_text(&mut hash, report_input.exchange_code);
    hash_with_text(&mut hash, report_input.exchange_timezone_name);

    for source_record_position in report_input.source_record_positions {
        hash_with_u64(&mut hash, *source_record_position);
    }

    for timestamp_utc_nanos in report_input.timestamp_utc_nanos {
        hash_with_i64(&mut hash, *timestamp_utc_nanos);
    }

    for minute_of_day in report_input.exchange_local_minute_of_day {
        hash_with_u16(&mut hash, *minute_of_day);
    }

    for day_of_week in report_input.exchange_local_day_of_week {
        hash_with_u8(&mut hash, *day_of_week);
    }

    for day_of_year in report_input.exchange_local_day_of_year {
        hash_with_u16(&mut hash, *day_of_year);
    }

    for market_state_code in report_input.market_state_codes {
        hash_with_u8(&mut hash, *market_state_code);
    }

    format!("{hash:016x}")
}

fn hash_with_text(hash: &mut u64, text_value: &str) {
    hash_with_u64(hash, text_value.len() as u64);
    hash_with_bytes(hash, text_value.as_bytes());
}

fn hash_with_u8(hash: &mut u64, value: u8) {
    hash_with_bytes(hash, &[value]);
}

fn hash_with_u16(hash: &mut u64, value: u16) {
    hash_with_bytes(hash, &value.to_le_bytes());
}

fn hash_with_u64(hash: &mut u64, value: u64) {
    hash_with_bytes(hash, &value.to_le_bytes());
}

fn hash_with_i64(hash: &mut u64, value: i64) {
    hash_with_bytes(hash, &value.to_le_bytes());
}

fn hash_with_bytes(hash: &mut u64, raw_bytes: &[u8]) {
    const FNV_PRIME: u64 = 1_099_511_628_211;
    for raw_byte in raw_bytes {
        *hash ^= u64::from(*raw_byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}
