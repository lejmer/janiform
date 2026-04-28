use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use janiform::hawkes::HawkesExcitationState;
use janiform::hourly_factors::ONE_HOUR_NANOS;
use janiform::pricing::{
    AnchoredVanillaOptionPricingBoundaryInput, PricingIntervalSchedulePolicy,
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingExecutionInput,
    VanillaOptionPricingMarketInputs, VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
    VanillaOptionPricingRunResult, materialize_schedule_interval_factors,
    run_vanilla_option_pricing,
};
use janiform_foundations::time::deserialize_utc_datetime;
use serde::Deserialize;
use serde_json::{Value, json};

mod artifacts;
mod maturity;
mod state;

/// Continuation observations used by fitted-model pricing.
pub use crate::model::ModelStateObservationData as FittedModelPricingContinuationData;
pub use artifacts::PricingArtifactPaths;

use crate::error::WorkflowError;
use crate::model::{
    build_hourly_factor_contract, fitted_model_context, model_reference_price,
    pricing_nig_shared_plus_shift_configuration,
};
use artifacts::{pricing_artifact_paths, write_pricing_artifacts};
use maturity::{
    finalize_single_maturity_resolution, resolve_single_maturity,
    runtime_schedule_hourly_factor_contract_coverage,
};
use state::resolve_pricing_boundary_state;

const DEFAULT_PRICING_INTERVAL_DURATION_NANOS: i64 = ONE_HOUR_NANOS;

/// Explicit fitted-model single-option pricing request.
#[derive(Debug, Deserialize)]
pub struct FittedModelSingleOptionPricingRequest {
    /// Persisted `JaniformModel` payload.
    pub model_payload: Value,
    /// Optional stable pricing run id.
    pub run_id: Option<String>,
    /// Pricing valuation timestamp.
    #[serde(deserialize_with = "deserialize_utc_datetime")]
    pub valuation_time_utc: DateTime<Utc>,
    /// Current spot price.
    pub spot: f64,
    /// Vanilla option strike.
    pub strike: f64,
    /// Exact option expiry timestamp.
    pub expiry_timestamp_utc: Option<String>,
    /// Option expiry date in YYYY-MM-DD form.
    pub expiry_date: Option<String>,
    /// Relative option tenor.
    pub tenor: Option<String>,
    /// Vanilla payoff side.
    pub payoff_type: VanillaOptionPayoffType,
    /// Continuously compounded risk-free rate.
    pub risk_free_rate: f64,
    /// Continuously compounded dividend yield.
    pub dividend_yield: f64,
    /// Optional pricing output selections.
    #[serde(default)]
    pub optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    /// Pricing interval schedule policy.
    #[serde(default)]
    pub pricing_interval_schedule_policy: PricingIntervalSchedulePolicy,
    /// Optional continuation observations used to estimate the pricing boundary state.
    pub continuation_data: Option<FittedModelPricingContinuationData>,
    /// Optional explicit initial Hawkes state.
    pub initial_excitation_state: Option<HawkesExcitationState>,
    /// Primary output path.
    pub output_path: Option<PathBuf>,
}

/// Completed fitted-model single-option pricing workflow.
pub struct FittedModelSingleOptionPricingOutput {
    /// Stable pricing run id.
    pub run_id: String,
    /// Native pricing result.
    pub run_result: VanillaOptionPricingRunResult,
    /// Optional artifact paths.
    pub artifact_paths: Option<PricingArtifactPaths>,
    /// Native workflow duration.
    pub execution_duration: Duration,
}

/// Run a single vanilla option pricing workflow from a persisted model payload.
pub fn run_fitted_model_single_option_pricing(
    request: FittedModelSingleOptionPricingRequest,
) -> Result<FittedModelSingleOptionPricingOutput, WorkflowError> {
    validate_single_option_request(&request)?;

    let started_at = Instant::now();

    let context = fitted_model_context(
        &request.model_payload,
        request.valuation_time_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
        1,
    )?;

    let run_id = match &request.run_id {
        // Intentional copy because the workflow owns the final run id while the
        // borrowed request remains available for validation and request building.
        Some(run_id) => run_id.clone(),
        None => pricing_run_id(&context.model.run_id),
    };

    let initial_maturity_resolution = resolve_single_maturity(&request)?;

    let prepared_pricing_input =
        prepare_pricing_input(&request, &context, initial_maturity_resolution)?;

    let execution_input = build_execution_input(
        &request,
        prepared_pricing_input.runtime_input,
        prepared_pricing_input.maturity_resolution,
    )?;

    let run_result = run_vanilla_option_pricing(execution_input).map_err(|source| {
        WorkflowError::PricingExecutionFailed {
            message: source.to_string(),
        }
    })?;

    let artifact_paths = if let Some(output_path) = request.output_path.as_deref() {
        let artifact_paths = pricing_artifact_paths(output_path);
        write_pricing_artifacts(&artifact_paths, &run_id, &context.model.run_id, &run_result)?;
        Some(artifact_paths)
    } else {
        None
    };

    Ok(FittedModelSingleOptionPricingOutput {
        run_id,
        run_result,
        artifact_paths,
        execution_duration: started_at.elapsed(),
    })
}

impl FittedModelSingleOptionPricingOutput {
    /// Serialize the completion payload for machine output.
    pub fn completion_payload(&self, model_path: &Path) -> Result<Value, WorkflowError> {
        let response = serde_json::to_value(&self.run_result.response)
            .map_err(|source| WorkflowError::OutputSerialize { source })?;

        let run_diagnostics = serde_json::to_value(&self.run_result.run_diagnostics)
            .map_err(|source| WorkflowError::OutputSerialize { source })?;

        Ok(json!({
            "schema_version": "janiform.price_run.v1",
            "command_id": "price",
            "status": "completed",
            "run_id": self.run_id,
            "model_path": model_path,
            "primary_output_path": self.artifact_paths.as_ref().map(|paths| &paths.primary_output_path),
            "report_path": self.artifact_paths.as_ref().map(|paths| &paths.report_path),
            "manifest_path": self.artifact_paths.as_ref().map(|paths| &paths.manifest_path),
            "run_directory": self.artifact_paths.as_ref().map(|paths| &paths.run_directory),
            "priced_node_count": 1,
            "failed_node_count": single_failed_node_count(&self.run_result),
            "execution_duration_seconds": self.execution_duration.as_secs_f64(),
            "response": response,
            "run_diagnostics": run_diagnostics,
        }))
    }
}

struct PreparedPricingRuntimeInput {
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    model_parameters: VanillaOptionPricingModelParameters,
    interval_factors: Vec<janiform::pricing::VanillaOptionPricingIntervalFactors>,
}

struct PreparedPricingInput {
    runtime_input: PreparedPricingRuntimeInput,
    maturity_resolution: janiform::pricing::HighLevelPricingMaturityResolution,
}

fn validate_single_option_request(
    request: &FittedModelSingleOptionPricingRequest,
) -> Result<(), WorkflowError> {
    if !request.spot.is_finite() || request.spot <= 0.0 {
        return Err(pricing_validation_error(
            "spot",
            "must be finite and strictly positive",
        ));
    }

    if !request.strike.is_finite() || request.strike <= 0.0 {
        return Err(pricing_validation_error(
            "strike",
            "must be finite and strictly positive",
        ));
    }

    if !request.risk_free_rate.is_finite() {
        return Err(pricing_validation_error("risk_free_rate", "must be finite"));
    }

    if !request.dividend_yield.is_finite() {
        return Err(pricing_validation_error("dividend_yield", "must be finite"));
    }

    let maturity_input_count = [
        request.expiry_timestamp_utc.is_some(),
        request.expiry_date.is_some(),
        request.tenor.is_some(),
    ]
    .into_iter()
    .filter(|is_present| *is_present)
    .count();

    if maturity_input_count != 1 {
        return Err(pricing_validation_error(
            "maturity_input",
            "exactly one of expiry_timestamp_utc, expiry_date, or tenor must be provided",
        ));
    }

    Ok(())
}

fn prepare_pricing_input(
    request: &FittedModelSingleOptionPricingRequest,
    context: &crate::model::FittedModelContext,
    initial_maturity_resolution: janiform::pricing::HighLevelPricingMaturityResolution,
) -> Result<PreparedPricingInput, WorkflowError> {
    let boundary_hourly_factor_contract = build_hourly_factor_contract(
        context,
        request.valuation_time_utc,
        context.range_end_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
    )?;

    let (pricing_coverage_start_utc, pricing_coverage_end_utc) =
        runtime_schedule_hourly_factor_contract_coverage(
            request.valuation_time_utc,
            &initial_maturity_resolution.pricing_interval_schedule,
        )?;

    let pricing_hourly_factor_contract = build_hourly_factor_contract(
        context,
        pricing_coverage_start_utc,
        pricing_coverage_end_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
    )?;

    let maturity_resolution = finalize_single_maturity_resolution(
        request,
        initial_maturity_resolution,
        &pricing_hourly_factor_contract,
    )?;

    let reference_price = model_reference_price(&context.model)?;
    let observed_log_price = (request.spot / reference_price).ln();
    let boundary_state = resolve_pricing_boundary_state(
        request,
        context,
        &boundary_hourly_factor_contract,
        observed_log_price,
    )?;

    let interval_delta = maturity_resolution
        .pricing_interval_schedule
        .interval_delta_hours(0)
        .ok_or_else(|| {
            pricing_validation_error("runtime_interval_schedule", "must not be empty")
        })?;

    let boundary_input = AnchoredVanillaOptionPricingBoundaryInput::try_new(
        0,
        boundary_state.excitation_state,
        context
            .return_components_fitted_parameters
            .mean_reversion_fitted_parameters
            // Intentional copy because the pricing boundary owns a validated
            // mean-reversion contract independent of the shared model context.
            .clone(),
        interval_delta,
        boundary_state.mean_reversion_boundary_state,
    )
    .map_err(|source| pricing_validation_error("boundary_input", source.to_string()))?;

    let model_parameters = VanillaOptionPricingModelParameters::try_new(
        context.hawkes_fitted_parameters.kernel_parameters,
        context
            .return_components_fitted_parameters
            .news_process_configuration
            // Intentional copy because pricing model parameters own their
            // native contract after workflow preparation completes.
            .clone(),
        pricing_nig_shared_plus_shift_configuration(context)?,
    )
    .map_err(|source| pricing_validation_error("model_parameters", source.to_string()))?;

    let interval_factors = materialize_schedule_interval_factors(
        &maturity_resolution.pricing_interval_schedule,
        &pricing_hourly_factor_contract,
    )
    .map_err(|source| pricing_validation_error("runtime_interval_schedule", source.to_string()))?;

    Ok(PreparedPricingInput {
        runtime_input: PreparedPricingRuntimeInput {
            boundary_input,
            model_parameters,
            interval_factors,
        },
        maturity_resolution,
    })
}

fn build_execution_input(
    request: &FittedModelSingleOptionPricingRequest,
    runtime_input: PreparedPricingRuntimeInput,
    maturity_resolution: janiform::pricing::HighLevelPricingMaturityResolution,
) -> Result<VanillaOptionPricingExecutionInput, WorkflowError> {
    let maturity_interval = maturity_resolution
        .maturity_intervals
        .first()
        .copied()
        .ok_or_else(|| {
            pricing_validation_error("maturity_intervals", "must contain one maturity")
        })?;

    let interval_delta = maturity_resolution
        .pricing_interval_schedule
        .interval_delta_hours(0)
        .ok_or_else(|| {
            pricing_validation_error("runtime_interval_schedule", "must not be empty")
        })?;

    let pricing_request = VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new_single_node(
            request.spot,
            request.strike,
            maturity_interval,
            request.risk_free_rate,
            request.dividend_yield,
        )
        .map_err(|source| pricing_validation_error("market_inputs", source.to_string()))?,
        VanillaOptionPayoffConfiguration {
            payoff_type: request.payoff_type,
        },
        runtime_input.boundary_input,
        runtime_input.model_parameters,
        interval_delta,
    )
    .map_err(|source| pricing_validation_error("pricing_request", source.to_string()))?;

    VanillaOptionPricingExecutionInput::try_new(
        pricing_request,
        maturity_resolution.pricing_interval_schedule,
        runtime_input.interval_factors,
        request.optional_output_selection,
    )
    .map_err(|source| pricing_validation_error("execution_input", source.to_string()))
}

fn single_failed_node_count(run_result: &VanillaOptionPricingRunResult) -> usize {
    if run_result.response.price.is_some() {
        0
    } else {
        1
    }
}

fn pricing_run_id(model_run_id: &str) -> String {
    format!(
        "{model_run_id}_price_{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    )
}

fn pricing_validation_error(field: &'static str, message: impl Into<String>) -> WorkflowError {
    WorkflowError::PricingValidationFailed {
        field,
        message: message.into(),
    }
}
