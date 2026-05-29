use std::path::Path;

use janiform::governance::deterministic_regression::{
    CalendarStateRegressionCase, HourlyFactorRegressionCase, HourlyNewsIntensityRegressionSource,
    HourlySeasonalityRegressionSource,
};
use janiform::time::calendar::MarketState;
use serde_json::{Map, Value};

use super::shared::{
    array_field, finite_f64_array_field, finite_f64_field, malformed_artifact, multiplier_entries,
    non_empty_string_field, object_field, root_object, timestamp_field,
    timestamp_nanos_array_field, usize_field,
};
use crate::error::CliError;

pub(crate) fn extract_calendar_cases(
    path: &Path,
    document: &Value,
) -> Result<Vec<CalendarStateRegressionCase>, CliError> {
    let cases = array_field(
        root_object(document, "calendar cases", path)?,
        "cases",
        "calendar cases document",
        "calendar cases",
        path,
    )?;

    let mut parsed_cases = Vec::with_capacity(cases.len());

    for (case_position, raw_case) in cases.iter().enumerate() {
        let context = format!("calendar_cases[{case_position}]");
        let case = raw_case.as_object().ok_or_else(|| {
            malformed_artifact(
                "calendar cases",
                path,
                format!("{context} must be a JSON object"),
            )
        })?;

        parsed_cases.push(CalendarStateRegressionCase {
            fixture_id: non_empty_string_field(
                case,
                "fixture_id",
                &context,
                "calendar cases",
                path,
            )?,
            exchange_code: non_empty_string_field(
                case,
                "exchange_code",
                &context,
                "calendar cases",
                path,
            )?,
            timestamp_utc: timestamp_field(
                case,
                "timestamp_utc",
                &context,
                "calendar cases",
                path,
            )?,
            expected_state: expected_market_state(
                &non_empty_string_field(case, "expected_state", &context, "calendar cases", path)?,
                path,
            )?,
        });
    }

    Ok(parsed_cases)
}

pub(crate) fn extract_seasonality_cases(
    path: &Path,
    document: &Value,
) -> Result<Vec<HourlyFactorRegressionCase>, CliError> {
    let cases = array_field(
        root_object(document, "seasonality cases", path)?,
        "canonical_output_cases",
        "seasonality cases document",
        "seasonality cases",
        path,
    )?;

    let mut parsed_cases = Vec::with_capacity(cases.len());

    for (case_position, raw_case) in cases.iter().enumerate() {
        let context = format!("seasonality_cases[{case_position}]");
        let case = raw_case.as_object().ok_or_else(|| {
            malformed_artifact(
                "seasonality cases",
                path,
                format!("{context} must be a JSON object"),
            )
        })?;

        parsed_cases.push(HourlyFactorRegressionCase {
            fixture_id: non_empty_string_field(
                case,
                "fixture_id",
                &context,
                "seasonality cases",
                path,
            )?,
            exchange_code: non_empty_string_field(
                case,
                "exchange_code",
                &context,
                "seasonality cases",
                path,
            )?,
            range_start_utc: timestamp_field(
                case,
                "range_start_utc",
                &context,
                "seasonality cases",
                path,
            )?,
            range_end_utc: timestamp_field(
                case,
                "range_end_utc",
                &context,
                "seasonality cases",
                path,
            )?,
            seasonality_source: seasonality_source(
                object_field(
                    case,
                    "seasonality_source",
                    &context,
                    "seasonality cases",
                    path,
                )?,
                &format!("{context}.seasonality_source"),
                path,
            )?,
            news_intensity_source: news_intensity_source(
                object_field(
                    case,
                    "news_intensity_source",
                    &context,
                    "seasonality cases",
                    path,
                )?,
                &format!("{context}.news_intensity_source"),
                path,
            )?,
            expected_s_h: finite_f64_array_field(
                case,
                "expected_s_h",
                &context,
                "seasonality cases",
                path,
            )?,
            expected_nu_h: finite_f64_array_field(
                case,
                "expected_nu_h",
                &context,
                "seasonality cases",
                path,
            )?,
        });
    }

    Ok(parsed_cases)
}

fn seasonality_source(
    source: &Map<String, Value>,
    context: &str,
    path: &Path,
) -> Result<HourlySeasonalityRegressionSource, CliError> {
    match non_empty_string_field(source, "source_kind", context, "seasonality cases", path)?
        .as_str()
    {
        "explicit" => Ok(HourlySeasonalityRegressionSource::Explicit {
            hourly_factors: finite_f64_array_field(
                source,
                "hourly_factors",
                context,
                "seasonality cases",
                path,
            )?,
            materialization_method: non_empty_string_field(
                source,
                "materialization_method",
                context,
                "seasonality cases",
                path,
            )?,
        }),
        "materialized" => {
            if source.contains_key("source_timestamp_utc") {
                return Err(malformed_artifact(
                    "seasonality cases",
                    path,
                    format!(
                        "{context} uses unsupported field 'source_timestamp_utc'; use 'source_timestamps_utc'"
                    ),
                ));
            }

            Ok(HourlySeasonalityRegressionSource::Materialized {
                source_timestamp_utc_nanos: timestamp_nanos_array_field(
                    source,
                    "source_timestamps_utc",
                    context,
                    "seasonality cases",
                    path,
                )?,
                source_seasonality_factors: finite_f64_array_field(
                    source,
                    "source_seasonality_factors",
                    context,
                    "seasonality cases",
                    path,
                )?,
                interpolation_method: non_empty_string_field(
                    source,
                    "interpolation_method",
                    context,
                    "seasonality cases",
                    path,
                )?,
                aggregation_method: non_empty_string_field(
                    source,
                    "aggregation_method",
                    context,
                    "seasonality cases",
                    path,
                )?,
                sparse_fallback_policy: non_empty_string_field(
                    source,
                    "sparse_fallback_policy",
                    context,
                    "seasonality cases",
                    path,
                )?,
                sparse_hour_minimum_observation_count: usize_field(
                    source,
                    "sparse_hour_minimum_observation_count",
                    context,
                    "seasonality cases",
                    path,
                )?,
            })
        }
        value => Err(malformed_artifact(
            "seasonality cases",
            path,
            format!(
                "{context}.source_kind must be either 'explicit' or 'materialized', found `{value}`"
            ),
        )),
    }
}

fn news_intensity_source(
    source: &Map<String, Value>,
    context: &str,
    path: &Path,
) -> Result<HourlyNewsIntensityRegressionSource, CliError> {
    match non_empty_string_field(source, "source_kind", context, "seasonality cases", path)?
        .as_str()
    {
        "explicit" => Ok(HourlyNewsIntensityRegressionSource::Explicit {
            hourly_factors: finite_f64_array_field(
                source,
                "hourly_factors",
                context,
                "seasonality cases",
                path,
            )?,
            materialization_method: non_empty_string_field(
                source,
                "materialization_method",
                context,
                "seasonality cases",
                path,
            )?,
        }),
        "materialized" => Ok(HourlyNewsIntensityRegressionSource::Materialized {
            known_scheduled_event_timestamps_utc_nanos: timestamp_nanos_array_field(
                source,
                "known_scheduled_event_timestamps_utc",
                context,
                "seasonality cases",
                path,
            )?,
            surprise_open_baseline_intensity: finite_f64_field(
                source,
                "surprise_open_baseline_intensity",
                context,
                "seasonality cases",
                path,
            )?,
            surprise_closed_baseline_intensity: finite_f64_field(
                source,
                "surprise_closed_baseline_intensity",
                context,
                "seasonality cases",
                path,
            )?,
            surprise_deterministic_time_multiplier_by_interval_index: multiplier_entries(
                object_field(
                    source,
                    "surprise_deterministic_time_multiplier_by_interval_index",
                    context,
                    "seasonality cases",
                    path,
                )?,
                &format!("{context}.surprise_deterministic_time_multiplier_by_interval_index"),
                path,
            )?,
        }),
        value => Err(malformed_artifact(
            "seasonality cases",
            path,
            format!(
                "{context}.source_kind must be either 'explicit' or 'materialized', found `{value}`"
            ),
        )),
    }
}

fn expected_market_state(raw_value: &str, path: &Path) -> Result<MarketState, CliError> {
    match raw_value {
        "open" => Ok(MarketState::Open),
        "closed" => Ok(MarketState::Closed),
        _ => Err(malformed_artifact(
            "calendar cases",
            path,
            format!("expected_state must be either 'open' or 'closed', found `{raw_value}`"),
        )),
    }
}
