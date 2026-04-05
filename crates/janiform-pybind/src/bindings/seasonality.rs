use janiform::seasonality::{
    DayOfWeekSeasonalityEstimationError, DayOfWeekSeasonalityEstimatorSettings,
    DayOfYearSeasonalityEstimationError, DayOfYearSeasonalityEstimatorSettings,
    DeterministicSeasonalityCompositionError, IntradaySeasonalityEstimationError,
    IntradaySeasonalityEstimatorSettings, IntradaySmoothingKernel, ScheduledEventOverlayDefinition,
    SeasonalityDiagnosticsReportError, SeasonalityDiagnosticsReportInput,
    compose_deterministic_seasonality_signal as compose_deterministic_seasonality_signal_core,
    estimate_day_of_week_seasonality as estimate_day_of_week_seasonality_core,
    estimate_day_of_year_seasonality as estimate_day_of_year_seasonality_core,
    estimate_intraday_seasonality as estimate_intraday_seasonality_core,
    generate_seasonality_diagnostics_report as generate_seasonality_diagnostics_report_core,
};
use pyo3::prelude::{Bound, PyErr, PyResult, Python};
use pyo3::pyfunction;
use pyo3::types::{PyAny, PyAnyMethods, PyMapping, PyMappingMethods, PyModule, PyModuleMethods};
use serde_json::to_string as serialize_to_json_string;

use super::calendar::parse_timestamp_utc_from_python;
use crate::SeasonalityInputValidationError;
use crate::types::{
    PyDayOfWeekSeasonalityEstimateResult, PyDayOfYearSeasonalityEstimateResult,
    PyDeterministicSeasonalityCompositionResult, PyIntradaySeasonalityEstimateResult,
    PyTimestampEventPreprocessingResult,
};

#[pyfunction]
#[pyo3(signature = (exchange_local_minute_of_day, *, minutes_per_bin, smoothing_kernel, smoothing_bandwidth_bins, minimum_bin_events, pseudocount))]
fn estimate_intraday_seasonality(
    py: Python<'_>,
    exchange_local_minute_of_day: &Bound<'_, PyAny>,
    minutes_per_bin: u16,
    smoothing_kernel: &str,
    smoothing_bandwidth_bins: f64,
    minimum_bin_events: usize,
    pseudocount: f64,
) -> PyResult<PyIntradaySeasonalityEstimateResult> {
    let minute_of_day_values: Vec<u16> = exchange_local_minute_of_day.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "exchange_local_minute_of_day must be an iterable of integer minute-of-day values",
        )
    })?;

    let parsed_smoothing_kernel = parse_intraday_smoothing_kernel(smoothing_kernel)?;

    let estimator_settings = IntradaySeasonalityEstimatorSettings {
        minutes_per_bin,
        smoothing_kernel: parsed_smoothing_kernel,
        smoothing_bandwidth_bins,
        minimum_bin_events,
        pseudocount,
    };

    let output = py
        .allow_threads(|| {
            estimate_intraday_seasonality_core(&minute_of_day_values, estimator_settings)
        })
        .map_err(map_intraday_estimation_error)?;

    Ok(PyIntradaySeasonalityEstimateResult { output })
}

#[pyfunction]
#[pyo3(signature = (exchange_local_day_of_week, *, minimum_bucket_events, pseudocount))]
fn estimate_day_of_week_seasonality(
    py: Python<'_>,
    exchange_local_day_of_week: &Bound<'_, PyAny>,
    minimum_bucket_events: usize,
    pseudocount: f64,
) -> PyResult<PyDayOfWeekSeasonalityEstimateResult> {
    let day_of_week_values: Vec<u8> = exchange_local_day_of_week.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "exchange_local_day_of_week must be an iterable of integer day-of-week values",
        )
    })?;

    let estimator_settings = DayOfWeekSeasonalityEstimatorSettings {
        minimum_bucket_events,
        pseudocount,
    };

    let output = py
        .allow_threads(|| {
            estimate_day_of_week_seasonality_core(&day_of_week_values, estimator_settings)
        })
        .map_err(map_day_of_week_estimation_error)?;

    Ok(PyDayOfWeekSeasonalityEstimateResult { output })
}

#[pyfunction]
#[pyo3(signature = (exchange_local_day_of_year, *, minimum_bucket_events, pseudocount))]
fn estimate_day_of_year_seasonality(
    py: Python<'_>,
    exchange_local_day_of_year: &Bound<'_, PyAny>,
    minimum_bucket_events: usize,
    pseudocount: f64,
) -> PyResult<PyDayOfYearSeasonalityEstimateResult> {
    let day_of_year_values: Vec<u16> = exchange_local_day_of_year.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "exchange_local_day_of_year must be an iterable of integer day-of-year values",
        )
    })?;

    let estimator_settings = DayOfYearSeasonalityEstimatorSettings {
        minimum_bucket_events,
        pseudocount,
    };

    let output = py
        .allow_threads(|| {
            estimate_day_of_year_seasonality_core(&day_of_year_values, estimator_settings)
        })
        .map_err(map_day_of_year_estimation_error)?;

    Ok(PyDayOfYearSeasonalityEstimateResult { output })
}

#[pyfunction]
#[pyo3(signature = (timestamp_utc_nanos, exchange_local_minute_of_day, exchange_local_day_of_week, exchange_local_day_of_year, intraday_log_component_by_bin, day_of_week_log_component, day_of_year_log_component, *, intraday_minutes_per_bin, scheduled_event_overlays = None))]
fn compose_deterministic_seasonality_signal(
    py: Python<'_>,
    timestamp_utc_nanos: &Bound<'_, PyAny>,
    exchange_local_minute_of_day: &Bound<'_, PyAny>,
    exchange_local_day_of_week: &Bound<'_, PyAny>,
    exchange_local_day_of_year: &Bound<'_, PyAny>,
    intraday_log_component_by_bin: &Bound<'_, PyAny>,
    day_of_week_log_component: &Bound<'_, PyAny>,
    day_of_year_log_component: &Bound<'_, PyAny>,
    intraday_minutes_per_bin: u16,
    scheduled_event_overlays: Option<&Bound<'_, PyAny>>,
) -> PyResult<PyDeterministicSeasonalityCompositionResult> {
    let timestamp_utc_nanos_values: Vec<i64> = timestamp_utc_nanos.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "timestamp_utc_nanos must be an iterable of integer UTC nanosecond values",
        )
    })?;

    let minute_of_day_values: Vec<u16> = exchange_local_minute_of_day.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "exchange_local_minute_of_day must be an iterable of integer minute-of-day values",
        )
    })?;

    let day_of_week_values: Vec<u8> = exchange_local_day_of_week.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "exchange_local_day_of_week must be an iterable of integer day-of-week values",
        )
    })?;

    let day_of_year_values: Vec<u16> = exchange_local_day_of_year.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "exchange_local_day_of_year must be an iterable of integer day-of-year values",
        )
    })?;

    let intraday_log_values: Vec<f64> = intraday_log_component_by_bin.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "intraday_log_component_by_bin must be an iterable of float values",
        )
    })?;

    let day_of_week_log_values: Vec<f64> = day_of_week_log_component.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "day_of_week_log_component must be an iterable of float values",
        )
    })?;

    let day_of_year_log_values: Vec<f64> = day_of_year_log_component.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "day_of_year_log_component must be an iterable of float values",
        )
    })?;

    let overlay_definitions = parse_scheduled_event_overlay_definitions(scheduled_event_overlays)?;

    let output = py
        .allow_threads(|| {
            compose_deterministic_seasonality_signal_core(
                &timestamp_utc_nanos_values,
                &minute_of_day_values,
                &day_of_week_values,
                &day_of_year_values,
                intraday_minutes_per_bin,
                &intraday_log_values,
                &day_of_week_log_values,
                &day_of_year_log_values,
                &overlay_definitions,
            )
        })
        .map_err(map_deterministic_composition_error)?;

    Ok(PyDeterministicSeasonalityCompositionResult { output })
}

#[pyfunction]
#[pyo3(signature = (dataset_id, preprocessed_events, intraday_estimate, day_of_week_estimate, day_of_year_estimate, composition, *, estimator_version))]
fn generate_seasonality_diagnostics_report(
    py: Python<'_>,
    dataset_id: &str,
    preprocessed_events: &PyTimestampEventPreprocessingResult,
    intraday_estimate: &PyIntradaySeasonalityEstimateResult,
    day_of_week_estimate: &PyDayOfWeekSeasonalityEstimateResult,
    day_of_year_estimate: &PyDayOfYearSeasonalityEstimateResult,
    composition: &PyDeterministicSeasonalityCompositionResult,
    estimator_version: &str,
) -> PyResult<String> {
    let report_output = py
        .allow_threads(|| {
            generate_seasonality_diagnostics_report_core(SeasonalityDiagnosticsReportInput {
                dataset_id,
                input_schema_version: preprocessed_events.output.input_schema_version,
                preprocessed_schema_version: preprocessed_events.output.schema_version,
                exchange_code: &preprocessed_events.output.exchange_code,
                exchange_timezone_name: &preprocessed_events.output.exchange_timezone_name,
                source_record_positions: &preprocessed_events.output.source_record_positions,
                timestamp_utc_nanos: &preprocessed_events.output.timestamp_utc_nanos,
                exchange_local_minute_of_day: &preprocessed_events
                    .output
                    .exchange_local_minute_of_day,
                exchange_local_day_of_week: &preprocessed_events.output.exchange_local_day_of_week,
                exchange_local_day_of_year: &preprocessed_events.output.exchange_local_day_of_year,
                market_state_codes: &preprocessed_events.output.market_state_codes,
                intraday_estimate: &intraday_estimate.output,
                day_of_week_estimate: &day_of_week_estimate.output,
                day_of_year_estimate: &day_of_year_estimate.output,
                composition: &composition.output,
                estimator_version,
            })
        })
        .map_err(map_seasonality_diagnostics_report_error)?;

    serialize_to_json_string(&report_output).map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "failed to serialize seasonality diagnostics report to json",
        )
    })
}

fn parse_scheduled_event_overlay_definitions(
    scheduled_event_overlays: Option<&Bound<'_, PyAny>>,
) -> PyResult<Vec<ScheduledEventOverlayDefinition>> {
    let Some(raw_scheduled_event_overlays) = scheduled_event_overlays else {
        return Ok(Vec::new());
    };

    if raw_scheduled_event_overlays.is_none() {
        return Ok(Vec::new());
    }

    let overlay_iterator = raw_scheduled_event_overlays.try_iter().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "scheduled_event_overlays must be an iterable of mapping records",
        )
    })?;

    let mut overlays: Vec<ScheduledEventOverlayDefinition> = Vec::new();

    for (overlay_position, overlay_record_result) in overlay_iterator.enumerate() {
        let overlay_record = overlay_record_result?;
        let overlay_mapping = overlay_record.downcast::<PyMapping>().map_err(|_| {
            SeasonalityInputValidationError::new_err(format!(
                "scheduled_event_overlay at position {overlay_position} must be a mapping"
            ))
        })?;

        let overlay_id_value = overlay_mapping
            .contains("overlay_id")?
            .then(|| overlay_mapping.get_item("overlay_id"))
            .ok_or_else(|| {
                SeasonalityInputValidationError::new_err(format!(
                    "scheduled_event_overlay at position {overlay_position} is missing required field 'overlay_id'"
                ))
            })??;

        let overlay_id: String = overlay_id_value.extract().map_err(|_| {
            SeasonalityInputValidationError::new_err(format!(
                "scheduled_event_overlay at position {overlay_position} field 'overlay_id' must be a string"
            ))
        })?;

        let start_timestamp_value = overlay_mapping
            .contains("start_timestamp_utc")?
            .then(|| overlay_mapping.get_item("start_timestamp_utc"))
            .ok_or_else(|| {
                SeasonalityInputValidationError::new_err(format!(
                    "scheduled_event_overlay at position {overlay_position} is missing required field 'start_timestamp_utc'"
                ))
            })??;

        let start_timestamp_utc = parse_timestamp_utc_from_python(
            &start_timestamp_value,
            overlay_position,
            "start_timestamp_utc",
        )?;

        let start_timestamp_utc_nanos =
            start_timestamp_utc
                .timestamp_nanos_opt()
                .ok_or_else(|| {
                    SeasonalityInputValidationError::new_err(format!(
                        "scheduled_event_overlay at position {overlay_position} field 'start_timestamp_utc' is outside the supported nanosecond range"
                    ))
                })?;

        let end_timestamp_value = overlay_mapping
            .contains("end_timestamp_utc")?
            .then(|| overlay_mapping.get_item("end_timestamp_utc"))
            .ok_or_else(|| {
                SeasonalityInputValidationError::new_err(format!(
                    "scheduled_event_overlay at position {overlay_position} is missing required field 'end_timestamp_utc'"
                ))
            })??;

        let end_timestamp_utc = parse_timestamp_utc_from_python(
            &end_timestamp_value,
            overlay_position,
            "end_timestamp_utc",
        )?;

        let end_timestamp_utc_nanos = end_timestamp_utc.timestamp_nanos_opt().ok_or_else(|| {
            SeasonalityInputValidationError::new_err(format!(
                "scheduled_event_overlay at position {overlay_position} field 'end_timestamp_utc' is outside the supported nanosecond range"
            ))
        })?;

        let log_adjustment_value = overlay_mapping
            .contains("log_adjustment")?
            .then(|| overlay_mapping.get_item("log_adjustment"))
            .ok_or_else(|| {
                SeasonalityInputValidationError::new_err(format!(
                    "scheduled_event_overlay at position {overlay_position} is missing required field 'log_adjustment'"
                ))
            })??;

        let log_adjustment: f64 = log_adjustment_value.extract().map_err(|_| {
            SeasonalityInputValidationError::new_err(format!(
                "scheduled_event_overlay at position {overlay_position} field 'log_adjustment' must be a float"
            ))
        })?;

        let precedence = if overlay_mapping.contains("precedence")? {
            let precedence_value = overlay_mapping.get_item("precedence")?;
            precedence_value.extract::<u16>().map_err(|_| {
                SeasonalityInputValidationError::new_err(format!(
                    "scheduled_event_overlay at position {overlay_position} field 'precedence' must be an integer in the range 0..65535"
                ))
            })?
        } else {
            0_u16
        };

        let enabled = if overlay_mapping.contains("enabled")? {
            let enabled_value = overlay_mapping.get_item("enabled")?;
            enabled_value.extract::<bool>().map_err(|_| {
                SeasonalityInputValidationError::new_err(format!(
                    "scheduled_event_overlay at position {overlay_position} field 'enabled' must be a boolean"
                ))
            })?
        } else {
            true
        };

        let source_label = if overlay_mapping.contains("source_label")? {
            let source_label_value = overlay_mapping.get_item("source_label")?;
            source_label_value.extract::<String>().map_err(|_| {
                SeasonalityInputValidationError::new_err(format!(
                    "scheduled_event_overlay at position {overlay_position} field 'source_label' must be a string"
                ))
            })?
        } else {
            "configuration".to_string()
        };

        overlays.push(ScheduledEventOverlayDefinition {
            overlay_id,
            start_timestamp_utc_nanos,
            end_timestamp_utc_nanos,
            log_adjustment,
            precedence,
            enabled,
            source_label,
        });
    }

    Ok(overlays)
}

fn parse_intraday_smoothing_kernel(
    raw_smoothing_kernel: &str,
) -> PyResult<IntradaySmoothingKernel> {
    match raw_smoothing_kernel {
        "gaussian" => Ok(IntradaySmoothingKernel::Gaussian),
        "uniform" => Ok(IntradaySmoothingKernel::Uniform),
        _ => Err(SeasonalityInputValidationError::new_err(format!(
            "unsupported intraday smoothing kernel value: {raw_smoothing_kernel}"
        ))),
    }
}

fn map_intraday_estimation_error(error: IntradaySeasonalityEstimationError) -> PyErr {
    match error {
        IntradaySeasonalityEstimationError::EmptyInputEvents => {
            SeasonalityInputValidationError::new_err(
                "intraday seasonality estimation requires at least one accepted event",
            )
        }
        IntradaySeasonalityEstimationError::InvalidMinutesPerBin { minutes_per_bin } => {
            SeasonalityInputValidationError::new_err(format!(
                "minutes_per_bin must be a positive divisor of 1440, received {minutes_per_bin}"
            ))
        }
        IntradaySeasonalityEstimationError::InvalidSmoothingBandwidthBins {
            smoothing_bandwidth_bins,
        } => SeasonalityInputValidationError::new_err(format!(
            "smoothing_bandwidth_bins must be a finite positive value, received {smoothing_bandwidth_bins}"
        )),
        IntradaySeasonalityEstimationError::InvalidMinimumBinEvents { minimum_bin_events } => {
            SeasonalityInputValidationError::new_err(format!(
                "minimum_bin_events must be at least 1, received {minimum_bin_events}"
            ))
        }
        IntradaySeasonalityEstimationError::InvalidPseudocount { pseudocount } => {
            SeasonalityInputValidationError::new_err(format!(
                "pseudocount must be a finite positive value, received {pseudocount}"
            ))
        }
        IntradaySeasonalityEstimationError::InvalidMinuteOfDay {
            record_position,
            minute_of_day,
        } => SeasonalityInputValidationError::new_err(format!(
            "event record at position {record_position} has invalid local minute-of-day value {minute_of_day}"
        )),
        IntradaySeasonalityEstimationError::UnstableNormalization => {
            SeasonalityInputValidationError::new_err(
                "intraday seasonality estimation produced a non-positive normalization constant",
            )
        }
    }
}

fn map_day_of_week_estimation_error(error: DayOfWeekSeasonalityEstimationError) -> PyErr {
    match error {
        DayOfWeekSeasonalityEstimationError::EmptyInputEvents => {
            SeasonalityInputValidationError::new_err(
                "day-of-week seasonality estimation requires at least one accepted event",
            )
        }
        DayOfWeekSeasonalityEstimationError::InvalidMinimumBucketEvents {
            minimum_bucket_events,
        } => SeasonalityInputValidationError::new_err(format!(
            "minimum_bucket_events must be at least 1, received {minimum_bucket_events}"
        )),
        DayOfWeekSeasonalityEstimationError::InvalidPseudocount { pseudocount } => {
            SeasonalityInputValidationError::new_err(format!(
                "pseudocount must be a finite positive value, received {pseudocount}"
            ))
        }
        DayOfWeekSeasonalityEstimationError::InvalidDayOfWeek {
            record_position,
            day_of_week,
        } => SeasonalityInputValidationError::new_err(format!(
            "event record at position {record_position} has invalid local day-of-week value {day_of_week}"
        )),
        DayOfWeekSeasonalityEstimationError::UnstableNormalization => {
            SeasonalityInputValidationError::new_err(
                "day-of-week seasonality estimation produced a non-positive normalization constant",
            )
        }
    }
}

fn map_day_of_year_estimation_error(error: DayOfYearSeasonalityEstimationError) -> PyErr {
    match error {
        DayOfYearSeasonalityEstimationError::EmptyInputEvents => {
            SeasonalityInputValidationError::new_err(
                "day-of-year seasonality estimation requires at least one accepted event",
            )
        }
        DayOfYearSeasonalityEstimationError::InvalidMinimumBucketEvents {
            minimum_bucket_events,
        } => SeasonalityInputValidationError::new_err(format!(
            "minimum_bucket_events must be at least 1, received {minimum_bucket_events}"
        )),
        DayOfYearSeasonalityEstimationError::InvalidPseudocount { pseudocount } => {
            SeasonalityInputValidationError::new_err(format!(
                "pseudocount must be a finite positive value, received {pseudocount}"
            ))
        }
        DayOfYearSeasonalityEstimationError::InvalidDayOfYear {
            record_position,
            day_of_year,
        } => SeasonalityInputValidationError::new_err(format!(
            "event record at position {record_position} has invalid local day-of-year value {day_of_year}"
        )),
        DayOfYearSeasonalityEstimationError::UnstableNormalization => {
            SeasonalityInputValidationError::new_err(
                "day-of-year seasonality estimation produced a non-positive normalization constant",
            )
        }
    }
}

fn map_deterministic_composition_error(error: DeterministicSeasonalityCompositionError) -> PyErr {
    match error {
        DeterministicSeasonalityCompositionError::EmptyInputEvents => {
            SeasonalityInputValidationError::new_err(
                "deterministic seasonality composition requires at least one accepted event",
            )
        }
        DeterministicSeasonalityCompositionError::MismatchedInputLengths {
            timestamp_utc_nanos_count,
            minute_of_day_count,
            day_of_week_count,
            day_of_year_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "composition inputs must have matching lengths, received timestamp_utc_nanos={timestamp_utc_nanos_count}, minute_of_day={minute_of_day_count}, day_of_week={day_of_week_count}, day_of_year={day_of_year_count}"
        )),
        DeterministicSeasonalityCompositionError::InvalidIntradayMinutesPerBin {
            intraday_minutes_per_bin,
        } => SeasonalityInputValidationError::new_err(format!(
            "intraday_minutes_per_bin must be a positive divisor of 1440, received {intraday_minutes_per_bin}"
        )),
        DeterministicSeasonalityCompositionError::InvalidIntradayComponentLength {
            expected_intraday_bin_count,
            actual_intraday_bin_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "intraday component length mismatch, expected {expected_intraday_bin_count} bins and received {actual_intraday_bin_count}"
        )),
        DeterministicSeasonalityCompositionError::InvalidDayOfWeekComponentLength {
            expected_day_of_week_bucket_count,
            actual_day_of_week_bucket_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "day-of-week component length mismatch, expected {expected_day_of_week_bucket_count} buckets and received {actual_day_of_week_bucket_count}"
        )),
        DeterministicSeasonalityCompositionError::InvalidDayOfYearComponentLength {
            expected_day_of_year_bucket_count,
            actual_day_of_year_bucket_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "day-of-year component length mismatch, expected {expected_day_of_year_bucket_count} buckets and received {actual_day_of_year_bucket_count}"
        )),
        DeterministicSeasonalityCompositionError::InvalidMinuteOfDay {
            record_position,
            minute_of_day,
        } => SeasonalityInputValidationError::new_err(format!(
            "event record at position {record_position} has invalid local minute-of-day value {minute_of_day}"
        )),
        DeterministicSeasonalityCompositionError::InvalidDayOfWeek {
            record_position,
            day_of_week,
        } => SeasonalityInputValidationError::new_err(format!(
            "event record at position {record_position} has invalid local day-of-week value {day_of_week}"
        )),
        DeterministicSeasonalityCompositionError::InvalidDayOfYear {
            record_position,
            day_of_year,
        } => SeasonalityInputValidationError::new_err(format!(
            "event record at position {record_position} has invalid local day-of-year value {day_of_year}"
        )),
        DeterministicSeasonalityCompositionError::EmptyOverlayId { overlay_position } => {
            SeasonalityInputValidationError::new_err(format!(
                "scheduled event overlay at position {overlay_position} is missing a non-empty overlay_id"
            ))
        }
        DeterministicSeasonalityCompositionError::DuplicateOverlayId { overlay_id } => {
            SeasonalityInputValidationError::new_err(format!(
                "scheduled event overlay id '{overlay_id}' is duplicated and must be unique"
            ))
        }
        DeterministicSeasonalityCompositionError::InvalidOverlayWindow {
            overlay_position,
            start_timestamp_utc_nanos,
            end_timestamp_utc_nanos,
        } => SeasonalityInputValidationError::new_err(format!(
            "scheduled event overlay at position {overlay_position} has an invalid window where start={start_timestamp_utc_nanos} and end={end_timestamp_utc_nanos}"
        )),
        DeterministicSeasonalityCompositionError::InvalidOverlayLogAdjustment {
            overlay_position,
        } => SeasonalityInputValidationError::new_err(format!(
            "scheduled event overlay at position {overlay_position} has a non-finite log_adjustment"
        )),
        DeterministicSeasonalityCompositionError::InvalidSpecialOverlayLogComponent {
            record_position,
        } => SeasonalityInputValidationError::new_err(format!(
            "deterministic seasonality composition produced a non-finite special overlay log component at record position {record_position}"
        )),
        DeterministicSeasonalityCompositionError::InvalidCombinedLogComponent {
            record_position,
        } => SeasonalityInputValidationError::new_err(format!(
            "deterministic seasonality composition produced a non-finite combined log component at record position {record_position}"
        )),
        DeterministicSeasonalityCompositionError::UnstableCombinedFactor { record_position } => {
            SeasonalityInputValidationError::new_err(format!(
                "deterministic seasonality composition produced a non-finite combined factor at record position {record_position}"
            ))
        }
        DeterministicSeasonalityCompositionError::InvalidNormalizationInputFactor {
            record_position,
        } => SeasonalityInputValidationError::new_err(format!(
            "deterministic seasonality normalization received a non-finite or non-positive factor at record position {record_position}"
        )),
        DeterministicSeasonalityCompositionError::InvalidNormalizationInputLogComponent {
            record_position,
        } => SeasonalityInputValidationError::new_err(format!(
            "deterministic seasonality normalization received a non-finite log component at record position {record_position}"
        )),
        DeterministicSeasonalityCompositionError::UnstableNormalizationConstant => {
            SeasonalityInputValidationError::new_err(
                "deterministic seasonality normalization produced a non-positive normalization constant",
            )
        }
        DeterministicSeasonalityCompositionError::IdentifiabilityGuardViolation {
            anchored_mean_factor,
            target_mean_factor,
            mean_anchor_tolerance,
        } => SeasonalityInputValidationError::new_err(format!(
            "deterministic seasonality normalization failed identifiability guard, anchored mean={anchored_mean_factor}, target mean={target_mean_factor}, tolerance={mean_anchor_tolerance}"
        )),
    }
}

fn map_seasonality_diagnostics_report_error(error: SeasonalityDiagnosticsReportError) -> PyErr {
    match error {
        SeasonalityDiagnosticsReportError::EmptyDatasetId => {
            SeasonalityInputValidationError::new_err(
                "seasonality diagnostics report requires a non-empty dataset_id",
            )
        }
        SeasonalityDiagnosticsReportError::EmptyEstimatorVersion => {
            SeasonalityInputValidationError::new_err(
                "seasonality diagnostics report requires a non-empty estimator_version",
            )
        }
        SeasonalityDiagnosticsReportError::EmptyInputSchemaVersion => {
            SeasonalityInputValidationError::new_err(
                "seasonality diagnostics report requires a non-empty input_schema_version",
            )
        }
        SeasonalityDiagnosticsReportError::EmptyPreprocessedSchemaVersion => {
            SeasonalityInputValidationError::new_err(
                "seasonality diagnostics report requires a non-empty preprocessed_schema_version",
            )
        }
        SeasonalityDiagnosticsReportError::EmptyExchangeCode => {
            SeasonalityInputValidationError::new_err(
                "seasonality diagnostics report requires a non-empty exchange_code",
            )
        }
        SeasonalityDiagnosticsReportError::EmptyExchangeTimezoneName => {
            SeasonalityInputValidationError::new_err(
                "seasonality diagnostics report requires a non-empty exchange_timezone_name",
            )
        }
        SeasonalityDiagnosticsReportError::EmptyInputEvents => {
            SeasonalityInputValidationError::new_err(
                "seasonality diagnostics report requires at least one accepted input event",
            )
        }
        SeasonalityDiagnosticsReportError::MismatchedInputLengths {
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
        } => SeasonalityInputValidationError::new_err(format!(
            "seasonality diagnostics report inputs must have matching lengths, received timestamp_utc_nanos={timestamp_utc_nanos_count}, source_record_positions={source_record_position_count}, minute_of_day={minute_of_day_count}, day_of_week={day_of_week_count}, day_of_year={day_of_year_count}, market_state_codes={market_state_code_count}, combined_log_component={combined_log_component_count}, combined_factor={combined_factor_count}, special_log_component={special_log_component_count}, special_overlay_flags={special_overlay_flag_count}"
        )),
        SeasonalityDiagnosticsReportError::InputEventCountMismatch {
            component_name,
            expected_input_event_count,
            actual_input_event_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "seasonality diagnostics report component '{component_name}' input_event_count mismatch, expected {expected_input_event_count} and received {actual_input_event_count}"
        )),
        SeasonalityDiagnosticsReportError::ComponentLengthMismatch {
            component_name,
            expected_bucket_count,
            actual_bucket_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "seasonality diagnostics report component '{component_name}' length mismatch, expected {expected_bucket_count} buckets and received {actual_bucket_count}"
        )),
        SeasonalityDiagnosticsReportError::InvalidComponentPseudocount {
            component_name,
            pseudocount,
        } => SeasonalityInputValidationError::new_err(format!(
            "seasonality diagnostics report component '{component_name}' has invalid pseudocount value {pseudocount}"
        )),
        SeasonalityDiagnosticsReportError::InvalidComponentFactor {
            component_name,
            bucket_position,
        } => SeasonalityInputValidationError::new_err(format!(
            "seasonality diagnostics report component '{component_name}' has non-finite or non-positive factor at bucket position {bucket_position}"
        )),
        SeasonalityDiagnosticsReportError::InvalidCombinedFactor { record_position } => {
            SeasonalityInputValidationError::new_err(format!(
                "seasonality diagnostics report received non-finite or non-positive combined factor at record position {record_position}"
            ))
        }
        SeasonalityDiagnosticsReportError::InvalidCombinedLogComponent { record_position } => {
            SeasonalityInputValidationError::new_err(format!(
                "seasonality diagnostics report received non-finite combined log component at record position {record_position}"
            ))
        }
        SeasonalityDiagnosticsReportError::InvalidSpecialOverlayFlag {
            record_position,
            overlay_active_flag,
        } => SeasonalityInputValidationError::new_err(format!(
            "seasonality diagnostics report received invalid special overlay active flag {overlay_active_flag} at record position {record_position}"
        )),
        SeasonalityDiagnosticsReportError::EmptyBatchInput => {
            SeasonalityInputValidationError::new_err(
                "seasonality diagnostics batch report requires at least one dataset input",
            )
        }
        SeasonalityDiagnosticsReportError::DuplicateDatasetId { dataset_id } => {
            SeasonalityInputValidationError::new_err(format!(
                "seasonality diagnostics batch report received duplicate dataset id '{dataset_id}'"
            ))
        }
    }
}

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(pyo3::wrap_pyfunction!(
        estimate_intraday_seasonality,
        module
    )?)?;
    module.add_function(pyo3::wrap_pyfunction!(
        estimate_day_of_week_seasonality,
        module
    )?)?;
    module.add_function(pyo3::wrap_pyfunction!(
        estimate_day_of_year_seasonality,
        module
    )?)?;
    module.add_function(pyo3::wrap_pyfunction!(
        compose_deterministic_seasonality_signal,
        module
    )?)?;
    module.add_function(pyo3::wrap_pyfunction!(
        generate_seasonality_diagnostics_report,
        module
    )?)?;

    Ok(())
}
