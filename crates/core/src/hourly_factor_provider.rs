//! Shared hourly factor provider used by pricing and simulation consumers.
//!
//! This module centralizes factor source handling so downstream engines reuse one
//! interval-aligned `s_h` and `nu_h` integration path.

use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};

use crate::calendar::ExchangeCalendarRegistry;
use crate::hourly_factors::{
    HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
    HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION, HourlyFactorContractError,
    HourlyFactorContractOutput, build_hourly_factor_contract,
    materialize_hourly_news_intensity_factors, materialize_hourly_seasonality_factors,
};

/// Stable provider method identifier when `s_h` is materialized from source samples.
pub const HOURLY_FACTOR_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD: &str =
    HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION;

/// Stable provider method identifier when `nu_h` is materialized from calendar policy.
pub const HOURLY_FACTOR_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD: &str =
    HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION;

/// Seasonality source accepted by the shared hourly factor provider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HourlySeasonalityFactorSource<'a> {
    /// Use already materialized hourly values.
    Explicit {
        hourly_factors: &'a [f64],
        materialization_method: &'a str,
    },

    /// Materialize hourly values from timestamped seasonality samples.
    Materialized {
        source_timestamp_utc_nanos: &'a [i64],
        source_seasonality_factors: &'a [f64],
        interpolation_method: &'a str,
        aggregation_method: &'a str,
        sparse_fallback_policy: &'a str,
        sparse_hour_minimum_observation_count: usize,
    },
}

/// News intensity source accepted by the shared hourly factor provider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HourlyNewsIntensityFactorSource<'a> {
    /// Use already materialized hourly values.
    Explicit {
        hourly_factors: &'a [f64],
        materialization_method: &'a str,
    },

    /// Materialize hourly values from calendar policy and optional open-window overrides.
    Materialized {
        closed_window_baseline_intensity: f64,
        open_window_override_by_interval_index: &'a [(u64, f64)],
    },
}

/// Request model for the shared hourly factor provider.
#[derive(Debug, Clone, PartialEq)]
pub struct HourlyFactorProviderRequest<'a> {
    pub exchange_code: &'a str,
    pub range_start_utc: DateTime<Utc>,
    pub range_end_utc: DateTime<Utc>,
    pub seasonality_source: Option<HourlySeasonalityFactorSource<'a>>,
    pub news_intensity_source: Option<HourlyNewsIntensityFactorSource<'a>>,
}

/// Structured provider failures for missing inputs and invalid request ranges.
#[derive(Debug, Clone, PartialEq)]
pub enum HourlyFactorProviderError {
    MissingSeasonalitySource,
    MissingNewsIntensitySource,
    UnsupportedRange { source: HourlyFactorContractError },
    SeasonalitySourceError { source: HourlyFactorContractError },
    NewsIntensitySourceError { source: HourlyFactorContractError },
    ContractValidationError { source: HourlyFactorContractError },
}

impl Display for HourlyFactorProviderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSeasonalitySource => write!(
                formatter,
                "seasonality_source is required, provide explicit `s_h` values or materialization input",
            ),
            Self::MissingNewsIntensitySource => write!(
                formatter,
                "news_intensity_source is required, provide explicit `nu_h` values or materialization input",
            ),
            Self::UnsupportedRange { source } => {
                write!(
                    formatter,
                    "hourly factor provider request range is unsupported: {source}"
                )
            }
            Self::SeasonalitySourceError { source } => {
                write!(formatter, "failed to resolve seasonality source: {source}")
            }
            Self::NewsIntensitySourceError { source } => {
                write!(
                    formatter,
                    "failed to resolve news intensity source: {source}"
                )
            }
            Self::ContractValidationError { source } => {
                write!(
                    formatter,
                    "failed to build hourly factor contract: {source}"
                )
            }
        }
    }
}

impl Error for HourlyFactorProviderError {}

/// Resolve interval-aligned hourly factors through one shared provider API.
pub fn provide_hourly_factor_contract(
    calendar_registry: &ExchangeCalendarRegistry,
    provider_request: HourlyFactorProviderRequest<'_>,
) -> Result<HourlyFactorContractOutput, HourlyFactorProviderError> {
    let HourlyFactorProviderRequest {
        exchange_code,
        range_start_utc,
        range_end_utc,
        seasonality_source,
        news_intensity_source,
    } = provider_request;

    let resolved_seasonality_source =
        seasonality_source.ok_or(HourlyFactorProviderError::MissingSeasonalitySource)?;
    let resolved_news_intensity_source =
        news_intensity_source.ok_or(HourlyFactorProviderError::MissingNewsIntensitySource)?;

    let (seasonality_hourly_factors, seasonality_materialization_method) =
        match resolved_seasonality_source {
            HourlySeasonalityFactorSource::Explicit {
                hourly_factors,
                materialization_method,
            } => (hourly_factors.to_vec(), materialization_method.to_string()),
            HourlySeasonalityFactorSource::Materialized {
                source_timestamp_utc_nanos,
                source_seasonality_factors,
                interpolation_method,
                aggregation_method,
                sparse_fallback_policy,
                sparse_hour_minimum_observation_count,
            } => {
                let seasonality_materialization_output = materialize_hourly_seasonality_factors(
                    calendar_registry,
                    exchange_code,
                    range_start_utc,
                    range_end_utc,
                    source_timestamp_utc_nanos,
                    source_seasonality_factors,
                    interpolation_method,
                    aggregation_method,
                    sparse_fallback_policy,
                    sparse_hour_minimum_observation_count,
                )
                .map_err(map_seasonality_source_error)?;
                (
                    seasonality_materialization_output.s_h,
                    HOURLY_FACTOR_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD.to_string(),
                )
            }
        };

    let (news_intensity_hourly_factors, news_intensity_materialization_method) =
        match resolved_news_intensity_source {
            HourlyNewsIntensityFactorSource::Explicit {
                hourly_factors,
                materialization_method,
            } => (hourly_factors.to_vec(), materialization_method.to_string()),
            HourlyNewsIntensityFactorSource::Materialized {
                closed_window_baseline_intensity,
                open_window_override_by_interval_index,
            } => {
                let news_materialization_output = materialize_hourly_news_intensity_factors(
                    calendar_registry,
                    exchange_code,
                    range_start_utc,
                    range_end_utc,
                    closed_window_baseline_intensity,
                    open_window_override_by_interval_index,
                )
                .map_err(map_news_source_error)?;
                (
                    news_materialization_output.nu_h,
                    HOURLY_FACTOR_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD.to_string(),
                )
            }
        };

    build_hourly_factor_contract(
        calendar_registry,
        exchange_code,
        range_start_utc,
        range_end_utc,
        &seasonality_hourly_factors,
        &news_intensity_hourly_factors,
        &seasonality_materialization_method,
        &news_intensity_materialization_method,
    )
    .map_err(map_contract_validation_error)
}

fn map_seasonality_source_error(source: HourlyFactorContractError) -> HourlyFactorProviderError {
    if is_unsupported_range_error(&source) {
        return HourlyFactorProviderError::UnsupportedRange { source };
    }

    HourlyFactorProviderError::SeasonalitySourceError { source }
}

fn map_news_source_error(source: HourlyFactorContractError) -> HourlyFactorProviderError {
    if is_unsupported_range_error(&source) {
        return HourlyFactorProviderError::UnsupportedRange { source };
    }

    HourlyFactorProviderError::NewsIntensitySourceError { source }
}

fn map_contract_validation_error(source: HourlyFactorContractError) -> HourlyFactorProviderError {
    if is_unsupported_range_error(&source) {
        return HourlyFactorProviderError::UnsupportedRange { source };
    }

    HourlyFactorProviderError::ContractValidationError { source }
}

fn is_unsupported_range_error(error: &HourlyFactorContractError) -> bool {
    matches!(
        error,
        HourlyFactorContractError::TimestampOutOfRange { .. }
            | HourlyFactorContractError::NonIncreasingRange { .. }
            | HourlyFactorContractError::RangeBoundaryNotHourAligned { .. }
    )
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use crate::calendar::ExchangeCalendarRegistryLoader;
    use crate::calibration::{
        evolve_hawkes_state_for_calibration_step, load_hourly_factor_contract_for_calibration,
        project_hawkes_intensities_for_calibration_interval,
    };
    use crate::hawkes::{
        HawkesEventSide, HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
        HawkesTransitionStep,
    };
    use crate::hourly_factor_provider::{
        HOURLY_FACTOR_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD,
        HOURLY_FACTOR_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD, HourlyFactorProviderError,
        HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource,
        HourlySeasonalityFactorSource, provide_hourly_factor_contract,
    };
    use crate::hourly_factors::HourlyFactorContractError;
    use crate::pricing::{
        evolve_hawkes_state_for_coarse_interval_step, load_hourly_factor_contract_for_pricing,
        project_hawkes_intensities_for_coarse_interval,
    };
    use crate::simulation::{
        evolve_hawkes_state_for_simulation_step, load_hourly_factor_contract_for_simulation,
        project_hawkes_intensities_for_simulation_interval,
    };

    #[test]
    fn provider_returns_structured_error_for_missing_seasonality_source() {
        let registry = build_calendar_registry();

        let request = HourlyFactorProviderRequest {
            exchange_code: "XNYS",
            range_start_utc: parse_timestamp("2026-02-02T14:00:00Z"),
            range_end_utc: parse_timestamp("2026-02-02T16:00:00Z"),
            seasonality_source: None,
            news_intensity_source: Some(HourlyNewsIntensityFactorSource::Explicit {
                hourly_factors: &[0.1, 0.0],
                materialization_method: "explicit_hourly_input.v1",
            }),
        };

        let error = provide_hourly_factor_contract(&registry, request)
            .expect_err("missing seasonality source must fail");

        assert!(matches!(
            error,
            HourlyFactorProviderError::MissingSeasonalitySource
        ));
    }

    #[test]
    fn provider_returns_structured_error_for_missing_news_intensity_source() {
        let registry = build_calendar_registry();

        let request = HourlyFactorProviderRequest {
            exchange_code: "XNYS",
            range_start_utc: parse_timestamp("2026-02-02T14:00:00Z"),
            range_end_utc: parse_timestamp("2026-02-02T16:00:00Z"),
            seasonality_source: Some(HourlySeasonalityFactorSource::Explicit {
                hourly_factors: &[1.0, 1.0],
                materialization_method: "explicit_hourly_input.v1",
            }),
            news_intensity_source: None,
        };

        let error = provide_hourly_factor_contract(&registry, request)
            .expect_err("missing news intensity source must fail");

        assert!(matches!(
            error,
            HourlyFactorProviderError::MissingNewsIntensitySource
        ));
    }

    #[test]
    fn provider_returns_structured_error_for_unsupported_range_requests() {
        let registry = build_calendar_registry();

        let request = HourlyFactorProviderRequest {
            exchange_code: "XNYS",
            range_start_utc: parse_timestamp("2026-02-02T14:30:00Z"),
            range_end_utc: parse_timestamp("2026-02-02T16:00:00Z"),
            seasonality_source: Some(HourlySeasonalityFactorSource::Explicit {
                hourly_factors: &[1.0],
                materialization_method: "explicit_hourly_input.v1",
            }),
            news_intensity_source: Some(HourlyNewsIntensityFactorSource::Explicit {
                hourly_factors: &[0.0],
                materialization_method: "explicit_hourly_input.v1",
            }),
        };

        let error = provide_hourly_factor_contract(&registry, request)
            .expect_err("hour-unaligned range must fail");

        assert!(matches!(
            error,
            HourlyFactorProviderError::UnsupportedRange {
                source: HourlyFactorContractError::RangeBoundaryNotHourAligned { .. }
            }
        ));
    }

    #[test]
    fn pricing_path_consumes_shared_hourly_factor_provider() {
        let registry = build_calendar_registry();

        let request = HourlyFactorProviderRequest {
            exchange_code: "XNYS",
            range_start_utc: parse_timestamp("2026-02-02T14:00:00Z"),
            range_end_utc: parse_timestamp("2026-02-02T17:00:00Z"),
            seasonality_source: Some(HourlySeasonalityFactorSource::Materialized {
                source_timestamp_utc_nanos: &[
                    parse_timestamp("2026-02-02T14:05:00Z")
                        .timestamp_nanos_opt()
                        .expect("timestamp fixture must be in range"),
                    parse_timestamp("2026-02-02T14:20:00Z")
                        .timestamp_nanos_opt()
                        .expect("timestamp fixture must be in range"),
                    parse_timestamp("2026-02-02T15:10:00Z")
                        .timestamp_nanos_opt()
                        .expect("timestamp fixture must be in range"),
                    parse_timestamp("2026-02-02T16:05:00Z")
                        .timestamp_nanos_opt()
                        .expect("timestamp fixture must be in range"),
                ],
                source_seasonality_factors: &[1.0, 1.2, 0.8, 1.4],
                interpolation_method: "previous_observation_carry_forward.v1",
                aggregation_method: "mean_of_observations.v1",
                sparse_fallback_policy: "global_mean.v1",
                sparse_hour_minimum_observation_count: 1,
            }),
            news_intensity_source: Some(HourlyNewsIntensityFactorSource::Materialized {
                closed_window_baseline_intensity: 0.6,
                open_window_override_by_interval_index: &[(0, 1.0), (1, 1.0)],
            }),
        };

        let output = load_hourly_factor_contract_for_pricing(&registry, request)
            .expect("pricing path should resolve hourly factors from provider");

        assert_eq!(output.s_h, vec![1.1, 0.8, 1.4]);
        assert_eq!(output.nu_h, vec![0.8, 1.0, 0.0]);
        assert_eq!(
            output.metadata.seasonality_materialization_method,
            HOURLY_FACTOR_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD
        );
        assert_eq!(
            output.metadata.news_intensity_materialization_method,
            HOURLY_FACTOR_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD
        );
    }

    #[test]
    fn simulation_path_consumes_shared_hourly_factor_provider() {
        let registry = build_calendar_registry();

        let request = HourlyFactorProviderRequest {
            exchange_code: "XNYS",
            range_start_utc: parse_timestamp("2026-02-02T14:00:00Z"),
            range_end_utc: parse_timestamp("2026-02-02T17:00:00Z"),
            seasonality_source: Some(HourlySeasonalityFactorSource::Explicit {
                hourly_factors: &[1.0, 0.9, 1.1],
                materialization_method: "shared_precomputed_hourly_s_h.v1",
            }),
            news_intensity_source: Some(HourlyNewsIntensityFactorSource::Explicit {
                hourly_factors: &[0.2, 0.0, 0.3],
                materialization_method: "shared_precomputed_hourly_nu_h.v1",
            }),
        };

        let output = load_hourly_factor_contract_for_simulation(&registry, request)
            .expect("simulation path should resolve hourly factors from provider");

        assert_eq!(output.s_h, vec![1.0, 0.9, 1.1]);
        assert_eq!(output.nu_h, vec![0.2, 0.0, 0.3]);
        assert_eq!(
            output.metadata.seasonality_materialization_method,
            "shared_precomputed_hourly_s_h.v1"
        );
        assert_eq!(
            output.metadata.news_intensity_materialization_method,
            "shared_precomputed_hourly_nu_h.v1"
        );
    }

    #[test]
    fn calibration_path_consumes_shared_hourly_factor_provider() {
        let registry = build_calendar_registry();

        let request = HourlyFactorProviderRequest {
            exchange_code: "XNYS",
            range_start_utc: parse_timestamp("2026-02-02T14:00:00Z"),
            range_end_utc: parse_timestamp("2026-02-02T17:00:00Z"),
            seasonality_source: Some(HourlySeasonalityFactorSource::Explicit {
                hourly_factors: &[1.3, 0.95, 1.05],
                materialization_method: "shared_precomputed_hourly_s_h.v1",
            }),
            news_intensity_source: Some(HourlyNewsIntensityFactorSource::Explicit {
                hourly_factors: &[0.0, 0.4, 0.1],
                materialization_method: "shared_precomputed_hourly_nu_h.v1",
            }),
        };

        let output = load_hourly_factor_contract_for_calibration(&registry, request)
            .expect("calibration path should resolve hourly factors from provider");

        assert_eq!(output.s_h, vec![1.3, 0.95, 1.05]);
        assert_eq!(output.nu_h, vec![0.0, 0.4, 0.1]);
    }

    #[test]
    fn simulation_coarse_and_calibration_paths_share_one_hawkes_intensity_projection_api() {
        let registry = build_calendar_registry();
        let request = HourlyFactorProviderRequest {
            exchange_code: "XNYS",
            range_start_utc: parse_timestamp("2026-02-02T14:00:00Z"),
            range_end_utc: parse_timestamp("2026-02-02T17:00:00Z"),
            seasonality_source: Some(HourlySeasonalityFactorSource::Explicit {
                hourly_factors: &[1.0, 0.9, 1.1],
                materialization_method: "shared_precomputed_hourly_s_h.v1",
            }),
            news_intensity_source: Some(HourlyNewsIntensityFactorSource::Explicit {
                hourly_factors: &[0.2, 0.0, 0.3],
                materialization_method: "shared_precomputed_hourly_nu_h.v1",
            }),
        };
        let hourly_factor_contract = load_hourly_factor_contract_for_simulation(&registry, request)
            .expect("shared provider output should be valid for projection use");
        let state = HawkesExcitationState {
            y_plus_plus: 0.20,
            y_plus_minus: 0.10,
            y_minus_plus: 0.30,
            y_minus_minus: 0.05,
        };

        let parameters = HawkesKernelParameters::try_new(
            0.50,
            1.1,
            HawkesKernelLoadings {
                plus_plus: 0.25,
                plus_minus: 0.20,
                minus_plus: 0.12,
                minus_minus: 0.30,
            },
            HawkesKernelLoadings {
                plus_plus: 0.05,
                plus_minus: 0.10,
                minus_plus: 0.08,
                minus_minus: 0.04,
            },
        )
        .expect("parameter fixture must be valid");

        let simulation_projection = project_hawkes_intensities_for_simulation_interval(
            state,
            parameters,
            &hourly_factor_contract,
            1,
        )
        .expect("simulation projection should resolve interval seasonality");

        let coarse_projection = project_hawkes_intensities_for_coarse_interval(
            state,
            parameters,
            &hourly_factor_contract,
            1,
        )
        .expect("coarse projection should resolve interval seasonality");
        let calibration_projection = project_hawkes_intensities_for_calibration_interval(
            state,
            parameters,
            &hourly_factor_contract,
            1,
        )
        .expect("calibration projection should resolve interval seasonality");

        assert_eq!(simulation_projection, coarse_projection);
        assert_eq!(simulation_projection, calibration_projection);
        assert!((simulation_projection.deseasonalized.lambda_plus - 0.8).abs() <= 1e-12);
        assert!((simulation_projection.deseasonalized.lambda_minus - 0.85).abs() <= 1e-12);
        assert!((simulation_projection.seasonality_scaled.lambda_plus - 0.72).abs() <= 1e-12);
        assert!((simulation_projection.seasonality_scaled.lambda_minus - 0.765).abs() <= 1e-12);
    }

    #[test]
    fn simulation_coarse_and_calibration_paths_share_one_hawkes_transition_api() {
        let initial_state = HawkesExcitationState {
            y_plus_plus: 0.20,
            y_plus_minus: 0.10,
            y_minus_plus: 0.30,
            y_minus_minus: 0.05,
        };

        let kernel_parameters = HawkesKernelParameters::try_new(
            0.50,
            1.1,
            HawkesKernelLoadings {
                plus_plus: 0.25,
                plus_minus: 0.20,
                minus_plus: 0.12,
                minus_minus: 0.30,
            },
            HawkesKernelLoadings {
                plus_plus: 0.05,
                plus_minus: 0.10,
                minus_plus: 0.08,
                minus_minus: 0.04,
            },
        )
        .expect("kernel fixture must be valid");

        let transition_step =
            HawkesTransitionStep::new(0.25, Some(HawkesEventSide::Buy), Some(1.5));

        let reference_state = initial_state
            .apply_transition_step(kernel_parameters, transition_step)
            .expect("reference transition must succeed");
        let simulation_state = evolve_hawkes_state_for_simulation_step(
            initial_state,
            kernel_parameters,
            transition_step,
        )
        .expect("simulation transition must use canonical transition API");

        let coarse_state = evolve_hawkes_state_for_coarse_interval_step(
            initial_state,
            kernel_parameters,
            transition_step,
        )
        .expect("coarse transition must use canonical transition API");

        let calibration_state = evolve_hawkes_state_for_calibration_step(
            initial_state,
            kernel_parameters,
            transition_step,
        )
        .expect("calibration transition must use canonical transition API");

        assert_eq!(simulation_state, reference_state);
        assert_eq!(coarse_state, reference_state);
        assert_eq!(calibration_state, reference_state);
    }

    fn build_calendar_registry() -> crate::calendar::ExchangeCalendarRegistry {
        ExchangeCalendarRegistryLoader::load_from_json(
            r#"
            {
              "schema_version": "calendar.registry.v1",
              "artifact_date": "2026-01-31",
              "exchanges": [
                {
                  "schema_version": "calendar.v1",
                  "exchange_code": "XNYS",
                  "exchange_name": "New York Stock Exchange",
                  "timezone_name": "America/New_York",
                  "rule_metadata": {
                    "holiday_calendar_code": "US_EQUITIES",
                    "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
                  },
                  "regular_sessions": [
                    {
                      "weekday": "Monday",
                      "windows": [
                        {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                      ]
                    },
                    {
                      "weekday": "Tuesday",
                      "windows": [
                        {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                      ]
                    },
                    {
                      "weekday": "Wednesday",
                      "windows": [
                        {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                      ]
                    },
                    {
                      "weekday": "Thursday",
                      "windows": [
                        {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                      ]
                    },
                    {
                      "weekday": "Friday",
                      "windows": [
                        {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
                      ]
                    }
                  ],
                  "holiday_dates": [],
                  "early_close_sessions": []
                }
              ]
            }
            "#,
        )
        .expect("fixture calendar registry should load")
    }

    fn parse_timestamp(raw_timestamp: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw_timestamp)
            .expect("timestamp fixture must parse")
            .with_timezone(&Utc)
    }
}
