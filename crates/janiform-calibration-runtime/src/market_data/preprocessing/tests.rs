use chrono::{TimeZone, Utc};
use janiform_foundations::time::{NANOS_PER_SECOND, utc_nanos};

use super::close_open_boundaries::derive_close_open_boundary_observations;
use super::hawkes_inputs::{
    build_structured_rho_candidate, derive_hawkes_beta_candidates,
    derive_positive_scale_candidates, open_session_transition_metrics,
};
use super::jump_candidates::derive_intraday_jump_candidates;
use super::known_news_samples::activity_lift;
use super::return_components_inputs::{
    infer_tick_size_from_open_prices, timestamp_batching_surface,
};
use super::session_windows::resolve_market_state_and_session_window;
use super::shared::OpenTickRecord;
use janiform_market_time::calendar::{
    CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarDefinition,
    MarketState, RegularSessionArtifact, SessionWindowArtifact, TradingWeekday,
};

#[test]
fn full_day_session_window_uses_next_day_midnight_for_24_00_close() {
    let definition = ExchangeCalendarDefinition::from_artifact(ExchangeCalendarArtifact {
        schema_version: "calendar.v1".to_string(),
        exchange_code: "FXSPOT".to_string(),
        exchange_name: "OTC FX Spot".to_string(),
        timezone_name: Some("UTC".to_string()),
        rule_metadata: CalendarRuleMetadataArtifact {
            holiday_calendar_code: "FX_24_5".to_string(),
            early_close_rule_set: None,
        },
        regular_sessions: vec![RegularSessionArtifact {
            weekday: TradingWeekday::Tuesday,
            windows: vec![SessionWindowArtifact {
                label: "regular".to_string(),
                open_time: "00:00".to_string(),
                close_time: "24:00".to_string(),
            }],
        }],
        holiday_dates: vec![],
        early_close_sessions: vec![],
    })
    .expect("fixture calendar should be valid");

    let timestamp_utc = chrono_tz::UTC
        .with_ymd_and_hms(2026, 2, 3, 12, 0, 0)
        .single()
        .expect("fixture timestamp should be valid")
        .with_timezone(&Utc);

    let (market_state, session_window) = resolve_market_state_and_session_window(
        &definition,
        chrono_tz::UTC,
        "FXSPOT",
        timestamp_utc,
    )
    .expect("24:00 close boundary should resolve through preprocessing");

    let session_window = session_window.expect("open timestamp should resolve a session window");

    assert_eq!(market_state, MarketState::Open);
    assert_eq!(
        session_window.close_utc_nanos,
        chrono_tz::UTC
            .with_ymd_and_hms(2026, 2, 4, 0, 0, 0)
            .single()
            .expect("fixture timestamp should be valid")
            .with_timezone(&Utc)
            .timestamp_nanos_opt()
            .expect("fixture timestamp should fit in nanos"),
    );
}

#[test]
fn full_day_adjacent_sessions_do_not_emit_zero_width_close_open_boundaries() {
    let definition = fx_spot_weekend_definition();

    let open_price_ticks = vec![
        OpenTickRecord {
            timestamp_utc_nanos: chrono_tz::UTC
                .with_ymd_and_hms(2026, 2, 3, 23, 30, 0)
                .single()
                .expect("fixture timestamp should be valid")
                .with_timezone(&Utc)
                .timestamp_nanos_opt()
                .expect("fixture timestamp should fit in nanos"),
            price: 1.08,
            trade_sign: None,
            session_open_utc_nanos: chrono_tz::UTC
                .with_ymd_and_hms(2026, 2, 3, 0, 0, 0)
                .single()
                .expect("fixture timestamp should be valid")
                .with_timezone(&Utc)
                .timestamp_nanos_opt()
                .expect("fixture timestamp should fit in nanos"),
            session_close_utc_nanos: chrono_tz::UTC
                .with_ymd_and_hms(2026, 2, 4, 0, 0, 0)
                .single()
                .expect("fixture timestamp should be valid")
                .with_timezone(&Utc)
                .timestamp_nanos_opt()
                .expect("fixture timestamp should fit in nanos"),
        },
        OpenTickRecord {
            timestamp_utc_nanos: chrono_tz::UTC
                .with_ymd_and_hms(2026, 2, 4, 0, 30, 0)
                .single()
                .expect("fixture timestamp should be valid")
                .with_timezone(&Utc)
                .timestamp_nanos_opt()
                .expect("fixture timestamp should fit in nanos"),
            price: 1.081,
            trade_sign: None,
            session_open_utc_nanos: chrono_tz::UTC
                .with_ymd_and_hms(2026, 2, 4, 0, 0, 0)
                .single()
                .expect("fixture timestamp should be valid")
                .with_timezone(&Utc)
                .timestamp_nanos_opt()
                .expect("fixture timestamp should fit in nanos"),
            session_close_utc_nanos: chrono_tz::UTC
                .with_ymd_and_hms(2026, 2, 5, 0, 0, 0)
                .single()
                .expect("fixture timestamp should be valid")
                .with_timezone(&Utc)
                .timestamp_nanos_opt()
                .expect("fixture timestamp should fit in nanos"),
        },
    ];

    let observations = derive_close_open_boundary_observations(
        &definition,
        chrono_tz::America::New_York,
        "FXSPOT",
        open_price_ticks.as_slice(),
        &open_price_ticks
            .iter()
            .map(|tick| tick.timestamp_utc_nanos)
            .collect::<Vec<_>>(),
    )
    .expect("adjacent sessions should resolve without boundary errors");

    assert!(
        observations.is_empty(),
        "adjacent full-day sessions should not emit zero-width close-open boundaries"
    );
}

#[test]
fn fxspot_weekend_gap_uses_sunday_reopen_even_when_first_tick_arrives_monday() {
    let definition = fx_spot_weekend_definition();

    let open_price_ticks = vec![
        OpenTickRecord {
            timestamp_utc_nanos: chrono_tz::UTC
                .with_ymd_and_hms(2026, 3, 20, 18, 30, 0)
                .single()
                .expect("fixture timestamp should be valid")
                .with_timezone(&Utc)
                .timestamp_nanos_opt()
                .expect("fixture timestamp should fit in nanos"),
            price: 1.01268,
            trade_sign: None,
            session_open_utc_nanos: chrono_tz::UTC
                .with_ymd_and_hms(2026, 3, 20, 0, 0, 0)
                .single()
                .expect("fixture timestamp should be valid")
                .with_timezone(&Utc)
                .timestamp_nanos_opt()
                .expect("fixture timestamp should fit in nanos"),
            session_close_utc_nanos: chrono_tz::UTC
                .with_ymd_and_hms(2026, 3, 20, 21, 0, 0)
                .single()
                .expect("fixture timestamp should be valid")
                .with_timezone(&Utc)
                .timestamp_nanos_opt()
                .expect("fixture timestamp should fit in nanos"),
        },
        OpenTickRecord {
            timestamp_utc_nanos: chrono_tz::UTC
                .with_ymd_and_hms(2026, 3, 23, 0, 0, 0)
                .single()
                .expect("fixture timestamp should be valid")
                .with_timezone(&Utc)
                .timestamp_nanos_opt()
                .expect("fixture timestamp should fit in nanos"),
            price: 1.0113,
            trade_sign: None,
            session_open_utc_nanos: chrono_tz::UTC
                .with_ymd_and_hms(2026, 3, 23, 0, 0, 0)
                .single()
                .expect("fixture timestamp should be valid")
                .with_timezone(&Utc)
                .timestamp_nanos_opt()
                .expect("fixture timestamp should fit in nanos"),
            session_close_utc_nanos: chrono_tz::UTC
                .with_ymd_and_hms(2026, 3, 24, 0, 0, 0)
                .single()
                .expect("fixture timestamp should be valid")
                .with_timezone(&Utc)
                .timestamp_nanos_opt()
                .expect("fixture timestamp should fit in nanos"),
        },
    ];

    let observations = derive_close_open_boundary_observations(
        &definition,
        chrono_tz::America::New_York,
        "FXSPOT",
        open_price_ticks.as_slice(),
        &open_price_ticks
            .iter()
            .map(|tick| tick.timestamp_utc_nanos)
            .collect::<Vec<_>>(),
    )
    .expect("weekend reopen boundary should resolve through the calendar");

    assert_eq!(observations.len(), 1);
    assert_eq!(
        observations[0].close_timestamp_utc_nanos,
        chrono_tz::UTC
            .with_ymd_and_hms(2026, 3, 20, 21, 0, 0)
            .single()
            .expect("fixture timestamp should be valid")
            .with_timezone(&Utc)
            .timestamp_nanos_opt()
            .expect("fixture timestamp should fit in nanos"),
    );
    assert_eq!(
        observations[0].open_timestamp_utc_nanos,
        chrono_tz::UTC
            .with_ymd_and_hms(2026, 3, 22, 21, 0, 0)
            .single()
            .expect("fixture timestamp should be valid")
            .with_timezone(&Utc)
            .timestamp_nanos_opt()
            .expect("fixture timestamp should fit in nanos"),
    );
    assert_eq!(observations[0].open_price, 1.0113);
}

#[test]
fn positive_scale_candidates_remain_strictly_positive_after_rounding() {
    let candidate_values = derive_positive_scale_candidates(1.0e-15);

    assert!(!candidate_values.is_empty());
    assert!(
        candidate_values.iter().all(|value| *value > 0.0),
        "positive scale candidate generation must preserve strict positivity"
    );
}

#[test]
fn hawkes_beta_candidates_include_slow_counterfactuals() {
    let candidate_values = derive_hawkes_beta_candidates(1_641.6);

    assert!(candidate_values.contains(&60.0));
    assert!(candidate_values.contains(&120.0));
    assert!(candidate_values.contains(&300.0));
}

#[test]
fn structured_rho_candidate_preserves_positive_self_mass_without_news_activity_share() {
    let structured_rho = build_structured_rho_candidate(1.0e-15, 0.22, 0.4, 120.0, 0.0, 0.0, 1.0);

    assert!(
        structured_rho.r_self > 0.0,
        "structured rho self-loading must stay inside the Hawkes domain"
    );
    assert!(structured_rho.r_cross >= 0.0);
    assert!(structured_rho.delta_self_r >= 0.0);
    assert!(structured_rho.delta_cross_r >= 0.0);
}

#[test]
fn zero_inflated_tick_returns_do_not_label_every_non_zero_move_as_a_jump() {
    let open_price_tick_timestamps = vec![
        utc_nanos(2026, 3, 2, 9, 0, 0),
        utc_nanos(2026, 3, 2, 9, 0, 1),
        utc_nanos(2026, 3, 2, 9, 0, 2),
        utc_nanos(2026, 3, 2, 9, 0, 3),
        utc_nanos(2026, 3, 2, 9, 0, 4),
        utc_nanos(2026, 3, 2, 9, 0, 5),
        utc_nanos(2026, 3, 2, 9, 0, 6),
    ];

    let open_price_ticks = open_price_tick_timestamps
        .iter()
        .map(|timestamp_utc_nanos| OpenTickRecord {
            timestamp_utc_nanos: *timestamp_utc_nanos,
            price: 100.0,
            trade_sign: None,
            session_open_utc_nanos: utc_nanos(2026, 3, 2, 9, 0, 0),
            session_close_utc_nanos: utc_nanos(2026, 3, 2, 10, 0, 0),
        })
        .collect::<Vec<_>>();

    let same_session_returns = vec![
        (open_price_tick_timestamps[1], 0.0),
        (open_price_tick_timestamps[2], 0.0),
        (open_price_tick_timestamps[3], 1.0e-4),
        (open_price_tick_timestamps[4], -2.0e-4),
        (open_price_tick_timestamps[5], 3.0e-4),
        (open_price_tick_timestamps[6], 5.0e-3),
    ];

    let detected_candidates = derive_intraday_jump_candidates(
        same_session_returns.as_slice(),
        open_price_ticks.as_slice(),
        open_price_tick_timestamps.as_slice(),
        &[],
        4.0,
    );

    assert_eq!(detected_candidates.len(), 1);
    assert_eq!(
        detected_candidates[0].jump_timestamp_utc_nanos,
        open_price_tick_timestamps[6]
    );
    assert!((detected_candidates[0].jump_return - 5.0e-3).abs() <= 1.0e-12);
}

#[test]
fn jump_candidate_exclusion_uses_exact_attributed_open_return_timestamp() {
    let open_price_tick_timestamps = vec![
        utc_nanos(2026, 3, 2, 9, 0, 0),
        utc_nanos(2026, 3, 2, 9, 0, 1),
        utc_nanos(2026, 3, 2, 9, 0, 2),
    ];

    let open_price_ticks = open_price_tick_timestamps
        .iter()
        .map(|timestamp_utc_nanos| OpenTickRecord {
            timestamp_utc_nanos: *timestamp_utc_nanos,
            price: 100.0,
            trade_sign: None,
            session_open_utc_nanos: utc_nanos(2026, 3, 2, 9, 0, 0),
            session_close_utc_nanos: utc_nanos(2026, 3, 2, 10, 0, 0),
        })
        .collect::<Vec<_>>();

    let same_session_returns = vec![
        (open_price_tick_timestamps[1], 1.0e-2),
        (open_price_tick_timestamps[2], 2.0e-2),
    ];

    let detected_candidates = derive_intraday_jump_candidates(
        same_session_returns.as_slice(),
        open_price_ticks.as_slice(),
        open_price_tick_timestamps.as_slice(),
        &[open_price_tick_timestamps[1]],
        0.0,
    );

    assert_eq!(detected_candidates.len(), 1);
    assert_eq!(
        detected_candidates[0].jump_timestamp_utc_nanos,
        open_price_tick_timestamps[2]
    );
}

#[test]
fn open_session_transition_metrics_use_exact_source_window_exposure() {
    let open_price_ticks = vec![
        OpenTickRecord {
            timestamp_utc_nanos: utc_nanos(2026, 3, 2, 9, 30, 0),
            price: 100.0,
            trade_sign: None,
            session_open_utc_nanos: utc_nanos(2026, 3, 2, 9, 0, 0),
            session_close_utc_nanos: utc_nanos(2026, 3, 2, 10, 0, 0),
        },
        OpenTickRecord {
            timestamp_utc_nanos: utc_nanos(2026, 3, 2, 9, 45, 0),
            price: 100.1,
            trade_sign: None,
            session_open_utc_nanos: utc_nanos(2026, 3, 2, 9, 0, 0),
            session_close_utc_nanos: utc_nanos(2026, 3, 2, 10, 0, 0),
        },
        OpenTickRecord {
            timestamp_utc_nanos: utc_nanos(2026, 3, 2, 10, 30, 0),
            price: 100.2,
            trade_sign: None,
            session_open_utc_nanos: utc_nanos(2026, 3, 2, 10, 0, 0),
            session_close_utc_nanos: utc_nanos(2026, 3, 2, 11, 0, 0),
        },
    ];

    let (open_session_duration_hours, interarrival_seconds) = open_session_transition_metrics(
        open_price_ticks.as_slice(),
        (
            utc_nanos(2026, 3, 2, 9, 40, 0),
            utc_nanos(2026, 3, 2, 10, 40, 0),
        ),
    );

    assert!((open_session_duration_hours - 1.0).abs() <= 1.0e-12);
    assert_eq!(interarrival_seconds, vec![900.0]);
}

#[test]
fn activity_lift_uses_local_same_session_tick_geometry() {
    let open_price_ticks = vec![
        OpenTickRecord {
            timestamp_utc_nanos: utc_nanos(2026, 3, 2, 9, 0, 0),
            price: 100.0,
            trade_sign: None,
            session_open_utc_nanos: utc_nanos(2026, 3, 2, 9, 0, 0),
            session_close_utc_nanos: utc_nanos(2026, 3, 2, 10, 0, 0),
        },
        OpenTickRecord {
            timestamp_utc_nanos: utc_nanos(2026, 3, 2, 9, 0, 4),
            price: 100.1,
            trade_sign: None,
            session_open_utc_nanos: utc_nanos(2026, 3, 2, 9, 0, 0),
            session_close_utc_nanos: utc_nanos(2026, 3, 2, 10, 0, 0),
        },
        OpenTickRecord {
            timestamp_utc_nanos: utc_nanos(2026, 3, 2, 9, 0, 5),
            price: 100.2,
            trade_sign: None,
            session_open_utc_nanos: utc_nanos(2026, 3, 2, 9, 0, 0),
            session_close_utc_nanos: utc_nanos(2026, 3, 2, 10, 0, 0),
        },
    ];

    let open_price_tick_timestamps = open_price_ticks
        .iter()
        .map(|tick| tick.timestamp_utc_nanos)
        .collect::<Vec<_>>();

    let derived_activity_lift = activity_lift(
        utc_nanos(2026, 3, 2, 9, 0, 4),
        open_price_ticks.as_slice(),
        open_price_tick_timestamps.as_slice(),
    );

    assert_eq!(derived_activity_lift, 4.0);
}

#[test]
fn tick_size_inference_uses_chronological_same_session_price_changes() {
    let first_session_open = utc_nanos(2026, 3, 2, 9, 30, 0);
    let first_session_close = utc_nanos(2026, 3, 2, 16, 0, 0);
    let second_session_open = utc_nanos(2026, 3, 3, 9, 30, 0);
    let second_session_close = utc_nanos(2026, 3, 3, 16, 0, 0);

    let open_price_ticks = vec![
        OpenTickRecord {
            timestamp_utc_nanos: utc_nanos(2026, 3, 2, 9, 30, 0),
            price: 6817.00,
            trade_sign: None,
            session_open_utc_nanos: first_session_open,
            session_close_utc_nanos: first_session_close,
        },
        OpenTickRecord {
            timestamp_utc_nanos: utc_nanos(2026, 3, 2, 9, 30, 1),
            price: 6817.25,
            trade_sign: None,
            session_open_utc_nanos: first_session_open,
            session_close_utc_nanos: first_session_close,
        },
        OpenTickRecord {
            timestamp_utc_nanos: utc_nanos(2026, 3, 3, 9, 30, 0),
            price: 6301.574718886245,
            trade_sign: None,
            session_open_utc_nanos: second_session_open,
            session_close_utc_nanos: second_session_close,
        },
        OpenTickRecord {
            timestamp_utc_nanos: utc_nanos(2026, 3, 3, 9, 30, 1),
            price: 6301.824718886245,
            trade_sign: None,
            session_open_utc_nanos: second_session_open,
            session_close_utc_nanos: second_session_close,
        },
    ];

    let inferred_tick_size = infer_tick_size_from_open_prices(open_price_ticks.as_slice())
        .expect("same-session price changes should identify a tick size");

    assert_eq!(inferred_tick_size, 0.25);
}

#[test]
fn timestamp_batching_surface_keeps_same_session_duplicates() {
    let session_open = utc_nanos(2026, 3, 2, 9, 30, 0);
    let session_close = utc_nanos(2026, 3, 2, 16, 0, 0);
    let next_session_open = utc_nanos(2026, 3, 3, 9, 30, 0);
    let next_session_close = utc_nanos(2026, 3, 3, 16, 0, 0);
    let batch_width_nanos = 5 * NANOS_PER_SECOND;

    let open_price_ticks = vec![
        OpenTickRecord {
            timestamp_utc_nanos: session_open,
            price: 6817.00,
            trade_sign: None,
            session_open_utc_nanos: session_open,
            session_close_utc_nanos: session_close,
        },
        OpenTickRecord {
            timestamp_utc_nanos: session_open + batch_width_nanos,
            price: 6817.25,
            trade_sign: None,
            session_open_utc_nanos: session_open,
            session_close_utc_nanos: session_close,
        },
        OpenTickRecord {
            timestamp_utc_nanos: session_open + batch_width_nanos,
            price: 6817.50,
            trade_sign: None,
            session_open_utc_nanos: session_open,
            session_close_utc_nanos: session_close,
        },
        OpenTickRecord {
            timestamp_utc_nanos: session_open + 3 * batch_width_nanos,
            price: 6817.75,
            trade_sign: None,
            session_open_utc_nanos: session_open,
            session_close_utc_nanos: session_close,
        },
        OpenTickRecord {
            timestamp_utc_nanos: next_session_open,
            price: 6818.00,
            trade_sign: None,
            session_open_utc_nanos: next_session_open,
            session_close_utc_nanos: next_session_close,
        },
    ];

    let surface = timestamp_batching_surface(open_price_ticks.as_slice())
        .expect("timestamp batching surface should fit");

    assert_eq!(surface.bucket_width_nanos, Some(batch_width_nanos));
    assert_eq!(surface.same_session_observation_transition_count, Some(3));
    assert_eq!(surface.same_timestamp_continuation_count, Some(1));
}

fn fx_spot_weekend_definition() -> ExchangeCalendarDefinition {
    ExchangeCalendarDefinition::from_artifact(ExchangeCalendarArtifact {
        schema_version: "calendar.v1".to_string(),
        exchange_code: "FXSPOT".to_string(),
        exchange_name: "OTC FX Spot".to_string(),
        timezone_name: Some("America/New_York".to_string()),
        rule_metadata: CalendarRuleMetadataArtifact {
            holiday_calendar_code: "FX_24_5".to_string(),
            early_close_rule_set: None,
        },
        regular_sessions: vec![
            RegularSessionArtifact {
                weekday: TradingWeekday::Friday,
                windows: vec![SessionWindowArtifact {
                    label: "spot".to_string(),
                    open_time: "00:00".to_string(),
                    close_time: "17:00".to_string(),
                }],
            },
            RegularSessionArtifact {
                weekday: TradingWeekday::Sunday,
                windows: vec![SessionWindowArtifact {
                    label: "spot".to_string(),
                    open_time: "17:00".to_string(),
                    close_time: "24:00".to_string(),
                }],
            },
            RegularSessionArtifact {
                weekday: TradingWeekday::Monday,
                windows: vec![SessionWindowArtifact {
                    label: "spot".to_string(),
                    open_time: "00:00".to_string(),
                    close_time: "24:00".to_string(),
                }],
            },
        ],
        holiday_dates: vec![],
        early_close_sessions: vec![],
    })
    .expect("fixture calendar should be valid")
}
