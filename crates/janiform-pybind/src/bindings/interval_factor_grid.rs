use chrono::{DateTime, Utc};
use janiform::model::factors::interval_factor_grid::{
    DEFAULT_INTERVAL_FACTOR_GRID_MATERIALIZATION_METHOD, DEFAULT_SEASONALITY_AGGREGATION_METHOD,
    DEFAULT_SEASONALITY_INTERPOLATION_METHOD, DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY,
    DEFAULT_SPARSE_INTERVAL_MINIMUM_OBSERVATION_COUNT, IntervalFactorGridBuildInput,
    IntervalFactorGridError, IntervalFactorGridPayload, IntervalNewsIntensityMaterializationInput,
    IntervalSeasonalityMaterializationInput, IntervalSeasonalityMaterializationPolicy,
    IntervalSeasonalitySourceSignal, build_interval_factor_grid as build_interval_factor_grid_core,
    load_interval_factor_grid as load_interval_factor_grid_core,
    materialize_interval_news_intensity_factors as materialize_interval_news_intensity_factors_core,
    materialize_interval_seasonality_factors as materialize_interval_seasonality_factors_core,
};
use janiform::model::factors::interval_factor_grid::{
    IntervalFactorGridProviderError, IntervalFactorGridProviderRequest,
    IntervalNewsIntensityFactorSource, IntervalSeasonalityFactorSource,
    provide_interval_factor_grid as provide_interval_factor_grid_core,
};
use pyo3::prelude::{Bound, PyErr, PyResult, Python};
use pyo3::types::{PyAny, PyAnyMethods, PyMapping, PyMappingMethods, PyModule, PyModuleMethods};
use pyo3::{pyfunction, wrap_pyfunction};

use super::calendar::parse_timestamp_utc;
use crate::types::{
    PyCalendarRegistry, PyIntervalFactorGridContractResult,
    PyIntervalNewsIntensityMaterializationResult, PyIntervalSeasonalityMaterializationResult,
};
use crate::{
    CalendarInvalidTimezoneError, CalendarMissingExchangeError,
    IntervalFactorGridProviderMissingInputError, IntervalFactorGridProviderUnsupportedRangeError,
    SeasonalityInputValidationError,
};

pub(super) enum ParsedIntervalProviderSeasonalitySource {
    Explicit {
        interval_factors: Vec<f64>,
        materialization_method: String,
    },
    Materialized {
        source_timestamp_utc_nanos: Vec<i64>,
        source_seasonality_factors: Vec<f64>,
        interpolation_method: String,
        aggregation_method: String,
        sparse_fallback_policy: String,
        sparse_interval_minimum_observation_count: usize,
    },
}

pub(super) enum ParsedIntervalProviderNewsIntensitySource {
    Explicit {
        interval_factors: Vec<f64>,
        materialization_method: String,
    },
    Materialized {
        known_scheduled_event_timestamps_utc_nanos: Vec<i64>,
        surprise_open_baseline_intensity: f64,
        surprise_closed_baseline_intensity: f64,
        surprise_deterministic_time_multiplier_by_interval_index: Vec<(u64, f64)>,
    },
}

#[pyfunction]
#[pyo3(signature = (calendar_registry, exchange_code, range_start_utc, range_end_utc, interval_duration_nanos, seasonality_factors, news_intensity_factors, *, seasonality_materialization_method = DEFAULT_INTERVAL_FACTOR_GRID_MATERIALIZATION_METHOD, news_intensity_materialization_method = DEFAULT_INTERVAL_FACTOR_GRID_MATERIALIZATION_METHOD))]
#[expect(
    clippy::too_many_arguments,
    reason = "PyO3 boundary mirrors the public Python keyword API."
)]
fn build_interval_factor_grid(
    py: Python<'_>,
    calendar_registry: &PyCalendarRegistry,
    exchange_code: &str,
    range_start_utc: &str,
    range_end_utc: &str,
    interval_duration_nanos: i64,
    seasonality_factors: &Bound<'_, PyAny>,
    news_intensity_factors: &Bound<'_, PyAny>,
    seasonality_materialization_method: &str,
    news_intensity_materialization_method: &str,
) -> PyResult<PyIntervalFactorGridContractResult> {
    let parsed_range_start_utc = parse_timestamp_utc(range_start_utc)?;
    let parsed_range_end_utc = parse_timestamp_utc(range_end_utc)?;

    let parsed_seasonality_factors: Vec<f64> = seasonality_factors.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "seasonality_factors must be an iterable of float values",
        )
    })?;

    let parsed_news_intensity_factors: Vec<f64> =
        news_intensity_factors.extract().map_err(|_| {
            SeasonalityInputValidationError::new_err(
                "news_intensity_factors must be an iterable of float values",
            )
        })?;

    let output = py
        .detach(|| {
            build_interval_factor_grid_core(IntervalFactorGridBuildInput {
                calendar_registry: &calendar_registry.registry,
                exchange_code,
                range_utc: parsed_range_start_utc..parsed_range_end_utc,
                interval_duration_nanos,
                seasonality_factors: &parsed_seasonality_factors,
                news_intensity_factors: &parsed_news_intensity_factors,
                seasonality_materialization_method,
                news_intensity_materialization_method,
            })
        })
        .map_err(map_interval_factor_grid_error)?;

    Ok(PyIntervalFactorGridContractResult {
        output,
        calendar_registry: Some(calendar_registry.registry.clone()),
    })
}

#[pyfunction]
fn load_interval_factor_grid_payload(
    interval_factor_grid_payload: &Bound<'_, PyAny>,
) -> PyResult<PyIntervalFactorGridContractResult> {
    let payload = serde_json::from_value::<IntervalFactorGridPayload>(
        crate::python_value::py_any_to_json_value(interval_factor_grid_payload)?,
    )
    .map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "interval_factor_grid payload failed to deserialize: {source}"
        ))
    })?;

    let output = load_interval_factor_grid_core(payload).map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "interval_factor_grid payload failed validation: {source}"
        ))
    })?;

    Ok(PyIntervalFactorGridContractResult {
        output,
        calendar_registry: None,
    })
}

#[pyfunction]
#[pyo3(signature = (calendar_registry, exchange_code, range_start_utc, range_end_utc, interval_duration_nanos, source_timestamps_utc, source_seasonality_factors, *, interpolation_method = DEFAULT_SEASONALITY_INTERPOLATION_METHOD, aggregation_method = DEFAULT_SEASONALITY_AGGREGATION_METHOD, sparse_fallback_policy = DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY, sparse_interval_minimum_observation_count = DEFAULT_SPARSE_INTERVAL_MINIMUM_OBSERVATION_COUNT))]
#[expect(
    clippy::too_many_arguments,
    reason = "PyO3 boundary mirrors the public Python keyword API."
)]
fn materialize_interval_seasonality_factors(
    py: Python<'_>,
    calendar_registry: &PyCalendarRegistry,
    exchange_code: &str,
    range_start_utc: &str,
    range_end_utc: &str,
    interval_duration_nanos: i64,
    source_timestamps_utc: &Bound<'_, PyAny>,
    source_seasonality_factors: &Bound<'_, PyAny>,
    interpolation_method: &str,
    aggregation_method: &str,
    sparse_fallback_policy: &str,
    sparse_interval_minimum_observation_count: usize,
) -> PyResult<PyIntervalSeasonalityMaterializationResult> {
    let parsed_range_start_utc = parse_timestamp_utc(range_start_utc)?;
    let parsed_range_end_utc = parse_timestamp_utc(range_end_utc)?;

    let parsed_source_timestamps_utc: Vec<String> =
        source_timestamps_utc.extract().map_err(|_| {
            SeasonalityInputValidationError::new_err(
                "source_timestamps_utc must be an iterable of RFC3339 timestamp strings",
            )
        })?;

    let parsed_source_seasonality_factors: Vec<f64> =
        source_seasonality_factors.extract().map_err(|_| {
            SeasonalityInputValidationError::new_err(
                "source_seasonality_factors must be an iterable of float values",
            )
        })?;

    let mut parsed_source_timestamp_utc_nanos: Vec<i64> =
        Vec::with_capacity(parsed_source_timestamps_utc.len());

    for (source_position, raw_timestamp_utc) in parsed_source_timestamps_utc.iter().enumerate() {
        let parsed_timestamp_utc = DateTime::parse_from_rfc3339(raw_timestamp_utc)
            .map(|timestamp| timestamp.with_timezone(&Utc))
            .map_err(|_| {
                SeasonalityInputValidationError::new_err(format!(
                    "source_timestamps_utc at position {source_position} must be a RFC3339 timestamp with UTC offset, received `{raw_timestamp_utc}`"
                ))
            })?;

        let parsed_timestamp_utc_nanos =
            parsed_timestamp_utc
                .timestamp_nanos_opt()
                .ok_or_else(|| {
                    SeasonalityInputValidationError::new_err(format!(
                        "source_timestamps_utc at position {source_position} is outside the supported nanosecond timestamp range"
                    ))
                })?;

        parsed_source_timestamp_utc_nanos.push(parsed_timestamp_utc_nanos);
    }

    let output = py
        .detach(|| {
            materialize_interval_seasonality_factors_core(IntervalSeasonalityMaterializationInput {
                calendar_registry: &calendar_registry.registry,
                exchange_code,
                range_utc: parsed_range_start_utc..parsed_range_end_utc,
                interval_duration_nanos,
                source_signal: IntervalSeasonalitySourceSignal {
                    timestamp_utc_nanos: &parsed_source_timestamp_utc_nanos,
                    seasonality_factors: &parsed_source_seasonality_factors,
                },
                policy: IntervalSeasonalityMaterializationPolicy {
                    interpolation_method,
                    aggregation_method,
                    sparse_fallback_policy,
                    sparse_interval_minimum_observation_count,
                },
            })
        })
        .map_err(map_interval_factor_grid_error)?;

    Ok(PyIntervalSeasonalityMaterializationResult { output })
}

#[pyfunction]
#[pyo3(signature = (calendar_registry, exchange_code, range_start_utc, range_end_utc, interval_duration_nanos, surprise_open_baseline_intensity, surprise_closed_baseline_intensity, *, known_scheduled_event_timestamps_utc = None, surprise_deterministic_time_multiplier_by_interval_index = None))]
#[expect(
    clippy::too_many_arguments,
    reason = "PyO3 boundary mirrors the public Python keyword API."
)]
fn materialize_interval_news_intensity_factors(
    py: Python<'_>,
    calendar_registry: &PyCalendarRegistry,
    exchange_code: &str,
    range_start_utc: &str,
    range_end_utc: &str,
    interval_duration_nanos: i64,
    surprise_open_baseline_intensity: f64,
    surprise_closed_baseline_intensity: f64,
    known_scheduled_event_timestamps_utc: Option<&Bound<'_, PyAny>>,
    surprise_deterministic_time_multiplier_by_interval_index: Option<&Bound<'_, PyAny>>,
) -> PyResult<PyIntervalNewsIntensityMaterializationResult> {
    let parsed_range_start_utc = parse_timestamp_utc(range_start_utc)?;
    let parsed_range_end_utc = parse_timestamp_utc(range_end_utc)?;

    let parsed_known_scheduled_event_timestamps_utc_nanos: Vec<i64> = if let Some(
        raw_known_scheduled_event_timestamps_utc,
    ) =
        known_scheduled_event_timestamps_utc
    {
        if raw_known_scheduled_event_timestamps_utc.is_none() {
            Vec::new()
        } else {
            let raw_timestamp_values: Vec<String> = raw_known_scheduled_event_timestamps_utc
                    .extract()
                    .map_err(|_| {
                        SeasonalityInputValidationError::new_err(
                            "known_scheduled_event_timestamps_utc must be an iterable of RFC3339 timestamps",
                        )
                    })?;

            let mut parsed_timestamps_utc_nanos: Vec<i64> =
                Vec::with_capacity(raw_timestamp_values.len());

            for raw_timestamp_value in raw_timestamp_values {
                let parsed_timestamp_utc = parse_timestamp_utc(&raw_timestamp_value)?;

                parsed_timestamps_utc_nanos.push(
                        parsed_timestamp_utc
                            .timestamp_nanos_opt()
                            .ok_or_else(|| {
                                SeasonalityInputValidationError::new_err(format!(
                                    "known_scheduled_event_timestamps_utc contains an out-of-range timestamp: {raw_timestamp_value}"
                                ))
                            })?,
                    );
            }

            parsed_timestamps_utc_nanos
        }
    } else {
        Vec::new()
    };

    let parsed_surprise_deterministic_time_multiplier_by_interval_index: Vec<(u64, f64)> =
        if let Some(raw_surprise_deterministic_time_multiplier_by_interval_index) =
            surprise_deterministic_time_multiplier_by_interval_index
        {
            if raw_surprise_deterministic_time_multiplier_by_interval_index.is_none() {
                Vec::new()
            } else {
                let parsed_mapping: std::collections::BTreeMap<u64, f64> =
                    raw_surprise_deterministic_time_multiplier_by_interval_index
                        .extract()
                        .map_err(|_| {
                            SeasonalityInputValidationError::new_err(
                                "surprise_deterministic_time_multiplier_by_interval_index must be a mapping of integer interval indices to float multiplier values",
                            )
                        })?;

                parsed_mapping.into_iter().collect()
            }
        } else {
            Vec::new()
        };

    let output = py
        .detach(|| {
            materialize_interval_news_intensity_factors_core(
                IntervalNewsIntensityMaterializationInput {
                    calendar_registry: &calendar_registry.registry,
                    exchange_code,
                    range_utc: parsed_range_start_utc..parsed_range_end_utc,
                    interval_duration_nanos,
                    known_scheduled_event_timestamps_utc_nanos:
                        &parsed_known_scheduled_event_timestamps_utc_nanos,
                    surprise_open_baseline_intensity,
                    surprise_closed_baseline_intensity,
                    surprise_deterministic_time_multiplier_by_interval_index:
                        &parsed_surprise_deterministic_time_multiplier_by_interval_index,
                },
            )
        })
        .map_err(map_interval_factor_grid_error)?;

    Ok(PyIntervalNewsIntensityMaterializationResult { output })
}

#[pyfunction]
#[pyo3(signature = (calendar_registry, exchange_code, range_start_utc, range_end_utc, interval_duration_nanos, *, seasonality_source = None, news_intensity_source = None))]
#[expect(
    clippy::too_many_arguments,
    reason = "PyO3 boundary mirrors the public Python keyword API."
)]
fn provide_interval_factor_grid(
    py: Python<'_>,
    calendar_registry: &PyCalendarRegistry,
    exchange_code: &str,
    range_start_utc: &str,
    range_end_utc: &str,
    interval_duration_nanos: i64,
    seasonality_source: Option<&Bound<'_, PyAny>>,
    news_intensity_source: Option<&Bound<'_, PyAny>>,
) -> PyResult<PyIntervalFactorGridContractResult> {
    let parsed_range_start_utc = parse_timestamp_utc(range_start_utc)?;
    let parsed_range_end_utc = parse_timestamp_utc(range_end_utc)?;

    let parsed_seasonality_source = if let Some(raw_seasonality_source) = seasonality_source {
        if raw_seasonality_source.is_none() {
            None
        } else {
            Some(parse_interval_provider_seasonality_source(
                raw_seasonality_source,
            )?)
        }
    } else {
        None
    };

    let parsed_news_intensity_source =
        if let Some(raw_news_intensity_source) = news_intensity_source {
            if raw_news_intensity_source.is_none() {
                None
            } else {
                Some(parse_interval_provider_news_intensity_source(
                    raw_news_intensity_source,
                )?)
            }
        } else {
            None
        };

    let output = py
        .detach(move || {
            let provider_seasonality_source = match &parsed_seasonality_source {
                Some(ParsedIntervalProviderSeasonalitySource::Explicit {
                    interval_factors,
                    materialization_method,
                }) => Some(IntervalSeasonalityFactorSource::Explicit {
                    interval_factors,
                    materialization_method,
                }),
                Some(ParsedIntervalProviderSeasonalitySource::Materialized {
                    source_timestamp_utc_nanos,
                    source_seasonality_factors,
                    interpolation_method,
                    aggregation_method,
                    sparse_fallback_policy,
                    sparse_interval_minimum_observation_count,
                }) => Some(IntervalSeasonalityFactorSource::Materialized {
                    source_timestamp_utc_nanos,
                    source_seasonality_factors,
                    interpolation_method,
                    aggregation_method,
                    sparse_fallback_policy,
                    sparse_interval_minimum_observation_count:
                        *sparse_interval_minimum_observation_count,
                }),
                None => None,
            };

            let provider_news_intensity_source = match &parsed_news_intensity_source {
                Some(ParsedIntervalProviderNewsIntensitySource::Explicit {
                    interval_factors,
                    materialization_method,
                }) => Some(IntervalNewsIntensityFactorSource::Explicit {
                    interval_factors,
                    materialization_method,
                }),
                Some(ParsedIntervalProviderNewsIntensitySource::Materialized {
                    known_scheduled_event_timestamps_utc_nanos,
                    surprise_open_baseline_intensity,
                    surprise_closed_baseline_intensity,
                    surprise_deterministic_time_multiplier_by_interval_index,
                }) => Some(IntervalNewsIntensityFactorSource::Materialized {
                    known_scheduled_event_timestamps_utc_nanos,
                    surprise_open_baseline_intensity: *surprise_open_baseline_intensity,
                    surprise_closed_baseline_intensity: *surprise_closed_baseline_intensity,
                    surprise_deterministic_time_multiplier_by_interval_index,
                }),
                None => None,
            };

            provide_interval_factor_grid_core(
                &calendar_registry.registry,
                IntervalFactorGridProviderRequest {
                    exchange_code,
                    range_start_utc: parsed_range_start_utc,
                    range_end_utc: parsed_range_end_utc,
                    interval_duration_nanos,
                    seasonality_source: provider_seasonality_source,
                    news_intensity_source: provider_news_intensity_source,
                },
            )
        })
        .map_err(map_interval_factor_grid_provider_error)?;

    Ok(PyIntervalFactorGridContractResult {
        output,
        calendar_registry: Some(calendar_registry.registry.clone()),
    })
}

pub(super) fn parse_interval_provider_seasonality_source(
    raw_seasonality_source: &Bound<'_, PyAny>,
) -> PyResult<ParsedIntervalProviderSeasonalitySource> {
    let source_mapping = raw_seasonality_source.cast::<PyMapping>().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "seasonality_source must be a mapping with source_kind and source fields",
        )
    })?;

    let source_kind_value = source_mapping
        .contains("source_kind")?
        .then(|| source_mapping.get_item("source_kind"))
        .ok_or_else(|| {
            SeasonalityInputValidationError::new_err(
                "seasonality_source is missing required field 'source_kind'",
            )
        })??;

    let source_kind: String = source_kind_value.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "seasonality_source field 'source_kind' must be a string",
        )
    })?;

    match source_kind.as_str() {
        "explicit" => {
            let interval_factors_value = source_mapping
                .contains("interval_factors")?
                .then(|| source_mapping.get_item("interval_factors"))
                .ok_or_else(|| {
                    SeasonalityInputValidationError::new_err(
                        "seasonality_source with source_kind='explicit' is missing required field 'interval_factors'",
                    )
                })??;

            let interval_factors: Vec<f64> = interval_factors_value.extract().map_err(|_| {
                SeasonalityInputValidationError::new_err(
                    "seasonality_source field 'interval_factors' must be an iterable of float values",
                )
            })?;

            let materialization_method = if source_mapping.contains("materialization_method")? {
                let materialization_method_value =
                    source_mapping.get_item("materialization_method")?;
                materialization_method_value.extract().map_err(|_| {
                    SeasonalityInputValidationError::new_err(
                        "seasonality_source field 'materialization_method' must be a string",
                    )
                })?
            } else {
                DEFAULT_INTERVAL_FACTOR_GRID_MATERIALIZATION_METHOD.to_string()
            };

            Ok(ParsedIntervalProviderSeasonalitySource::Explicit {
                interval_factors,
                materialization_method,
            })
        }
        "materialized" => {
            if source_mapping.contains("source_timestamp_utc")? {
                return Err(SeasonalityInputValidationError::new_err(
                    "seasonality_source with source_kind='materialized' uses unsupported field 'source_timestamp_utc'; use 'source_timestamps_utc'",
                ));
            }

            if !source_mapping.contains("source_timestamps_utc")? {
                return Err(SeasonalityInputValidationError::new_err(
                    "seasonality_source with source_kind='materialized' is missing required field 'source_timestamps_utc'",
                ));
            }

            let source_timestamps_utc_value = source_mapping.get_item("source_timestamps_utc")?;

            let source_timestamps_utc: Vec<String> =
                source_timestamps_utc_value.extract().map_err(|_| {
                    SeasonalityInputValidationError::new_err(
                        "seasonality_source field 'source_timestamps_utc' must be an iterable of RFC3339 timestamp strings",
                    )
                })?;

            let mut source_timestamp_utc_nanos: Vec<i64> =
                Vec::with_capacity(source_timestamps_utc.len());

            for (source_position, raw_timestamp_utc) in source_timestamps_utc.iter().enumerate() {
                let parsed_timestamp_utc = DateTime::parse_from_rfc3339(raw_timestamp_utc)
                    .map(|timestamp| timestamp.with_timezone(&Utc))
                    .map_err(|_| {
                        SeasonalityInputValidationError::new_err(format!(
                            "seasonality_source field 'source_timestamps_utc' at position {source_position} must be a RFC3339 timestamp with UTC offset, received `{raw_timestamp_utc}`",
                        ))
                    })?;

                let parsed_timestamp_utc_nanos =
                    parsed_timestamp_utc
                        .timestamp_nanos_opt()
                        .ok_or_else(|| {
                            SeasonalityInputValidationError::new_err(format!(
                                "seasonality_source field 'source_timestamps_utc' at position {source_position} is outside the supported nanosecond timestamp range",
                            ))
                        })?;

                source_timestamp_utc_nanos.push(parsed_timestamp_utc_nanos);
            }

            let source_seasonality_factors_value = source_mapping
                .contains("source_seasonality_factors")?
                .then(|| source_mapping.get_item("source_seasonality_factors"))
                .ok_or_else(|| {
                    SeasonalityInputValidationError::new_err(
                        "seasonality_source with source_kind='materialized' is missing required field 'source_seasonality_factors'",
                    )
                })??;

            let source_seasonality_factors: Vec<f64> =
                source_seasonality_factors_value.extract().map_err(|_| {
                    SeasonalityInputValidationError::new_err(
                        "seasonality_source field 'source_seasonality_factors' must be an iterable of float values",
                    )
                })?;

            let interpolation_method = if source_mapping.contains("interpolation_method")? {
                let interpolation_method_value = source_mapping.get_item("interpolation_method")?;

                interpolation_method_value.extract().map_err(|_| {
                    SeasonalityInputValidationError::new_err(
                        "seasonality_source field 'interpolation_method' must be a string",
                    )
                })?
            } else {
                DEFAULT_SEASONALITY_INTERPOLATION_METHOD.to_string()
            };

            let aggregation_method = if source_mapping.contains("aggregation_method")? {
                let aggregation_method_value = source_mapping.get_item("aggregation_method")?;

                aggregation_method_value.extract().map_err(|_| {
                    SeasonalityInputValidationError::new_err(
                        "seasonality_source field 'aggregation_method' must be a string",
                    )
                })?
            } else {
                DEFAULT_SEASONALITY_AGGREGATION_METHOD.to_string()
            };

            let sparse_fallback_policy = if source_mapping.contains("sparse_fallback_policy")? {
                let sparse_fallback_policy_value =
                    source_mapping.get_item("sparse_fallback_policy")?;

                sparse_fallback_policy_value.extract().map_err(|_| {
                    SeasonalityInputValidationError::new_err(
                        "seasonality_source field 'sparse_fallback_policy' must be a string",
                    )
                })?
            } else {
                DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY.to_string()
            };

            let sparse_interval_minimum_observation_count = if source_mapping
                .contains("sparse_interval_minimum_observation_count")?
            {
                let sparse_threshold_value =
                    source_mapping.get_item("sparse_interval_minimum_observation_count")?;

                sparse_threshold_value.extract().map_err(|_| {
                        SeasonalityInputValidationError::new_err(
                            "seasonality_source field 'sparse_interval_minimum_observation_count' must be an integer",
                        )
                    })?
            } else {
                DEFAULT_SPARSE_INTERVAL_MINIMUM_OBSERVATION_COUNT
            };

            Ok(ParsedIntervalProviderSeasonalitySource::Materialized {
                source_timestamp_utc_nanos,
                source_seasonality_factors,
                interpolation_method,
                aggregation_method,
                sparse_fallback_policy,
                sparse_interval_minimum_observation_count,
            })
        }
        _ => Err(SeasonalityInputValidationError::new_err(
            "seasonality_source field 'source_kind' must be either 'explicit' or 'materialized'",
        )),
    }
}

pub(super) fn parse_interval_provider_news_intensity_source(
    raw_news_intensity_source: &Bound<'_, PyAny>,
) -> PyResult<ParsedIntervalProviderNewsIntensitySource> {
    let source_mapping = raw_news_intensity_source.cast::<PyMapping>().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "news_intensity_source must be a mapping with source_kind and source fields",
        )
    })?;

    let source_kind_value = source_mapping
        .contains("source_kind")?
        .then(|| source_mapping.get_item("source_kind"))
        .ok_or_else(|| {
            SeasonalityInputValidationError::new_err(
                "news_intensity_source is missing required field 'source_kind'",
            )
        })??;

    let source_kind: String = source_kind_value.extract().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "news_intensity_source field 'source_kind' must be a string",
        )
    })?;

    match source_kind.as_str() {
        "explicit" => {
            let interval_factors_value = source_mapping
                .contains("interval_factors")?
                .then(|| source_mapping.get_item("interval_factors"))
                .ok_or_else(|| {
                    SeasonalityInputValidationError::new_err(
                        "news_intensity_source with source_kind='explicit' is missing required field 'interval_factors'",
                    )
                })??;

            let interval_factors: Vec<f64> = interval_factors_value.extract().map_err(|_| {
                SeasonalityInputValidationError::new_err(
                    "news_intensity_source field 'interval_factors' must be an iterable of float values",
                )
            })?;

            let materialization_method = if source_mapping.contains("materialization_method")? {
                let materialization_method_value =
                    source_mapping.get_item("materialization_method")?;
                materialization_method_value.extract().map_err(|_| {
                    SeasonalityInputValidationError::new_err(
                        "news_intensity_source field 'materialization_method' must be a string",
                    )
                })?
            } else {
                DEFAULT_INTERVAL_FACTOR_GRID_MATERIALIZATION_METHOD.to_string()
            };

            Ok(ParsedIntervalProviderNewsIntensitySource::Explicit {
                interval_factors,
                materialization_method,
            })
        }
        "materialized" => {
            let known_scheduled_event_timestamps_utc_value = source_mapping
                .contains("known_scheduled_event_timestamps_utc")?
                .then(|| source_mapping.get_item("known_scheduled_event_timestamps_utc"))
                .ok_or_else(|| {
                    SeasonalityInputValidationError::new_err(
                        "news_intensity_source with source_kind='materialized' is missing required field 'known_scheduled_event_timestamps_utc'",
                    )
                })??;

            let raw_known_scheduled_event_timestamps_utc: Vec<String> =
                known_scheduled_event_timestamps_utc_value
                    .extract()
                    .map_err(|_| {
                        SeasonalityInputValidationError::new_err(
                            "news_intensity_source field 'known_scheduled_event_timestamps_utc' must be an iterable of RFC3339 timestamps",
                        )
                    })?;

            let mut known_scheduled_event_timestamps_utc_nanos: Vec<i64> =
                Vec::with_capacity(raw_known_scheduled_event_timestamps_utc.len());

            for raw_known_timestamp_utc in raw_known_scheduled_event_timestamps_utc {
                let parsed_timestamp_utc = parse_timestamp_utc(&raw_known_timestamp_utc)?;

                known_scheduled_event_timestamps_utc_nanos.push(
                    parsed_timestamp_utc
                        .timestamp_nanos_opt()
                        .ok_or_else(|| {
                            SeasonalityInputValidationError::new_err(format!(
                                "news_intensity_source field 'known_scheduled_event_timestamps_utc' contains an out-of-range timestamp: {raw_known_timestamp_utc}"
                            ))
                        })?,
                );
            }

            let surprise_open_baseline_value = source_mapping
                .contains("surprise_open_baseline_intensity")?
                .then(|| source_mapping.get_item("surprise_open_baseline_intensity"))
                .ok_or_else(|| {
                    SeasonalityInputValidationError::new_err(
                        "news_intensity_source with source_kind='materialized' is missing required field 'surprise_open_baseline_intensity'",
                    )
                })??;

            let surprise_open_baseline_intensity: f64 =
                surprise_open_baseline_value.extract().map_err(|_| {
                    SeasonalityInputValidationError::new_err(
                        "news_intensity_source field 'surprise_open_baseline_intensity' must be a float",
                    )
                })?;

            let surprise_closed_baseline_value = source_mapping
                .contains("surprise_closed_baseline_intensity")?
                .then(|| source_mapping.get_item("surprise_closed_baseline_intensity"))
                .ok_or_else(|| {
                    SeasonalityInputValidationError::new_err(
                        "news_intensity_source with source_kind='materialized' is missing required field 'surprise_closed_baseline_intensity'",
                    )
                })??;

            let surprise_closed_baseline_intensity: f64 =
                surprise_closed_baseline_value.extract().map_err(|_| {
                    SeasonalityInputValidationError::new_err(
                        "news_intensity_source field 'surprise_closed_baseline_intensity' must be a float",
                    )
                })?;

            let surprise_deterministic_time_multiplier_by_interval_index: Vec<(u64, f64)> =
                if source_mapping
                    .contains("surprise_deterministic_time_multiplier_by_interval_index")?
                {
                    let surprise_multiplier_value = source_mapping
                        .get_item("surprise_deterministic_time_multiplier_by_interval_index")?;

                    if let Ok(parsed_mapping) =
                        surprise_multiplier_value.extract::<std::collections::BTreeMap<u64, f64>>()
                    {
                        parsed_mapping.into_iter().collect()
                    } else {
                        let parsed_string_mapping = surprise_multiplier_value
                        .extract::<std::collections::BTreeMap<String, f64>>()
                        .map_err(|_| {
                            SeasonalityInputValidationError::new_err(
                                "news_intensity_source field 'surprise_deterministic_time_multiplier_by_interval_index' must be a mapping of integer interval indices to float multiplier values",
                            )
                        })?;

                        let mut parsed_mapping: Vec<(u64, f64)> = Vec::new();

                        for (raw_interval_index, multiplier_value) in parsed_string_mapping {
                            let parsed_interval_index =
                            raw_interval_index.parse::<u64>().map_err(|_| {
                                SeasonalityInputValidationError::new_err(format!(
                                    "news_intensity_source field 'surprise_deterministic_time_multiplier_by_interval_index' key '{raw_interval_index}' must parse to an integer interval index",
                                ))
                            })?;
                            parsed_mapping.push((parsed_interval_index, multiplier_value));
                        }

                        parsed_mapping.sort_by_key(|(interval_index, _)| *interval_index);

                        parsed_mapping
                    }
                } else {
                    Vec::new()
                };

            Ok(ParsedIntervalProviderNewsIntensitySource::Materialized {
                known_scheduled_event_timestamps_utc_nanos,
                surprise_open_baseline_intensity,
                surprise_closed_baseline_intensity,
                surprise_deterministic_time_multiplier_by_interval_index,
            })
        }
        _ => Err(SeasonalityInputValidationError::new_err(
            "news_intensity_source field 'source_kind' must be either 'explicit' or 'materialized'",
        )),
    }
}

fn map_interval_factor_grid_error(error: IntervalFactorGridError) -> PyErr {
    match error {
        IntervalFactorGridError::MissingExchangeId { exchange_code } => {
            CalendarMissingExchangeError::new_err(format!(
                "missing exchange id `{exchange_code}` in registry"
            ))
        }
        IntervalFactorGridError::InvalidTimezoneName {
            exchange_code,
            timezone_name,
        } => CalendarInvalidTimezoneError::new_err(format!(
            "invalid timezone `{timezone_name}` configured for exchange `{exchange_code}`"
        )),
        IntervalFactorGridError::EmptyExchangeCode => {
            SeasonalityInputValidationError::new_err("exchange_code must be a non-empty string")
        }
        IntervalFactorGridError::EmptySeasonalityMaterializationMethod => {
            SeasonalityInputValidationError::new_err(
                "seasonality_materialization_method must be a non-empty string",
            )
        }
        IntervalFactorGridError::EmptyNewsIntensityMaterializationMethod => {
            SeasonalityInputValidationError::new_err(
                "news_intensity_materialization_method must be a non-empty string",
            )
        }
        IntervalFactorGridError::TimestampOutOfRange { field } => {
            SeasonalityInputValidationError::new_err(format!(
                "{field} must be within the supported nanosecond timestamp range"
            ))
        }
        IntervalFactorGridError::NonIncreasingRange {
            range_start_utc_nanos,
            range_end_utc_nanos,
        } => SeasonalityInputValidationError::new_err(format!(
            "range_start_utc must be earlier than range_end_utc, received start={range_start_utc_nanos}, end={range_end_utc_nanos}"
        )),
        IntervalFactorGridError::InvalidIntervalDurationNanos { value } => {
            SeasonalityInputValidationError::new_err(format!(
                "interval_duration_nanos must be strictly positive, received {value}"
            ))
        }
        IntervalFactorGridError::RangeDurationNotDivisibleByInterval {
            range_start_utc_nanos,
            range_end_utc_nanos,
            interval_duration_nanos,
            remainder_nanos,
        } => SeasonalityInputValidationError::new_err(format!(
            "range [{range_start_utc_nanos}, {range_end_utc_nanos}) must be divisible by interval_duration_nanos={interval_duration_nanos}, remainder={remainder_nanos}"
        )),
        IntervalFactorGridError::FactorLengthMismatch {
            expected_interval_count,
            seasonality_factor_count,
            news_intensity_factor_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "interval factor grid length mismatch, expected {expected_interval_count} intervals and received seasonality_factor={seasonality_factor_count}, news_intensity={news_intensity_factor_count}"
        )),
        IntervalFactorGridError::InvalidSeasonalityFactor {
            interval_index,
            value,
        } => SeasonalityInputValidationError::new_err(format!(
            "seasonality_factor must contain finite positive values, found {value} at interval index {interval_index}"
        )),
        IntervalFactorGridError::InvalidNewsIntensityFactor {
            interval_index,
            value,
        } => SeasonalityInputValidationError::new_err(format!(
            "news_intensity must contain finite non-negative values, found {value} at interval index {interval_index}"
        )),
        IntervalFactorGridError::InvalidKnownScheduledTimestampUtcNanos { value } => {
            SeasonalityInputValidationError::new_err(format!(
                "known_scheduled_event_timestamps_utc_nanos contains an invalid UTC timestamp nanos value {value}"
            ))
        }
        IntervalFactorGridError::KnownScheduledTimestampsNotStrictlyIncreasing {
            previous_timestamp_utc_nanos,
            current_timestamp_utc_nanos,
        } => SeasonalityInputValidationError::new_err(format!(
            "known_scheduled_event_timestamps_utc_nanos must be strictly increasing, found previous={previous_timestamp_utc_nanos} and current={current_timestamp_utc_nanos}"
        )),
        IntervalFactorGridError::InvalidSurpriseOpenBaselineIntensity { value } => {
            SeasonalityInputValidationError::new_err(format!(
                "surprise_open_baseline_intensity must be a finite non-negative value, found {value}"
            ))
        }
        IntervalFactorGridError::InvalidSurpriseClosedBaselineIntensity { value } => {
            SeasonalityInputValidationError::new_err(format!(
                "surprise_closed_baseline_intensity must be a finite non-negative value, found {value}"
            ))
        }
        IntervalFactorGridError::SurpriseDeterministicTimeMultiplierIntervalOutOfRange {
            interval_index,
            interval_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "surprise_deterministic_time_multiplier_by_interval_index contains interval index {interval_index}, expected a value in [0, {interval_count})"
        )),
        IntervalFactorGridError::InvalidSurpriseDeterministicTimeMultiplier {
            interval_index,
            value,
        } => SeasonalityInputValidationError::new_err(format!(
            "surprise_deterministic_time_multiplier_by_interval_index value at interval index {interval_index} must be finite and non-negative, found {value}"
        )),
        IntervalFactorGridError::KnownScheduledArrivalCountOverflow {
            interval_index,
            value,
        } => SeasonalityInputValidationError::new_err(format!(
            "known scheduled arrival count overflowed at interval index {interval_index}, found {value}"
        )),
        IntervalFactorGridError::InvalidKnownComponentNewsIntensityFactor {
            interval_index,
            value,
        } => SeasonalityInputValidationError::new_err(format!(
            "known_news_intensity must contain finite non-negative values, found {value} at interval index {interval_index}"
        )),
        IntervalFactorGridError::InvalidSurpriseComponentNewsIntensityFactor {
            interval_index,
            value,
        } => SeasonalityInputValidationError::new_err(format!(
            "surprise_news_intensity must contain finite non-negative values, found {value} at interval index {interval_index}"
        )),
        IntervalFactorGridError::SourceLengthMismatch {
            source_timestamp_count,
            source_factor_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "seasonality source length mismatch, received timestamp_count={source_timestamp_count}, factor_count={source_factor_count}"
        )),
        IntervalFactorGridError::EmptySeasonalitySourceSignal => {
            SeasonalityInputValidationError::new_err(
                "seasonality source must contain at least one timestamp and factor sample",
            )
        }
        IntervalFactorGridError::InvalidSeasonalitySourceFactor {
            source_position,
            value,
        } => SeasonalityInputValidationError::new_err(format!(
            "seasonality source factors must be finite positive values, found {value} at source position {source_position}"
        )),
        IntervalFactorGridError::InvalidSeasonalityInterpolationMethod { method } => {
            SeasonalityInputValidationError::new_err(format!(
                "invalid seasonality interpolation_method `{method}`, supported values are `previous_observation_carry_forward.v1` and `linear_between_observations.v1`"
            ))
        }
        IntervalFactorGridError::InvalidSeasonalityAggregationMethod { method } => {
            SeasonalityInputValidationError::new_err(format!(
                "invalid seasonality aggregation_method `{method}`, supported values are `mean_of_observations.v1` and `last_observation.v1`"
            ))
        }
        IntervalFactorGridError::InvalidSeasonalitySparseFallbackPolicy { policy } => {
            SeasonalityInputValidationError::new_err(format!(
                "invalid seasonality sparse_fallback_policy `{policy}`, supported values are `global_mean.v1` and `previous_materialized_then_global_mean.v1`"
            ))
        }
        IntervalFactorGridError::InvalidSparseIntervalMinimumObservationCount {
            sparse_interval_minimum_observation_count,
        } => SeasonalityInputValidationError::new_err(format!(
            "sparse_interval_minimum_observation_count must be at least 1, received {sparse_interval_minimum_observation_count}"
        )),
        IntervalFactorGridError::UnexpectedContractFieldValue { .. }
        | IntervalFactorGridError::IntervalFieldLengthMismatch { .. }
        | IntervalFactorGridError::IntervalStartMismatch { .. }
        | IntervalFactorGridError::IntervalIndexMismatch { .. }
        | IntervalFactorGridError::InvalidIntervalBounds { .. }
        | IntervalFactorGridError::NonContiguousIntervals { .. } => {
            SeasonalityInputValidationError::new_err(error.to_string())
        }
    }
}

fn map_interval_factor_grid_provider_error(error: IntervalFactorGridProviderError) -> PyErr {
    match error {
        IntervalFactorGridProviderError::MissingSeasonalitySource
        | IntervalFactorGridProviderError::MissingNewsIntensitySource => {
            IntervalFactorGridProviderMissingInputError::new_err(error.to_string())
        }
        IntervalFactorGridProviderError::UnsupportedRange { source: _ } => {
            IntervalFactorGridProviderUnsupportedRangeError::new_err(error.to_string())
        }
        IntervalFactorGridProviderError::SeasonalitySourceError { source }
        | IntervalFactorGridProviderError::NewsIntensitySourceError { source }
        | IntervalFactorGridProviderError::ContractValidationError { source } => {
            map_interval_factor_grid_error(source)
        }
    }
}

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(build_interval_factor_grid, module)?)?;
    module.add_function(wrap_pyfunction!(load_interval_factor_grid_payload, module)?)?;

    module.add_function(wrap_pyfunction!(
        materialize_interval_seasonality_factors,
        module
    )?)?;

    module.add_function(wrap_pyfunction!(
        materialize_interval_news_intensity_factors,
        module
    )?)?;

    module.add_function(wrap_pyfunction!(provide_interval_factor_grid, module)?)?;

    Ok(())
}
