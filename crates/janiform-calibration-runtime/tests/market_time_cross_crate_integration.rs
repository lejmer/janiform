use chrono::{DateTime, Utc};
use janiform_foundations::time::NANOS_PER_HOUR;

use janiform_calibration_runtime::{
    evolve_hawkes_state_for_calibration_step, load_interval_factor_grid_for_calibration,
    project_hawkes_intensities_for_calibration_interval,
};
use janiform_engine_pricing::{
    evolve_hawkes_state as evolve_hawkes_state_coarse_interval_step,
    load_interval_factor_grid as load_interval_factor_grid_for_pricing_path,
    project_hawkes_intensities_coarse_interval,
};
use janiform_engine_simulation::{
    evolve_hawkes_state as evolve_hawkes_state_simulation_step,
    load_interval_factor_grid as load_interval_factor_grid_for_simulation_path,
    project_hawkes_intensities_simulation_interval,
};
use janiform_market_time::calendar::ExchangeCalendarRegistryLoader;
use janiform_market_time::interval_factor_grids::{
    INTERVAL_FACTOR_GRID_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD,
    INTERVAL_FACTOR_GRID_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD,
    IntervalFactorGridProviderRequest, IntervalNewsIntensityFactorSource,
    IntervalSeasonalityFactorSource,
};
use janiform_model_kernels::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
    HawkesTransitionStep,
};

const SIMULATION_SHARED_SEASONALITY_INTERVAL_FACTORS: [f64; 3] = [1.0, 0.9, 1.1];
const SIMULATION_SHARED_NEWS_INTERVAL_FACTORS: [f64; 3] = [0.2, 0.0, 0.3];
const SURPRISE_NEWS_TIME_MULTIPLIERS: [(u64, f64); 3] = [(0, 1.0), (1, 0.5), (2, 2.0)];
#[test]
fn pricing_path_consumes_market_time_interval_factor_grid_provider() {
    let registry = build_calendar_registry();

    let request = IntervalFactorGridProviderRequest {
        exchange_code: "XNYS",
        range_start_utc: parse_timestamp("2026-02-02T14:00:00Z"),
        range_end_utc: parse_timestamp("2026-02-02T17:00:00Z"),
        interval_duration_nanos: NANOS_PER_HOUR,
        seasonality_source: Some(IntervalSeasonalityFactorSource::Materialized {
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
            sparse_interval_minimum_observation_count: 1,
        }),
        news_intensity_source: Some(IntervalNewsIntensityFactorSource::Materialized {
            known_scheduled_event_timestamps_utc_nanos: &[
                parse_timestamp("2026-02-02T14:05:00Z")
                    .timestamp_nanos_opt()
                    .expect("timestamp fixture must be in range"),
                parse_timestamp("2026-02-02T15:10:00Z")
                    .timestamp_nanos_opt()
                    .expect("timestamp fixture must be in range"),
            ],
            surprise_open_baseline_intensity: 0.1,
            surprise_closed_baseline_intensity: 0.1,
            surprise_deterministic_time_multiplier_by_interval_index: &[
                (0, 1.0),
                (1, 2.0),
                (2, 1.0),
            ],
        }),
    };

    let output = load_interval_factor_grid_for_pricing_path(&registry, request)
        .expect("pricing path should resolve interval factor grids from provider");

    assert_eq!(output.seasonality_factor, vec![1.1, 0.8, 1.4]);
    assert_eq!(output.news_intensity, vec![1.1, 1.2, 0.1]);
    assert_eq!(
        output.metadata.seasonality_materialization_method,
        INTERVAL_FACTOR_GRID_PROVIDER_SEASONALITY_MATERIALIZATION_METHOD
    );
    assert_eq!(
        output.metadata.news_intensity_materialization_method,
        INTERVAL_FACTOR_GRID_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD
    );
}

#[test]
fn provider_supports_deterministic_news_intensity_inputs_for_pricing_and_simulation_paths() {
    let registry = build_calendar_registry();

    let deterministic_news_intensity_request = deterministic_news_intensity_provider_request();

    let pricing_output = load_interval_factor_grid_for_pricing_path(
        &registry,
        deterministic_news_intensity_request.clone(),
    )
    .expect("deterministic news-intensity source should resolve for pricing");

    assert_eq!(pricing_output.news_intensity, vec![0.4, 0.2, 0.8]);
    assert_eq!(
        pricing_output
            .metadata
            .news_intensity_materialization_method,
        INTERVAL_FACTOR_GRID_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD
    );

    let simulation_output = load_interval_factor_grid_for_simulation_path(
        &registry,
        deterministic_news_intensity_request,
    )
    .expect("deterministic news-intensity source should resolve for simulation");

    assert_eq!(simulation_output.news_intensity, vec![0.4, 0.2, 0.8]);
    assert_eq!(
        simulation_output
            .metadata
            .news_intensity_materialization_method,
        INTERVAL_FACTOR_GRID_PROVIDER_NEWS_INTENSITY_MATERIALIZATION_METHOD
    );
}

#[test]
fn simulation_path_consumes_market_time_interval_factor_grid_provider() {
    let registry = build_calendar_registry();

    let request = shared_simulation_interval_factor_grid_provider_request();
    let output = load_interval_factor_grid_for_simulation_path(&registry, request)
        .expect("simulation path should resolve interval factor grids from provider");

    assert_eq!(output.seasonality_factor, vec![1.0, 0.9, 1.1]);
    assert_eq!(output.news_intensity, vec![0.2, 0.0, 0.3]);
    assert_eq!(
        output.metadata.seasonality_materialization_method,
        "shared_precomputed_interval_seasonality_factor.v1"
    );
    assert_eq!(
        output.metadata.news_intensity_materialization_method,
        "shared_precomputed_interval_news_intensity.v1"
    );
}

#[test]
fn simulation_path_rejects_explicit_sources_not_aligned_to_requested_interval_grid() {
    let registry = build_calendar_registry();

    let request = IntervalFactorGridProviderRequest {
        exchange_code: "XNYS",
        range_start_utc: parse_timestamp("2026-02-02T14:00:00Z"),
        range_end_utc: parse_timestamp("2026-02-02T16:00:00Z"),
        interval_duration_nanos: 1_800_000_000_000,
        seasonality_source: Some(IntervalSeasonalityFactorSource::Explicit {
            interval_factors: &[1.2, 0.8],
            materialization_method: "shared_precomputed_interval_seasonality_factor.v1",
        }),
        news_intensity_source: Some(IntervalNewsIntensityFactorSource::Explicit {
            interval_factors: &[0.3, 0.1],
            materialization_method: "shared_precomputed_interval_news_intensity.v1",
        }),
    };

    let error = load_interval_factor_grid_for_simulation_path(&registry, request)
        .expect_err("simulation path should reject explicit inputs with the wrong interval count");

    assert!(
        error.to_string().contains(
            "interval factor grid length mismatch, expected 4 intervals and received seasonality_factor=2, news_intensity=2"
        )
    );
}

#[test]
fn calibration_path_consumes_market_time_interval_factor_grid_provider() {
    let registry = build_calendar_registry();

    let request = IntervalFactorGridProviderRequest {
        exchange_code: "XNYS",
        range_start_utc: parse_timestamp("2026-02-02T14:00:00Z"),
        range_end_utc: parse_timestamp("2026-02-02T17:00:00Z"),
        interval_duration_nanos: NANOS_PER_HOUR,
        seasonality_source: Some(IntervalSeasonalityFactorSource::Explicit {
            interval_factors: &[1.3, 0.95, 1.05],
            materialization_method: "shared_precomputed_interval_seasonality_factor.v1",
        }),
        news_intensity_source: Some(IntervalNewsIntensityFactorSource::Explicit {
            interval_factors: &[0.0, 0.4, 0.1],
            materialization_method: "shared_precomputed_interval_news_intensity.v1",
        }),
    };

    let output = load_interval_factor_grid_for_calibration(&registry, request)
        .expect("calibration path should resolve interval factor grids from provider");

    assert_eq!(output.seasonality_factor, vec![1.3, 0.95, 1.05]);
    assert_eq!(output.news_intensity, vec![0.0, 0.4, 0.1]);
}

#[test]
fn simulation_coarse_and_calibration_paths_share_one_hawkes_intensity_projection_api() {
    let registry = build_calendar_registry();

    let request = shared_simulation_interval_factor_grid_provider_request();
    let interval_factor_grid = load_interval_factor_grid_for_simulation_path(&registry, request)
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

    let simulation_projection =
        project_hawkes_intensities_simulation_interval(state, parameters, &interval_factor_grid, 1)
            .expect("simulation projection should resolve interval seasonality");

    let coarse_projection =
        project_hawkes_intensities_coarse_interval(state, parameters, &interval_factor_grid, 1)
            .expect("coarse projection should resolve interval seasonality");
    let calibration_projection = project_hawkes_intensities_for_calibration_interval(
        state,
        parameters,
        &interval_factor_grid,
        1,
    )
    .expect("calibration projection should resolve interval seasonality");

    assert_eq!(simulation_projection, coarse_projection);
    assert_eq!(simulation_projection, calibration_projection);
    assert!((simulation_projection.deseasonalized.lambda_plus - 0.8).abs() <= 1e-12);
    assert!((simulation_projection.deseasonalized.lambda_minus - 0.85).abs() <= 1e-12);
    assert!((simulation_projection.seasonality_scaled.lambda_plus - 0.75).abs() <= 1e-12);
    assert!((simulation_projection.seasonality_scaled.lambda_minus - 0.80).abs() <= 1e-12);
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

    let transition_step = HawkesTransitionStep::new(0.25, Some(HawkesEventSide::Buy), Some(1.5));

    let reference_state = initial_state
        .apply_transition_step(kernel_parameters, transition_step)
        .expect("reference transition must succeed");
    let simulation_state =
        evolve_hawkes_state_simulation_step(initial_state, kernel_parameters, transition_step)
            .expect("simulation transition must use canonical transition API");

    let coarse_state =
        evolve_hawkes_state_coarse_interval_step(initial_state, kernel_parameters, transition_step)
            .expect("coarse transition must use canonical transition API");

    let calibration_state =
        evolve_hawkes_state_for_calibration_step(initial_state, kernel_parameters, transition_step)
            .expect("calibration transition must use canonical transition API");

    assert_eq!(simulation_state, reference_state);
    assert_eq!(coarse_state, reference_state);
    assert_eq!(calibration_state, reference_state);
}

fn deterministic_news_intensity_provider_request() -> IntervalFactorGridProviderRequest<'static> {
    IntervalFactorGridProviderRequest {
        exchange_code: "XNYS",
        range_start_utc: parse_timestamp("2026-02-07T00:00:00Z"),
        range_end_utc: parse_timestamp("2026-02-07T03:00:00Z"),
        interval_duration_nanos: NANOS_PER_HOUR,
        seasonality_source: Some(IntervalSeasonalityFactorSource::Explicit {
            interval_factors: &[1.0, 1.0, 1.0],
            materialization_method: "shared_precomputed_interval_seasonality_factor.v1",
        }),
        news_intensity_source: Some(IntervalNewsIntensityFactorSource::Materialized {
            known_scheduled_event_timestamps_utc_nanos: &[],
            surprise_open_baseline_intensity: 0.4,
            surprise_closed_baseline_intensity: 0.4,
            surprise_deterministic_time_multiplier_by_interval_index:
                &SURPRISE_NEWS_TIME_MULTIPLIERS,
        }),
    }
}

fn shared_simulation_interval_factor_grid_provider_request()
-> IntervalFactorGridProviderRequest<'static> {
    IntervalFactorGridProviderRequest {
        exchange_code: "XNYS",
        range_start_utc: parse_timestamp("2026-02-02T14:00:00Z"),
        range_end_utc: parse_timestamp("2026-02-02T17:00:00Z"),
        interval_duration_nanos: NANOS_PER_HOUR,
        seasonality_source: Some(IntervalSeasonalityFactorSource::Explicit {
            interval_factors: &SIMULATION_SHARED_SEASONALITY_INTERVAL_FACTORS,
            materialization_method: "shared_precomputed_interval_seasonality_factor.v1",
        }),
        news_intensity_source: Some(IntervalNewsIntensityFactorSource::Explicit {
            interval_factors: &SIMULATION_SHARED_NEWS_INTERVAL_FACTORS,
            materialization_method: "shared_precomputed_interval_news_intensity.v1",
        }),
    }
}

fn build_calendar_registry() -> janiform_market_time::calendar::ExchangeCalendarRegistry {
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
