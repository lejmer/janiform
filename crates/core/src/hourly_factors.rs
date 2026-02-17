//! Canonical hourly factor contract and deterministic interval indexing.
//!
//! The contract is the shared boundary object consumed by downstream engines.
//! It is intentionally independent from pricing and simulation internals.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::Serialize;

use crate::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryLookupError};

pub const HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION: &str = "seasonality.hourly_factor.contract.v1";
pub const HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY: &str = "start_inclusive_end_exclusive.v1";
pub const HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY: &str = "utc_hour_aligned.v1";
pub const DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD: &str = "explicit_hourly_input.v1";

const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;
const ONE_SECOND_NANOS: i64 = 1_000_000_000;

/// Audit metadata emitted with hourly factor materialization outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HourlyFactorMaterializationMetadata {
    pub seasonality_materialization_method: String,
    pub news_intensity_materialization_method: String,
    pub interval_boundary_policy: &'static str,
    pub interval_alignment_policy: &'static str,
}

/// Deterministic diagnostics for hourly factor interval coverage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HourlyFactorContractDiagnostics {
    pub interval_count: usize,
    pub first_interval_start_utc_nanos: i64,
    pub last_interval_end_utc_nanos: i64,
}

/// Canonical hourly factor output contract for downstream engines.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HourlyFactorContractOutput {
    pub schema_version: &'static str,
    pub exchange_code: String,
    pub exchange_timezone_name: String,
    pub request_start_utc_nanos: i64,
    pub request_end_utc_nanos: i64,
    pub interval_indices: Vec<u64>,
    pub interval_start_utc_nanos: Vec<i64>,
    pub interval_end_utc_nanos: Vec<i64>,
    pub interval_start_exchange_local: Vec<String>,
    pub interval_end_exchange_local: Vec<String>,
    pub s_h: Vec<f64>,
    pub nu_h: Vec<f64>,
    pub metadata: HourlyFactorMaterializationMetadata,
    pub diagnostics: HourlyFactorContractDiagnostics,
}

/// Validation failures and contract build errors for hourly factor outputs.
#[derive(Debug, Clone, PartialEq)]
pub enum HourlyFactorContractError {
    EmptyExchangeCode,
    EmptySeasonalityMaterializationMethod,
    EmptyNewsIntensityMaterializationMethod,
    TimestampOutOfRange {
        field: &'static str,
    },
    NonIncreasingRange {
        range_start_utc_nanos: i64,
        range_end_utc_nanos: i64,
    },
    RangeBoundaryNotHourAligned {
        field: &'static str,
        timestamp_utc_nanos: i64,
    },
    MissingExchangeId {
        exchange_code: String,
    },
    InvalidTimezoneName {
        exchange_code: String,
        timezone_name: String,
    },
    FactorLengthMismatch {
        expected_interval_count: usize,
        seasonality_factor_count: usize,
        news_intensity_factor_count: usize,
    },
    InvalidSeasonalityFactor {
        interval_index: usize,
        value: f64,
    },
    InvalidNewsIntensityFactor {
        interval_index: usize,
        value: f64,
    },
}

impl Display for HourlyFactorContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyExchangeCode => {
                write!(formatter, "exchange_code must be a non-empty string")
            }
            Self::EmptySeasonalityMaterializationMethod => write!(
                formatter,
                "seasonality_materialization_method must be a non-empty string"
            ),
            Self::EmptyNewsIntensityMaterializationMethod => write!(
                formatter,
                "news_intensity_materialization_method must be a non-empty string"
            ),
            Self::TimestampOutOfRange { field } => {
                write!(
                    formatter,
                    "{field} must be within the supported nanosecond timestamp range"
                )
            }
            Self::NonIncreasingRange {
                range_start_utc_nanos,
                range_end_utc_nanos,
            } => write!(
                formatter,
                "range_start_utc must be earlier than range_end_utc, received start={range_start_utc_nanos}, end={range_end_utc_nanos}"
            ),
            Self::RangeBoundaryNotHourAligned {
                field,
                timestamp_utc_nanos,
            } => write!(
                formatter,
                "{field} must be aligned to an exact UTC hour boundary, received {timestamp_utc_nanos}"
            ),
            Self::MissingExchangeId { exchange_code } => {
                write!(
                    formatter,
                    "missing exchange id `{exchange_code}` in registry"
                )
            }
            Self::InvalidTimezoneName {
                exchange_code,
                timezone_name,
            } => write!(
                formatter,
                "invalid timezone `{timezone_name}` configured for exchange `{exchange_code}`"
            ),
            Self::FactorLengthMismatch {
                expected_interval_count,
                seasonality_factor_count,
                news_intensity_factor_count,
            } => write!(
                formatter,
                "hourly factor length mismatch, expected {expected_interval_count} intervals and received s_h={seasonality_factor_count}, nu_h={news_intensity_factor_count}"
            ),
            Self::InvalidSeasonalityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "s_h must contain finite positive values, found {value} at interval index {interval_index}"
            ),
            Self::InvalidNewsIntensityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "nu_h must contain finite non-negative values, found {value} at interval index {interval_index}"
            ),
        }
    }
}

impl Error for HourlyFactorContractError {}

/// Build canonical hourly factor output for an exchange and UTC date range.
///
/// Interval policy:
/// - The request range is interpreted as `[range_start_utc, range_end_utc)`.
/// - Both boundaries must be exact UTC hour boundaries.
/// - Buckets are emitted as consecutive non-overlapping intervals `[start, end)`.
/// - `interval_indices` are zero-based and strictly increasing by one.
pub fn build_hourly_factor_contract(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    range_start_utc: DateTime<Utc>,
    range_end_utc: DateTime<Utc>,
    seasonality_factors: &[f64],
    news_intensity_factors: &[f64],
    seasonality_materialization_method: &str,
    news_intensity_materialization_method: &str,
) -> Result<HourlyFactorContractOutput, HourlyFactorContractError> {
    if exchange_code.trim().is_empty() {
        return Err(HourlyFactorContractError::EmptyExchangeCode);
    }
    if seasonality_materialization_method.trim().is_empty() {
        return Err(HourlyFactorContractError::EmptySeasonalityMaterializationMethod);
    }
    if news_intensity_materialization_method.trim().is_empty() {
        return Err(HourlyFactorContractError::EmptyNewsIntensityMaterializationMethod);
    }

    let range_start_utc_nanos = timestamp_to_utc_nanos(range_start_utc, "range_start_utc")?;
    let range_end_utc_nanos = timestamp_to_utc_nanos(range_end_utc, "range_end_utc")?;
    if range_start_utc_nanos >= range_end_utc_nanos {
        return Err(HourlyFactorContractError::NonIncreasingRange {
            range_start_utc_nanos,
            range_end_utc_nanos,
        });
    }
    if range_start_utc_nanos % ONE_HOUR_NANOS != 0 {
        return Err(HourlyFactorContractError::RangeBoundaryNotHourAligned {
            field: "range_start_utc",
            timestamp_utc_nanos: range_start_utc_nanos,
        });
    }
    if range_end_utc_nanos % ONE_HOUR_NANOS != 0 {
        return Err(HourlyFactorContractError::RangeBoundaryNotHourAligned {
            field: "range_end_utc",
            timestamp_utc_nanos: range_end_utc_nanos,
        });
    }

    let interval_count = ((range_end_utc_nanos - range_start_utc_nanos) / ONE_HOUR_NANOS) as usize;
    if seasonality_factors.len() != interval_count || news_intensity_factors.len() != interval_count
    {
        return Err(HourlyFactorContractError::FactorLengthMismatch {
            expected_interval_count: interval_count,
            seasonality_factor_count: seasonality_factors.len(),
            news_intensity_factor_count: news_intensity_factors.len(),
        });
    }

    for (interval_index, seasonality_factor) in seasonality_factors.iter().copied().enumerate() {
        if !seasonality_factor.is_finite() || seasonality_factor <= 0.0 {
            return Err(HourlyFactorContractError::InvalidSeasonalityFactor {
                interval_index,
                value: seasonality_factor,
            });
        }
    }
    for (interval_index, news_intensity_factor) in
        news_intensity_factors.iter().copied().enumerate()
    {
        if !news_intensity_factor.is_finite() || news_intensity_factor < 0.0 {
            return Err(HourlyFactorContractError::InvalidNewsIntensityFactor {
                interval_index,
                value: news_intensity_factor,
            });
        }
    }

    let definition = calendar_registry
        .get_required(exchange_code)
        .map_err(map_registry_lookup_error)?;
    let exchange_timezone_name = definition.timezone_name().to_string();
    let timezone = Tz::from_str(definition.timezone_name()).map_err(|_| {
        HourlyFactorContractError::InvalidTimezoneName {
            exchange_code: exchange_code.to_string(),
            timezone_name: definition.timezone_name().to_string(),
        }
    })?;

    let mut interval_indices: Vec<u64> = Vec::with_capacity(interval_count);
    let mut interval_start_utc_nanos: Vec<i64> = Vec::with_capacity(interval_count);
    let mut interval_end_utc_nanos: Vec<i64> = Vec::with_capacity(interval_count);
    let mut interval_start_exchange_local: Vec<String> = Vec::with_capacity(interval_count);
    let mut interval_end_exchange_local: Vec<String> = Vec::with_capacity(interval_count);
    for interval_index in 0..interval_count {
        let interval_offset_nanos = interval_index as i64 * ONE_HOUR_NANOS;
        let interval_start_nanos = range_start_utc_nanos + interval_offset_nanos;
        let interval_end_nanos = interval_start_nanos + ONE_HOUR_NANOS;

        let start_utc = utc_timestamp_from_nanos(interval_start_nanos, "range_start_utc")?;
        let end_utc = utc_timestamp_from_nanos(interval_end_nanos, "range_end_utc")?;
        interval_indices.push(interval_index as u64);
        interval_start_utc_nanos.push(interval_start_nanos);
        interval_end_utc_nanos.push(interval_end_nanos);
        interval_start_exchange_local.push(start_utc.with_timezone(&timezone).to_rfc3339());
        interval_end_exchange_local.push(end_utc.with_timezone(&timezone).to_rfc3339());
    }

    Ok(HourlyFactorContractOutput {
        schema_version: HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
        exchange_code: exchange_code.to_string(),
        exchange_timezone_name,
        request_start_utc_nanos: range_start_utc_nanos,
        request_end_utc_nanos: range_end_utc_nanos,
        interval_indices,
        interval_start_utc_nanos,
        interval_end_utc_nanos,
        interval_start_exchange_local,
        interval_end_exchange_local,
        s_h: seasonality_factors.to_vec(),
        nu_h: news_intensity_factors.to_vec(),
        metadata: HourlyFactorMaterializationMetadata {
            seasonality_materialization_method: seasonality_materialization_method.to_string(),
            news_intensity_materialization_method: news_intensity_materialization_method
                .to_string(),
            interval_boundary_policy: HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
            interval_alignment_policy: HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
        },
        diagnostics: HourlyFactorContractDiagnostics {
            interval_count,
            first_interval_start_utc_nanos: range_start_utc_nanos,
            last_interval_end_utc_nanos: range_end_utc_nanos,
        },
    })
}

fn map_registry_lookup_error(
    source: ExchangeCalendarRegistryLookupError,
) -> HourlyFactorContractError {
    match source {
        ExchangeCalendarRegistryLookupError::MissingExchangeId { exchange_code } => {
            HourlyFactorContractError::MissingExchangeId { exchange_code }
        }
    }
}

fn timestamp_to_utc_nanos(
    timestamp_utc: DateTime<Utc>,
    field: &'static str,
) -> Result<i64, HourlyFactorContractError> {
    timestamp_utc
        .timestamp_nanos_opt()
        .ok_or(HourlyFactorContractError::TimestampOutOfRange { field })
}

fn utc_timestamp_from_nanos(
    timestamp_utc_nanos: i64,
    field: &'static str,
) -> Result<DateTime<Utc>, HourlyFactorContractError> {
    let seconds = timestamp_utc_nanos.div_euclid(ONE_SECOND_NANOS);
    let subsecond_nanos = timestamp_utc_nanos.rem_euclid(ONE_SECOND_NANOS) as u32;
    DateTime::from_timestamp(seconds, subsecond_nanos)
        .ok_or(HourlyFactorContractError::TimestampOutOfRange { field })
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use crate::calendar::ExchangeCalendarRegistryLoader;
    use crate::hourly_factors::{
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD, HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
        HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY, HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
        HourlyFactorContractError, build_hourly_factor_contract,
    };

    #[test]
    fn hourly_factor_contract_builds_expected_schema_and_metadata() {
        let registry = build_calendar_registry();
        let output = build_hourly_factor_contract(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-02T14:00:00Z"),
            parse_timestamp("2026-02-02T17:00:00Z"),
            &[1.0, 1.1, 0.9],
            &[0.4, 0.0, 0.2],
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
            "calendar_closed_window_baseline.v1",
        )
        .expect("hourly factor contract build should succeed");

        assert_eq!(output.schema_version, HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION);
        assert_eq!(output.exchange_code, "XNYS");
        assert_eq!(output.exchange_timezone_name, "America/New_York");
        assert_eq!(
            output.metadata.interval_boundary_policy,
            HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY
        );
        assert_eq!(
            output.metadata.interval_alignment_policy,
            HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY
        );
        assert_eq!(
            output.metadata.seasonality_materialization_method,
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD
        );
        assert_eq!(
            output.metadata.news_intensity_materialization_method,
            "calendar_closed_window_baseline.v1"
        );
        assert_eq!(output.diagnostics.interval_count, 3);
    }

    #[test]
    fn hourly_factor_contract_intervals_are_consecutive_and_non_overlapping() {
        let registry = build_calendar_registry();
        let output = build_hourly_factor_contract(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-02T14:00:00Z"),
            parse_timestamp("2026-02-02T18:00:00Z"),
            &[1.0, 1.0, 1.0, 1.0],
            &[0.0, 0.0, 0.0, 0.0],
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        )
        .expect("hourly factor contract build should succeed");

        assert_eq!(output.interval_indices, vec![0, 1, 2, 3]);
        for interval_position in 1..output.interval_start_utc_nanos.len() {
            assert_eq!(
                output.interval_start_utc_nanos[interval_position],
                output.interval_end_utc_nanos[interval_position - 1]
            );
        }
        for interval_position in 0..output.interval_start_utc_nanos.len() {
            let interval_start = output.interval_start_utc_nanos[interval_position];
            let interval_end = output.interval_end_utc_nanos[interval_position];
            assert!(interval_start < interval_end);
            assert_eq!(interval_end - interval_start, 3_600_000_000_000);
        }
    }

    #[test]
    fn hourly_factor_contract_rejects_invalid_ranges_and_unknown_exchange_ids() {
        let registry = build_calendar_registry();

        let non_increasing_range_error = build_hourly_factor_contract(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-02T14:00:00Z"),
            parse_timestamp("2026-02-02T14:00:00Z"),
            &[],
            &[],
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        )
        .expect_err("non-increasing range must fail");
        assert!(matches!(
            non_increasing_range_error,
            HourlyFactorContractError::NonIncreasingRange { .. }
        ));

        let unaligned_range_error = build_hourly_factor_contract(
            &registry,
            "XNYS",
            parse_timestamp("2026-02-02T14:30:00Z"),
            parse_timestamp("2026-02-02T16:00:00Z"),
            &[1.0],
            &[0.0],
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        )
        .expect_err("hour-unaligned range must fail");
        assert!(matches!(
            unaligned_range_error,
            HourlyFactorContractError::RangeBoundaryNotHourAligned { .. }
        ));

        let missing_exchange_error = build_hourly_factor_contract(
            &registry,
            "MISSING",
            parse_timestamp("2026-02-02T14:00:00Z"),
            parse_timestamp("2026-02-02T16:00:00Z"),
            &[1.0, 1.0],
            &[0.0, 0.0],
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
            DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        )
        .expect_err("missing exchange id must fail");
        assert!(matches!(
            missing_exchange_error,
            HourlyFactorContractError::MissingExchangeId { .. }
        ));
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
