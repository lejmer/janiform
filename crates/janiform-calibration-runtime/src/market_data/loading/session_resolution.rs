use crate::market_data::preprocessing::MarketDataTranslationTickRecord;
use janiform_market_time::calendar::ExchangeCalendarRegistryLoader;

use super::bundled_sessions::bundled_session_template;
use super::calendar_materialization::{
    automatic_resolution_reasons, automatic_resolution_warnings, build_calendar_registry_json,
    build_recurring_closed_windows, compute_open_session_duration_hours_per_week,
    confidence_label_for_gap_alignment, confidence_score_for_inference, evaluate_gap_alignment,
    looks_like_continuous_weekday_template, regular_session_windows_from_definition,
    trading_weekdays,
};
use super::contracts::{
    MarketDataInputFileFormat, MarketDataInputFileReference, MarketDataLoadingError,
    MarketDataSessionConfiguration, MarketDataSessionRegularWindowPayload,
    MarketDataSessionResolutionPayload,
};
use super::session_gap_spectrum::ObservedGapSpectrum;
use super::shared::{
    ResolvedMarketDataSession, SESSION_RESOLUTION_SCHEMA_VERSION, SessionTemplate,
};

pub(super) fn resolve_market_data_session_configuration_from_tick_records(
    session_configuration: &MarketDataSessionConfiguration,
    tick_records: &[MarketDataTranslationTickRecord],
) -> Result<ResolvedMarketDataSession, MarketDataLoadingError> {
    let observed_gap_spectrum = ObservedGapSpectrum::from_tick_records(tick_records);

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
        &observed_gap_spectrum,
    )?;

    resolved_market_data_session_from_template(
        session_configuration,
        inferred_template,
        tick_records,
        &observed_gap_spectrum,
    )
}

pub(super) fn try_resolve_market_data_session_configuration_without_tick_records(
    session_configuration: &MarketDataSessionConfiguration,
) -> Result<Option<ResolvedMarketDataSession>, MarketDataLoadingError> {
    if let Some(session_source) = &session_configuration.session_definition_source {
        return Ok(Some(resolve_explicit_session_definition(
            session_configuration.exchange_code.as_str(),
            session_source,
            session_configuration.session_timezone.as_deref(),
        )?));
    }

    let Some(inferred_template) = infer_session_template_without_tick_records(
        session_configuration.exchange_code.as_str(),
        session_configuration.session_timezone.as_deref(),
    )?
    else {
        return Ok(None);
    };
    let observed_gap_spectrum = ObservedGapSpectrum::from_tick_records(&[]);

    Ok(Some(resolved_market_data_session_from_template(
        session_configuration,
        inferred_template,
        &[],
        &observed_gap_spectrum,
    )?))
}

fn resolved_market_data_session_from_template(
    session_configuration: &MarketDataSessionConfiguration,
    inferred_template: SessionTemplate,
    tick_records: &[MarketDataTranslationTickRecord],
    observed_gap_spectrum: &ObservedGapSpectrum,
) -> Result<ResolvedMarketDataSession, MarketDataLoadingError> {
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
        observed_gap_spectrum,
    )?;

    Ok(ResolvedMarketDataSession {
        session_resolution: MarketDataSessionResolutionPayload {
            schema_version: SESSION_RESOLUTION_SCHEMA_VERSION.to_string(),
            resolution_kind: "automatic_inference".to_string(),
            provenance_kind: inferred_template.provenance_kind.clone(),
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
        provenance_kind: "explicit_definition".to_string(),
        exchange_name: exchange_name.clone(),
        timezone_name: resolved_timezone_name.clone(),
        timezone_source: "session_definition_source".to_string(),
        holiday_calendar_code,
        early_close_rule_set,
        regular_session_windows: regular_session_windows.clone(),
        recurring_closed_windows: Vec::new(),
        continuous_weekday_template: looks_like_continuous_weekday_template(
            exchange_code,
            regular_session_windows.as_slice(),
        ),
    };

    Ok(ResolvedMarketDataSession {
        session_resolution: MarketDataSessionResolutionPayload {
            schema_version: SESSION_RESOLUTION_SCHEMA_VERSION.to_string(),
            resolution_kind: "explicit_override".to_string(),
            provenance_kind: "explicit_definition".to_string(),
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
    observed_gap_spectrum: &ObservedGapSpectrum,
) -> Result<SessionTemplate, MarketDataLoadingError> {
    if let Some(template) =
        infer_session_template_without_tick_records(exchange_code, configured_session_timezone)?
    {
        return Ok(template);
    }

    if exchange_code.starts_with("FX_") || exchange_code.starts_with("CFD_") {
        let generic_template =
            build_generic_24_5_template(exchange_code, configured_session_timezone);

        validate_generic_24_5_inference(exchange_code, observed_gap_spectrum)?;

        return Ok(generic_template);
    }

    let Some(timezone_name) = configured_session_timezone else {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration".to_string(),
            message: "could not infer session behavior because no explicit session_definition_source or session_timezone was provided for an exchange code without built-in defaults. Provide a session-definition override.".to_string(),
        });
    };

    let generic_template = build_generic_24_5_template(exchange_code, Some(timezone_name));

    validate_generic_24_5_inference(exchange_code, observed_gap_spectrum)?;

    Ok(generic_template)
}

fn infer_session_template_without_tick_records(
    exchange_code: &str,
    configured_session_timezone: Option<&str>,
) -> Result<Option<SessionTemplate>, MarketDataLoadingError> {
    let Some(template) = bundled_session_template(exchange_code)? else {
        return Ok(None);
    };

    if let Some(timezone_name) = configured_session_timezone
        && timezone_name != template.timezone_name
    {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration.session_timezone".to_string(),
            message: format!(
                "conflicts with the bundled session preset for {exchange_code}. Provide an explicit session_definition_source to override the default."
            ),
        });
    }

    Ok(Some(template))
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
        provenance_kind: "probe_fallback".to_string(),
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
        recurring_closed_windows: Vec::new(),
        continuous_weekday_template: true,
    }
}

pub(super) fn validate_generic_24_5_inference(
    exchange_code: &str,
    observed_gap_spectrum: &ObservedGapSpectrum,
) -> Result<(), MarketDataLoadingError> {
    let weekday_support = observed_gap_spectrum.weekday_support();
    let weekend_tick_count = weekday_support[5] + weekday_support[6];

    if weekend_tick_count > 0 {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration".to_string(),
            message: format!(
                "automatic 24/5 session inference is ambiguous because the observed weekday support includes weekend trading. Provide an explicit session-definition override. exchange_code={exchange_code}"
            ),
        });
    }

    let weekday_continuous_support_count = weekday_support[..trading_weekdays().len()]
        .iter()
        .filter(|support_count| **support_count > 0)
        .count();
    let weekend_inactivity_gap_count = observed_gap_spectrum
        .inactivity_gap_midpoint_weekday_support()[5]
        + observed_gap_spectrum.inactivity_gap_midpoint_weekday_support()[6];

    if weekend_inactivity_gap_count == 0
        && weekday_continuous_support_count < trading_weekdays().len()
    {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration".to_string(),
            message: format!(
                "automatic 24/5 session inference is ambiguous because the derived inactivity-gap support does not isolate a weekend closure and the observed weekday support does not span the full weekday-continuous template. Provide an explicit session-definition override. exchange_code={exchange_code}"
            ),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ObservedGapSpectrum, infer_session_template, validate_generic_24_5_inference};
    use crate::market_data::loading::contracts::MarketDataLoadingError;
    use crate::market_data::loading::test_support::tick;

    #[test]
    fn generic_24_5_inference_accepts_full_weekday_support_without_weekend_gap() {
        let tick_records = vec![
            tick("2026-04-06T08:00:00Z"),
            tick("2026-04-06T17:00:00Z"),
            tick("2026-04-07T08:30:00Z"),
            tick("2026-04-08T15:30:00Z"),
            tick("2026-04-09T12:00:00Z"),
            tick("2026-04-10T18:00:00Z"),
        ];
        let observed_gap_spectrum = ObservedGapSpectrum::from_tick_records(&tick_records);

        let inferred_template =
            infer_session_template("FX_EURUSD", Some("UTC"), &observed_gap_spectrum)
                .expect("weekday-only FX support should remain inferable");

        assert!(inferred_template.continuous_weekday_template);
        assert_eq!(inferred_template.timezone_name, "UTC");
    }

    #[test]
    fn generic_24_5_inference_rejects_weekend_tick_support() {
        let observed_gap_spectrum = ObservedGapSpectrum::from_tick_records(&[
            tick("2026-04-10T18:00:00Z"),
            tick("2026-04-11T09:00:00Z"),
            tick("2026-04-13T08:00:00Z"),
        ]);

        let error = validate_generic_24_5_inference("FX_EURUSD", &observed_gap_spectrum)
            .expect_err("weekend tick support must reject generic 24/5 inference");

        match error {
            MarketDataLoadingError::InvalidValue {
                field_path,
                message,
            } => {
                assert_eq!(field_path, "session_configuration");
                assert_eq!(
                    message,
                    "automatic 24/5 session inference is ambiguous because the observed weekday support includes weekend trading. Provide an explicit session-definition override. exchange_code=FX_EURUSD"
                );
            }
            other_error => panic!("unexpected error variant: {other_error}"),
        }
    }

    #[test]
    fn generic_24_5_inference_rejects_partial_weekday_support_without_weekend_gap() {
        let observed_gap_spectrum = ObservedGapSpectrum::from_tick_records(&[
            tick("2026-04-06T08:00:00Z"),
            tick("2026-04-06T12:00:00Z"),
            tick("2026-04-07T08:00:00Z"),
        ]);

        let error = validate_generic_24_5_inference("BROKER_SPOT", &observed_gap_spectrum)
            .expect_err("partial weekday support without weekend inactivity must stay ambiguous");

        match error {
            MarketDataLoadingError::InvalidValue {
                field_path,
                message,
            } => {
                assert_eq!(field_path, "session_configuration");
                assert_eq!(
                    message,
                    "automatic 24/5 session inference is ambiguous because the derived inactivity-gap support does not isolate a weekend closure and the observed weekday support does not span the full weekday-continuous template. Provide an explicit session-definition override. exchange_code=BROKER_SPOT"
                );
            }
            other_error => panic!("unexpected error variant: {other_error}"),
        }
    }
}
