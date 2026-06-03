use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::Path;

use chrono::Utc;
use janiform_foundations::identifiers::sanitize_ascii_identifier_component;
use janiform_foundations::strings::is_non_empty_string;
use serde::{Deserialize, Serialize};

use super::loading::column_resolution::{
    TickColumnCandidates, resolve_tick_data_column_mapping_with_alias_hints, tick_column_candidates,
};
use super::loading::{
    MarketDataDuplicateRecordPolicy, MarketDataInputFileFormat, MarketDataInputFileReference,
    MarketDataLoadingError, MarketDataNormalizationPolicy, MarketDataOptionQuoteInputPolicy,
    MarketDataSourceProbePayload, MarketDataTickDataColumnMapping, MarketDataTickPriceInputPolicy,
    probe_market_data_source,
};
use super::preparation::CalibrationMarketDataWorkflowScope;
use super::presets::MarketDataColumnAliasHint;

/// Default timestamp timezone used when no explicit or profile default exists.
pub const DEFAULT_MARKET_DATA_TIMESTAMP_TIMEZONE: &str = "UTC";

/// Fit-facing market-data input inference request.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataFitInputInferenceRequest {
    /// Tick-data source path.
    pub tick_data_path: String,
    /// Explicit source format override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick_data_format: Option<MarketDataInputFileFormat>,
    /// Provenance label assigned to the tick-data source.
    pub tick_data_provenance_label: String,
    /// Calibration workflow scope that determines whether tick prices are required.
    pub workflow_scope: CalibrationMarketDataWorkflowScope,
    /// Explicit run identifier override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Explicit column overrides.
    pub column_overrides: MarketDataTickDataColumnMapping,
    /// Explicit normalization overrides.
    pub normalization_overrides: MarketDataFitNormalizationOverrides,
    /// Market profile defaults used for profile-specific inference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_profile_defaults: Option<MarketDataFitMarketProfileDefaults>,
}

/// Optional normalization values supplied by high-level callers.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataFitNormalizationOverrides {
    /// Explicit timestamp timezone override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_timezone: Option<String>,
    /// Explicit source ordering override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_records_by_timestamp: Option<bool>,
    /// Explicit duplicate record policy override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_record_policy: Option<MarketDataDuplicateRecordPolicy>,
    /// Explicit tick price input policy override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick_price_input_policy: Option<MarketDataTickPriceInputPolicy>,
    /// Explicit option quote input policy override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_quote_input_policy: Option<MarketDataOptionQuoteInputPolicy>,
}

/// Market profile defaults that affect fit input inference.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataFitMarketProfileDefaults {
    /// Timestamp timezone default supplied by the profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_timezone_name: Option<String>,
    /// Optional profile tick-size fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick_size: Option<f64>,
    /// Profile-specific source-column aliases.
    #[serde(default)]
    pub column_alias_hints: Vec<MarketDataColumnAliasHint>,
    /// Default calibration hint identifiers associated with the profile.
    #[serde(default)]
    pub default_calibration_hint_ids: Vec<String>,
}

/// Fit-facing market-data input inference output.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataFitInputInferenceOutput {
    /// Resolved run identifier.
    pub run_id: String,
    /// Resolved tick-data source reference.
    pub tick_data_source: MarketDataInputFileReference,
    /// Resolved tick-data column mapping.
    pub tick_data_column_mapping: MarketDataTickDataColumnMapping,
    /// Resolved normalization policy.
    pub normalization_policy: MarketDataNormalizationPolicy,
    /// Probe details for the tick-data source.
    pub source_probe: MarketDataSourceProbePayload,
    /// Candidate column names grouped by canonical role.
    pub candidate_columns: MarketDataTickColumnCandidatesPayload,
    /// Source of the selected timestamp timezone.
    pub timestamp_timezone_source: String,
    /// Default calibration hint identifiers from the selected profile.
    pub default_calibration_hint_ids: Vec<String>,
}

/// Candidate tick-data columns grouped by canonical role.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDataTickColumnCandidatesPayload {
    /// Timestamp column candidates.
    pub timestamp: Vec<String>,
    /// Trade price column candidates.
    pub price: Vec<String>,
    /// Trade sign column candidates.
    pub trade_sign: Vec<String>,
    /// Bid column candidates.
    pub bid: Vec<String>,
    /// Ask column candidates.
    pub ask: Vec<String>,
}

/// Errors raised while inferring fit input contracts.
#[derive(Debug)]
pub enum MarketDataFitInputInferenceError {
    /// A request field is invalid.
    InvalidValue { field_path: String, message: String },
    /// A required source column could not be inferred.
    MissingColumn { field_path: String, message: String },
    /// Source probing failed.
    Loading(MarketDataLoadingError),
}

impl Display for MarketDataFitInputInferenceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidValue {
                field_path,
                message,
            } => write!(formatter, "{field_path}: {message}"),
            Self::MissingColumn {
                field_path,
                message,
            } => write!(formatter, "{field_path}: {message}"),
            Self::Loading(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for MarketDataFitInputInferenceError {}

impl From<MarketDataLoadingError> for MarketDataFitInputInferenceError {
    fn from(error: MarketDataLoadingError) -> Self {
        Self::Loading(error)
    }
}

/// Infer fit-facing market-data input contracts.
pub fn infer_market_data_fit_inputs(
    request: MarketDataFitInputInferenceRequest,
) -> Result<MarketDataFitInputInferenceOutput, MarketDataFitInputInferenceError> {
    validate_non_empty(
        "tick_data_path",
        &request.tick_data_path,
        "must be a non-empty string",
    )?;

    validate_non_empty(
        "tick_data_provenance_label",
        &request.tick_data_provenance_label,
        "must be a non-empty string",
    )?;

    validate_optional_non_empty(
        "column_overrides.timestamp_column",
        request.column_overrides.timestamp_column.as_deref(),
    )?;

    validate_optional_non_empty(
        "column_overrides.price_column",
        request.column_overrides.price_column.as_deref(),
    )?;

    validate_optional_non_empty(
        "column_overrides.trade_sign_column",
        request.column_overrides.trade_sign_column.as_deref(),
    )?;

    validate_optional_non_empty(
        "column_overrides.bid_column",
        request.column_overrides.bid_column.as_deref(),
    )?;

    validate_optional_non_empty(
        "column_overrides.ask_column",
        request.column_overrides.ask_column.as_deref(),
    )?;

    validate_optional_non_empty(
        "normalization_overrides.timestamp_timezone",
        request
            .normalization_overrides
            .timestamp_timezone
            .as_deref(),
    )?;

    let tick_data_format = match request.tick_data_format {
        Some(file_format) => file_format,
        None => infer_market_data_input_file_format_from_path(Path::new(&request.tick_data_path))?,
    };

    // The source reference owns path metadata while the request remains borrowed
    // for the rest of inference.
    let tick_data_source = MarketDataInputFileReference {
        path: request.tick_data_path.clone(),
        file_format: tick_data_format,
        provenance_label: request.tick_data_provenance_label.clone(),
    };

    let source_probe = probe_market_data_source(&tick_data_source)?;

    let alias_hints = request
        .market_profile_defaults
        .as_ref()
        .map(|profile_defaults| profile_defaults.column_alias_hints.as_slice())
        .unwrap_or(&[]);

    let candidates = tick_column_candidates(source_probe.available_columns.as_slice(), alias_hints);

    let tick_price_input_policy = resolve_fit_tick_price_input_policy(&request, &candidates)?;

    let (timestamp_timezone, timestamp_timezone_source) = resolve_timestamp_timezone(&request);

    let normalization_policy = MarketDataNormalizationPolicy {
        timestamp_timezone,
        sort_records_by_timestamp: request
            .normalization_overrides
            .sort_records_by_timestamp
            .unwrap_or(true),
        duplicate_record_policy: request.normalization_overrides.duplicate_record_policy,
        tick_price_input_policy,
        option_quote_input_policy: request.normalization_overrides.option_quote_input_policy,
    };

    let tick_data_column_mapping = resolve_tick_data_column_mapping_with_alias_hints(
        source_probe.available_columns.as_slice(),
        &request.column_overrides,
        &normalization_policy,
        &tick_data_source.path,
        alias_hints,
    )
    .map_err(MarketDataFitInputInferenceError::from)?;

    Ok(MarketDataFitInputInferenceOutput {
        run_id: infer_market_data_run_id_from_path(
            request.run_id.as_deref(),
            Path::new(&request.tick_data_path),
        )?,
        tick_data_source,
        tick_data_column_mapping,
        normalization_policy,
        source_probe,
        candidate_columns: MarketDataTickColumnCandidatesPayload::from(candidates),
        timestamp_timezone_source,
        default_calibration_hint_ids: request
            .market_profile_defaults
            .map(|profile_defaults| profile_defaults.default_calibration_hint_ids)
            .unwrap_or_default(),
    })
}

/// Infer a market-data input file format from a path extension.
pub fn infer_market_data_input_file_format_from_path(
    path: &Path,
) -> Result<MarketDataInputFileFormat, MarketDataFitInputInferenceError> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("csv") => Ok(MarketDataInputFileFormat::Csv),
        Some("parquet") | Some("pq") => Ok(MarketDataInputFileFormat::Parquet),
        Some("json") => Ok(MarketDataInputFileFormat::Json),
        _ => Err(MarketDataFitInputInferenceError::InvalidValue {
            field_path: "tick_data_format".to_string(),
            message: format!(
                "could not infer tick-data format from `{}`. Supported extensions are .csv, .parquet, .pq, and .json.",
                path.file_name()
                    .and_then(|file_name| file_name.to_str())
                    .unwrap_or_default()
            ),
        }),
    }
}

/// Resolve a run identifier from an explicit override or tick-data path.
pub fn infer_market_data_run_id_from_path(
    run_id: Option<&str>,
    tick_data_path: &Path,
) -> Result<String, MarketDataFitInputInferenceError> {
    if let Some(run_id) = run_id {
        validate_non_empty("run_id", run_id, "must be a non-empty string")?;

        return Ok(run_id.to_string());
    }

    let source_label = tick_data_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(sanitize_ascii_identifier_component)
        .unwrap_or_else(|| "fit".to_string());

    let timestamp_text = Utc::now().format("%Y%m%dT%H%M%SZ");

    Ok(format!("{source_label}_{timestamp_text}"))
}

fn resolve_fit_tick_price_input_policy(
    request: &MarketDataFitInputInferenceRequest,
    candidates: &TickColumnCandidates,
) -> Result<Option<MarketDataTickPriceInputPolicy>, MarketDataFitInputInferenceError> {
    if request.workflow_scope == CalibrationMarketDataWorkflowScope::SeasonalityOnly {
        return Ok(None);
    }

    if let Some(policy) = request.normalization_overrides.tick_price_input_policy {
        return Ok(Some(policy));
    }

    if request.column_overrides.price_column.is_some() || !candidates.price.is_empty() {
        return Ok(Some(MarketDataTickPriceInputPolicy::ExplicitPriceColumn));
    }

    let has_bid_source =
        request.column_overrides.bid_column.is_some() || !candidates.bid.is_empty();
    let has_ask_source =
        request.column_overrides.ask_column.is_some() || !candidates.ask.is_empty();
    if has_bid_source && has_ask_source {
        return Ok(Some(MarketDataTickPriceInputPolicy::BidAskMidpoint));
    }

    // The diagnostic list combines candidate groups without mutating the
    // candidate payload returned to callers.
    let mut price_candidates = candidates.price.clone();
    for candidate in candidates.bid.iter().chain(candidates.ask.iter()) {
        if !price_candidates.contains(candidate) {
            price_candidates.push(candidate.clone());
        }
    }

    Err(MarketDataFitInputInferenceError::MissingColumn {
        field_path: "normalization_policy.tick_price_input_policy".to_string(),
        message: format!(
            "could not infer a tick-price input policy. Found price candidates: {}. Please set a price-column override or tick-price-input-policy explicitly.",
            format_candidate_list(price_candidates.as_slice())
        ),
    })
}

fn resolve_timestamp_timezone(request: &MarketDataFitInputInferenceRequest) -> (String, String) {
    if let Some(timezone) = &request.normalization_overrides.timestamp_timezone {
        return (timezone.clone(), "explicit_override".to_string());
    }

    if let Some(timezone) = request
        .market_profile_defaults
        .as_ref()
        .and_then(|profile_defaults| profile_defaults.timestamp_timezone_name.as_ref())
    {
        return (timezone.clone(), "market_profile".to_string());
    }

    (
        DEFAULT_MARKET_DATA_TIMESTAMP_TIMEZONE.to_string(),
        "default_utc".to_string(),
    )
}

fn validate_non_empty(
    field_path: &str,
    value: &str,
    message: &str,
) -> Result<(), MarketDataFitInputInferenceError> {
    if !is_non_empty_string(value) {
        return Err(MarketDataFitInputInferenceError::InvalidValue {
            field_path: field_path.to_string(),
            message: message.to_string(),
        });
    }

    Ok(())
}

fn validate_optional_non_empty(
    field_path: &str,
    value: Option<&str>,
) -> Result<(), MarketDataFitInputInferenceError> {
    if let Some(value) = value {
        validate_non_empty(field_path, value, "must be non-empty when provided")?;
    }

    Ok(())
}

fn format_candidate_list(candidates: &[String]) -> String {
    if candidates.is_empty() {
        return "none".to_string();
    }

    candidates.join(", ")
}

impl From<TickColumnCandidates> for MarketDataTickColumnCandidatesPayload {
    fn from(candidates: TickColumnCandidates) -> Self {
        Self {
            timestamp: candidates.timestamp,
            price: candidates.price,
            trade_sign: candidates.trade_sign,
            bid: candidates.bid,
            ask: candidates.ask,
        }
    }
}
