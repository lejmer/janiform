use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

use super::contracts::{
    MarketDataLoadingError, MarketDataSessionRecurringClosedWindowPayload,
    MarketDataSessionRegularWindowPayload,
};
use super::shared::SessionTemplate;

const BUNDLED_SESSION_PRESETS_SOURCE_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../presets/sources/sessions/session_presets.json"
));

static BUNDLED_SESSION_TEMPLATES_BY_EXCHANGE_CODE: OnceLock<BTreeMap<String, SessionTemplate>> =
    OnceLock::new();

#[derive(Debug, Deserialize)]
struct BundledSessionPresetSourceDocument {
    presets: Vec<BundledSessionPresetSourceEntry>,
}

#[derive(Debug, Deserialize)]
struct BundledSessionPresetSourceEntry {
    source_manifest_ids: Vec<String>,
    configuration: BundledSessionPresetConfiguration,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundledSessionPresetConfiguration {
    exchange_code: String,
    exchange_name: String,
    timezone_name: String,
    regular_session_windows: Vec<MarketDataSessionRegularWindowPayload>,
    recurring_closed_windows: Vec<MarketDataSessionRecurringClosedWindowPayload>,
}

pub(super) fn bundled_session_template(
    exchange_code: &str,
) -> Result<Option<SessionTemplate>, MarketDataLoadingError> {
    Ok(BUNDLED_SESSION_TEMPLATES_BY_EXCHANGE_CODE
        .get_or_init(load_bundled_session_templates)
        .get(exchange_code)
        .cloned())
}

fn load_bundled_session_templates() -> BTreeMap<String, SessionTemplate> {
    let source_document: BundledSessionPresetSourceDocument =
        serde_json::from_str(BUNDLED_SESSION_PRESETS_SOURCE_JSON).expect(
            "bundled session preset source JSON must remain valid because calibration session \
resolution depends on it",
        );

    source_document
        .presets
        .into_iter()
        .map(|entry| {
            let policy_id = entry
                .source_manifest_ids
                .first()
                .cloned()
                .unwrap_or_else(|| "bundled_session_preset.unknown_source_manifest.v1".to_string());

            (
                entry.configuration.exchange_code.clone(),
                SessionTemplate {
                    policy_id,
                    provenance_kind: "bundled_preset".to_string(),
                    exchange_name: entry.configuration.exchange_name,
                    timezone_name: entry.configuration.timezone_name,
                    timezone_source: "bundled_session_preset".to_string(),
                    holiday_calendar_code: "BUNDLED_SESSION_PRESET".to_string(),
                    early_close_rule_set: None,
                    regular_session_windows: entry.configuration.regular_session_windows,
                    recurring_closed_windows: entry.configuration.recurring_closed_windows,
                    continuous_weekday_template: false,
                },
            )
        })
        .collect()
}
