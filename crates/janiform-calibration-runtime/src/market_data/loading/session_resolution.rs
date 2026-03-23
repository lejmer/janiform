use chrono::{DateTime, Datelike, Duration, Utc};
use std::collections::HashSet;

use crate::calendar::ExchangeCalendarRegistryLoader;
use crate::market_data::preprocessing::MarketDataTranslationTickRecord;

use super::calendar_materialization::{
    automatic_resolution_reasons, automatic_resolution_warnings, build_calendar_registry_json,
    build_recurring_closed_windows, compute_open_session_duration_hours_per_week,
    confidence_label_for_gap_alignment, confidence_score_for_inference, evaluate_gap_alignment,
    looks_like_continuous_weekday_template, regular_session_windows_from_definition,
    trading_weekdays, weekday_names,
};
use super::contracts::{
    MarketDataInputFileFormat, MarketDataInputFileReference, MarketDataLoadingError,
    MarketDataSessionConfiguration, MarketDataSessionRegularWindowPayload,
    MarketDataSessionResolutionPayload,
};
use super::shared::{
    LONG_GAP_THRESHOLD_HOURS, MINIMUM_GENERIC_24_5_WEEKDAY_COUNT, ResolvedMarketDataSession,
    SESSION_RESOLUTION_SCHEMA_VERSION, SessionTemplate,
};

pub(super) fn resolve_market_data_session_configuration_from_tick_records(
    session_configuration: &MarketDataSessionConfiguration,
    tick_records: &[MarketDataTranslationTickRecord],
) -> Result<ResolvedMarketDataSession, MarketDataLoadingError> {
    if let Some(session_source) = &session_configuration.session_definition_source {
        return resolve_explicit_session_definition(
            session_configuration.exchange_code.as_str(),
            session_source,
            session_configuration.session_timezone.as_deref(),
        );
    }

    let inferred_template = infer_session_template(
        session_configuration.exchange_code.as_str(),
        session_configuration.session_timezone.as_deref(),
        tick_records,
    )?;

    let registry_json = build_calendar_registry_json(
        session_configuration.exchange_code.as_str(),
        &inferred_template,
    )?;

    let calendar_registry = ExchangeCalendarRegistryLoader::load_from_json(registry_json.as_str())
        .map_err(|error| MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration".to_string(),
            message: format!("does not contain a valid calendar registry: {error}"),
        })?;

    let gap_alignment_diagnostics = evaluate_gap_alignment(
        &calendar_registry,
        session_configuration.exchange_code.as_str(),
        tick_records,
    )?;

    Ok(ResolvedMarketDataSession {
        calendar_registry,
        session_resolution: MarketDataSessionResolutionPayload {
            schema_version: SESSION_RESOLUTION_SCHEMA_VERSION.to_string(),
            resolution_kind: "automatic_inference".to_string(),
            selected_policy_id: inferred_template.policy_id.clone(),
            manual_override_applied: false,
            exchange_code: session_configuration.exchange_code.clone(),
            exchange_name: inferred_template.exchange_name.clone(),
            resolved_timezone_name: inferred_template.timezone_name.clone(),
            timezone_source: inferred_template.timezone_source.clone(),
            open_session_duration_hours_per_week: compute_open_session_duration_hours_per_week(
                &inferred_template,
            ),
            confidence_score: confidence_score_for_inference(
                &inferred_template,
                gap_alignment_diagnostics,
            ),
            confidence_label: confidence_label_for_gap_alignment(
                &inferred_template,
                gap_alignment_diagnostics,
            ),
            regular_session_windows: inferred_template.regular_session_windows.clone(),
            recurring_closed_windows: build_recurring_closed_windows(&inferred_template),
            reasons: automatic_resolution_reasons(
                session_configuration.exchange_code.as_str(),
                &inferred_template,
                gap_alignment_diagnostics,
            ),
            ambiguity_warnings: automatic_resolution_warnings(
                &inferred_template,
                gap_alignment_diagnostics,
            ),
            metadata_discrepancies: Vec::new(),
            resolved_session_definition_json: registry_json,
        },
    })
}

pub(super) fn resolve_explicit_session_definition(
    exchange_code: &str,
    session_source: &MarketDataInputFileReference,
    configured_session_timezone: Option<&str>,
) -> Result<ResolvedMarketDataSession, MarketDataLoadingError> {
    if session_source.file_format != MarketDataInputFileFormat::Json {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration.session_definition_source.file_format".to_string(),
            message: "must be json when a session-definition source is provided".to_string(),
        });
    }

    let calendar_registry_json =
        std::fs::read_to_string(&session_source.path).map_err(|error| {
            let field_path = "session_configuration.session_definition_source.path".to_string();

            match error.kind() {
                std::io::ErrorKind::NotFound => MarketDataLoadingError::Io {
                    field_path,
                    message: "does not exist".to_string(),
                },
                _ => MarketDataLoadingError::Io {
                    field_path,
                    message: format!("could not be read: {error}"),
                },
            }
        })?;

    let calendar_registry = ExchangeCalendarRegistryLoader::load_from_json(
        calendar_registry_json.as_str(),
    )
    .map_err(|error| MarketDataLoadingError::InvalidValue {
        field_path: "session_configuration.session_definition_source.path".to_string(),
        message: format!("does not contain a valid calendar registry: {error}"),
    })?;

    let definition = calendar_registry.get_required(exchange_code).map_err(|_| {
        MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration.exchange_code".to_string(),
            message: "must exist in the resolved calendar registry".to_string(),
        }
    })?;

    let resolved_timezone_name = definition.timezone_name().to_string();
    let exchange_name = definition.exchange_metadata().exchange_name().to_string();
    let holiday_calendar_code = definition
        .rule_metadata()
        .holiday_calendar_code()
        .to_string();
    let early_close_rule_set = definition
        .rule_metadata()
        .early_close_rule_set()
        .map(ToOwned::to_owned);

    let metadata_discrepancies = match configured_session_timezone {
        Some(configured_timezone) if configured_timezone != resolved_timezone_name => {
            vec![format!(
                "configured session_timezone does not match explicit session-definition timezone for `{exchange_code}` and the session-definition timezone was used instead"
            )]
        }
        _ => Vec::new(),
    };

    let regular_session_windows = regular_session_windows_from_definition(definition);

    let session_template = SessionTemplate {
        policy_id: "session_definition_source.calendar_registry_json.v1".to_string(),
        exchange_name: exchange_name.clone(),
        timezone_name: resolved_timezone_name.clone(),
        timezone_source: "session_definition_source".to_string(),
        holiday_calendar_code,
        early_close_rule_set,
        regular_session_windows: regular_session_windows.clone(),
        continuous_weekday_template: looks_like_continuous_weekday_template(
            exchange_code,
            regular_session_windows.as_slice(),
        ),
    };

    Ok(ResolvedMarketDataSession {
        calendar_registry,
        session_resolution: MarketDataSessionResolutionPayload {
            schema_version: SESSION_RESOLUTION_SCHEMA_VERSION.to_string(),
            resolution_kind: "explicit_override".to_string(),
            selected_policy_id: "session_definition_source.calendar_registry_json.v1".to_string(),
            manual_override_applied: true,
            exchange_code: exchange_code.to_string(),
            exchange_name,
            resolved_timezone_name,
            timezone_source: "session_definition_source".to_string(),
            open_session_duration_hours_per_week: compute_open_session_duration_hours_per_week(
                &session_template,
            ),
            confidence_score: 1.0,
            confidence_label: "explicit_override".to_string(),
            regular_session_windows,
            recurring_closed_windows: build_recurring_closed_windows(&session_template),
            reasons: vec![
                "loaded session behavior from the explicit session-definition override and reused it for calibration and simulation preparation".to_string(),
            ],
            ambiguity_warnings: Vec::new(),
            metadata_discrepancies,
            resolved_session_definition_json: calendar_registry_json,
        },
    })
}

pub(super) fn infer_session_template(
    exchange_code: &str,
    configured_session_timezone: Option<&str>,
    tick_records: &[MarketDataTranslationTickRecord],
) -> Result<SessionTemplate, MarketDataLoadingError> {
    match exchange_code {
        "XNYS" | "XNAS" => {
            let exchange_name = if exchange_code == "XNYS" {
                "New York Stock Exchange"
            } else {
                "Nasdaq Stock Market"
            };

            if let Some(timezone_name) = configured_session_timezone {
                if timezone_name != "America/New_York" {
                    return Err(MarketDataLoadingError::InvalidValue {
                        field_path: "session_configuration.session_timezone".to_string(),
                        message: format!(
                            "conflicts with the built-in exchange default for {exchange_code}. Provide an explicit session_definition_source to override the default."
                        ),
                    });
                }
            }

            return Ok(SessionTemplate {
                policy_id: format!(
                    "exchange_code_builtin_default.{}_regular_session.v1",
                    exchange_code.to_ascii_lowercase()
                ),
                exchange_name: exchange_name.to_string(),
                timezone_name: "America/New_York".to_string(),
                timezone_source: "exchange_code_builtin_default".to_string(),
                holiday_calendar_code: "US_EQUITIES".to_string(),
                early_close_rule_set: Some("US_EQUITIES_EARLY_CLOSE".to_string()),
                regular_session_windows: trading_weekdays()
                    .iter()
                    .map(|weekday: &&str| MarketDataSessionRegularWindowPayload {
                        weekday: weekday.to_string(),
                        label: "regular".to_string(),
                        open_time: "09:30".to_string(),
                        close_time: "16:00".to_string(),
                    })
                    .collect(),
                continuous_weekday_template: false,
            });
        }
        "FXSPOT" => {
            if let Some(timezone_name) = configured_session_timezone {
                if timezone_name != "UTC" {
                    return Err(MarketDataLoadingError::InvalidValue {
                        field_path: "session_configuration.session_timezone".to_string(),
                        message: "conflicts with the built-in exchange default for FXSPOT. Provide an explicit session_definition_source to override the default.".to_string(),
                    });
                }
            }

            return Ok(SessionTemplate {
                policy_id: "exchange_code_builtin_default.fxspot_weekend_window.v1".to_string(),
                exchange_name: "OTC FX Spot 24/5".to_string(),
                timezone_name: "UTC".to_string(),
                timezone_source: "exchange_code_builtin_default".to_string(),
                holiday_calendar_code: "FX_24_5".to_string(),
                early_close_rule_set: None,
                regular_session_windows: vec![
                    MarketDataSessionRegularWindowPayload {
                        weekday: "Monday".to_string(),
                        label: "spot".to_string(),
                        open_time: "00:00".to_string(),
                        close_time: "24:00".to_string(),
                    },
                    MarketDataSessionRegularWindowPayload {
                        weekday: "Tuesday".to_string(),
                        label: "spot".to_string(),
                        open_time: "00:00".to_string(),
                        close_time: "24:00".to_string(),
                    },
                    MarketDataSessionRegularWindowPayload {
                        weekday: "Wednesday".to_string(),
                        label: "spot".to_string(),
                        open_time: "00:00".to_string(),
                        close_time: "24:00".to_string(),
                    },
                    MarketDataSessionRegularWindowPayload {
                        weekday: "Thursday".to_string(),
                        label: "spot".to_string(),
                        open_time: "00:00".to_string(),
                        close_time: "24:00".to_string(),
                    },
                    MarketDataSessionRegularWindowPayload {
                        weekday: "Friday".to_string(),
                        label: "spot".to_string(),
                        open_time: "00:00".to_string(),
                        close_time: "19:00".to_string(),
                    },
                    MarketDataSessionRegularWindowPayload {
                        weekday: "Sunday".to_string(),
                        label: "spot".to_string(),
                        open_time: "21:00".to_string(),
                        close_time: "24:00".to_string(),
                    },
                ],
                continuous_weekday_template: false,
            });
        }
        "CRYPTO" => {
            if let Some(timezone_name) = configured_session_timezone {
                if timezone_name != "UTC" {
                    return Err(MarketDataLoadingError::InvalidValue {
                        field_path: "session_configuration.session_timezone".to_string(),
                        message: "conflicts with the built-in exchange default for CRYPTO. Provide an explicit session_definition_source to override the default.".to_string(),
                    });
                }
            }

            return Ok(SessionTemplate {
                policy_id: "exchange_code_builtin_default.crypto_continuous_session.v1".to_string(),
                exchange_name: "Crypto Spot 24/7".to_string(),
                timezone_name: "UTC".to_string(),
                timezone_source: "exchange_code_builtin_default".to_string(),
                holiday_calendar_code: "CRYPTO_24_7".to_string(),
                early_close_rule_set: None,
                regular_session_windows: weekday_names()
                    .iter()
                    .map(|weekday: &&str| MarketDataSessionRegularWindowPayload {
                        weekday: weekday.to_string(),
                        label: "spot".to_string(),
                        open_time: "00:00".to_string(),
                        close_time: "24:00".to_string(),
                    })
                    .collect(),
                continuous_weekday_template: false,
            });
        }
        _ => {}
    }

    if exchange_code.starts_with("FX_") || exchange_code.starts_with("CFD_") {
        let generic_template =
            build_generic_24_5_template(exchange_code, configured_session_timezone);

        validate_generic_24_5_inference(exchange_code, tick_records)?;

        return Ok(generic_template);
    }

    let Some(timezone_name) = configured_session_timezone else {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration".to_string(),
            message: "could not infer session behavior because no explicit session_definition_source or session_timezone was provided for an exchange code without built-in defaults. Provide a session-definition override.".to_string(),
        });
    };

    let generic_template = build_generic_24_5_template(exchange_code, Some(timezone_name));

    validate_generic_24_5_inference(exchange_code, tick_records)?;

    Ok(generic_template)
}

pub(super) fn build_generic_24_5_template(
    exchange_code: &str,
    configured_session_timezone: Option<&str>,
) -> SessionTemplate {
    let timezone_name = configured_session_timezone.unwrap_or("UTC").to_string();
    let exchange_label = exchange_code.replace('_', " ");

    let (holiday_calendar_code, exchange_name) = if exchange_code.starts_with("CFD_") {
        ("CFD_24_5", format!("{} CFD 24/5", exchange_label))
    } else {
        ("FX_24_5", format!("{} 24/5", exchange_label))
    };

    SessionTemplate {
        policy_id: "recurring_weekly_inactivity.generic_24_5_broker_market.v1".to_string(),
        exchange_name,
        timezone_name,
        timezone_source: configured_session_timezone
            .map(|_| "session_configuration.session_timezone".to_string())
            .unwrap_or_else(|| "default_utc_for_generic_24_5_inference".to_string()),
        holiday_calendar_code: holiday_calendar_code.to_string(),
        early_close_rule_set: None,
        regular_session_windows: trading_weekdays()
            .iter()
            .map(|weekday: &&str| MarketDataSessionRegularWindowPayload {
                weekday: weekday.to_string(),
                label: "regular".to_string(),
                open_time: "00:00".to_string(),
                close_time: "24:00".to_string(),
            })
            .collect(),
        continuous_weekday_template: true,
    }
}

pub(super) fn validate_generic_24_5_inference(
    exchange_code: &str,
    tick_records: &[MarketDataTranslationTickRecord],
) -> Result<(), MarketDataLoadingError> {
    let mut distinct_weekdays = HashSet::new();

    let mut long_gap_count = 0;
    let mut weekend_gap_count = 0;
    let mut unexpected_weekday_gap_count = 0;

    for tick_record in tick_records {
        let weekday = DateTime::<Utc>::from_timestamp_nanos(tick_record.timestamp_utc_nanos)
            .weekday()
            .num_days_from_monday();
        distinct_weekdays.insert(weekday);
    }

    for pair in tick_records.windows(2) {
        let gap_nanos = pair[1].timestamp_utc_nanos - pair[0].timestamp_utc_nanos;
        if gap_nanos
            < Duration::hours(LONG_GAP_THRESHOLD_HOURS)
                .num_nanoseconds()
                .unwrap_or(0)
        {
            continue;
        }

        long_gap_count += 1;

        let midpoint_nanos = pair[0].timestamp_utc_nanos + (gap_nanos / 2);
        let midpoint_weekday = DateTime::<Utc>::from_timestamp_nanos(midpoint_nanos)
            .weekday()
            .num_days_from_monday();

        if midpoint_weekday >= 5 {
            weekend_gap_count += 1;
        } else {
            unexpected_weekday_gap_count += 1;
        }
    }

    if distinct_weekdays.len() < MINIMUM_GENERIC_24_5_WEEKDAY_COUNT {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration".to_string(),
            message: format!(
                "automatic 24/5 session inference is ambiguous because the observed tick window does not span enough weekdays. Provide an explicit session-definition override. exchange_code={exchange_code}"
            ),
        });
    }

    if long_gap_count == 0 || weekend_gap_count == 0 || unexpected_weekday_gap_count > 0 {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration".to_string(),
            message: format!(
                "automatic 24/5 session inference is ambiguous because the observed gap structure does not clearly support a weekday-continuous broker market. Provide an explicit session-definition override. exchange_code={exchange_code}"
            ),
        });
    }

    Ok(())
}
