use janiform_foundations::time::{NANOS_PER_HOUR, utc_nanos};
use janiform_market_time::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryLoader};

use super::{
    RuntimeHourlySeasonalityFactorsBuildInput,
    build_runtime_hourly_seasonality_factors_from_components,
};

#[test]
fn runtime_materialization_keeps_closed_hours_zero_without_calendar_mean_scaling() {
    let calendar_registry = partial_day_utc_calendar_registry();

    let factors = build_runtime_hourly_seasonality_factors_from_components(
        RuntimeHourlySeasonalityFactorsBuildInput {
            calendar_registry: &calendar_registry,
            exchange_code: "XUTC",
            intraday_minutes_per_bin: 60,
            intraday_factor_by_bin: &[1.0; 24],
            day_of_week_factor: &[1.0; 7],
            day_of_year_factor: &vec![1.0; 366],
            range_start_utc_nanos: utc_nanos(2026, 1, 5, 8, 0, 0),
            range_end_utc_nanos: utc_nanos(2026, 1, 5, 11, 0, 0),
            interval_duration_nanos: NANOS_PER_HOUR,
        },
    )
    .expect("runtime factors should materialize");

    assert_eq!(factors, vec![0.0, 0.5, 1.0]);
}

#[test]
fn runtime_materialization_uses_weekday_component() {
    let calendar_registry = full_day_utc_calendar_registry();

    let mut day_of_week_factor = vec![1.0; 7];
    day_of_week_factor[1] = 3.0;

    let factors = build_runtime_hourly_seasonality_factors_from_components(
        RuntimeHourlySeasonalityFactorsBuildInput {
            calendar_registry: &calendar_registry,
            exchange_code: "XUTC",
            intraday_minutes_per_bin: 60,
            intraday_factor_by_bin: &[1.0; 24],
            day_of_week_factor: day_of_week_factor.as_slice(),
            day_of_year_factor: &vec![1.0; 366],
            range_start_utc_nanos: utc_nanos(2026, 1, 5, 23, 0, 0),
            range_end_utc_nanos: utc_nanos(2026, 1, 6, 1, 0, 0),
            interval_duration_nanos: NANOS_PER_HOUR,
        },
    )
    .expect("runtime factors should materialize");

    assert_eq!(factors, vec![0.5, 1.5]);
}

fn partial_day_utc_calendar_registry() -> ExchangeCalendarRegistry {
    ExchangeCalendarRegistryLoader::load_from_json(
        r#"
{
  "schema_version": "calendar.registry.v1",
  "artifact_date": "2026-01-01",
  "exchanges": [
    {
      "schema_version": "calendar.v1",
      "exchange_code": "XUTC",
      "exchange_name": "UTC Test Exchange",
      "timezone_name": "UTC",
      "rule_metadata": {
        "holiday_calendar_code": "NONE",
        "early_close_rule_set": "NONE"
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
    .expect("test registry should load")
}

fn full_day_utc_calendar_registry() -> ExchangeCalendarRegistry {
    ExchangeCalendarRegistryLoader::load_from_json(
        r#"
{
  "schema_version": "calendar.registry.v1",
  "artifact_date": "2026-01-01",
  "exchanges": [
    {
      "schema_version": "calendar.v1",
      "exchange_code": "XUTC",
      "exchange_name": "UTC Test Exchange",
      "timezone_name": "UTC",
      "rule_metadata": {
        "holiday_calendar_code": "NONE",
        "early_close_rule_set": "NONE"
      },
      "regular_sessions": [
        {
          "weekday": "Monday",
          "windows": [
            {"label": "regular", "open_time": "00:00", "close_time": "24:00"}
          ]
        },
        {
          "weekday": "Tuesday",
          "windows": [
            {"label": "regular", "open_time": "00:00", "close_time": "24:00"}
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
    .expect("test registry should load")
}
