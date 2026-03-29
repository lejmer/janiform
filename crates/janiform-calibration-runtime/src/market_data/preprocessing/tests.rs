use chrono::{TimeZone, Utc};

use super::close_open_boundaries::derive_close_open_boundary_observations;
use super::hawkes_inputs::{build_structured_rho_candidate, derive_positive_scale_candidates};
use super::jump_candidates::derive_intraday_jump_candidates;
use super::session_windows::resolve_market_state_and_session_window;
use super::shared::OpenTickRecord;
use crate::calendar::{
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
        chrono_tz::UTC,
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
                .with_ymd_and_hms(2026, 3, 20, 19, 0, 0)
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
        chrono_tz::UTC,
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
            .with_ymd_and_hms(2026, 3, 20, 19, 0, 0)
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

fn utc_nanos(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
    Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
        .single()
        .expect("fixture timestamp should be valid")
        .timestamp_nanos_opt()
        .expect("fixture timestamp nanos should fit in i64")
}

fn fx_spot_weekend_definition() -> ExchangeCalendarDefinition {
    ExchangeCalendarDefinition::from_artifact(ExchangeCalendarArtifact {
        schema_version: "calendar.v1".to_string(),
        exchange_code: "FXSPOT".to_string(),
        exchange_name: "OTC FX Spot".to_string(),
        timezone_name: Some("UTC".to_string()),
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
                    close_time: "19:00".to_string(),
                }],
            },
            RegularSessionArtifact {
                weekday: TradingWeekday::Sunday,
                windows: vec![SessionWindowArtifact {
                    label: "spot".to_string(),
                    open_time: "21:00".to_string(),
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
