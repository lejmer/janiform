use super::{
    CalendarRuleMetadataArtifact, EarlyCloseSessionArtifact, ExchangeCalendarArtifact,
    ExchangeCalendarDefinition, ExchangeCalendarRegistry, ExchangeCalendarRegistryArtifact,
    ExchangeCalendarRegistryLoadError, ExchangeCalendarRegistryLoader,
    ExchangeCalendarRegistryLookupError, ExchangeCalendarResolveError,
    ExchangeCalendarValidationError, MarketState, RegularSessionArtifact, SessionWindowArtifact,
    TimestampEventInputRecord, TimestampEventPreprocessingError, TimestampEventSessionFilter,
    TradingWeekday,
};
use crate::calendar::definition::resolve_market_state_for_definition_at_utc;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::json;

#[test]
fn missing_timezone_fails_validation() {
    let mut artifact = valid_exchange_artifact();
    artifact.timezone_name = None;

    let result = ExchangeCalendarDefinition::from_artifact(artifact);
    assert_eq!(
        result,
        Err(ExchangeCalendarValidationError::MissingField {
            field: "timezone_name",
        })
    );
}

#[test]
fn invalid_session_window_fails_validation() {
    let artifact = ExchangeCalendarArtifact {
        schema_version: "calendar.v1".to_string(),
        exchange_code: "XNYS".to_string(),
        exchange_name: "New York Stock Exchange".to_string(),
        timezone_name: Some("America/New_York".to_string()),
        rule_metadata: CalendarRuleMetadataArtifact {
            holiday_calendar_code: "US_EQUITIES".to_string(),
            early_close_rule_set: Some("US_EQUITIES_EARLY_CLOSE".to_string()),
        },
        regular_sessions: vec![RegularSessionArtifact {
            weekday: TradingWeekday::Monday,
            windows: vec![SessionWindowArtifact {
                label: "regular".to_string(),
                open_time: "16:00".to_string(),
                close_time: "09:30".to_string(),
            }],
        }],
        holiday_dates: vec![],
        early_close_sessions: vec![],
    };

    let result = ExchangeCalendarDefinition::from_artifact(artifact);
    match result {
        Err(ExchangeCalendarValidationError::InvalidSessionWindow {
            weekday,
            label,
            open_time,
            close_time,
            reason,
        }) => {
            assert_eq!(weekday, TradingWeekday::Monday);
            assert_eq!(label, "regular");
            assert_eq!(open_time, "16:00");
            assert_eq!(close_time, "09:30");
            assert_eq!(reason, "open_time must be earlier than close_time");
        }
        _ => panic!("expected InvalidSessionWindow"),
    }
}

#[test]
fn overlapping_regular_sessions_fail_validation() {
    let artifact = ExchangeCalendarArtifact {
        schema_version: "calendar.v1".to_string(),
        exchange_code: "XTKS".to_string(),
        exchange_name: "Tokyo Stock Exchange".to_string(),
        timezone_name: Some("Asia/Tokyo".to_string()),
        rule_metadata: CalendarRuleMetadataArtifact {
            holiday_calendar_code: "JP_EQUITIES".to_string(),
            early_close_rule_set: None,
        },
        regular_sessions: vec![RegularSessionArtifact {
            weekday: TradingWeekday::Monday,
            windows: vec![
                SessionWindowArtifact {
                    label: "morning".to_string(),
                    open_time: "09:00".to_string(),
                    close_time: "11:30".to_string(),
                },
                SessionWindowArtifact {
                    label: "overlap".to_string(),
                    open_time: "11:00".to_string(),
                    close_time: "15:00".to_string(),
                },
            ],
        }],
        holiday_dates: vec![],
        early_close_sessions: vec![],
    };

    let result = ExchangeCalendarDefinition::from_artifact(artifact);
    match result {
        Err(ExchangeCalendarValidationError::OverlappingRegularSessions {
            weekday,
            first_label,
            second_label,
            first_window,
            second_window,
        }) => {
            assert_eq!(weekday, TradingWeekday::Monday);
            assert_eq!(first_label, "morning");
            assert_eq!(second_label, "overlap");
            assert_eq!(first_window, "09:00-11:30");
            assert_eq!(second_window, "11:00-15:00");
        }
        _ => panic!("expected OverlappingRegularSessions"),
    }
}

#[test]
fn valid_definition_with_multiple_weekdays_builds_successfully() {
    let artifact = valid_exchange_artifact();

    let definition = ExchangeCalendarDefinition::from_artifact(artifact)
        .expect("valid artifact should build an exchange calendar definition");

    assert_eq!(definition.schema_version(), "calendar.v1");
    assert_eq!(definition.exchange_metadata().exchange_code(), "XNYS");
    assert_eq!(
        definition.exchange_metadata().exchange_name(),
        "New York Stock Exchange"
    );
    assert_eq!(definition.timezone_name(), "America/New_York");
    assert_eq!(
        definition.rule_metadata().holiday_calendar_code(),
        "US_EQUITIES"
    );
    assert_eq!(
        definition.rule_metadata().early_close_rule_set(),
        Some("US_EQUITIES_EARLY_CLOSE")
    );
    assert_eq!(definition.regular_sessions().len(), 3);
    assert_eq!(
        definition.regular_sessions()[0].weekday(),
        TradingWeekday::Monday
    );
    assert_eq!(
        definition.regular_sessions()[1].weekday(),
        TradingWeekday::Tuesday
    );
    assert_eq!(
        definition.regular_sessions()[2].weekday(),
        TradingWeekday::Wednesday
    );
}

#[test]
fn full_day_window_accepts_24_00_close_and_treats_it_as_exclusive_end_of_day() {
    let artifact = ExchangeCalendarArtifact {
        schema_version: "calendar.v1".to_string(),
        exchange_code: "FXSPOT".to_string(),
        exchange_name: "OTC FX Spot".to_string(),
        timezone_name: Some("UTC".to_string()),
        rule_metadata: CalendarRuleMetadataArtifact {
            holiday_calendar_code: "FX_24_5".to_string(),
            early_close_rule_set: None,
        },
        regular_sessions: vec![RegularSessionArtifact {
            weekday: TradingWeekday::Monday,
            windows: vec![SessionWindowArtifact {
                label: "regular".to_string(),
                open_time: "00:00".to_string(),
                close_time: "24:00".to_string(),
            }],
        }],
        holiday_dates: vec![],
        early_close_sessions: vec![],
    };

    let definition = ExchangeCalendarDefinition::from_artifact(artifact)
        .expect("24:00 close should be accepted as the end-of-day boundary");
    let monday_late = timestamp_utc(2026, 2, 2, 23, 59, 0);
    let tuesday_boundary = timestamp_utc(2026, 2, 3, 0, 0, 0);

    assert_eq!(
        resolve_market_state_for_definition_at_utc(&definition, chrono_tz::UTC, monday_late),
        MarketState::Open
    );
    assert_eq!(
        resolve_market_state_for_definition_at_utc(&definition, chrono_tz::UTC, tuesday_boundary),
        MarketState::Closed
    );
}

#[test]
fn loader_ingests_registry_with_multiple_exchanges() {
    let artifact_json = r#"
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
                                {
                                    "label": "regular",
                                    "open_time": "09:30",
                                    "close_time": "16:00"
                                }
                            ]
                        }
                    ]
                },
                {
                    "schema_version": "calendar.v1",
                    "exchange_code": "XTKS",
                    "exchange_name": "Tokyo Stock Exchange",
                    "timezone_name": "Asia/Tokyo",
                    "rule_metadata": {
                        "holiday_calendar_code": "JP_EQUITIES",
                        "early_close_rule_set": null
                    },
                    "regular_sessions": [
                        {
                            "weekday": "Monday",
                            "windows": [
                                {
                                    "label": "morning",
                                    "open_time": "09:00",
                                    "close_time": "11:30"
                                },
                                {
                                    "label": "afternoon",
                                    "open_time": "12:30",
                                    "close_time": "15:00"
                                }
                            ]
                        }
                    ]
                }
            ]
        }
        "#;

    let registry = ExchangeCalendarRegistryLoader::load_from_json(artifact_json)
        .expect("valid registry artifact should load");
    let xnys = registry
        .get_required("XNYS")
        .expect("XNYS must be present in registry");
    let xtks = registry
        .get_required("XTKS")
        .expect("XTKS must be present in registry");

    assert_eq!(xnys.timezone_name(), "America/New_York");
    assert_eq!(xtks.timezone_name(), "Asia/Tokyo");
}

#[test]
fn loader_ingests_holiday_and_early_close_schedule_fields() {
    let artifact_json = r#"
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
                            "weekday": "Tuesday",
                            "windows": [
                                {
                                    "label": "regular",
                                    "open_time": "09:30",
                                    "close_time": "16:00"
                                }
                            ]
                        },
                        {
                            "weekday": "Wednesday",
                            "windows": [
                                {
                                    "label": "regular",
                                    "open_time": "09:30",
                                    "close_time": "16:00"
                                }
                            ]
                        }
                    ],
                    "holiday_dates": ["2026-02-03"],
                    "early_close_sessions": [
                        {
                            "date": "2026-02-04",
                            "windows": [
                                {
                                    "label": "early_close",
                                    "open_time": "09:30",
                                    "close_time": "13:00"
                                }
                            ]
                        }
                    ]
                }
            ]
        }
        "#;

    let registry = ExchangeCalendarRegistryLoader::load_from_json(artifact_json)
        .expect("registry with holiday and early-close fields should load");
    let holiday_timestamp = timestamp_utc(2026, 2, 3, 15, 0, 0);
    let early_close_boundary = timestamp_utc(2026, 2, 4, 18, 0, 0);

    assert_eq!(
        registry.resolve_market_state_at_utc("XNYS", holiday_timestamp),
        Ok(MarketState::Closed)
    );
    assert_eq!(
        registry.resolve_market_state_at_utc("XNYS", early_close_boundary),
        Ok(MarketState::Closed)
    );
}

#[test]
fn loader_rejects_unknown_fields() {
    let artifact_json = r#"
        {
            "schema_version": "calendar.registry.v1",
            "artifact_date": "2026-01-31",
            "unexpected_field": true,
            "exchanges": []
        }
        "#;

    let result = ExchangeCalendarRegistryLoader::load_from_json(artifact_json);
    assert_eq!(
        result,
        Err(ExchangeCalendarRegistryLoadError::UnknownField {
            field: "unexpected_field".to_string(),
        })
    );
}

#[test]
fn loader_rejects_malformed_dates() {
    let artifact_json = r#"
        {
            "schema_version": "calendar.registry.v1",
            "artifact_date": "2026-13-01",
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
                                {
                                    "label": "regular",
                                    "open_time": "09:30",
                                    "close_time": "16:00"
                                }
                            ]
                        }
                    ]
                }
            ]
        }
        "#;

    let result = ExchangeCalendarRegistryLoader::load_from_json(artifact_json);
    match result {
        Err(ExchangeCalendarRegistryLoadError::MalformedDate {
            field,
            value,
            reason,
        }) => {
            assert_eq!(field, "artifact_date");
            assert_eq!(value, "2026-13-01");
            assert_eq!(reason, "month must be between 01 and 12");
        }
        _ => panic!("expected malformed date error"),
    }
}

#[test]
fn loader_rejects_duplicate_exchange_ids() {
    let artifact_json = r#"
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
                                {
                                    "label": "regular",
                                    "open_time": "09:30",
                                    "close_time": "16:00"
                                }
                            ]
                        }
                    ]
                },
                {
                    "schema_version": "calendar.v1",
                    "exchange_code": "XNYS",
                    "exchange_name": "New York Stock Exchange Copy",
                    "timezone_name": "America/New_York",
                    "rule_metadata": {
                        "holiday_calendar_code": "US_EQUITIES",
                        "early_close_rule_set": null
                    },
                    "regular_sessions": [
                        {
                            "weekday": "Tuesday",
                            "windows": [
                                {
                                    "label": "regular",
                                    "open_time": "09:30",
                                    "close_time": "16:00"
                                }
                            ]
                        }
                    ]
                }
            ]
        }
        "#;

    let result = ExchangeCalendarRegistryLoader::load_from_json(artifact_json);
    assert_eq!(
        result,
        Err(ExchangeCalendarRegistryLoadError::DuplicateExchangeId {
            exchange_code: "XNYS".to_string(),
        })
    );
}

#[test]
fn registry_lookup_returns_structured_error_for_missing_exchange() {
    let artifact_json = r#"
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
                                {
                                    "label": "regular",
                                    "open_time": "09:30",
                                    "close_time": "16:00"
                                }
                            ]
                        }
                    ]
                }
            ]
        }
        "#;

    let registry = ExchangeCalendarRegistryLoader::load_from_json(artifact_json)
        .expect("valid artifact should load");

    assert_eq!(registry.get("MISSING"), None);
    assert_eq!(
        registry.get_required("MISSING"),
        Err(ExchangeCalendarRegistryLookupError::MissingExchangeId {
            exchange_code: "MISSING".to_string(),
        })
    );
}

#[test]
fn resolver_returns_expected_boundaries_for_regular_session() {
    let registry = resolver_test_registry();

    let pre_open = timestamp_utc(2026, 2, 2, 14, 29, 59);
    let at_open = timestamp_utc(2026, 2, 2, 14, 30, 0);
    let in_session = timestamp_utc(2026, 2, 2, 20, 59, 59);
    let at_close = timestamp_utc(2026, 2, 2, 21, 0, 0);
    let post_close = timestamp_utc(2026, 2, 2, 21, 0, 1);

    assert_eq!(
        registry.resolve_market_state_at_utc("XNYS", pre_open),
        Ok(MarketState::Closed)
    );
    assert_eq!(
        registry.resolve_market_state_at_utc("XNYS", at_open),
        Ok(MarketState::Open)
    );
    assert_eq!(
        registry.resolve_market_state_at_utc("XNYS", in_session),
        Ok(MarketState::Open)
    );
    assert_eq!(
        registry.resolve_market_state_at_utc("XNYS", at_close),
        Ok(MarketState::Closed)
    );
    assert_eq!(
        registry.resolve_market_state_at_utc("XNYS", post_close),
        Ok(MarketState::Closed)
    );
}

#[test]
fn resolver_returns_closed_on_weekend_timestamp() {
    let registry = resolver_test_registry();
    let weekend_timestamp = timestamp_utc(2026, 2, 1, 3, 0, 0);

    assert_eq!(
        registry.resolve_market_state_at_utc("XNYS", weekend_timestamp),
        Ok(MarketState::Closed)
    );
}

#[test]
fn resolver_returns_closed_for_holiday_during_regular_weekday_session() {
    let mut artifact = xnys_full_week_artifact();
    artifact.holiday_dates = vec!["2026-02-03".to_string()];

    let registry = single_exchange_registry(artifact);
    let holiday_timestamp = timestamp_utc(2026, 2, 3, 15, 0, 0);

    assert_eq!(
        registry.resolve_market_state_at_utc("XNYS", holiday_timestamp),
        Ok(MarketState::Closed)
    );
}

#[test]
fn resolver_uses_truncated_early_close_window_for_market_state_transitions() {
    let mut artifact = xnys_full_week_artifact();
    artifact.early_close_sessions = vec![EarlyCloseSessionArtifact {
        date: "2026-02-04".to_string(),
        windows: vec![SessionWindowArtifact {
            label: "early_close".to_string(),
            open_time: "09:30".to_string(),
            close_time: "13:00".to_string(),
        }],
    }];

    let registry = single_exchange_registry(artifact);
    let before_early_close = timestamp_utc(2026, 2, 4, 17, 59, 59);
    let at_early_close = timestamp_utc(2026, 2, 4, 18, 0, 0);
    let after_early_close = timestamp_utc(2026, 2, 4, 18, 0, 1);

    assert_eq!(
        registry.resolve_market_state_at_utc("XNYS", before_early_close),
        Ok(MarketState::Open)
    );
    assert_eq!(
        registry.resolve_market_state_at_utc("XNYS", at_early_close),
        Ok(MarketState::Closed)
    );
    assert_eq!(
        registry.resolve_market_state_at_utc("XNYS", after_early_close),
        Ok(MarketState::Closed)
    );
}

#[test]
fn resolver_applies_holiday_precedence_over_early_close_schedule() {
    let mut artifact = xnys_full_week_artifact();
    artifact.holiday_dates = vec!["2026-02-05".to_string()];
    artifact.early_close_sessions = vec![EarlyCloseSessionArtifact {
        date: "2026-02-05".to_string(),
        windows: vec![SessionWindowArtifact {
            label: "early_close".to_string(),
            open_time: "09:30".to_string(),
            close_time: "13:00".to_string(),
        }],
    }];

    let registry = single_exchange_registry(artifact);
    let timestamp_inside_early_close_window = timestamp_utc(2026, 2, 5, 16, 0, 0);

    assert_eq!(
        registry.resolve_market_state_at_utc("XNYS", timestamp_inside_early_close_window),
        Ok(MarketState::Closed)
    );
}

#[test]
fn resolver_returns_structured_error_for_missing_exchange_id() {
    let registry = resolver_test_registry();
    let timestamp = timestamp_utc(2026, 2, 2, 14, 30, 0);

    assert_eq!(
        registry.resolve_market_state_at_utc("MISSING", timestamp),
        Err(ExchangeCalendarResolveError::MissingExchangeId {
            exchange_code: "MISSING".to_string(),
        })
    );
}

#[test]
fn resolver_uses_exchange_timezone_and_cross_day_local_date() {
    let registry = resolver_test_registry();
    let same_utc_timestamp = timestamp_utc(2026, 2, 2, 0, 30, 0);

    assert_eq!(
        registry.resolve_market_state_at_utc("XNYS", same_utc_timestamp),
        Ok(MarketState::Closed)
    );
    assert_eq!(
        registry.resolve_market_state_at_utc("XTKS", same_utc_timestamp),
        Ok(MarketState::Open)
    );
}

#[test]
fn resolver_handles_cross_day_transition_for_ahead_timezone_exchange() {
    let registry = resolver_test_registry();

    let before_utc_midnight = timestamp_utc(2026, 2, 2, 23, 59, 0);
    let after_utc_midnight = timestamp_utc(2026, 2, 3, 0, 30, 0);

    assert_eq!(
        registry.resolve_market_state_at_utc("XTKS", before_utc_midnight),
        Ok(MarketState::Closed)
    );
    assert_eq!(
        registry.resolve_market_state_at_utc("XTKS", after_utc_midnight),
        Ok(MarketState::Open)
    );
}

#[test]
fn resolver_returns_structured_error_for_invalid_timezone_name() {
    let artifact = ExchangeCalendarRegistryArtifact {
        schema_version: "calendar.registry.v1".to_string(),
        artifact_date: "2026-01-31".to_string(),
        exchanges: vec![ExchangeCalendarArtifact {
            schema_version: "calendar.v1".to_string(),
            exchange_code: "XBAD".to_string(),
            exchange_name: "Broken Exchange".to_string(),
            timezone_name: Some("Not/A_Real_Timezone".to_string()),
            rule_metadata: CalendarRuleMetadataArtifact {
                holiday_calendar_code: "BROKEN".to_string(),
                early_close_rule_set: None,
            },
            regular_sessions: full_week_regular_sessions("09:30", "16:00"),
            holiday_dates: vec![],
            early_close_sessions: vec![],
        }],
    };

    let registry = ExchangeCalendarRegistry::from_artifact(artifact)
        .expect("registry artifact should validate before timezone resolution");
    let timestamp = timestamp_utc(2026, 2, 2, 14, 30, 0);

    assert_eq!(
        registry.resolve_market_state_at_utc("XBAD", timestamp),
        Err(ExchangeCalendarResolveError::InvalidTimezoneName {
            exchange_code: "XBAD".to_string(),
            timezone_name: "Not/A_Real_Timezone".to_string(),
        })
    );
}

#[test]
fn preprocessing_applies_session_filter_and_local_context() {
    let registry = resolver_test_registry();

    let input_records = vec![
        timestamp_input_record(0, 2026, 2, 2, 14, 30, 0),
        timestamp_input_record(1, 2026, 2, 2, 3, 0, 0),
    ];

    let output = registry
        .preprocess_timestamp_events(
            "XNYS",
            input_records,
            TimestampEventSessionFilter::OpenOnly,
            1,
        )
        .expect("preprocessing should succeed");

    assert_eq!(output.exchange_timezone_name, "America/New_York");
    assert_eq!(output.source_record_positions, vec![0]);
    assert_eq!(
        output.timestamp_utc_nanos,
        vec![
            timestamp_utc(2026, 2, 2, 14, 30, 0)
                .timestamp_nanos_opt()
                .expect("fixture timestamp should be in nanos range")
        ]
    );
    assert_eq!(output.exchange_local_minute_of_day, vec![570]);
    assert_eq!(output.exchange_local_day_of_week, vec![1]);
    assert_eq!(output.market_state_codes, vec![1]);
    assert_eq!(output.diagnostics.accepted_record_count, 1);
    assert_eq!(output.diagnostics.filtered_record_count, 1);
}

#[test]
fn preprocessing_is_deterministic_and_emits_sparse_hour_diagnostics() {
    let registry = resolver_test_registry();

    let input_records = vec![
        timestamp_input_record(0, 2026, 2, 2, 16, 30, 0),
        timestamp_input_record(1, 2026, 2, 2, 14, 30, 0),
        timestamp_input_record(2, 2026, 2, 2, 16, 31, 0),
    ];

    let first_output = registry
        .preprocess_timestamp_events(
            "XNYS",
            input_records.clone(),
            TimestampEventSessionFilter::All,
            2,
        )
        .expect("preprocessing should succeed");
    let second_output = registry
        .preprocess_timestamp_events("XNYS", input_records, TimestampEventSessionFilter::All, 2)
        .expect("preprocessing should succeed");

    assert_eq!(first_output, second_output);
    assert_eq!(first_output.source_record_positions, vec![1, 0, 2]);
    assert_eq!(
        first_output.missing_hour_bucket_utc_nanos,
        vec![
            timestamp_utc(2026, 2, 2, 15, 0, 0)
                .timestamp_nanos_opt()
                .expect("fixture timestamp should be in nanos range")
        ]
    );
    assert_eq!(
        first_output.sparse_hour_bucket_utc_nanos,
        vec![
            timestamp_utc(2026, 2, 2, 14, 0, 0)
                .timestamp_nanos_opt()
                .expect("fixture timestamp should be in nanos range")
        ]
    );
}

#[test]
fn preprocessing_rejects_invalid_sparse_hour_threshold() {
    let registry = resolver_test_registry();

    let result = registry.preprocess_timestamp_events(
        "XNYS",
        vec![timestamp_input_record(0, 2026, 2, 2, 14, 30, 0)],
        TimestampEventSessionFilter::All,
        0,
    );

    assert_eq!(
        result,
        Err(
            TimestampEventPreprocessingError::InvalidSparseHourEventThreshold {
                sparse_hour_event_threshold: 0,
            },
        ),
    );
}

#[test]
fn registry_loader_accepts_wrapper_document() {
    let document = json!({
        "calendar_registry": {
            "schema_version": "calendar.registry.v1",
            "artifact_date": "2026-01-31",
            "exchanges": [valid_exchange_artifact()],
        }
    });

    let registry = ExchangeCalendarRegistryLoader::load_from_json_value(document)
        .expect("wrapped registry document should load");

    assert!(registry.get("XNYS").is_some());
}

#[test]
fn registry_loader_rejects_wrapper_without_registry_payload() {
    let result = ExchangeCalendarRegistryLoader::load_from_json_value(json!({
        "schema_version": "calendar.fixture.v1",
    }));

    assert_eq!(
        result,
        Err(ExchangeCalendarRegistryLoadError::MalformedArtifact {
            reason: "must be a calendar.registry.v1 object or contain calendar_registry"
                .to_string(),
        }),
    );
}

fn valid_exchange_artifact() -> ExchangeCalendarArtifact {
    ExchangeCalendarArtifact {
        schema_version: "calendar.v1".to_string(),
        exchange_code: "XNYS".to_string(),
        exchange_name: "New York Stock Exchange".to_string(),
        timezone_name: Some("America/New_York".to_string()),
        rule_metadata: CalendarRuleMetadataArtifact {
            holiday_calendar_code: "US_EQUITIES".to_string(),
            early_close_rule_set: Some("US_EQUITIES_EARLY_CLOSE".to_string()),
        },
        regular_sessions: vec![
            RegularSessionArtifact {
                weekday: TradingWeekday::Monday,
                windows: vec![SessionWindowArtifact {
                    label: "regular".to_string(),
                    open_time: "09:30".to_string(),
                    close_time: "16:00".to_string(),
                }],
            },
            RegularSessionArtifact {
                weekday: TradingWeekday::Tuesday,
                windows: vec![SessionWindowArtifact {
                    label: "regular".to_string(),
                    open_time: "09:30".to_string(),
                    close_time: "16:00".to_string(),
                }],
            },
            RegularSessionArtifact {
                weekday: TradingWeekday::Wednesday,
                windows: vec![
                    SessionWindowArtifact {
                        label: "pre_lunch".to_string(),
                        open_time: "09:30".to_string(),
                        close_time: "11:30".to_string(),
                    },
                    SessionWindowArtifact {
                        label: "post_lunch".to_string(),
                        open_time: "12:30".to_string(),
                        close_time: "16:00".to_string(),
                    },
                ],
            },
        ],
        holiday_dates: vec![],
        early_close_sessions: vec![],
    }
}

fn resolver_test_registry() -> ExchangeCalendarRegistry {
    let artifact = ExchangeCalendarRegistryArtifact {
        schema_version: "calendar.registry.v1".to_string(),
        artifact_date: "2026-01-31".to_string(),
        exchanges: vec![xnys_full_week_artifact(), xtks_full_week_artifact()],
    };

    ExchangeCalendarRegistry::from_artifact(artifact)
        .expect("resolver test registry artifact should be valid")
}

fn single_exchange_registry(
    exchange_artifact: ExchangeCalendarArtifact,
) -> ExchangeCalendarRegistry {
    let artifact = ExchangeCalendarRegistryArtifact {
        schema_version: "calendar.registry.v1".to_string(),
        artifact_date: "2026-01-31".to_string(),
        exchanges: vec![exchange_artifact],
    };

    ExchangeCalendarRegistry::from_artifact(artifact)
        .expect("single-exchange registry artifact should be valid")
}

fn xnys_full_week_artifact() -> ExchangeCalendarArtifact {
    ExchangeCalendarArtifact {
        schema_version: "calendar.v1".to_string(),
        exchange_code: "XNYS".to_string(),
        exchange_name: "New York Stock Exchange".to_string(),
        timezone_name: Some("America/New_York".to_string()),
        rule_metadata: CalendarRuleMetadataArtifact {
            holiday_calendar_code: "US_EQUITIES".to_string(),
            early_close_rule_set: Some("US_EQUITIES_EARLY_CLOSE".to_string()),
        },
        regular_sessions: full_week_regular_sessions("09:30", "16:00"),
        holiday_dates: vec![],
        early_close_sessions: vec![],
    }
}

fn xtks_full_week_artifact() -> ExchangeCalendarArtifact {
    ExchangeCalendarArtifact {
        schema_version: "calendar.v1".to_string(),
        exchange_code: "XTKS".to_string(),
        exchange_name: "Tokyo Stock Exchange".to_string(),
        timezone_name: Some("Asia/Tokyo".to_string()),
        rule_metadata: CalendarRuleMetadataArtifact {
            holiday_calendar_code: "JP_EQUITIES".to_string(),
            early_close_rule_set: None,
        },
        regular_sessions: full_week_regular_sessions("09:30", "15:00"),
        holiday_dates: vec![],
        early_close_sessions: vec![],
    }
}

fn full_week_regular_sessions(open_time: &str, close_time: &str) -> Vec<RegularSessionArtifact> {
    [
        TradingWeekday::Monday,
        TradingWeekday::Tuesday,
        TradingWeekday::Wednesday,
        TradingWeekday::Thursday,
        TradingWeekday::Friday,
    ]
    .into_iter()
    .map(|weekday| RegularSessionArtifact {
        weekday,
        windows: vec![SessionWindowArtifact {
            label: "regular".to_string(),
            open_time: open_time.to_string(),
            close_time: close_time.to_string(),
        }],
    })
    .collect()
}

fn timestamp_utc(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
        .single()
        .expect("timestamp fixture must resolve to a unique UTC datetime")
}

fn timestamp_input_record(
    source_record_position: u64,
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> TimestampEventInputRecord {
    TimestampEventInputRecord {
        source_record_position,
        timestamp_utc: timestamp_utc(year, month, day, hour, minute, second),
    }
}
