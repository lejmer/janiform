use std::path::PathBuf;

use chrono::{DateTime, Utc};
use janiform::pricing::{
    PricingIntervalSchedulePolicy, VanillaOptionPayoffType,
    VanillaOptionPricingOptionalOutputSelection,
};
use janiform_foundations::time::parse_utc_datetime;
use janiform_workflows::pricing::{
    FittedModelSingleOptionPricingOutput, FittedModelSingleOptionPricingRequest,
    FittedModelSingleOptionPricingWorkflowRequest, ObservedOptionTypeFilter,
    ObservedOptionsPricingOutput, ObservedOptionsPricingRequest, ObservedOptionsPricingScope,
    PricingWorkflowObserver, run_fitted_model_single_option_pricing_workflow,
    run_observed_options_pricing_with_observer,
};
use serde_json::Value;

use super::{
    FittedModelPricingCommand, ObservedOptionPricingCommand, PriceCommand, PricePayoffType,
    VanillaOptionPricingOptions,
};
use crate::commands::options::TickDataOptions;
use crate::commands::shared::artifacts::load_json_artifact;
use crate::commands::shared::market_data::prepare_market_data_command_inputs;
use crate::commands::shared::output::write_json_payload;
use crate::commands::shared::output_paths::default_run_output_directory;
use crate::commands::shared::terminal::{
    terminal_output_for_context, terminal_progress_adapter_for_context,
};
use crate::context::{CommandContext, OutputMode};
use crate::error::CliError;

pub(crate) fn run_price_command(
    context: &CommandContext,
    command: PriceCommand,
) -> Result<(), CliError> {
    run_fitted_model_pricing(
        context,
        command.model,
        command.option,
        command.output_path.output_path,
    )
}

pub(crate) fn run_price_fitted_model_command(
    context: &CommandContext,
    command: FittedModelPricingCommand,
) -> Result<(), CliError> {
    run_fitted_model_pricing(
        context,
        command.model,
        command.option,
        command.output_path.output_path,
    )
}

pub(crate) fn run_price_observed_options_command(
    context: &CommandContext,
    mut command: ObservedOptionPricingCommand,
) -> Result<(), CliError> {
    let model_path = required_model_path(context, command.model.model_path)?;

    let model_payload = load_json_artifact(&model_path, "model")?;

    let model_run_id = model_run_id(&model_payload)?;

    let run_id = pricing_run_id(model_run_id);

    inject_model_exchange_code_if_missing(&model_payload, &mut command.market_data)?;

    let prepared_market_data = prepare_market_data_command_inputs(
        context,
        &command.market_data,
        janiform::calibration::staged::CalibrationMarketDataWorkflowScope::FullWithOptionSurface,
    )?;

    let option_surface_source =
        prepared_market_data
            .option_surface_source
            .ok_or(CliError::MissingRequiredArgument {
                argument_name: "--option-surface-path",
            })?;

    let option_surface_column_mapping = prepared_market_data.column_mapping.option_surface.ok_or(
        CliError::MissingRequiredArgument {
            argument_name: "--option-surface-path",
        },
    )?;

    let valuation_time_utc = optional_valuation_time(
        command.valuation_time_utc.as_deref().or(command
            .market_data
            .option_surface_valuation_time_utc
            .as_deref()),
    )?;

    let output_path =
        resolve_pricing_output_path(context, command.output_path.output_path, &run_id);

    let workflow_request = ObservedOptionsPricingRequest {
        model_payload,
        run_id: Some(run_id),
        tick_data_source: prepared_market_data.tick_data_source,
        tick_data_column_mapping: prepared_market_data.column_mapping.tick_data,
        option_surface_source,
        option_surface_column_mapping,
        normalization_policy: prepared_market_data.normalization_policy,
        valuation_time_utc,
        option_type: command.option_type.map(observed_option_type_filter),
        scope: if command.surface {
            ObservedOptionsPricingScope::Surface
        } else {
            ObservedOptionsPricingScope::Snapshot
        },
        risk_free_rate: command.risk_free_rate,
        dividend_yield: command.dividend_yield,
        valuation_count: command.valuation_count,
        use_all_valuation_times: command.use_all_valuation_times,
        cpu_count: command.cpu_count,
        optional_output_selection: VanillaOptionPricingOptionalOutputSelection::default(),
        pricing_interval_schedule_policy: PricingIntervalSchedulePolicy::default(),
        output_path: Some(output_path),
    };

    let mut progress_adapter = terminal_progress_adapter_for_context(context);

    let output = run_observed_options_pricing_with_observer(
        workflow_request,
        PricingWorkflowObserver {
            progress_adapter: progress_adapter.as_mut(),
            terminal_output: terminal_output_for_context(context),
        },
    )?;

    emit_observed_completion(context, &model_path, &output)
}

fn run_fitted_model_pricing(
    context: &CommandContext,
    model: crate::commands::options::ModelPathOptions,
    option: VanillaOptionPricingOptions,
    output_path: Option<PathBuf>,
) -> Result<(), CliError> {
    let model_path = required_model_path(context, model.model_path)?;

    let model_payload = load_json_artifact(&model_path, "model")?;

    let valuation_time_utc = required_valuation_time(option.valuation_time_utc.as_deref())?;

    let model_run_id = model_run_id(&model_payload)?;

    let run_id = pricing_run_id(model_run_id);

    let output_path = resolve_pricing_output_path(context, output_path, &run_id);

    let workflow_request = FittedModelSingleOptionPricingRequest {
        model_payload,
        run_id: Some(run_id),
        valuation_time_utc,
        spot: required_f64("--spot", option.spot)?,
        strike: required_f64("--strike", option.strike)?,
        expiry_timestamp_utc: option.expiry_timestamp_utc,
        expiry_date: option.expiry_date,
        tenor: option.tenor,
        payoff_type: payoff_type(option.payoff_type),
        risk_free_rate: option.risk_free_rate,
        dividend_yield: option.dividend_yield,
        optional_output_selection: VanillaOptionPricingOptionalOutputSelection::new(
            option.include_transform_diagnostics,
            option.include_integration_quality_diagnostics,
            option.include_calibration_hooks,
        ),
        pricing_interval_schedule_policy: PricingIntervalSchedulePolicy::default(),
        continuation_data: None,
        initial_excitation_state: None,
        output_path: Some(output_path),
    };

    let mut progress_adapter = terminal_progress_adapter_for_context(context);
    let output = run_fitted_model_single_option_pricing_workflow(
        FittedModelSingleOptionPricingWorkflowRequest {
            pricing_request: workflow_request,
            model_path: Some(model_path.clone()),
        },
        PricingWorkflowObserver {
            progress_adapter: progress_adapter.as_mut(),
            terminal_output: terminal_output_for_context(context),
        },
    )?;

    emit_completion(context, &model_path, &output)
}

fn required_model_path(
    context: &CommandContext,
    path: Option<PathBuf>,
) -> Result<PathBuf, CliError> {
    let path = path.ok_or(CliError::MissingRequiredArgument {
        argument_name: "--model-path",
    })?;

    context.paths().input_file("--model-path", &path)
}

fn required_valuation_time(value: Option<&str>) -> Result<DateTime<Utc>, CliError> {
    let value = value.ok_or(CliError::MissingRequiredArgument {
        argument_name: "--valuation-time-utc",
    })?;

    parse_utc_datetime(value).map_err(|source| CliError::InvalidCommandArgument {
        argument_name: "--valuation-time-utc",
        message: format!("must be an RFC 3339 timestamp: {source}"),
    })
}

fn optional_valuation_time(value: Option<&str>) -> Result<Option<DateTime<Utc>>, CliError> {
    value
        .map(|value| {
            parse_utc_datetime(value).map_err(|source| CliError::InvalidCommandArgument {
                argument_name: "--valuation-time-utc",
                message: format!("must be an RFC 3339 timestamp: {source}"),
            })
        })
        .transpose()
}

fn required_f64(argument_name: &'static str, value: Option<f64>) -> Result<f64, CliError> {
    value.ok_or(CliError::MissingRequiredArgument { argument_name })
}

fn inject_model_exchange_code_if_missing(
    model_payload: &Value,
    market_data: &mut TickDataOptions,
) -> Result<(), CliError> {
    if market_data.exchange_code.is_some() {
        return Ok(());
    }

    let exchange_code = model_payload
        .pointer("/session_resolution/exchange_code")
        .and_then(Value::as_str)
        .ok_or_else(|| CliError::ArtifactMalformed {
            artifact_name: "model",
            path: PathBuf::from("model_payload"),
            message: "missing string field `session_resolution.exchange_code`".to_string(),
        })?;

    market_data.exchange_code = Some(exchange_code.to_string());

    Ok(())
}

fn model_run_id(model_payload: &Value) -> Result<&str, CliError> {
    model_payload
        .get("run_id")
        .and_then(Value::as_str)
        .ok_or_else(|| CliError::ArtifactMalformed {
            artifact_name: "model",
            path: PathBuf::from("model_payload"),
            message: "missing string field `run_id`".to_string(),
        })
}

fn resolve_pricing_output_path(
    context: &CommandContext,
    output_path: Option<PathBuf>,
    run_id: &str,
) -> PathBuf {
    let Some(output_path) = output_path else {
        return default_run_output_directory(context, run_id).join("pricing.json");
    };

    let mut resolved_path = context.paths().resolve_process_path(&output_path);
    if resolved_path.extension().is_none() {
        resolved_path.set_extension("json");
    }

    resolved_path
}

fn emit_completion(
    context: &CommandContext,
    model_path: &std::path::Path,
    output: &FittedModelSingleOptionPricingOutput,
) -> Result<(), CliError> {
    match context.output_mode() {
        OutputMode::Json => write_json_payload(&output.completion_payload(model_path)?),
        OutputMode::Terminal | OutputMode::Quiet | OutputMode::Silent => Ok(()),
    }
}

fn emit_observed_completion(
    context: &CommandContext,
    model_path: &std::path::Path,
    output: &ObservedOptionsPricingOutput,
) -> Result<(), CliError> {
    match context.output_mode() {
        OutputMode::Json => write_json_payload(&output.completion_payload(model_path)?),
        OutputMode::Terminal | OutputMode::Quiet | OutputMode::Silent => Ok(()),
    }
}

fn payoff_type(payoff_type: PricePayoffType) -> VanillaOptionPayoffType {
    match payoff_type {
        PricePayoffType::Call => VanillaOptionPayoffType::Call,
        PricePayoffType::Put => VanillaOptionPayoffType::Put,
    }
}

fn observed_option_type_filter(payoff_type: PricePayoffType) -> ObservedOptionTypeFilter {
    match payoff_type {
        PricePayoffType::Call => ObservedOptionTypeFilter::Call,
        PricePayoffType::Put => ObservedOptionTypeFilter::Put,
    }
}

fn pricing_run_id(model_run_id: &str) -> String {
    format!(
        "{model_run_id}_price_{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    )
}
