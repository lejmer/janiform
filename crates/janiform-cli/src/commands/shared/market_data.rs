use std::path::PathBuf;

use janiform::calibration::market_data::{
    DEFAULT_MARKET_DATA_TIMESTAMP_TIMEZONE, MarketDataInputFileReference,
    MarketDataKnownNewsColumnMapping, MarketDataNormalizationPolicy,
    MarketDataOptionSurfaceColumnMapping, MarketDataSessionConfiguration,
    MarketDataTickDataColumnMapping, infer_bundled_market_profile_from_tick_data_path,
    infer_market_data_option_quote_input_policy, infer_market_data_tick_price_input_policy,
    resolve_market_data_option_surface_column_mapping_for_source,
    resolve_market_data_tick_data_column_mapping_for_source,
};
use janiform::calibration::presets::ResolvedMarketProfile;
use janiform::calibration::staged::{
    CalibrationMarketDataColumnMapping, CalibrationMarketDataKnownNewsInput,
    CalibrationMarketDataWorkflowScope,
};

use crate::commands::options;
use crate::commands::options::TickDataOptions;
use crate::commands::shared::calibration::duplicate_record_policy;
use crate::commands::shared::calibration::option_quote_input_policy;
use crate::commands::shared::calibration::tick_price_input_policy;
use crate::commands::shared::inputs::market_data_input_file_reference;
use crate::commands::shared::inputs::optional_input_file;
use crate::commands::shared::inputs::required_input_file;
use crate::commands::shared::market_profiles::resolve_explicit_market_profile;
use crate::context::CommandContext;
use crate::error::CliError;

const DEFAULT_KNOWN_NEWS_TIMESTAMP_COLUMN: &str = "event_timestamp_utc";

#[derive(Debug)]
pub(crate) struct PreparedMarketDataCommandInputs {
    pub(crate) tick_data_path: PathBuf,
    pub(crate) option_surface_path: Option<PathBuf>,
    pub(crate) known_news_path: Option<PathBuf>,
    pub(crate) session_definition_path: Option<PathBuf>,
    pub(crate) tick_data_source: MarketDataInputFileReference,
    pub(crate) option_surface_source: Option<MarketDataInputFileReference>,
    pub(crate) known_news_inputs: Option<CalibrationMarketDataKnownNewsInput>,
    pub(crate) market_profile: Option<ResolvedMarketProfile>,
    pub(crate) session_configuration: MarketDataSessionConfiguration,
    pub(crate) column_mapping: CalibrationMarketDataColumnMapping,
    pub(crate) normalization_policy: MarketDataNormalizationPolicy,
}

pub(crate) fn prepare_market_data_command_inputs(
    context: &CommandContext,
    inputs: &TickDataOptions,
    workflow_scope: CalibrationMarketDataWorkflowScope,
) -> Result<PreparedMarketDataCommandInputs, CliError> {
    validate_option_arguments(inputs)?;
    validate_known_news_arguments(inputs)?;

    let tick_data_path = required_input_file(context, "--tick-data-path", &inputs.tick_data_path)?;
    let explicit_market_profile = resolve_explicit_market_profile(context, inputs)?;

    let market_profile = if explicit_market_profile.is_some() {
        explicit_market_profile
    } else if inputs.exchange_code.is_none()
        && inputs.session_definition_path.is_none()
        && inputs.session_timezone.is_none()
    {
        infer_bundled_market_profile_from_tick_data_path(&tick_data_path).map_err(|source| {
            CliError::InvalidCommandArgument {
                argument_name: "--tick-data-path",
                message: source.to_string(),
            }
        })?
    } else {
        None
    };

    let tick_data_source = market_data_input_file_reference(
        "--tick-data-path",
        &tick_data_path,
        inputs.tick_data_format,
        inputs.tick_provenance_label.as_deref(),
        "ticks",
    )?;

    let unresolved_tick_mapping = MarketDataTickDataColumnMapping {
        timestamp_column: inputs.tick_timestamp_column.clone(),
        price_column: inputs.tick_price_column.clone(),
        trade_sign_column: inputs.tick_trade_sign_column.clone(),
        bid_column: inputs.tick_bid_column.clone(),
        ask_column: inputs.tick_ask_column.clone(),
    };

    let tick_price_input_policy = match (workflow_scope, inputs.tick_price_input_policy) {
        (CalibrationMarketDataWorkflowScope::SeasonalityOnly, _) => None,
        (_, Some(policy)) => Some(tick_price_input_policy(policy)),
        (_, None) => {
            infer_market_data_tick_price_input_policy(&tick_data_source, &unresolved_tick_mapping)
                .map_err(|source| CliError::InvalidCommandArgument {
                argument_name: "--tick-data-path",
                message: source.to_string(),
            })?
        }
    };

    let mut normalization_policy = MarketDataNormalizationPolicy {
        timestamp_timezone: inputs
            .timestamp_timezone
            .clone()
            .or_else(|| {
                market_profile
                    .as_ref()
                    .and_then(|profile| profile.timestamp_timezone_name.clone())
            })
            .unwrap_or_else(|| DEFAULT_MARKET_DATA_TIMESTAMP_TIMEZONE.to_string()),
        sort_records_by_timestamp: !inputs.keep_tick_source_order,
        duplicate_record_policy: inputs.duplicate_record_policy.map(duplicate_record_policy),
        tick_price_input_policy,
        option_quote_input_policy: inputs
            .option_quote_input_policy
            .map(option_quote_input_policy),
    };

    let tick_mapping = resolve_market_data_tick_data_column_mapping_for_source(
        &tick_data_source,
        &unresolved_tick_mapping,
        &normalization_policy,
    )
    .map_err(|source| CliError::InvalidCommandArgument {
        argument_name: "--tick-data-path",
        message: source.to_string(),
    })?;

    let option_surface_path = optional_input_file(
        context,
        "--option-surface-path",
        inputs.option_surface_path.as_ref(),
    )?;

    let (option_surface_source, option_surface_mapping) =
        prepare_option_surface_inputs(inputs, &option_surface_path, &mut normalization_policy)?;

    let known_news_path = optional_input_file(
        context,
        "--known-news-path",
        inputs.known_news_path.as_ref(),
    )?;

    let known_news_inputs = prepare_known_news_inputs(inputs, &known_news_path)?;

    let known_news_mapping = known_news_inputs
        .as_ref()
        .map(|_| MarketDataKnownNewsColumnMapping {
            event_timestamp_column: inputs
                .known_news_timestamp_column
                .clone()
                .unwrap_or_else(|| DEFAULT_KNOWN_NEWS_TIMESTAMP_COLUMN.to_string()),
            event_identifier_column: inputs.known_news_identifier_column.clone(),
        });

    let session_definition_path = optional_input_file(
        context,
        "--session-definition-path",
        inputs.session_definition_path.as_ref(),
    )?;

    let session_definition_source = match &session_definition_path {
        Some(path) => Some(market_data_input_file_reference(
            "--session-definition-path",
            path,
            Some(options::InputFileFormat::Json),
            Some("session-definition"),
            "session",
        )?),
        None => None,
    };

    let exchange_code = match inputs.exchange_code.clone() {
        Some(exchange_code) => exchange_code,
        None => market_profile
            .as_ref()
            .map(|profile| profile.exchange_code.clone())
            .ok_or(CliError::MissingRequiredArgument {
                argument_name: "--exchange-code",
            })?,
    };

    Ok(PreparedMarketDataCommandInputs {
        tick_data_path,
        option_surface_path,
        known_news_path,
        session_definition_path,
        tick_data_source,
        option_surface_source,
        known_news_inputs,
        market_profile,
        session_configuration: MarketDataSessionConfiguration {
            exchange_code,
            session_definition_source,
            session_timezone: inputs.session_timezone.clone(),
        },
        column_mapping: CalibrationMarketDataColumnMapping {
            tick_data: tick_mapping,
            option_surface: option_surface_mapping,
            known_news: known_news_mapping,
        },
        normalization_policy,
    })
}

fn prepare_option_surface_inputs(
    inputs: &TickDataOptions,
    option_surface_path: &Option<PathBuf>,
    normalization_policy: &mut MarketDataNormalizationPolicy,
) -> Result<
    (
        Option<MarketDataInputFileReference>,
        Option<MarketDataOptionSurfaceColumnMapping>,
    ),
    CliError,
> {
    let Some(path) = option_surface_path else {
        return Ok((None, None));
    };

    let source = market_data_input_file_reference(
        "--option-surface-path",
        path,
        inputs.option_surface_format,
        inputs.option_surface_provenance_label.as_deref(),
        "options",
    )?;

    let unresolved_mapping = MarketDataOptionSurfaceColumnMapping {
        quote_timestamp_column: inputs.option_quote_timestamp_column.clone(),
        expiry_timestamp_column: inputs.option_expiry_timestamp_column.clone(),
        strike_column: inputs.option_strike_column.clone(),
        option_type_column: inputs.option_type_column.clone(),
        mid_price_column: inputs.option_mid_price_column.clone(),
        bid_column: inputs.option_bid_column.clone(),
        ask_column: inputs.option_ask_column.clone(),
        implied_volatility_column: inputs.option_implied_volatility_column.clone(),
    };

    if normalization_policy.option_quote_input_policy.is_none() {
        normalization_policy.option_quote_input_policy = Some(
            infer_market_data_option_quote_input_policy(
                &source,
                &unresolved_mapping,
                normalization_policy,
            )
            .map_err(|source| CliError::InvalidCommandArgument {
                argument_name: "--option-surface-path",
                message: source.to_string(),
            })?,
        );
    }

    let mapping = resolve_market_data_option_surface_column_mapping_for_source(
        &source,
        &unresolved_mapping,
        normalization_policy,
    )
    .map_err(|source| CliError::InvalidCommandArgument {
        argument_name: "--option-surface-path",
        message: source.to_string(),
    })?;

    Ok((Some(source), Some(mapping)))
}

fn prepare_known_news_inputs(
    inputs: &TickDataOptions,
    known_news_path: &Option<PathBuf>,
) -> Result<Option<CalibrationMarketDataKnownNewsInput>, CliError> {
    known_news_path
        .as_ref()
        .map(|path| {
            Ok(CalibrationMarketDataKnownNewsInput {
                calendar_source: market_data_input_file_reference(
                    "--known-news-path",
                    path,
                    inputs.known_news_format,
                    inputs.known_news_provenance_label.as_deref(),
                    "known-news",
                )?,
                calendar_timezone: inputs.known_news_timezone.clone(),
            })
        })
        .transpose()
}

fn validate_option_arguments(inputs: &TickDataOptions) -> Result<(), CliError> {
    if inputs.option_surface_path.is_some() {
        return Ok(());
    }

    if inputs.option_surface_format.is_some()
        || inputs.option_surface_provenance_label.is_some()
        || inputs.option_quote_timestamp_column.is_some()
        || inputs.option_expiry_timestamp_column.is_some()
        || inputs.option_strike_column.is_some()
        || inputs.option_type_column.is_some()
        || inputs.option_mid_price_column.is_some()
        || inputs.option_bid_column.is_some()
        || inputs.option_ask_column.is_some()
        || inputs.option_implied_volatility_column.is_some()
        || inputs.option_quote_input_policy.is_some()
        || inputs.option_surface_valuation_time_utc.is_some()
    {
        return Err(CliError::MissingRequiredArgument {
            argument_name: "--option-surface-path",
        });
    }

    Ok(())
}

fn validate_known_news_arguments(inputs: &TickDataOptions) -> Result<(), CliError> {
    if inputs.known_news_path.is_some() {
        return Ok(());
    }

    if inputs.known_news_format.is_some()
        || inputs.known_news_provenance_label.is_some()
        || inputs.known_news_timestamp_column.is_some()
        || inputs.known_news_identifier_column.is_some()
        || inputs.known_news_timezone.is_some()
    {
        return Err(CliError::MissingRequiredArgument {
            argument_name: "--known-news-path",
        });
    }

    Ok(())
}
