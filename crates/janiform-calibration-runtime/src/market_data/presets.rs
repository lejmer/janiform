use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Display;
use std::path::Path;
use std::sync::OnceLock;

use janiform_foundations::identifiers::sanitize_ascii_identifier_component;
use janiform_foundations::numeric::validate_positive_number;
use serde::Deserialize;
use serde::Serialize;

// Bundled market-profile roots include two-character symbols such as ES and US.
const MIN_MARKET_PROFILE_INSTRUMENT_TOKEN_LENGTH: usize = 2;
const ISO_4217_CURRENCY_CODE_LENGTH: usize = 3;
const BUNDLED_MARKET_PROFILE_SOURCE_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../presets/sources/market_profiles/market_profile_presets.json"
));
const BUNDLED_SESSION_SOURCE_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../presets/sources/sessions/session_presets.json"
));

// The bundled preset source documents are immutable after compile time. Lazy
// parsing keeps startup cheap for callers that do not need profile inference.
type BundledMarketProfileMap = BTreeMap<String, BundledMarketProfile>;
type BundledSessionMap = BTreeMap<String, BundledSession>;
type PresetInferenceResult<T> = Result<T, MarketDataPresetInferenceError>;

static BUNDLED_MARKET_PROFILES: OnceLock<Result<BundledMarketProfileMap, String>> = OnceLock::new();
static BUNDLED_SESSIONS_BY_PRESET_NAME: OnceLock<Result<BundledSessionMap, String>> =
    OnceLock::new();

/// Column aliases supplied by a bundled market profile.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MarketDataColumnAliasHint {
    /// Canonical column role, such as `timestamp` or `price`.
    pub canonical_name: String,
    /// Profile-specific aliases that should be considered before generic aliases.
    pub aliases: Vec<String>,
}

/// Bundled market profile selected for a tick-data source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResolvedMarketProfile {
    /// Bundled market profile preset name.
    pub profile_name: String,
    /// Bundled session preset referenced by the profile.
    pub session_preset_name: String,
    /// Exchange code resolved from the profile's session preset.
    pub exchange_code: String,
    /// Timestamp timezone default supplied by the profile.
    pub timestamp_timezone_name: Option<String>,
    /// Optional profile tick-size fallback.
    pub tick_size: Option<f64>,
    /// Bundled known-news preset recommended by the profile.
    pub known_news_preset_name: Option<String>,
    /// Policy for applying the known-news preset recommendation.
    pub known_news_policy: Option<String>,
    /// Profile-specific source-column aliases.
    pub column_alias_hints: Vec<MarketDataColumnAliasHint>,
}

/// Errors raised while resolving bundled market-data presets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MarketDataPresetInferenceError {
    /// A compile-time bundled preset JSON document is malformed.
    InvalidBundledPresetSource { message: String },
    /// A bundled market profile references a bundled session that is missing.
    MissingSessionPreset {
        market_profile_name: String,
        session_preset_name: String,
    },
}

impl Display for MarketDataPresetInferenceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBundledPresetSource { message } => formatter.write_str(message),
            Self::MissingSessionPreset {
                market_profile_name,
                session_preset_name,
            } => write!(
                formatter,
                "bundled market profile `{market_profile_name}` references missing session preset `{session_preset_name}`"
            ),
        }
    }
}

impl Error for MarketDataPresetInferenceError {}

#[derive(Clone, Debug, Deserialize)]
struct BundledMarketProfile {
    preset_name: String,
    configuration: BundledMarketProfileConfiguration,
}

#[derive(Clone, Debug, Deserialize)]
struct BundledMarketProfileConfiguration {
    session_preset_name: String,
    known_news_preset_name: Option<String>,
    known_news_policy: Option<String>,
    timestamp_timezone_name: Option<String>,
    #[serde(default)]
    tick_size: Option<f64>,
    #[serde(default)]
    column_alias_hints: Vec<MarketDataColumnAliasHint>,
}

#[derive(Debug, Deserialize)]
struct BundledMarketProfileSourceDocument {
    presets: Vec<BundledMarketProfile>,
}

#[derive(Clone, Debug, Deserialize)]
struct BundledSession {
    preset_name: String,
    configuration: BundledSessionConfiguration,
}

#[derive(Clone, Debug, Deserialize)]
struct BundledSessionConfiguration {
    exchange_code: String,
}

#[derive(Debug, Deserialize)]
struct BundledSessionSourceDocument {
    presets: Vec<BundledSession>,
}

/// Infer a bundled market profile from a tick-data path.
///
/// The inference uses the first bundled instrument token in the file stem, for
/// example `EURUSD_665k_ticks.parquet`, `EUR_USD_665k_ticks.parquet`, and
/// `Synthetic_EURUSD_Data_1M_ticks.parquet` map to `eurusd_spot` when that
/// bundled profile is available. Two-character roots are included because
/// listed futures contracts such as ES use two-character instrument tokens.
pub fn infer_bundled_market_profile_from_tick_data_path(
    path: &Path,
) -> Result<Option<ResolvedMarketProfile>, MarketDataPresetInferenceError> {
    let instrument_tokens = instrument_tokens_from_path(path);
    if instrument_tokens.is_empty() {
        return Ok(None);
    }

    let profiles = bundled_market_profiles()?;

    for instrument_token in instrument_tokens {
        if let Some(profile) = profiles.get(instrument_token.as_str()) {
            return resolve_bundled_market_profile(profile);
        }
    }

    Ok(None)
}

fn instrument_tokens_from_path(path: &Path) -> Vec<String> {
    let Some(file_stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return Vec::new();
    };
    let Some(normalized_stem) = sanitize_ascii_identifier_component(file_stem) else {
        return Vec::new();
    };

    let normalized_tokens = normalized_stem
        .split('_')
        .filter(|token| token.len() >= MIN_MARKET_PROFILE_INSTRUMENT_TOKEN_LENGTH)
        .collect::<Vec<_>>();

    let mut instrument_tokens = Vec::new();

    for (token_position, token) in normalized_tokens.iter().enumerate() {
        push_unique_instrument_token(&mut instrument_tokens, (*token).to_string());

        if let Some(next_token) = normalized_tokens.get(token_position + 1)
            && is_iso_currency_code_token(token)
            && is_iso_currency_code_token(next_token)
        {
            push_unique_instrument_token(&mut instrument_tokens, format!("{token}{next_token}"));
        }
    }

    instrument_tokens
}

fn push_unique_instrument_token(instrument_tokens: &mut Vec<String>, candidate: String) {
    if instrument_tokens
        .iter()
        .any(|instrument_token| instrument_token == &candidate)
    {
        return;
    }

    instrument_tokens.push(candidate);
}

fn is_iso_currency_code_token(token: &str) -> bool {
    token.len() == ISO_4217_CURRENCY_CODE_LENGTH
        && token
            .chars()
            .all(|character| character.is_ascii_alphabetic())
}

fn resolve_bundled_market_profile(
    profile: &BundledMarketProfile,
) -> Result<Option<ResolvedMarketProfile>, MarketDataPresetInferenceError> {
    let sessions_by_preset_name = bundled_sessions_by_preset_name()?;

    let Some(session) = sessions_by_preset_name.get(&profile.configuration.session_preset_name)
    else {
        // The error leaves the cache boundary, so it owns the preset names.
        return Err(MarketDataPresetInferenceError::MissingSessionPreset {
            market_profile_name: profile.preset_name.clone(),
            session_preset_name: profile.configuration.session_preset_name.clone(),
        });
    };

    let tick_size = validated_bundled_market_profile_tick_size(
        profile.preset_name.as_str(),
        profile.configuration.tick_size,
    )?;

    // Profile and session data live in process-wide bundled caches, so the
    // public inference result owns copies instead of borrowing from the cache.
    Ok(Some(ResolvedMarketProfile {
        profile_name: profile.preset_name.clone(),
        session_preset_name: profile.configuration.session_preset_name.clone(),
        exchange_code: session.configuration.exchange_code.clone(),
        timestamp_timezone_name: profile.configuration.timestamp_timezone_name.clone(),
        tick_size,
        known_news_preset_name: profile.configuration.known_news_preset_name.clone(),
        known_news_policy: profile.configuration.known_news_policy.clone(),
        column_alias_hints: profile.configuration.column_alias_hints.clone(),
    }))
}

fn validated_bundled_market_profile_tick_size(
    preset_name: &str,
    tick_size: Option<f64>,
) -> Result<Option<f64>, MarketDataPresetInferenceError> {
    let Some(tick_size) = tick_size else {
        return Ok(None);
    };

    if let Err(source) = validate_positive_number(tick_size, "tick_size") {
        return Err(MarketDataPresetInferenceError::InvalidBundledPresetSource {
            message: format!(
                "bundled market profile `{preset_name}` has invalid tick_size: {source}"
            ),
        });
    }

    Ok(Some(tick_size))
}

fn bundled_market_profiles() -> PresetInferenceResult<&'static BundledMarketProfileMap> {
    BUNDLED_MARKET_PROFILES
        .get_or_init(load_bundled_market_profiles)
        .as_ref()
        .map_err(
            |message| MarketDataPresetInferenceError::InvalidBundledPresetSource {
                message: message.clone(),
            },
        )
}

fn load_bundled_market_profiles() -> Result<BundledMarketProfileMap, String> {
    let source_document: BundledMarketProfileSourceDocument =
        serde_json::from_str(BUNDLED_MARKET_PROFILE_SOURCE_JSON)
            .map_err(|source| format!("bundled market-profile source JSON is invalid: {source}"))?;

    Ok(source_document
        .presets
        .into_iter()
        .filter_map(|profile| {
            market_profile_instrument_token(&profile.preset_name)
                .map(|instrument_token| (instrument_token, profile))
        })
        .collect())
}

fn market_profile_instrument_token(preset_name: &str) -> Option<String> {
    let instrument_token = preset_name.split('_').next()?;

    if instrument_token.len() < MIN_MARKET_PROFILE_INSTRUMENT_TOKEN_LENGTH {
        return None;
    }

    Some(instrument_token.to_string())
}

fn bundled_sessions_by_preset_name() -> PresetInferenceResult<&'static BundledSessionMap> {
    BUNDLED_SESSIONS_BY_PRESET_NAME
        .get_or_init(load_bundled_sessions_by_preset_name)
        .as_ref()
        .map_err(
            |message| MarketDataPresetInferenceError::InvalidBundledPresetSource {
                message: message.clone(),
            },
        )
}

fn load_bundled_sessions_by_preset_name() -> Result<BundledSessionMap, String> {
    let source_document: BundledSessionSourceDocument =
        serde_json::from_str(BUNDLED_SESSION_SOURCE_JSON)
            .map_err(|source| format!("bundled session source JSON is invalid: {source}"))?;

    Ok(source_document
        .presets
        .into_iter()
        .map(|session| (session.preset_name.clone(), session))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::infer_bundled_market_profile_from_tick_data_path;

    #[test]
    fn infers_eurusd_spot_profile_from_tick_path() {
        let inferred_profile = infer_bundled_market_profile_from_tick_data_path(Path::new(
            "Synthetic_EURUSD_Data_1M_ticks.parquet",
        ))
        .expect("bundled profile inference should succeed")
        .expect("EURUSD token should select a bundled profile");

        assert_eq!(inferred_profile.profile_name, "eurusd_spot");
        assert_eq!(inferred_profile.session_preset_name, "fx_spot_24_5");
        assert_eq!(inferred_profile.exchange_code, "FXSPOT");
        assert_eq!(
            inferred_profile.timestamp_timezone_name.as_deref(),
            Some("UTC")
        );
        assert_eq!(inferred_profile.tick_size, Some(0.00001));
        assert_eq!(
            inferred_profile.known_news_preset_name.as_deref(),
            Some("eu_us_macro_fx_core")
        );
    }

    #[test]
    fn infers_major_fx_spot_profiles_from_contiguous_pair_tokens() {
        let expected_profiles = [
            ("AUDCAD", "audcad_spot"),
            ("AUDCHF", "audchf_spot"),
            ("AUDJPY", "audjpy_spot"),
            ("AUDNZD", "audnzd_spot"),
            ("AUDUSD", "audusd_spot"),
            ("CADCHF", "cadchf_spot"),
            ("CADJPY", "cadjpy_spot"),
            ("CHFJPY", "chfjpy_spot"),
            ("EURAUD", "euraud_spot"),
            ("EURCAD", "eurcad_spot"),
            ("EURCHF", "eurchf_spot"),
            ("EURGBP", "eurgbp_spot"),
            ("EURJPY", "eurjpy_spot"),
            ("EURNZD", "eurnzd_spot"),
            ("EURUSD", "eurusd_spot"),
            ("GBPAUD", "gbpaud_spot"),
            ("GBPCAD", "gbpcad_spot"),
            ("GBPCHF", "gbpchf_spot"),
            ("GBPJPY", "gbpjpy_spot"),
            ("GBPNZD", "gbpnzd_spot"),
            ("GBPUSD", "gbpusd_spot"),
            ("NZDCAD", "nzdcad_spot"),
            ("NZDCHF", "nzdchf_spot"),
            ("NZDJPY", "nzdjpy_spot"),
            ("NZDUSD", "nzdusd_spot"),
            ("USDCAD", "usdcad_spot"),
            ("USDCHF", "usdchf_spot"),
            ("USDJPY", "usdjpy_spot"),
        ];

        for (instrument_token, expected_profile_name) in expected_profiles {
            let tick_data_path = format!("{instrument_token}_ticks.parquet");

            let inferred_profile =
                infer_bundled_market_profile_from_tick_data_path(Path::new(&tick_data_path))
                    .expect("bundled profile inference should succeed")
                    .expect("major FX token should select a bundled profile");

            assert_eq!(inferred_profile.profile_name, expected_profile_name);
            assert_eq!(inferred_profile.session_preset_name, "fx_spot_24_5");
            assert_eq!(inferred_profile.exchange_code, "FXSPOT");

            let expected_tick_size = if expected_profile_name
                .split('_')
                .next()
                .is_some_and(|instrument_token| instrument_token.ends_with("jpy"))
            {
                0.001
            } else {
                0.00001
            };

            assert_eq!(inferred_profile.tick_size, Some(expected_tick_size));
        }
    }

    #[test]
    fn infers_major_fx_spot_profile_from_separated_currency_tokens() {
        let inferred_profile = infer_bundled_market_profile_from_tick_data_path(Path::new(
            "broker_USD-JPY_1M_ticks.parquet",
        ))
        .expect("bundled profile inference should succeed")
        .expect("separated currency tokens should select a bundled profile");

        assert_eq!(inferred_profile.profile_name, "usdjpy_spot");
        assert_eq!(inferred_profile.session_preset_name, "fx_spot_24_5");
        assert_eq!(inferred_profile.exchange_code, "FXSPOT");
        assert_eq!(
            inferred_profile.known_news_preset_name.as_deref(),
            Some("none")
        );
        assert_eq!(inferred_profile.tick_size, Some(0.001));
    }

    #[test]
    fn infers_es_futures_globex_profile_from_two_character_instrument_token() {
        let inferred_profile = infer_bundled_market_profile_from_tick_data_path(Path::new(
            "ES_continuous_ticks.parquet",
        ))
        .expect("bundled profile inference should succeed")
        .expect("ES token should select a bundled profile");

        assert_eq!(inferred_profile.profile_name, "es_futures_globex");
        assert_eq!(inferred_profile.session_preset_name, "XCME");
        assert_eq!(inferred_profile.exchange_code, "XCME");
        assert_eq!(
            inferred_profile.timestamp_timezone_name.as_deref(),
            Some("UTC")
        );
        assert_eq!(inferred_profile.tick_size, Some(0.25));
        assert_eq!(
            inferred_profile.known_news_preset_name.as_deref(),
            Some("none")
        );
    }
}
