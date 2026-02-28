use janiform_core::calendar::MarketState;
use janiform_core::verification::{
    CalendarStateRegressionCase, HourlyFactorRegressionCase, HourlyNewsIntensityRegressionSource,
    HourlySeasonalityRegressionSource, OpenClosedGapLeakageRegressionCase,
    OpenClosedGapLeakageRegressionEventInput, OpenClosedGapLeakageRegressionHarnessToleranceConfig,
    RegressionHarnessToleranceConfig,
    run_deterministic_regression_harness as run_deterministic_regression_harness_core,
    run_open_closed_gap_leakage_regression_harness as run_open_closed_gap_leakage_regression_harness_core,
};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyMapping, PyModule};
use serde_json::to_string as serialize_to_json_string;

use super::calendar::parse_timestamp_utc;
use super::hourly::{
    ParsedHourlyProviderNewsIntensitySource, ParsedHourlyProviderSeasonalitySource,
    parse_hourly_provider_news_intensity_source, parse_hourly_provider_seasonality_source,
};
use crate::SeasonalityInputValidationError;
use crate::types::PyCalendarRegistry;

#[pyfunction]
#[pyo3(signature = (calendar_registry, calendar_cases, seasonality_cases, *, seasonality_absolute_tolerance = 1e-12, news_intensity_absolute_tolerance = 1e-12))]
fn run_deterministic_regression_harness(
    py: Python<'_>,
    calendar_registry: &PyCalendarRegistry,
    calendar_cases: &Bound<'_, PyAny>,
    seasonality_cases: &Bound<'_, PyAny>,
    seasonality_absolute_tolerance: f64,
    news_intensity_absolute_tolerance: f64,
) -> PyResult<String> {
    let parsed_calendar_cases = parse_regression_calendar_cases(calendar_cases)?;
    let parsed_seasonality_cases = parse_regression_seasonality_cases(seasonality_cases)?;
    let tolerance_config = RegressionHarnessToleranceConfig {
        seasonality_absolute_tolerance,
        news_intensity_absolute_tolerance,
    };

    let report = py
        .allow_threads(|| {
            run_deterministic_regression_harness_core(
                &calendar_registry.registry,
                &parsed_calendar_cases,
                &parsed_seasonality_cases,
                tolerance_config,
            )
        })
        .map_err(map_regression_harness_error)?;

    serialize_to_json_string(&report).map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "failed to serialize deterministic regression harness report to json",
        )
    })
}

#[pyfunction]
#[pyo3(signature = (calendar_registry, gap_behavior_cases, *, direct_gap_absolute_tolerance = 1e-12))]
fn run_open_closed_gap_leakage_regression_harness(
    py: Python<'_>,
    calendar_registry: &PyCalendarRegistry,
    gap_behavior_cases: &Bound<'_, PyAny>,
    direct_gap_absolute_tolerance: f64,
) -> PyResult<String> {
    let parsed_gap_behavior_cases =
        parse_open_closed_gap_leakage_regression_cases(gap_behavior_cases)?;
    let tolerance_config = OpenClosedGapLeakageRegressionHarnessToleranceConfig {
        direct_gap_absolute_tolerance,
    };

    let report = py
        .allow_threads(|| {
            run_open_closed_gap_leakage_regression_harness_core(
                &calendar_registry.registry,
                &parsed_gap_behavior_cases,
                tolerance_config,
            )
        })
        .map_err(map_open_closed_gap_leakage_regression_harness_error)?;

    serialize_to_json_string(&report).map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "failed to serialize open closed gap leakage regression harness report to json",
        )
    })
}

fn parse_regression_calendar_cases(
    raw_calendar_cases: &Bound<'_, PyAny>,
) -> PyResult<Vec<CalendarStateRegressionCase>> {
    let calendar_case_iter = raw_calendar_cases.try_iter().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "calendar_cases must be an iterable of mapping records",
        )
    })?;
    let mut parsed_calendar_cases: Vec<CalendarStateRegressionCase> = Vec::new();

    for (case_position, raw_case) in calendar_case_iter.enumerate() {
        let raw_case = raw_case?;

        let case_mapping = raw_case.downcast::<PyMapping>().map_err(|_| {
            SeasonalityInputValidationError::new_err(format!(
                "calendar_cases[{case_position}] must be a mapping",
            ))
        })?;

        let fixture_id = parse_required_string_field(
            &case_mapping,
            case_position,
            "calendar_cases",
            "fixture_id",
        )?;

        let exchange_code = parse_required_string_field(
            &case_mapping,
            case_position,
            "calendar_cases",
            "exchange_code",
        )?;

        let timestamp_text = parse_required_string_field(
            &case_mapping,
            case_position,
            "calendar_cases",
            "timestamp_utc",
        )?;

        let expected_state = parse_required_string_field(
            &case_mapping,
            case_position,
            "calendar_cases",
            "expected_state",
        )?;

        let timestamp_utc = parse_timestamp_utc(&timestamp_text)?;
        let expected_state = parse_expected_market_state(expected_state.as_str())?;

        parsed_calendar_cases.push(CalendarStateRegressionCase {
            fixture_id,
            exchange_code,
            timestamp_utc,
            expected_state,
        });
    }

    Ok(parsed_calendar_cases)
}

fn parse_regression_seasonality_cases(
    raw_seasonality_cases: &Bound<'_, PyAny>,
) -> PyResult<Vec<HourlyFactorRegressionCase>> {
    let seasonality_case_iter = raw_seasonality_cases.try_iter().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "seasonality_cases must be an iterable of mapping records",
        )
    })?;

    let mut parsed_seasonality_cases: Vec<HourlyFactorRegressionCase> = Vec::new();

    for (case_position, raw_case) in seasonality_case_iter.enumerate() {
        let raw_case = raw_case?;

        let case_mapping = raw_case.downcast::<PyMapping>().map_err(|_| {
            SeasonalityInputValidationError::new_err(format!(
                "seasonality_cases[{case_position}] must be a mapping",
            ))
        })?;

        let fixture_id = parse_required_string_field(
            &case_mapping,
            case_position,
            "seasonality_cases",
            "fixture_id",
        )?;

        let exchange_code = parse_required_string_field(
            &case_mapping,
            case_position,
            "seasonality_cases",
            "exchange_code",
        )?;

        let range_start_utc_text = parse_required_string_field(
            &case_mapping,
            case_position,
            "seasonality_cases",
            "range_start_utc",
        )?;

        let range_end_utc_text = parse_required_string_field(
            &case_mapping,
            case_position,
            "seasonality_cases",
            "range_end_utc",
        )?;

        let seasonality_source_value = parse_required_field(
            &case_mapping,
            case_position,
            "seasonality_cases",
            "seasonality_source",
        )?;

        let news_intensity_source_value = parse_required_field(
            &case_mapping,
            case_position,
            "seasonality_cases",
            "news_intensity_source",
        )?;

        let expected_s_h_value = parse_required_field(
            &case_mapping,
            case_position,
            "seasonality_cases",
            "expected_s_h",
        )?;

        let expected_nu_h_value = parse_required_field(
            &case_mapping,
            case_position,
            "seasonality_cases",
            "expected_nu_h",
        )?;

        let parsed_seasonality_source =
            parse_hourly_provider_seasonality_source(&seasonality_source_value)?;
        let parsed_news_intensity_source =
            parse_hourly_provider_news_intensity_source(&news_intensity_source_value)?;

        let expected_s_h: Vec<f64> = expected_s_h_value.extract().map_err(|_| {
            SeasonalityInputValidationError::new_err(format!(
                "seasonality_cases[{case_position}] field 'expected_s_h' must be an iterable of float values",
            ))
        })?;

        let expected_nu_h: Vec<f64> = expected_nu_h_value.extract().map_err(|_| {
            SeasonalityInputValidationError::new_err(format!(
                "seasonality_cases[{case_position}] field 'expected_nu_h' must be an iterable of float values",
            ))
        })?;

        parsed_seasonality_cases.push(HourlyFactorRegressionCase {
            fixture_id,
            exchange_code,
            range_start_utc: parse_timestamp_utc(&range_start_utc_text)?,
            range_end_utc: parse_timestamp_utc(&range_end_utc_text)?,
            seasonality_source: convert_regression_seasonality_source(parsed_seasonality_source),
            news_intensity_source: convert_regression_news_intensity_source(
                parsed_news_intensity_source,
            ),
            expected_s_h,
            expected_nu_h,
        });
    }

    Ok(parsed_seasonality_cases)
}

fn parse_open_closed_gap_leakage_regression_cases(
    raw_gap_behavior_cases: &Bound<'_, PyAny>,
) -> PyResult<Vec<OpenClosedGapLeakageRegressionCase>> {
    let gap_behavior_case_iter = raw_gap_behavior_cases.try_iter().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "gap_behavior_cases must be an iterable of mapping records",
        )
    })?;
    let mut parsed_gap_behavior_cases: Vec<OpenClosedGapLeakageRegressionCase> = Vec::new();

    for (case_position, raw_case) in gap_behavior_case_iter.enumerate() {
        let raw_case = raw_case?;
        let case_mapping = raw_case.downcast::<PyMapping>().map_err(|_| {
            SeasonalityInputValidationError::new_err(format!(
                "gap_behavior_cases[{case_position}] must be a mapping",
            ))
        })?;

        let fixture_id = parse_required_string_field(
            &case_mapping,
            case_position,
            "gap_behavior_cases",
            "fixture_id",
        )?;
        let exchange_code = parse_required_string_field(
            &case_mapping,
            case_position,
            "gap_behavior_cases",
            "exchange_code",
        )?;

        let initial_j_t_value = parse_required_field(
            &case_mapping,
            case_position,
            "gap_behavior_cases",
            "initial_j_t",
        )?;
        let initial_j_t = initial_j_t_value.extract::<f64>().map_err(|_| {
            SeasonalityInputValidationError::new_err(format!(
                "gap_behavior_cases[{case_position}] field 'initial_j_t' must be a float",
            ))
        })?;
        if !initial_j_t.is_finite() {
            return Err(SeasonalityInputValidationError::new_err(format!(
                "gap_behavior_cases[{case_position}] field 'initial_j_t' must be finite"
            )));
        }

        let initial_g_t_value = parse_required_field(
            &case_mapping,
            case_position,
            "gap_behavior_cases",
            "initial_g_t",
        )?;
        let initial_g_t = initial_g_t_value.extract::<f64>().map_err(|_| {
            SeasonalityInputValidationError::new_err(format!(
                "gap_behavior_cases[{case_position}] field 'initial_g_t' must be a float",
            ))
        })?;
        if !initial_g_t.is_finite() {
            return Err(SeasonalityInputValidationError::new_err(format!(
                "gap_behavior_cases[{case_position}] field 'initial_g_t' must be finite"
            )));
        }

        let injected_events_value = parse_required_field(
            &case_mapping,
            case_position,
            "gap_behavior_cases",
            "injected_events",
        )?;
        let injected_event_iter = injected_events_value.try_iter().map_err(|_| {
            SeasonalityInputValidationError::new_err(format!(
                "gap_behavior_cases[{case_position}] field 'injected_events' must be an iterable of mapping records",
            ))
        })?;
        let mut injected_events: Vec<OpenClosedGapLeakageRegressionEventInput> = Vec::new();

        for (event_position, raw_event) in injected_event_iter.enumerate() {
            let raw_event = raw_event?;
            let event_mapping = raw_event.downcast::<PyMapping>().map_err(|_| {
                SeasonalityInputValidationError::new_err(format!(
                    "gap_behavior_cases[{case_position}].injected_events[{event_position}] must be a mapping"
                ))
            })?;

            let event_id = parse_required_string_field(
                &event_mapping,
                event_position,
                &format!("gap_behavior_cases[{case_position}].injected_events"),
                "event_id",
            )?;
            let timestamp_utc_text = parse_required_string_field(
                &event_mapping,
                event_position,
                &format!("gap_behavior_cases[{case_position}].injected_events"),
                "timestamp_utc",
            )?;
            let shared_severity_mark_value = parse_required_field(
                &event_mapping,
                event_position,
                &format!("gap_behavior_cases[{case_position}].injected_events"),
                "shared_severity_mark",
            )?;
            let shared_severity_mark = shared_severity_mark_value.extract::<f64>().map_err(|_| {
                SeasonalityInputValidationError::new_err(format!(
                    "gap_behavior_cases[{case_position}].injected_events[{event_position}] field 'shared_severity_mark' must be a float"
                ))
            })?;

            if !shared_severity_mark.is_finite() || shared_severity_mark < 0.0 {
                return Err(SeasonalityInputValidationError::new_err(format!(
                    "gap_behavior_cases[{case_position}].injected_events[{event_position}] field 'shared_severity_mark' must be finite and non-negative"
                )));
            }

            let coupled_gap_jump_mark_value = parse_required_field(
                &event_mapping,
                event_position,
                &format!("gap_behavior_cases[{case_position}].injected_events"),
                "coupled_gap_jump_mark",
            )?;
            let coupled_gap_jump_mark = coupled_gap_jump_mark_value.extract::<f64>().map_err(|_| {
                SeasonalityInputValidationError::new_err(format!(
                    "gap_behavior_cases[{case_position}].injected_events[{event_position}] field 'coupled_gap_jump_mark' must be a float"
                ))
            })?;

            if !coupled_gap_jump_mark.is_finite() {
                return Err(SeasonalityInputValidationError::new_err(format!(
                    "gap_behavior_cases[{case_position}].injected_events[{event_position}] field 'coupled_gap_jump_mark' must be finite"
                )));
            }

            let observed_direct_gap_increment_override = parse_optional_f64_field(
                &event_mapping,
                "observed_direct_gap_increment_override",
                &format!("gap_behavior_cases[{case_position}].injected_events[{event_position}]"),
            )?;

            injected_events.push(OpenClosedGapLeakageRegressionEventInput {
                event_id,
                event_timestamp_utc: parse_timestamp_utc(&timestamp_utc_text)?,
                shared_severity_mark,
                coupled_gap_jump_mark,
                observed_direct_gap_increment_override,
            });
        }

        parsed_gap_behavior_cases.push(OpenClosedGapLeakageRegressionCase {
            fixture_id,
            exchange_code,
            initial_j_t,
            initial_g_t,
            injected_events,
        });
    }

    Ok(parsed_gap_behavior_cases)
}

fn parse_required_field<'py>(
    mapping: &Bound<'py, PyMapping>,
    case_position: usize,
    field_group_name: &str,
    field_name: &str,
) -> PyResult<Bound<'py, PyAny>> {
    Ok(mapping
        .contains(field_name)?
        .then(|| mapping.get_item(field_name))
        .ok_or_else(|| {
            SeasonalityInputValidationError::new_err(format!(
                "{field_group_name}[{case_position}] is missing required field '{field_name}'",
            ))
        })??)
}

fn parse_required_string_field(
    mapping: &Bound<'_, PyMapping>,
    case_position: usize,
    field_group_name: &str,
    field_name: &str,
) -> PyResult<String> {
    let field_value = parse_required_field(mapping, case_position, field_group_name, field_name)?;
    field_value.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(format!(
            "{field_group_name}[{case_position}] field '{field_name}' must be a string",
        ))
    })
}

fn parse_optional_f64_field(
    mapping: &Bound<'_, PyMapping>,
    field_name: &str,
    field_context: &str,
) -> PyResult<Option<f64>> {
    if !mapping.contains(field_name)? {
        return Ok(None);
    }

    let field_value = mapping.get_item(field_name)?;
    if field_value.is_none() {
        return Ok(None);
    }

    let field_value = field_value.extract::<f64>().map_err(|_| {
        SeasonalityInputValidationError::new_err(format!(
            "{field_context} field '{field_name}' must be a float when provided"
        ))
    })?;
    if !field_value.is_finite() {
        return Err(SeasonalityInputValidationError::new_err(format!(
            "{field_context} field '{field_name}' must be finite when provided"
        )));
    }

    Ok(Some(field_value))
}

fn parse_expected_market_state(raw_expected_state: &str) -> PyResult<MarketState> {
    match raw_expected_state {
        "open" => Ok(MarketState::Open),
        "closed" => Ok(MarketState::Closed),
        _ => Err(SeasonalityInputValidationError::new_err(format!(
            "expected_state must be either 'open' or 'closed', received '{raw_expected_state}'",
        ))),
    }
}

fn convert_regression_seasonality_source(
    parsed_source: ParsedHourlyProviderSeasonalitySource,
) -> HourlySeasonalityRegressionSource {
    match parsed_source {
        ParsedHourlyProviderSeasonalitySource::Explicit {
            hourly_factors,
            materialization_method,
        } => HourlySeasonalityRegressionSource::Explicit {
            hourly_factors,
            materialization_method,
        },
        ParsedHourlyProviderSeasonalitySource::Materialized {
            source_timestamp_utc_nanos,
            source_seasonality_factors,
            interpolation_method,
            aggregation_method,
            sparse_fallback_policy,
            sparse_hour_minimum_observation_count,
        } => HourlySeasonalityRegressionSource::Materialized {
            source_timestamp_utc_nanos,
            source_seasonality_factors,
            interpolation_method,
            aggregation_method,
            sparse_fallback_policy,
            sparse_hour_minimum_observation_count,
        },
    }
}

fn convert_regression_news_intensity_source(
    parsed_source: ParsedHourlyProviderNewsIntensitySource,
) -> HourlyNewsIntensityRegressionSource {
    match parsed_source {
        ParsedHourlyProviderNewsIntensitySource::Explicit {
            hourly_factors,
            materialization_method,
        } => HourlyNewsIntensityRegressionSource::Explicit {
            hourly_factors,
            materialization_method,
        },
        ParsedHourlyProviderNewsIntensitySource::Materialized {
            known_scheduled_event_timestamps_utc_nanos,
            surprise_baseline_intensity,
            surprise_deterministic_time_multiplier_by_interval_index,
        } => HourlyNewsIntensityRegressionSource::Materialized {
            known_scheduled_event_timestamps_utc_nanos,
            surprise_baseline_intensity,
            surprise_deterministic_time_multiplier_by_interval_index,
        },
    }
}

fn map_regression_harness_error(
    error: janiform_core::verification::RegressionHarnessError,
) -> PyErr {
    SeasonalityInputValidationError::new_err(error.to_string())
}

fn map_open_closed_gap_leakage_regression_harness_error(
    error: janiform_core::verification::OpenClosedGapLeakageRegressionHarnessError,
) -> PyErr {
    SeasonalityInputValidationError::new_err(error.to_string())
}

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(pyo3::wrap_pyfunction!(
        run_deterministic_regression_harness,
        module
    )?)?;
    module.add_function(pyo3::wrap_pyfunction!(
        run_open_closed_gap_leakage_regression_harness,
        module
    )?)?;

    Ok(())
}
