use chrono::{DateTime, Utc};
use janiform_foundations::time::NANOS_PER_HOUR;

use crate::calendar::ExchangeCalendarRegistryLoader;
use crate::interval_factor_grids::IntervalFactorGridError;
use crate::interval_factor_grids::{
    IntervalFactorGridProviderError, IntervalFactorGridProviderRequest,
    IntervalNewsIntensityFactorSource, IntervalSeasonalityFactorSource,
    provide_interval_factor_grid,
};
#[test]
fn provider_returns_structured_error_for_missing_seasonality_source() {
    let registry = build_calendar_registry();

    let request = IntervalFactorGridProviderRequest {
        exchange_code: "XNYS",
        range_start_utc: parse_timestamp("2026-02-02T14:00:00Z"),
        range_end_utc: parse_timestamp("2026-02-02T16:00:00Z"),
        interval_duration_nanos: NANOS_PER_HOUR,
        seasonality_source: None,
        news_intensity_source: Some(IntervalNewsIntensityFactorSource::Explicit {
            interval_factors: &[0.1, 0.0],
            materialization_method: "explicit_interval_grid_input.v1",
        }),
    };

    let error = provide_interval_factor_grid(&registry, request)
        .expect_err("missing seasonality source must fail");

    assert!(matches!(
        error,
        IntervalFactorGridProviderError::MissingSeasonalitySource
    ));
}

#[test]
fn provider_returns_structured_error_for_missing_news_intensity_source() {
    let registry = build_calendar_registry();

    let request = IntervalFactorGridProviderRequest {
        exchange_code: "XNYS",
        range_start_utc: parse_timestamp("2026-02-02T14:00:00Z"),
        range_end_utc: parse_timestamp("2026-02-02T16:00:00Z"),
        interval_duration_nanos: NANOS_PER_HOUR,
        seasonality_source: Some(IntervalSeasonalityFactorSource::Explicit {
            interval_factors: &[1.0, 1.0],
            materialization_method: "explicit_interval_grid_input.v1",
        }),
        news_intensity_source: None,
    };

    let error = provide_interval_factor_grid(&registry, request)
        .expect_err("missing news intensity source must fail");

    assert!(matches!(
        error,
        IntervalFactorGridProviderError::MissingNewsIntensitySource
    ));
}

#[test]
fn provider_returns_structured_error_for_unsupported_range_requests() {
    let registry = build_calendar_registry();

    let request = IntervalFactorGridProviderRequest {
        exchange_code: "XNYS",
        range_start_utc: parse_timestamp("2026-02-02T14:30:00Z"),
        range_end_utc: parse_timestamp("2026-02-02T16:00:00Z"),
        interval_duration_nanos: NANOS_PER_HOUR,
        seasonality_source: Some(IntervalSeasonalityFactorSource::Explicit {
            interval_factors: &[1.0],
            materialization_method: "explicit_interval_grid_input.v1",
        }),
        news_intensity_source: Some(IntervalNewsIntensityFactorSource::Explicit {
            interval_factors: &[0.0],
            materialization_method: "explicit_interval_grid_input.v1",
        }),
    };

    let error = provide_interval_factor_grid(&registry, request)
        .expect_err("range duration must divide evenly by interval width");

    assert!(matches!(
        error,
        IntervalFactorGridProviderError::UnsupportedRange {
            source: IntervalFactorGridError::RangeDurationNotDivisibleByInterval { .. }
        }
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
