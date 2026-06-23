use std::time::Instant;

use janiform::model::factors::interval_factor_grid::NANOS_PER_HOUR;
use janiform::pricing::{
    AnchoredVanillaOptionPricingBoundaryInput, VanillaOptionPayoffConfiguration,
    VanillaOptionPricingExecutionInput, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters, VanillaOptionPricingRequest,
    materialize_schedule_interval_factor_grids, run_vanilla_option_pricing,
};
use janiform_foundations::cancellation::CancellationToken;
use janiform_terminal_ui::{ProgressEvent, ProgressMeasure, TerminalSummaryPolicy};

use crate::error::WorkflowError;
use crate::model::{
    build_interval_factor_grid, build_risk_neutral_vanilla_option_model_parameters,
    fitted_model_context, model_reference_price,
};

use super::artifacts::{pricing_artifact_paths, write_pricing_artifacts};
use super::labels::fitted_model_single_option_pricing_progress_scope_label;
use super::maturity::{
    finalize_single_maturity_resolution, resolve_single_maturity,
    runtime_schedule_interval_factor_grid_coverage,
};
use super::output::FittedModelSingleOptionPricingOutput;
use super::output::{pricing_completion_summary, pricing_start_summary};
use super::progress::{
    cancel_pricing_progress, complete_pricing_progress, emit_pricing_completion_summary,
    emit_pricing_start_summary, start_pricing_progress,
};
use super::request::FittedModelSingleOptionPricingRequest;
use super::request::FittedModelSingleOptionPricingWorkflowRequest;
use super::state::resolve_pricing_boundary_state;
use super::validation::pricing_validation_error;
use super::workflow::PricingWorkflowObserver;

const DEFAULT_PRICING_INTERVAL_DURATION_NANOS: i64 = NANOS_PER_HOUR;

/// Run a single vanilla option pricing workflow from a persisted model payload.
pub fn run_fitted_model_single_option_pricing(
    request: FittedModelSingleOptionPricingRequest,
    cancellation_token: CancellationToken,
) -> Result<FittedModelSingleOptionPricingOutput, WorkflowError> {
    validate_single_option_request(&request)?;
    cancellation_token.check_cancelled()?;

    let started_at = Instant::now();

    let context = fitted_model_context(
        &request.model_payload,
        request.valuation_time_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
        1,
    )?;

    cancellation_token.check_cancelled()?;

    let run_id = match &request.run_id {
        // Intentional copy because the workflow owns the final run id while the
        // borrowed request remains available for validation and request building.
        Some(run_id) => run_id.clone(),
        None => pricing_run_id(&context.model.run_id),
    };

    let initial_maturity_resolution = resolve_single_maturity(&request)?;

    cancellation_token.check_cancelled()?;

    let prepared_pricing_input =
        prepare_pricing_input(&request, &context, initial_maturity_resolution)?;

    cancellation_token.check_cancelled()?;

    let execution_input = build_execution_input(
        &request,
        prepared_pricing_input.runtime_input,
        prepared_pricing_input.maturity_resolution,
    )?;

    cancellation_token.check_cancelled()?;

    let run_result = run_vanilla_option_pricing(execution_input).map_err(|source| {
        WorkflowError::PricingExecutionFailed {
            message: source.to_string(),
        }
    })?;

    cancellation_token.check_cancelled()?;

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

/// Run a fitted-model single-option pricing workflow.
pub fn run_fitted_model_single_option_pricing_workflow(
    request: FittedModelSingleOptionPricingWorkflowRequest,
    mut observer: PricingWorkflowObserver<'_>,
) -> Result<FittedModelSingleOptionPricingOutput, WorkflowError> {
    let cancellation_token = observer.cancellation_token.shared_handle();
    cancellation_token.check_cancelled()?;

    let FittedModelSingleOptionPricingWorkflowRequest {
        pricing_request,
        model_path,
    } = request;

    emit_start_summary(
        &observer.terminal_output,
        model_path.as_deref(),
        &pricing_request,
    )?;

    let progress_run_id = pricing_request.run_id.as_deref().unwrap_or("pricing");

    start_pricing_progress(
        observer.progress_adapter.as_deref_mut(),
        progress_run_id,
        fitted_model_single_option_pricing_progress_scope_label(),
        1,
        pricing_request.output_path.as_deref(),
    )?;

    if let Some(progress_adapter) = observer.progress_adapter.as_deref_mut() {
        progress_adapter.observe(ProgressEvent::RunProgress {
            phase: "Pricing workflow".to_string(),
            measure: ProgressMeasure::Unbounded,
            latest_signal: Some("preparing runtime inputs and executing pricing".to_string()),
        });
        progress_adapter
            .draw_latest()
            .map_err(|source| WorkflowError::OutputProgress { source })?;
    }

    let output =
        run_fitted_model_single_option_pricing(pricing_request, cancellation_token.shared_handle());

    let output = match output {
        Ok(output) => output,
        Err(WorkflowError::Cancelled { source }) => {
            cancel_pricing_progress(observer.progress_adapter.as_deref_mut())?;
            return Err(WorkflowError::Cancelled { source });
        }
        Err(error) => return Err(error),
    };

    if let Err(source) = cancellation_token.check_cancelled() {
        cancel_pricing_progress(observer.progress_adapter.as_deref_mut())?;
        return Err(WorkflowError::Cancelled { source });
    }

    complete_pricing_progress(observer.progress_adapter.as_deref_mut())?;
    emit_completion_summary(&observer.terminal_output, &output)?;

    Ok(output)
}

fn emit_start_summary(
    terminal_output: &Option<TerminalSummaryPolicy>,
    model_path: Option<&std::path::Path>,
    request: &FittedModelSingleOptionPricingRequest,
) -> Result<(), WorkflowError> {
    emit_pricing_start_summary(
        terminal_output,
        pricing_start_summary(
            request.run_id.as_deref().unwrap_or("pricing"),
            model_path,
            request.output_path.as_deref(),
            request.valuation_time_utc.to_rfc3339().as_str(),
        ),
    )
}

fn emit_completion_summary(
    terminal_output: &Option<TerminalSummaryPolicy>,
    output: &FittedModelSingleOptionPricingOutput,
) -> Result<(), WorkflowError> {
    emit_pricing_completion_summary(
        terminal_output,
        pricing_completion_summary(output, output.execution_duration),
    )
}

struct PreparedPricingRuntimeInput {
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    model_parameters: VanillaOptionPricingModelParameters,
    interval_factor_grids: Vec<janiform::pricing::VanillaOptionPricingIntervalFactors>,
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
    let boundary_interval_factor_grid = build_interval_factor_grid(
        context,
        request.valuation_time_utc,
        context.range_end_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
    )?;

    let (pricing_coverage_start_utc, pricing_coverage_end_utc) =
        runtime_schedule_interval_factor_grid_coverage(
            request.valuation_time_utc,
            &initial_maturity_resolution.pricing_interval_schedule,
        )?;

    let pricing_interval_factor_grid = build_interval_factor_grid(
        context,
        pricing_coverage_start_utc,
        pricing_coverage_end_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
    )?;

    let maturity_resolution = finalize_single_maturity_resolution(
        request,
        initial_maturity_resolution,
        &pricing_interval_factor_grid,
    )?;

    let reference_price = model_reference_price(&context.model)?;
    let observed_log_return = request.spot.ln() - reference_price.ln();

    let boundary_state = resolve_pricing_boundary_state(
        request,
        context,
        &boundary_interval_factor_grid,
        observed_log_return,
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
        boundary_state.long_variance_state,
        boundary_state.market_pressure_state,
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .mean_reversion_fitted_parameters,
        interval_delta,
        boundary_state.mean_reversion_boundary_state,
    )
    .map_err(|source| pricing_validation_error("boundary_input", source.to_string()))?;

    let model_parameters = build_risk_neutral_vanilla_option_model_parameters(context)?;

    let interval_factor_grids = materialize_schedule_interval_factor_grids(
        &maturity_resolution.pricing_interval_schedule,
        &pricing_interval_factor_grid,
    )
    .map_err(|source| pricing_validation_error("runtime_interval_schedule", source.to_string()))?;

    Ok(PreparedPricingInput {
        runtime_input: PreparedPricingRuntimeInput {
            boundary_input,
            model_parameters,
            interval_factor_grids,
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
        runtime_input.interval_factor_grids,
        request.optional_output_selection,
    )
    .map_err(|source| pricing_validation_error("execution_input", source.to_string()))
}

fn pricing_run_id(model_run_id: &str) -> String {
    format!(
        "{model_run_id}_price_{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    )
}
