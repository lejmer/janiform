use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use janiform::calibration::current_state::CurrentStateEstimationOutput;
use janiform::pricing::{
    AnchoredVanillaOptionPricingBoundaryInput, HighLevelPricingMaturityResolution,
    HighLevelPricingMaturityResolutionRequest, PricingIntervalSchedulePolicy, PricingProgressEvent,
    VanillaOptionGridBatchPricingExecutionInput,
    VanillaOptionGridBatchPricingOperatorApproximation, VanillaOptionGridBatchPricingRequest,
    VanillaOptionGridBatchPricingRunResult, VanillaOptionGridRequest,
    materialize_schedule_interval_factors, resolve_high_level_pricing_maturity_inputs,
    run_vanilla_option_grid_batch_pricing,
};
use janiform_terminal_ui::{ProgressAdapter, ProgressMeasure};

use crate::error::WorkflowError;
use crate::model::{
    build_hourly_factor_contract, fitted_model_context, model_reference_price, validation_error,
};

use super::super::maturity::{
    finalize_maturity_resolution, runtime_schedule_hourly_factor_contract_coverage,
};
use super::super::model_parameters::build_vanilla_option_pricing_model_parameters;
use super::super::validation::pricing_validation_error;
use super::progress::{
    ObservedWorkItemUpdate, emit_observed_stage_progress, emit_single_snapshot_pricing_progress,
};
use super::surface_progress::SurfaceTaskProgressPlan;
use super::types::{
    DEFAULT_PRICING_INTERVAL_DURATION_NANOS, ObservedOptionsPricingNode,
    ObservedOptionsPricingRequest, ObservedOptionsSnapshotResult, PreparedObservedSnapshot,
    PreparedSnapshotGroup,
};

pub(super) fn price_prepared_snapshot_with_terminal_progress(
    request: &ObservedOptionsPricingRequest,
    prepared_snapshot: PreparedObservedSnapshot,
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    cpu_count: Option<usize>,
) -> Result<ObservedOptionsSnapshotResult, WorkflowError> {
    let total_nodes = prepared_snapshot_node_count(&prepared_snapshot);

    emit_observed_stage_progress(
        progress_adapter,
        "price valuation surface",
        ProgressMeasure::bounded(0, 1)
            .map_err(|source| WorkflowError::OutputProgress { source })?,
        Some(format!(
            "pricing 1 valuation snapshot with {total_nodes} option nodes"
        )),
        ObservedWorkItemUpdate::none(),
    )?;

    let mut progress_error = None::<WorkflowError>;
    let mut observer = |event: PricingProgressEvent| {
        if progress_error.is_some() {
            return;
        }

        if let Err(source) = emit_single_snapshot_pricing_progress(progress_adapter, &event) {
            progress_error = Some(source);
        }
    };

    let result =
        price_prepared_snapshot(request, prepared_snapshot, Some(&mut observer), cpu_count)?;

    if let Some(progress_error) = progress_error {
        return Err(progress_error);
    }

    emit_observed_stage_progress(
        progress_adapter,
        "price valuation surface",
        ProgressMeasure::bounded(1, 1)
            .map_err(|source| WorkflowError::OutputProgress { source })?,
        Some("priced 1 valuation snapshot".to_string()),
        ObservedWorkItemUpdate::none(),
    )?;

    Ok(result)
}

pub(super) fn price_prepared_snapshot(
    request: &ObservedOptionsPricingRequest,
    prepared_snapshot: PreparedObservedSnapshot,
    progress_observer: Option<&mut dyn FnMut(PricingProgressEvent)>,
    cpu_count: Option<usize>,
) -> Result<ObservedOptionsSnapshotResult, WorkflowError> {
    let valuation_time_utc =
        DateTime::<Utc>::from_timestamp_nanos(prepared_snapshot.valuation_time_utc_nanos);

    let execution_input = build_grid_batch_execution_input(
        request,
        valuation_time_utc,
        prepared_snapshot.spot,
        &prepared_snapshot.snapshot_groups,
        &prepared_snapshot.estimated_state,
        cpu_count,
    )?;

    let run_result = if let Some(progress_observer) = progress_observer {
        janiform::pricing::run_vanilla_option_grid_batch_pricing_with_observer(
            execution_input,
            Some(progress_observer),
        )
    } else {
        run_vanilla_option_grid_batch_pricing(execution_input)
    }
    .map_err(|source| WorkflowError::PricingExecutionFailed {
        message: source.to_string(),
    })?;

    let nodes = build_priced_nodes(
        valuation_time_utc,
        &prepared_snapshot.snapshot_groups,
        &run_result,
    )?;

    Ok(ObservedOptionsSnapshotResult {
        valuation_time_utc,
        spot: prepared_snapshot.spot,
        nodes,
    })
}

fn build_grid_batch_execution_input(
    request: &ObservedOptionsPricingRequest,
    valuation_time_utc: DateTime<Utc>,
    spot: f64,
    snapshot_groups: &[PreparedSnapshotGroup],
    estimated_state: &CurrentStateEstimationOutput,
    cpu_count: Option<usize>,
) -> Result<VanillaOptionGridBatchPricingExecutionInput, WorkflowError> {
    let horizon_interval_count = 1;

    let context = fitted_model_context(
        &request.model_payload,
        valuation_time_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
        horizon_interval_count,
    )?;

    let maturity_resolution = resolve_snapshot_maturity_resolution(
        valuation_time_utc,
        snapshot_groups,
        &request.pricing_interval_schedule_policy,
    )?;

    let (pricing_coverage_start_utc, pricing_coverage_end_utc) =
        runtime_schedule_hourly_factor_contract_coverage(
            valuation_time_utc,
            &maturity_resolution.pricing_interval_schedule,
        )?;

    let pricing_hourly_factor_contract = build_hourly_factor_contract(
        &context,
        pricing_coverage_start_utc,
        pricing_coverage_end_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
    )?;

    let maturity_resolution = finalize_maturity_resolution(
        &request.pricing_interval_schedule_policy,
        maturity_resolution,
        &pricing_hourly_factor_contract,
    )?;

    let reference_price = model_reference_price(&context.model)?;
    let observed_log_return = spot.ln() - reference_price.ln();

    let interval_delta = maturity_resolution
        .pricing_interval_schedule
        .interval_delta_hours(0)
        .ok_or_else(|| {
            pricing_validation_error("runtime_interval_schedule", "must not be empty")
        })?;

    let boundary_input = AnchoredVanillaOptionPricingBoundaryInput::try_new(
        0,
        estimated_state.hawkes_excitation_state,
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .mean_reversion_fitted_parameters
            // Intentional copy because the pricing boundary owns a validated
            // mean-reversion contract independent of the shared model context.
            .clone(),
        interval_delta,
        // Intentional copy because the pricing boundary owns the estimated
        // state while prepared snapshots remain available for result assembly.
        estimated_state.mean_reversion_boundary_state.clone(),
    )
    .map_err(|source| pricing_validation_error("boundary_input", source.to_string()))?;

    if !observed_log_return.is_finite() {
        return Err(pricing_validation_error(
            "tick_data.last_price",
            "must produce a finite observed log return",
        ));
    }

    let model_parameters = build_vanilla_option_pricing_model_parameters(&context)?;

    let interval_factors = materialize_schedule_interval_factors(
        &maturity_resolution.pricing_interval_schedule,
        &pricing_hourly_factor_contract,
        context
            .fitted_parameters
            .return_components_fitted_parameters
            .return_mark_hour_of_week_scale_profile
            .hour_of_week_factors
            .as_slice(),
    )
    .map_err(|source| pricing_validation_error("runtime_interval_schedule", source.to_string()))?;

    Ok(VanillaOptionGridBatchPricingExecutionInput {
        request: VanillaOptionGridBatchPricingRequest {
            spot,
            grids: snapshot_groups
                .iter()
                .enumerate()
                .map(|(group_index, group)| VanillaOptionGridRequest {
                    payoff_type: group.payoff_type,
                    risk_free_rate: request.risk_free_rate,
                    dividend_yield: request.dividend_yield,
                    strikes: group.nodes.iter().map(|node| node.strike).collect(),
                    maturity_intervals: vec![maturity_resolution.maturity_intervals[group_index]],
                })
                .collect(),
        },
        boundary_input,
        model_parameters,
        runtime_interval_schedule: maturity_resolution.pricing_interval_schedule,
        interval_delta,
        interval_factors,
        optional_output_selection: request.optional_output_selection,
        spectral_collocation_node_count_override: None,
        operator_approximation: VanillaOptionGridBatchPricingOperatorApproximation::Exact,
        cpu_count,
    })
}

pub(super) fn resolve_surface_task_progress_plans(
    request: &ObservedOptionsPricingRequest,
    prepared_snapshots: &[PreparedObservedSnapshot],
) -> Result<Vec<SurfaceTaskProgressPlan>, WorkflowError> {
    prepared_snapshots
        .iter()
        .map(|prepared_snapshot| {
            resolve_prepared_snapshot_progress_plan(request, prepared_snapshot)
        })
        .collect()
}

fn resolve_prepared_snapshot_progress_plan(
    request: &ObservedOptionsPricingRequest,
    prepared_snapshot: &PreparedObservedSnapshot,
) -> Result<SurfaceTaskProgressPlan, WorkflowError> {
    let valuation_time_utc =
        DateTime::<Utc>::from_timestamp_nanos(prepared_snapshot.valuation_time_utc_nanos);

    let maturity_resolution = resolve_snapshot_maturity_resolution(
        valuation_time_utc,
        &prepared_snapshot.snapshot_groups,
        &request.pricing_interval_schedule_policy,
    )?;

    if !matches!(
        request.pricing_interval_schedule_policy,
        PricingIntervalSchedulePolicy::AutoActivityCompressed
    ) {
        return Ok(surface_task_progress_plan_from_resolution(
            prepared_snapshot,
            &maturity_resolution,
        )?);
    }

    let context = fitted_model_context(
        &request.model_payload,
        valuation_time_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
        1,
    )?;

    let (pricing_coverage_start_utc, pricing_coverage_end_utc) =
        runtime_schedule_hourly_factor_contract_coverage(
            valuation_time_utc,
            &maturity_resolution.pricing_interval_schedule,
        )?;

    let pricing_hourly_factor_contract = build_hourly_factor_contract(
        &context,
        pricing_coverage_start_utc,
        pricing_coverage_end_utc,
        DEFAULT_PRICING_INTERVAL_DURATION_NANOS,
    )?;

    let maturity_resolution = finalize_maturity_resolution(
        &request.pricing_interval_schedule_policy,
        maturity_resolution,
        &pricing_hourly_factor_contract,
    )?;

    surface_task_progress_plan_from_resolution(prepared_snapshot, &maturity_resolution)
}

fn surface_task_progress_plan_from_resolution(
    prepared_snapshot: &PreparedObservedSnapshot,
    maturity_resolution: &HighLevelPricingMaturityResolution,
) -> Result<SurfaceTaskProgressPlan, WorkflowError> {
    let pricing_interval_units = maturity_resolution
        .pricing_interval_schedule
        .interval_count();

    let maturity_operator_application_units = maturity_resolution
        .maturity_terminal_interval_indices
        .iter()
        .map(|terminal_interval_index| {
            terminal_interval_index.checked_add(1).ok_or_else(|| {
                pricing_validation_error("maturity_input", "terminal interval index overflowed")
            })
        })
        .collect::<Result<BTreeSet<_>, _>>()?
        .into_iter()
        .fold(0usize, usize::saturating_add);

    let strike_row_units = prepared_snapshot
        .snapshot_groups
        .iter()
        .map(|group| group.nodes.len())
        .fold(0usize, usize::saturating_add);

    Ok(SurfaceTaskProgressPlan::new(
        pricing_interval_units,
        maturity_operator_application_units,
        strike_row_units,
    ))
}

fn resolve_snapshot_maturity_resolution(
    valuation_time_utc: DateTime<Utc>,
    snapshot_groups: &[PreparedSnapshotGroup],
    pricing_interval_schedule_policy: &PricingIntervalSchedulePolicy,
) -> Result<HighLevelPricingMaturityResolution, WorkflowError> {
    let policy = match pricing_interval_schedule_policy {
        PricingIntervalSchedulePolicy::AutoActivityCompressed => {
            PricingIntervalSchedulePolicy::AutoExactSparse
        }
        // Intentional copy because maturity resolution owns the policy value.
        other => other.clone(),
    };

    resolve_high_level_pricing_maturity_inputs(HighLevelPricingMaturityResolutionRequest {
        valuation_time_utc: valuation_time_utc.to_rfc3339(),
        expiry_timestamps_utc: Some(
            snapshot_groups
                .iter()
                .map(|group| {
                    Some(
                        DateTime::<Utc>::from_timestamp_nanos(group.expiry_timestamp_utc_nanos)
                            .to_rfc3339(),
                    )
                })
                .collect(),
        ),
        expiry_dates: None,
        tenors: None,
        pricing_interval_schedule_policy: policy,
    })
    .map_err(|source| pricing_validation_error("maturity_input", source.to_string()))
}

fn build_priced_nodes(
    valuation_time_utc: DateTime<Utc>,
    snapshot_groups: &[PreparedSnapshotGroup],
    run_result: &VanillaOptionGridBatchPricingRunResult,
) -> Result<Vec<ObservedOptionsPricingNode>, WorkflowError> {
    let mut nodes = Vec::new();

    for (group, priced_group) in snapshot_groups.iter().zip(run_result.grids.iter()) {
        let maturity_interval = priced_group
            .maturity_intervals
            .first()
            .copied()
            .ok_or_else(|| {
                validation_error(
                    "pricing_result.maturity_intervals",
                    "must contain one maturity",
                )
            })?;

        for (node_index, prepared_node) in group.nodes.iter().enumerate() {
            nodes.push(ObservedOptionsPricingNode {
                valuation_time_utc,
                quote_timestamp_utc: DateTime::<Utc>::from_timestamp_nanos(
                    prepared_node.quote_timestamp_utc_nanos,
                ),
                expiry_timestamp_utc: DateTime::<Utc>::from_timestamp_nanos(
                    prepared_node.expiry_timestamp_utc_nanos,
                ),
                strike: prepared_node.strike,
                option_type: prepared_node.option_type,
                risk_free_rate: priced_group.risk_free_rate,
                dividend_yield: priced_group.dividend_yield,
                maturity_interval,
                observed_bid_price: prepared_node.observed_bid_price,
                observed_ask_price: prepared_node.observed_ask_price,
                observed_mid_price: prepared_node.observed_mid_price,
                model_price: priced_group.price_grid[0][node_index],
                model_implied_volatility: priced_group.implied_volatility_grid[0][node_index],
                implied_volatility_inversion_status: priced_group
                    .implied_volatility_inversion_status_grid[0][node_index],
                pricing_status: priced_group.pricing_status_grid[0][node_index],
                pricing_failure_stage: priced_group.pricing_failure_stage_grid[0][node_index],
                pricing_failure_reason: priced_group.pricing_failure_reason_grid[0][node_index]
                    // Intentional copy because the observed node owns the
                    // failure reason while the raw grid result is retained.
                    .clone(),
                forward_price: priced_group.forward_price_grid[0][node_index],
                discount_factor: priced_group.discount_factor_grid[0][node_index],
                boundary_state_norm: priced_group.boundary_state_norm_grid[0][node_index],
            });
        }
    }

    Ok(nodes)
}

pub(super) fn prepared_snapshot_node_count(prepared_snapshot: &PreparedObservedSnapshot) -> usize {
    prepared_snapshot
        .snapshot_groups
        .iter()
        .map(|group| group.nodes.len())
        .sum()
}
