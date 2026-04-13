use std::collections::BTreeMap;
use std::sync::mpsc::Sender;

use chrono::{DateTime, Utc};
use janiform_engine_pricing::{
    AnchoredVanillaOptionPricingBoundaryInput, PricingIntervalSchedule, PricingProgressEvent,
    PricingProgressPhase, VanillaOptionGridBatchPricingExecutionInput,
    VanillaOptionGridBatchPricingRequest, VanillaOptionGridBatchPricingRunResult,
    VanillaOptionGridRequest, VanillaOptionPayoffType, VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection, derive_pricing_interval_schedule,
    materialize_schedule_interval_factors, run_vanilla_option_grid_batch_pricing,
    run_vanilla_option_grid_batch_pricing_with_observer,
};
use janiform_foundations::time::{ONE_HOUR_NANOS, ceil_to_hour_utc_nanos, floor_to_hour_utc_nanos};
use janiform_market_time::hourly_factors::{
    HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource, HourlySeasonalityFactorSource,
};

use crate::adapters::load_hourly_factor_contract_for_calibration;
use crate::current_state::{
    CurrentStateEstimationInput, estimate_current_state_from_observed_data,
};
use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageOptionPricingInput,
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};
use crate::stages::seasonality::build_runtime_hourly_seasonality_factors_from_components;
use janiform_calibration_artifacts::{
    JointRefinementStageOptionAggregateDiagnostic,
    JointRefinementStageOptionCalibrationDiagnostics, JointRefinementStageOptionNodeDiagnostic,
    JointRefinementStageOptionSnapshotMetadata,
};
use janiform_market_time::calendar::ExchangeCalendarRegistry;

use super::super::objective::root_mean_square;
use super::super::setup::build_surprise_multipliers_for_interval_grid;
use super::state::{
    JointRefinementContext, JointRefinementOptionNodeDefinition, JointRefinementOptionNodeLocation,
    JointRefinementOptionPricingContext,
};

struct OptionPricingHorizon {
    runtime_interval_schedule: PricingIntervalSchedule,
    hourly_coverage_start_utc_nanos: i64,
    hourly_coverage_end_utc_nanos: i64,
    hourly_coverage_interval_count: usize,
}

struct OptionBucketDefinition {
    payoff_type: VanillaOptionPayoffType,
    maturity_interval: f64,
    strikes: Vec<f64>,
}

struct ResolvedOptionNode<'a> {
    node: &'a crate::stages::joint_refinement::JointRefinementStageOptionSnapshotNode,
    maturity_interval: f64,
}

struct ResolvedOptionMaturityBucket<'a> {
    definition: OptionBucketDefinition,
    nodes: Vec<ResolvedOptionNode<'a>>,
}

fn pricing_nig_configuration(
    fitted_parameters: &janiform_calibration_artifacts::JointRefinementStageFittedParameters,
) -> janiform_model_kernels::nig::NigSharedPlusShiftConfiguration {
    fitted_parameters
        .pricing_block_fitted_parameters
        .as_ref()
        .map(|pricing_block| pricing_block.nig_shared_plus_shift_configuration)
        .unwrap_or(
            fitted_parameters
                .return_components_fitted_parameters
                .nig_shared_plus_shift_configuration,
        )
}

pub(crate) fn build_option_pricing_context(
    run_spec: &janiform_calibration_artifacts::CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: &crate::stages::joint_refinement::JointRefinementStageExecutionInput,
    hawkes_payload: &crate::stages::hawkes::HawkesStageFittedPayload,
    return_components_payload: &crate::stages::return_components::ReturnComponentsStageFittedPayload,
    mean_reversion_payload: &crate::stages::mean_reversion::MeanReversionStageFittedPayload,
    cpu_count: usize,
) -> Result<Option<JointRefinementOptionPricingContext>, JointRefinementStageExecutionFailure> {
    let Some(option_pricing_input) = stage_input.option_pricing_input.as_ref() else {
        return Ok(None);
    };

    let horizon = resolve_option_pricing_horizon(option_pricing_input)?;

    let current_state_estimation_output = estimate_option_pricing_current_state(
        option_pricing_input,
        hawkes_payload,
        mean_reversion_payload,
    )?;

    let option_spot = validate_option_spot(option_pricing_input)?;

    let hourly_factor_contract = build_option_hourly_factor_contract(
        run_spec,
        calendar_registry,
        stage_input,
        return_components_payload,
        &horizon,
    )?;

    let interval_factors = materialize_schedule_interval_factors(
        &horizon.runtime_interval_schedule,
        &hourly_factor_contract,
    )
    .map_err(|source| {
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
            error_message: format!(
                "joint option calibration interval factor materialization failed: {source}"
            ),
        }
    })?;

    let boundary_input = build_option_boundary_input(
        mean_reversion_payload,
        &current_state_estimation_output,
        &horizon.runtime_interval_schedule,
    )?;

    let (
        grid_batch_pricing_request,
        observed_nodes,
        snapshot_maturity_intervals,
        snapshot_maturity_terminal_interval_indices,
    ) = build_option_grid_batch_pricing_request(
        option_spot,
        option_pricing_input,
        &horizon.runtime_interval_schedule,
    )?;

    Ok(Some(JointRefinementOptionPricingContext {
        valuation_time_utc_nanos: option_pricing_input.valuation_time_utc_nanos,
        grid_batch_pricing_request,
        boundary_input,
        runtime_interval_schedule: horizon.runtime_interval_schedule,
        interval_factors,
        snapshot_maturity_intervals,
        snapshot_maturity_terminal_interval_indices,
        carry_input: option_pricing_input.carry_input.clone(),
        observed_nodes,
        cpu_count: cpu_count.max(1),
    }))
}

fn estimate_option_pricing_current_state(
    option_pricing_input: &JointRefinementStageOptionPricingInput,
    hawkes_payload: &crate::stages::hawkes::HawkesStageFittedPayload,
    mean_reversion_payload: &crate::stages::mean_reversion::MeanReversionStageFittedPayload,
) -> Result<crate::current_state::CurrentStateEstimationOutput, JointRefinementStageExecutionFailure>
{
    estimate_current_state_from_observed_data(CurrentStateEstimationInput {
        kernel_parameters: hawkes_payload.fitted_parameters.kernel_parameters,
        // Intentional clone so pricing-context preparation does not consume the shared stage payload.
        mean_reversion_fitted_parameters: mean_reversion_payload.fitted_parameters.clone(),
        state_time_utc_nanos: option_pricing_input
            .current_state_estimation_input
            .state_time_utc_nanos,
        observed_log_price: option_pricing_input
            .current_state_estimation_input
            .observed_log_price,
        observation_timestamp_utc_nanos: option_pricing_input
            .current_state_estimation_input
            .observation_timestamp_utc_nanos
            .clone(),
        observation_trade_prices: option_pricing_input
            .current_state_estimation_input
            .observation_trade_prices
            .clone(),
        observation_trade_sides: option_pricing_input
            .current_state_estimation_input
            .observation_trade_sides
            .clone(),
    })
    .map_err(
        |source| JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
            error_message: format!(
                "joint option calibration current-state estimation failed: {source}"
            ),
        },
    )
}

fn validate_option_spot(
    option_pricing_input: &JointRefinementStageOptionPricingInput,
) -> Result<f64, JointRefinementStageExecutionFailure> {
    let option_spot = option_pricing_input
        .current_state_estimation_input
        .observed_spot;
    if option_spot.is_finite() && option_spot > 0.0 {
        return Ok(option_spot);
    }

    Err(
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
            error_message: "joint option calibration requires a strictly positive observed spot"
                .to_string(),
        },
    )
}

fn resolve_option_pricing_horizon(
    option_pricing_input: &JointRefinementStageOptionPricingInput,
) -> Result<OptionPricingHorizon, JointRefinementStageExecutionFailure> {
    let expiry_timestamps_utc_nanos = option_pricing_input
        .option_snapshot_nodes
        .iter()
        .map(|node| node.expiry_timestamp_utc_nanos)
        .collect::<Vec<_>>();

    let max_expiry_timestamp_utc_nanos = expiry_timestamps_utc_nanos
        .iter()
        .copied()
        .max()
        .ok_or_else(
            || JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option calibration requires a non-empty selected option snapshot"
                        .to_string(),
            },
        )?;
    if max_expiry_timestamp_utc_nanos <= option_pricing_input.valuation_time_utc_nanos {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option calibration requires option expiries strictly after valuation time"
                        .to_string(),
            },
        );
    }

    let runtime_interval_schedule = derive_pricing_interval_schedule(
        option_pricing_input.valuation_time_utc_nanos,
        expiry_timestamps_utc_nanos.as_slice(),
        &option_pricing_input.pricing_interval_schedule_policy,
    )
    .map_err(
        |source| JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
            error_message: format!(
                "joint option calibration runtime interval schedule is invalid: {source}"
            ),
        },
    )?
    .schedule;

    let last_schedule_end_utc_nanos = *runtime_interval_schedule
        .interval_end_timestamps_utc_nanos
        .last()
        .ok_or_else(
            || JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option calibration runtime interval schedule must contain at least one interval"
                        .to_string(),
            },
        )?;

    let hourly_coverage_start_utc_nanos =
        floor_to_hour_utc_nanos(option_pricing_input.valuation_time_utc_nanos);
    let hourly_coverage_end_utc_nanos = ceil_to_hour_utc_nanos(last_schedule_end_utc_nanos);

    let hourly_coverage_interval_count = ((hourly_coverage_end_utc_nanos
        - hourly_coverage_start_utc_nanos)
        / ONE_HOUR_NANOS) as usize;

    Ok(OptionPricingHorizon {
        runtime_interval_schedule,
        hourly_coverage_start_utc_nanos,
        hourly_coverage_end_utc_nanos,
        hourly_coverage_interval_count,
    })
}

fn build_option_hourly_factor_contract(
    run_spec: &janiform_calibration_artifacts::CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: &crate::stages::joint_refinement::JointRefinementStageExecutionInput,
    return_components_payload: &crate::stages::return_components::ReturnComponentsStageFittedPayload,
    horizon: &OptionPricingHorizon,
) -> Result<
    janiform_market_time::hourly_factors::HourlyFactorContractOutput,
    JointRefinementStageExecutionFailure,
> {
    let runtime_hourly_seasonality_factors =
        build_runtime_hourly_seasonality_factors_from_components(
            calendar_registry,
            run_spec.data_inputs.exchange_code.as_str(),
            stage_input
                .seasonality_factors_input
                .intraday_minutes_per_bin,
            stage_input
                .seasonality_factors_input
                .intraday_factor_by_bin
                .as_slice(),
            stage_input
                .seasonality_factors_input
                .day_of_week_factor
                .as_slice(),
            stage_input
                .seasonality_factors_input
                .day_of_year_factor
                .as_slice(),
            horizon.hourly_coverage_start_utc_nanos,
            horizon.hourly_coverage_end_utc_nanos,
            ONE_HOUR_NANOS,
        )
        .map_err(|error_message| {
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: format!(
                    "joint option calibration runtime seasonality materialization failed: {error_message}"
                ),
            }
        })?;

    let surprise_deterministic_time_multiplier_by_interval_index =
        build_surprise_multipliers_for_interval_grid(
            DateTime::<Utc>::from_timestamp_nanos(horizon.hourly_coverage_start_utc_nanos),
            horizon.hourly_coverage_interval_count,
            ONE_HOUR_NANOS,
            return_components_payload
                .fitted_parameters
                .surprise_deterministic_intensity_profile
                .weekly_hour_utc_multipliers
                .as_slice(),
        );

    load_hourly_factor_contract_for_calibration(
        calendar_registry,
        HourlyFactorProviderRequest {
            exchange_code: run_spec.data_inputs.exchange_code.as_str(),
            range_start_utc: DateTime::<Utc>::from_timestamp_nanos(
                horizon.hourly_coverage_start_utc_nanos,
            ),
            range_end_utc: DateTime::<Utc>::from_timestamp_nanos(
                horizon.hourly_coverage_end_utc_nanos,
            ),
            interval_duration_nanos: ONE_HOUR_NANOS,
            seasonality_source: Some(HourlySeasonalityFactorSource::Explicit {
                hourly_factors: runtime_hourly_seasonality_factors.as_slice(),
                materialization_method:
                    "runtime_compact_seasonality_parameters_mean_renormalized.v1",
            }),
            news_intensity_source: Some(HourlyNewsIntensityFactorSource::Materialized {
                known_scheduled_event_timestamps_utc_nanos: return_components_payload
                    .fitted_parameters
                    .news_process_configuration
                    .arrivals
                    .known
                    .scheduled_event_timestamps_utc_nanos
                    .as_slice(),
                surprise_baseline_intensity: return_components_payload
                    .fitted_parameters
                    .news_process_configuration
                    .arrivals
                    .surprise
                    .baseline_intensity,
                surprise_deterministic_time_multiplier_by_interval_index:
                    surprise_deterministic_time_multiplier_by_interval_index.as_slice(),
            }),
        },
    )
    .map_err(
        |source| JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
            error_message: format!(
                "joint option calibration hourly factor materialization failed: {source}"
            ),
        },
    )
}

fn build_option_boundary_input(
    mean_reversion_payload: &crate::stages::mean_reversion::MeanReversionStageFittedPayload,
    current_state_estimation_output: &crate::current_state::CurrentStateEstimationOutput,
    runtime_interval_schedule: &PricingIntervalSchedule,
) -> Result<AnchoredVanillaOptionPricingBoundaryInput, JointRefinementStageExecutionFailure> {
    AnchoredVanillaOptionPricingBoundaryInput::try_new(
        0,
        current_state_estimation_output.hawkes_excitation_state,
        // Intentional clone so the shared fitted payload remains available for later reporting.
        mean_reversion_payload.fitted_parameters.clone(),
        runtime_interval_schedule.interval_deltas_hours[0],
        // Intentional clone so the state estimate remains available to later pricing steps.
        current_state_estimation_output
            .mean_reversion_boundary_state
            .clone(),
    )
    .map_err(
        |source| JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
            error_message: format!(
                "joint option calibration pricing boundary input is invalid: {source}"
            ),
        },
    )
}

fn build_option_grid_batch_pricing_request(
    spot: f64,
    option_pricing_input: &JointRefinementStageOptionPricingInput,
    runtime_interval_schedule: &PricingIntervalSchedule,
) -> Result<
    (
        VanillaOptionGridBatchPricingRequest,
        Vec<JointRefinementOptionNodeDefinition>,
        Vec<f64>,
        Vec<usize>,
    ),
    JointRefinementStageExecutionFailure,
> {
    let risk_free_rate = option_pricing_input.carry_input.discount_rate;
    let dividend_yield = option_pricing_input.carry_input.discount_rate
        - option_pricing_input.carry_input.carry_rate;

    let mut grids = Vec::new();
    let mut observed_nodes = Vec::new();

    for payoff_type in [VanillaOptionPayoffType::Call, VanillaOptionPayoffType::Put] {
        let payoff_nodes = option_nodes_for_payoff_type(option_pricing_input, payoff_type);
        if payoff_nodes.is_empty() {
            continue;
        }

        for maturity_bucket in
            build_option_bucket_definitions(&payoff_nodes, runtime_interval_schedule)?
        {
            let grid_index = grids.len();

            observed_nodes.extend(build_observed_option_nodes(
                grid_index,
                &maturity_bucket.definition,
                maturity_bucket.nodes.as_slice(),
            )?);

            grids.push(VanillaOptionGridRequest {
                payoff_type: maturity_bucket.definition.payoff_type,
                risk_free_rate,
                dividend_yield,
                strikes: maturity_bucket.definition.strikes,
                maturity_intervals: vec![maturity_bucket.definition.maturity_interval],
            });
        }
    }

    let (snapshot_maturity_intervals, snapshot_maturity_terminal_interval_indices) =
        snapshot_maturity_terminal_mapping(option_pricing_input, runtime_interval_schedule)?;

    Ok((
        VanillaOptionGridBatchPricingRequest { spot, grids },
        observed_nodes,
        snapshot_maturity_intervals,
        snapshot_maturity_terminal_interval_indices,
    ))
}

fn option_nodes_for_payoff_type(
    option_pricing_input: &JointRefinementStageOptionPricingInput,
    payoff_type: VanillaOptionPayoffType,
) -> Vec<&crate::stages::joint_refinement::JointRefinementStageOptionSnapshotNode> {
    option_pricing_input
        .option_snapshot_nodes
        .iter()
        .filter(|node| node.payoff_type == payoff_type)
        .collect::<Vec<_>>()
}

fn build_option_bucket_definitions<'a>(
    payoff_nodes: &[&'a crate::stages::joint_refinement::JointRefinementStageOptionSnapshotNode],
    runtime_interval_schedule: &PricingIntervalSchedule,
) -> Result<Vec<ResolvedOptionMaturityBucket<'a>>, JointRefinementStageExecutionFailure> {
    let mut resolved_nodes = payoff_nodes
        .iter()
        .map(|node| {
            Ok(ResolvedOptionNode {
                node,
                maturity_interval: maturity_interval_hours(
                    node.expiry_timestamp_utc_nanos,
                    runtime_interval_schedule,
                )?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    resolved_nodes.sort_by(|left, right| {
        left.maturity_interval
            .total_cmp(&right.maturity_interval)
            .then_with(|| left.node.strike.total_cmp(&right.node.strike))
    });

    let mut buckets = Vec::<ResolvedOptionMaturityBucket<'a>>::new();

    for resolved_node in resolved_nodes {
        match buckets.last_mut() {
            Some(bucket)
                if bucket
                    .definition
                    .maturity_interval
                    .total_cmp(&resolved_node.maturity_interval)
                    .is_eq() =>
            {
                if bucket
                    .definition
                    .strikes
                    .last()
                    .is_none_or(|strike| !strike.total_cmp(&resolved_node.node.strike).is_eq())
                {
                    bucket.definition.strikes.push(resolved_node.node.strike);
                }
                bucket.nodes.push(resolved_node);
            }
            _ => {
                buckets.push(ResolvedOptionMaturityBucket {
                    definition: OptionBucketDefinition {
                        payoff_type: resolved_node.node.payoff_type,
                        maturity_interval: resolved_node.maturity_interval,
                        strikes: vec![resolved_node.node.strike],
                    },
                    nodes: vec![resolved_node],
                });
            }
        }
    }

    Ok(buckets)
}

fn build_observed_option_nodes(
    grid_index: usize,
    bucket_definition: &OptionBucketDefinition,
    resolved_nodes: &[ResolvedOptionNode<'_>],
) -> Result<Vec<JointRefinementOptionNodeDefinition>, JointRefinementStageExecutionFailure> {
    resolved_nodes
        .iter()
        .map(|resolved_node| {
            let strike_index = locate_option_grid_coordinate(
                &bucket_definition.strikes,
                resolved_node.node.strike,
                "strike",
            )?;

            Ok(JointRefinementOptionNodeDefinition {
                quote_timestamp_utc_nanos: resolved_node.node.quote_timestamp_utc_nanos,
                expiry_timestamp_utc_nanos: resolved_node.node.expiry_timestamp_utc_nanos,
                strike: resolved_node.node.strike,
                payoff_type: resolved_node.node.payoff_type,
                bid_price: resolved_node.node.bid_price,
                ask_price: resolved_node.node.ask_price,
                observed_mid_price: resolved_node.node.observed_mid_price,
                location: JointRefinementOptionNodeLocation {
                    grid_index,
                    maturity_index: 0,
                    strike_index,
                },
            })
        })
        .collect()
}

fn maturity_interval_hours(
    expiry_timestamp_utc_nanos: i64,
    runtime_interval_schedule: &PricingIntervalSchedule,
) -> Result<f64, JointRefinementStageExecutionFailure> {
    let terminal_interval_index = runtime_interval_schedule
        .terminal_interval_index_for_timestamp(expiry_timestamp_utc_nanos)
        .map_err(|source| {
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: format!(
                    "joint option calibration maturity resolution failed: {source}"
                ),
            }
        })?;

    runtime_interval_schedule
        .maturity_interval_hours(terminal_interval_index)
        .map_err(
            |source| JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: format!(
                    "joint option calibration maturity resolution failed: {source}"
                ),
            },
        )
}

fn locate_option_grid_coordinate(
    values: &[f64],
    target: f64,
    axis_label: &str,
) -> Result<usize, JointRefinementStageExecutionFailure> {
    values
        .iter()
        .position(|value| value.total_cmp(&target).is_eq())
        .ok_or_else(
            || JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: format!(
                    "joint option calibration {axis_label} grid indexing failed"
                ),
            },
        )
}

fn run_option_grid_batch_pricing(
    context: &JointRefinementContext,
    fitted_parameters: &janiform_calibration_artifacts::JointRefinementStageFittedParameters,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: Option<JointRefinementStageProgressPhase>,
    completed_units: Option<usize>,
    total_units: Option<usize>,
) -> Result<VanillaOptionGridBatchPricingRunResult, String> {
    let Some(option_pricing_context) = context.option_pricing_context.as_ref() else {
        return Err("joint option pricing context is unavailable".to_string());
    };

    let started_at = std::time::Instant::now();
    let execution_input = VanillaOptionGridBatchPricingExecutionInput {
        request: option_pricing_context.grid_batch_pricing_request.clone(),
        boundary_input: option_pricing_context.boundary_input.clone(),
        model_parameters: VanillaOptionPricingModelParameters::new(
            fitted_parameters.hawkes_fitted_parameters.kernel_parameters,
            fitted_parameters
                .return_components_fitted_parameters
                .news_process_configuration
                .clone(),
            pricing_nig_configuration(fitted_parameters),
        ),
        runtime_interval_schedule: option_pricing_context.runtime_interval_schedule.clone(),
        interval_delta: option_pricing_context.boundary_input.interval_delta,
        interval_factors: option_pricing_context.interval_factors.clone(),
        optional_output_selection: VanillaOptionPricingOptionalOutputSelection {
            include_transform_diagnostics: false,
            include_integration_quality_diagnostics: false,
            include_calibration_hooks: false,
        },
        cpu_count: Some(option_pricing_context.cpu_count),
    };

    let pricing_result = if let (
        Some(progress_sender),
        Some(progress_phase),
        Some(completed_units),
        Some(total_units),
    ) = (
        progress_sender,
        progress_phase,
        completed_units,
        total_units,
    ) {
        let mut pricing_progress_observer = |event: PricingProgressEvent| {
            let _ = progress_sender.send(JointRefinementStageProgressUpdate {
                phase: progress_phase,
                completed_units,
                total_units,
                current_item_label: Some(format_option_pricing_progress_item(&event)),
                valid_completed_units: None,
                cumulative_local_evaluation_count: None,
                current_weighted_objective: None,
                best_weighted_objective: None,
                current_coarse_consistency_fit: None,
                best_coarse_consistency_fit: None,
            });
        };

        run_vanilla_option_grid_batch_pricing_with_observer(
            execution_input,
            Some(&mut pricing_progress_observer),
        )
    } else {
        run_vanilla_option_grid_batch_pricing(execution_input)
    };

    context.profiling_state.record_option_pricing_run_duration(
        started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    );

    pricing_result.map_err(|source| format!("joint option pricing evaluation failed: {source}"))
}

pub(crate) fn option_spread_normalized_price_fit(
    context: &JointRefinementContext,
    fitted_parameters: &janiform_calibration_artifacts::JointRefinementStageFittedParameters,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: Option<JointRefinementStageProgressPhase>,
    completed_units: Option<usize>,
    total_units: Option<usize>,
) -> Result<f64, String> {
    let Some(option_pricing_context) = context.option_pricing_context.as_ref() else {
        return Ok(0.0);
    };

    let pricing_result = run_option_grid_batch_pricing(
        context,
        fitted_parameters,
        progress_sender,
        progress_phase,
        completed_units,
        total_units,
    )?;

    let mut normalized_residuals = Vec::with_capacity(option_pricing_context.observed_nodes.len());

    for observed_node in &option_pricing_context.observed_nodes {
        let model_price = resolve_model_option_price(
            &pricing_result,
            observed_node,
            "joint option pricing evaluation returned a grid inconsistent with the selected option snapshot",
        )?;

        normalized_residuals.push(
            (model_price - observed_node.observed_mid_price)
                / (observed_node.ask_price - observed_node.bid_price),
        );
    }

    Ok(root_mean_square(normalized_residuals.as_slice()))
}

pub(crate) fn build_option_calibration_diagnostics(
    context: &JointRefinementContext,
    fitted_parameters: &janiform_calibration_artifacts::JointRefinementStageFittedParameters,
) -> Result<Option<JointRefinementStageOptionCalibrationDiagnostics>, String> {
    let Some(option_pricing_context) = context.option_pricing_context.as_ref() else {
        return Ok(None);
    };

    let pricing_result =
        run_option_grid_batch_pricing(context, fitted_parameters, None, None, None, None)?;

    let (node_diagnostics, maturity_bucket_residuals, strike_bucket_residuals) =
        build_option_node_diagnostics(&pricing_result, option_pricing_context)?;
    let aggregate_diagnostics =
        build_option_aggregate_diagnostics(maturity_bucket_residuals, strike_bucket_residuals);

    Ok(Some(JointRefinementStageOptionCalibrationDiagnostics {
        snapshot_metadata: JointRefinementStageOptionSnapshotMetadata {
            valuation_time_utc_nanos: option_pricing_context.valuation_time_utc_nanos,
            node_count: node_diagnostics.len(),
            pricing_interval_schedule: option_pricing_context.runtime_interval_schedule.clone(),
            maturity_intervals: option_pricing_context.snapshot_maturity_intervals.clone(),
            maturity_terminal_interval_indices: option_pricing_context
                .snapshot_maturity_terminal_interval_indices
                .clone(),
        },
        carry_input: option_pricing_context.carry_input.clone(),
        node_diagnostics,
        aggregate_diagnostics,
    }))
}

fn format_option_pricing_progress_item(event: &PricingProgressEvent) -> String {
    match event.phase {
        PricingProgressPhase::GroupProgress => {
            format_progress_counts("grid_batch", event.total_units, event.current_group_index)
        }
        PricingProgressPhase::MaturityProgress => {
            let mut detail =
                format_progress_counts("maturity", event.total_units, event.current_maturity_index);
            if let Some(total_maturities) = event.total_maturities {
                detail.push_str(&format!(" of {total_maturities}"));
            }
            detail
        }
        PricingProgressPhase::IntervalProgress => {
            format_progress_counts("interval", event.total_units, event.current_interval_index)
        }
        PricingProgressPhase::TransitionRowProgress => {
            let total_rows = event.total_transition_rows.unwrap_or(event.total_units);
            let mut detail =
                format!("transition_rows {}/{}", event.completed_units.min(total_rows), total_rows);
            if let Some(total_intervals) = event.total_intervals {
                detail.push_str(&format!(
                    " interval {}/{}",
                    display_position(event.current_interval_index, event.completed_units),
                    total_intervals
                ));
            }
            detail
        }
        PricingProgressPhase::StrikePricingProgress => {
            let strike_index = display_position(event.current_strike_index, event.completed_units);
            let total_strikes = event.total_strikes.unwrap_or(event.total_units);
            let mut detail = format!("strike {strike_index}/{total_strikes}");
            if let Some(total_maturities) = event.total_maturities {
                detail.push_str(&format!(
                    " maturity {}/{}",
                    display_position(event.current_maturity_index, event.completed_units),
                    total_maturities
                ));
            }
            detail
        }
    }
}

fn format_progress_counts(
    label: &str,
    total_units: usize,
    current_index: Option<usize>,
) -> String {
    let current_position = current_index.map_or(0, |index| index.saturating_add(1));
    format!("{label} {current_position}/{total_units}")
}

fn display_position(index: Option<usize>, completed_units: usize) -> usize {
    index.map_or(completed_units, |value| value.saturating_add(1))
}

fn snapshot_maturity_terminal_mapping(
    option_pricing_input: &JointRefinementStageOptionPricingInput,
    runtime_interval_schedule: &PricingIntervalSchedule,
) -> Result<(Vec<f64>, Vec<usize>), JointRefinementStageExecutionFailure> {
    let mut maturity_terminal_pairs = option_pricing_input
        .option_snapshot_nodes
        .iter()
        .map(|node| {
            let terminal_interval_index = runtime_interval_schedule
                .terminal_interval_index_for_timestamp(node.expiry_timestamp_utc_nanos)
                .map_err(|source| {
                    JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                        error_message: format!(
                            "joint option calibration maturity mapping failed: {source}"
                        ),
                    }
                })?;
            let maturity_interval = runtime_interval_schedule
                .maturity_interval_hours(terminal_interval_index)
                .map_err(|source| {
                    JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                        error_message: format!(
                            "joint option calibration maturity mapping failed: {source}"
                        ),
                    }
                })?;
            Ok((maturity_interval, terminal_interval_index))
        })
        .collect::<Result<Vec<_>, _>>()?;

    maturity_terminal_pairs.sort_by(|left, right| left.0.total_cmp(&right.0));
    maturity_terminal_pairs.dedup_by(|left, right| left.0.total_cmp(&right.0).is_eq());

    Ok(maturity_terminal_pairs.into_iter().unzip())
}

type ResidualBuckets = BTreeMap<String, Vec<f64>>;

fn build_option_node_diagnostics(
    pricing_result: &VanillaOptionGridBatchPricingRunResult,
    option_pricing_context: &JointRefinementOptionPricingContext,
) -> Result<
    (
        Vec<JointRefinementStageOptionNodeDiagnostic>,
        ResidualBuckets,
        ResidualBuckets,
    ),
    String,
> {
    let mut node_diagnostics = Vec::with_capacity(option_pricing_context.observed_nodes.len());
    let mut maturity_bucket_residuals = BTreeMap::<String, Vec<f64>>::new();
    let mut strike_bucket_residuals = BTreeMap::<String, Vec<f64>>::new();

    for observed_node in &option_pricing_context.observed_nodes {
        let model_price = resolve_model_option_price(
            pricing_result,
            observed_node,
            "joint option pricing diagnostics returned a grid inconsistent with the selected option snapshot",
        )?;
        let spread_normalized_residual = (model_price - observed_node.observed_mid_price)
            / (observed_node.ask_price - observed_node.bid_price);

        node_diagnostics.push(JointRefinementStageOptionNodeDiagnostic {
            quote_timestamp_utc_nanos: observed_node.quote_timestamp_utc_nanos,
            expiry_timestamp_utc_nanos: observed_node.expiry_timestamp_utc_nanos,
            strike: observed_node.strike,
            option_type: match observed_node.payoff_type {
                VanillaOptionPayoffType::Call => "call".to_string(),
                VanillaOptionPayoffType::Put => "put".to_string(),
            },
            bid_price: observed_node.bid_price,
            ask_price: observed_node.ask_price,
            observed_mid_price: observed_node.observed_mid_price,
            model_price,
            spread_normalized_residual,
        });

        maturity_bucket_residuals
            .entry(format!(
                "{:.0}",
                (observed_node.expiry_timestamp_utc_nanos
                    - option_pricing_context.valuation_time_utc_nanos) as f64
                    / ONE_HOUR_NANOS as f64
            ))
            .or_default()
            .push(spread_normalized_residual);

        strike_bucket_residuals
            .entry(format!("{}", observed_node.strike))
            .or_default()
            .push(spread_normalized_residual);
    }

    Ok((
        node_diagnostics,
        maturity_bucket_residuals,
        strike_bucket_residuals,
    ))
}

fn build_option_aggregate_diagnostics(
    maturity_bucket_residuals: ResidualBuckets,
    strike_bucket_residuals: ResidualBuckets,
) -> Vec<JointRefinementStageOptionAggregateDiagnostic> {
    let mut aggregate_diagnostics = build_option_bucket_aggregate_diagnostics(
        "maturity_interval_hours",
        maturity_bucket_residuals,
    );

    aggregate_diagnostics.extend(build_option_bucket_aggregate_diagnostics(
        "strike",
        strike_bucket_residuals,
    ));

    aggregate_diagnostics
}

fn build_option_bucket_aggregate_diagnostics(
    bucket_kind: &str,
    residuals_by_bucket: ResidualBuckets,
) -> Vec<JointRefinementStageOptionAggregateDiagnostic> {
    residuals_by_bucket
        .into_iter()
        .map(
            |(bucket_label, residuals)| JointRefinementStageOptionAggregateDiagnostic {
                bucket_kind: bucket_kind.to_string(),
                bucket_label,
                node_count: residuals.len(),
                mean_absolute_spread_normalized_residual: residuals
                    .iter()
                    .map(|residual| residual.abs())
                    .sum::<f64>()
                    / residuals.len().max(1) as f64,
                root_mean_square_spread_normalized_residual: root_mean_square(residuals.as_slice()),
            },
        )
        .collect()
}

fn resolve_model_option_price(
    pricing_result: &VanillaOptionGridBatchPricingRunResult,
    observed_node: &JointRefinementOptionNodeDefinition,
    error_message: &str,
) -> Result<f64, String> {
    let grid = pricing_result
        .grids
        .get(observed_node.location.grid_index)
        .ok_or_else(|| error_message.to_string())?;

    let model_price = grid
        .price_grid
        .get(observed_node.location.maturity_index)
        .and_then(|row| row.get(observed_node.location.strike_index))
        .copied()
        .ok_or_else(|| error_message.to_string())?;

    if let Some(model_price) = model_price {
        return Ok(model_price);
    }

    let failure_stage = grid
        .pricing_failure_stage_grid
        .get(observed_node.location.maturity_index)
        .and_then(|row| row.get(observed_node.location.strike_index))
        .and_then(|stage| *stage)
        .map(|stage| format!("{stage:?}"))
        .unwrap_or_else(|| "unknown".to_string());
    let failure_reason = grid
        .pricing_failure_reason_grid
        .get(observed_node.location.maturity_index)
        .and_then(|row| row.get(observed_node.location.strike_index))
        .and_then(|reason| reason.as_ref())
        .cloned()
        .unwrap_or_else(|| "missing failure reason".to_string());

    Err(format!(
        "{error_message}: node pricing failed at stage={failure_stage} reason={failure_reason}"
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use janiform_calibration_artifacts::JointRefinementStageCarryInput;
    use janiform_calibration_artifacts::{
        CalibrationRunSpec, HawkesStageFittedParameters, MeanReversionStageDiagnostics,
        ReturnComponentsStageFittedParameters,
    };
    use janiform_engine_pricing::{
        AnchoredVanillaOptionPricingBoundaryInput, PricingIntervalSchedule,
        PricingIntervalSchedulePolicy, VanillaOptionGridBatchPricingExecutionInput,
        VanillaOptionGridRequest, VanillaOptionPayoffType, VanillaOptionPricingModelParameters,
        VanillaOptionPricingOptionalOutputSelection, materialize_schedule_interval_factors,
        run_vanilla_option_grid_batch_pricing,
    };
    use janiform_foundations::time::ONE_HOUR_NANOS;
    use janiform_market_time::calendar::ExchangeCalendarRegistry;
    use janiform_market_time::calendar::ExchangeCalendarRegistryLoader;
    use janiform_model_kernels::hawkes::HawkesEventSide;
    use serde::Deserialize;
    use serde_json::Value;

    use super::{
        build_option_boundary_input, build_option_grid_batch_pricing_request,
        build_surprise_multipliers_for_interval_grid, derive_pricing_interval_schedule,
        estimate_option_pricing_current_state, pricing_nig_configuration, validate_option_spot,
    };
    use crate::adapters::load_hourly_factor_contract_for_calibration;
    use crate::stages::hawkes::HawkesStageFittedPayload;
    use crate::stages::joint_refinement::contracts::{
        JointRefinementStageCurrentStateEstimationInput, JointRefinementStageOptionPricingInput,
        JointRefinementStageOptionSnapshotNode, JointRefinementStageSeasonalityFactorsInput,
    };
    use crate::stages::mean_reversion::MeanReversionStageFittedPayload;
    use crate::stages::return_components::ReturnComponentsStageFittedPayload;
    use crate::stages::seasonality::build_runtime_hourly_seasonality_factors_from_components;
    use janiform_calibration_artifacts::CalibrationStageHandoffTransport;
    use janiform_market_time::hourly_factors::{
        HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource, HourlySeasonalityFactorSource,
    };

    fn sample_option_pricing_input() -> JointRefinementStageOptionPricingInput {
        let valuation_time_utc_nanos = 0;
        let first_expiry_utc_nanos = ONE_HOUR_NANOS;
        let second_expiry_utc_nanos = 2 * ONE_HOUR_NANOS;
        let third_expiry_utc_nanos = 3 * ONE_HOUR_NANOS;

        JointRefinementStageOptionPricingInput {
            valuation_time_utc_nanos,
            pricing_interval_schedule_policy: PricingIntervalSchedulePolicy::AutoExactSparse,
            carry_input: JointRefinementStageCarryInput {
                discount_rate: 0.01,
                carry_rate: 0.005,
            },
            current_state_estimation_input: JointRefinementStageCurrentStateEstimationInput {
                state_time_utc_nanos: valuation_time_utc_nanos,
                observed_spot: 100.0,
                observed_log_price: 100.0_f64.ln(),
                observation_timestamp_utc_nanos: vec![valuation_time_utc_nanos],
                observation_trade_prices: vec![100.0],
                observation_trade_sides: vec![HawkesEventSide::Buy],
            },
            option_snapshot_nodes: vec![
                JointRefinementStageOptionSnapshotNode {
                    quote_timestamp_utc_nanos: valuation_time_utc_nanos,
                    expiry_timestamp_utc_nanos: first_expiry_utc_nanos,
                    strike: 100.0,
                    payoff_type: VanillaOptionPayoffType::Call,
                    bid_price: 1.0,
                    ask_price: 1.5,
                    observed_mid_price: 1.25,
                },
                JointRefinementStageOptionSnapshotNode {
                    quote_timestamp_utc_nanos: valuation_time_utc_nanos,
                    expiry_timestamp_utc_nanos: first_expiry_utc_nanos,
                    strike: 110.0,
                    payoff_type: VanillaOptionPayoffType::Call,
                    bid_price: 1.1,
                    ask_price: 1.6,
                    observed_mid_price: 1.35,
                },
                JointRefinementStageOptionSnapshotNode {
                    quote_timestamp_utc_nanos: valuation_time_utc_nanos,
                    expiry_timestamp_utc_nanos: second_expiry_utc_nanos,
                    strike: 100.0,
                    payoff_type: VanillaOptionPayoffType::Call,
                    bid_price: 0.9,
                    ask_price: 1.4,
                    observed_mid_price: 1.15,
                },
                JointRefinementStageOptionSnapshotNode {
                    quote_timestamp_utc_nanos: valuation_time_utc_nanos,
                    expiry_timestamp_utc_nanos: second_expiry_utc_nanos,
                    strike: 110.0,
                    payoff_type: VanillaOptionPayoffType::Call,
                    bid_price: 0.8,
                    ask_price: 1.3,
                    observed_mid_price: 1.05,
                },
                JointRefinementStageOptionSnapshotNode {
                    quote_timestamp_utc_nanos: valuation_time_utc_nanos,
                    expiry_timestamp_utc_nanos: third_expiry_utc_nanos,
                    strike: 95.0,
                    payoff_type: VanillaOptionPayoffType::Put,
                    bid_price: 1.2,
                    ask_price: 1.8,
                    observed_mid_price: 1.5,
                },
            ],
        }
    }

    fn sample_option_runtime_interval_schedule(
        option_pricing_input: &JointRefinementStageOptionPricingInput,
    ) -> PricingIntervalSchedule {
        derive_pricing_interval_schedule(
            option_pricing_input.valuation_time_utc_nanos,
            &option_pricing_input
                .option_snapshot_nodes
                .iter()
                .map(|node| node.expiry_timestamp_utc_nanos)
                .collect::<Vec<_>>(),
            &option_pricing_input.pricing_interval_schedule_policy,
        )
        .expect("sparse interval schedule should be valid")
        .schedule
    }

    #[test]
    fn option_grid_batch_pricing_request_uses_sparse_observed_maturity_buckets() {
        let option_pricing_input = sample_option_pricing_input();
        let runtime_interval_schedule =
            sample_option_runtime_interval_schedule(&option_pricing_input);

        let (request, observed_nodes, _, _) = build_option_grid_batch_pricing_request(
            100.0,
            &option_pricing_input,
            &runtime_interval_schedule,
        )
        .expect("sparse observed-node pricing request should build");

        assert_eq!(request.grids.len(), 3);
        assert_eq!(
            request.grids[0],
            VanillaOptionGridRequest {
                payoff_type: VanillaOptionPayoffType::Call,
                risk_free_rate: 0.01,
                dividend_yield: 0.005,
                strikes: vec![100.0, 110.0],
                maturity_intervals: vec![1.0],
            }
        );
        assert_eq!(
            request.grids[1],
            VanillaOptionGridRequest {
                payoff_type: VanillaOptionPayoffType::Call,
                risk_free_rate: 0.01,
                dividend_yield: 0.005,
                strikes: vec![100.0, 110.0],
                maturity_intervals: vec![2.0],
            }
        );
        assert_eq!(
            request.grids[2],
            VanillaOptionGridRequest {
                payoff_type: VanillaOptionPayoffType::Put,
                risk_free_rate: 0.01,
                dividend_yield: 0.005,
                strikes: vec![95.0],
                maturity_intervals: vec![3.0],
            }
        );

        assert_eq!(observed_nodes.len(), 5);
        assert_eq!(observed_nodes[0].location.grid_index, 0);
        assert_eq!(observed_nodes[0].location.maturity_index, 0);
        assert_eq!(observed_nodes[0].location.strike_index, 0);
        assert_eq!(observed_nodes[1].location.grid_index, 0);
        assert_eq!(observed_nodes[1].location.maturity_index, 0);
        assert_eq!(observed_nodes[1].location.strike_index, 1);
        assert_eq!(observed_nodes[2].location.grid_index, 1);
        assert_eq!(observed_nodes[2].location.maturity_index, 0);
        assert_eq!(observed_nodes[2].location.strike_index, 0);
        assert_eq!(observed_nodes[3].location.grid_index, 1);
        assert_eq!(observed_nodes[3].location.maturity_index, 0);
        assert_eq!(observed_nodes[3].location.strike_index, 1);
        assert_eq!(observed_nodes[4].location.grid_index, 2);
        assert_eq!(observed_nodes[4].location.maturity_index, 0);
        assert_eq!(observed_nodes[4].location.strike_index, 0);
    }

    #[derive(Debug, Deserialize)]
    struct SeasonalityFactorsFixture {
        intraday_minutes_per_bin: u16,
        intraday_factor_by_bin: Vec<f64>,
        day_of_week_factor: Vec<f64>,
        day_of_year_factor: Vec<f64>,
        timestamp_utc_nanos: Vec<i64>,
        normalized_combined_factor_by_event: Vec<f64>,
    }

    #[derive(Debug, Deserialize)]
    struct CurrentStateEstimationInputFixture {
        state_time_utc_nanos: i64,
        observed_spot: f64,
        observed_log_price: f64,
        observation_timestamp_utc_nanos: Vec<i64>,
        observation_trade_prices: Vec<f64>,
        observation_trade_sides: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    struct OptionSnapshotNodeFixture {
        quote_timestamp_utc_nanos: i64,
        expiry_timestamp_utc_nanos: i64,
        strike: f64,
        payoff_type: String,
        bid_price: f64,
        ask_price: f64,
        observed_mid_price: f64,
    }

    #[derive(Debug, Deserialize)]
    struct OptionPricingInputFixture {
        valuation_time_utc_nanos: i64,
        pricing_interval_schedule_policy: PricingIntervalSchedulePolicy,
        carry_input: JointRefinementStageCarryInput,
        current_state_estimation_input: CurrentStateEstimationInputFixture,
        option_snapshot_nodes: Vec<OptionSnapshotNodeFixture>,
    }

    fn debug_xcme_calendar_registry() -> ExchangeCalendarRegistry {
        ExchangeCalendarRegistryLoader::load_from_json(
            r#"
{
  "schema_version": "calendar.registry.v1",
  "artifact_date": "2026-04-11",
  "exchanges": [
    {
      "schema_version": "calendar.v1",
      "exchange_code": "XCME",
      "exchange_name": "CME Globex Futures",
      "timezone_name": "America/Chicago",
      "rule_metadata": {
        "holiday_calendar_code": "NONE",
        "early_close_rule_set": "NONE"
      },
      "regular_sessions": [
        {
          "weekday": "Monday",
          "windows": [
            {"label": "globex", "open_time": "00:00", "close_time": "16:00"},
            {"label": "globex", "open_time": "17:00", "close_time": "24:00"}
          ]
        },
        {
          "weekday": "Tuesday",
          "windows": [
            {"label": "globex", "open_time": "00:00", "close_time": "16:00"},
            {"label": "globex", "open_time": "17:00", "close_time": "24:00"}
          ]
        },
        {
          "weekday": "Wednesday",
          "windows": [
            {"label": "globex", "open_time": "00:00", "close_time": "16:00"},
            {"label": "globex", "open_time": "17:00", "close_time": "24:00"}
          ]
        },
        {
          "weekday": "Thursday",
          "windows": [
            {"label": "globex", "open_time": "00:00", "close_time": "16:00"},
            {"label": "globex", "open_time": "17:00", "close_time": "24:00"}
          ]
        },
        {
          "weekday": "Friday",
          "windows": [
            {"label": "globex", "open_time": "00:00", "close_time": "16:00"}
          ]
        },
        {
          "weekday": "Sunday",
          "windows": [
            {"label": "globex", "open_time": "17:00", "close_time": "24:00"}
          ]
        }
      ],
      "holiday_dates": [],
      "early_close_sessions": []
    }
  ]
}
"#,
        )
        .expect("XCME calendar registry fixture should load")
    }

    #[test]
    #[ignore = "debug-only ES fixture reproduction"]
    fn debug_es_joint_option_pricing_fixture_reports_local_measure_inputs() {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let prepared_payload_path = workspace_root.join(
            "scratch/prepared_joint_failure_debug/es_continuous_20260411T205815Z.prepared_calibration_payload.json",
        );
        let stage_payloads_path =
            workspace_root.join("scratch/joint_failure_stage_report_payloads.json");

        let prepared_payload: Value = serde_json::from_str(
            &fs::read_to_string(prepared_payload_path)
                .expect("prepared payload fixture should load"),
        )
        .expect("prepared payload fixture should deserialize");
        let stage_payloads: Vec<Value> = serde_json::from_str(
            &fs::read_to_string(stage_payloads_path).expect("stage payload fixture should load"),
        )
        .expect("stage payload fixture should deserialize");

        let run_spec: CalibrationRunSpec =
            serde_json::from_value(prepared_payload["request"].clone())
                .expect("run spec should deserialize");

        let seasonality_fixture: SeasonalityFactorsFixture = serde_json::from_value(
            stage_payloads
                .iter()
                .find(|payload| payload["stage_report_kind"] == "seasonality")
                .expect("seasonality stage payload should exist")["normalized_stage_payload"]
                .clone(),
        )
        .expect("seasonality factors should deserialize");

        let option_pricing_fixture: OptionPricingInputFixture = serde_json::from_value(
            prepared_payload["joint_refinement_option_pricing_input"].clone(),
        )
        .expect("joint option pricing input should deserialize");

        let hawkes_fitted_parameters: HawkesStageFittedParameters = serde_json::from_value(
            stage_payloads
                .iter()
                .find(|payload| payload["stage_report_kind"] == "hawkes")
                .expect("hawkes stage payload should exist")["fitted_stage_payload"]
                ["fitted_parameters"]
                .clone(),
        )
        .expect("hawkes fitted parameters should deserialize");
        let return_components_fitted_parameters: ReturnComponentsStageFittedParameters =
            serde_json::from_value(
                stage_payloads
                    .iter()
                    .find(|payload| payload["stage_report_kind"] == "return_components")
                    .expect("return-components stage payload should exist")["fitted_stage_payload"]
                    ["fitted_parameters"]
                    .clone(),
            )
            .expect("return-components fitted parameters should deserialize");
        let mean_reversion_fitted_parameters = serde_json::from_value(
            stage_payloads
                .iter()
                .find(|payload| payload["stage_report_kind"] == "mean_reversion")
                .expect("mean-reversion stage payload should exist")["fitted_stage_payload"]
                ["fitted_parameters"]
                .clone(),
        )
        .expect("mean-reversion fitted parameters should deserialize");
        let mean_reversion_diagnostics: MeanReversionStageDiagnostics = serde_json::from_value(
            stage_payloads
                .iter()
                .find(|payload| payload["stage_report_kind"] == "mean_reversion")
                .expect("mean-reversion stage payload should exist")["fitted_stage_payload"]
                ["diagnostics"]
                .clone(),
        )
        .expect("mean-reversion diagnostics should deserialize");

        let seasonality_factors_input = JointRefinementStageSeasonalityFactorsInput {
            intraday_minutes_per_bin: seasonality_fixture.intraday_minutes_per_bin,
            intraday_factor_by_bin: seasonality_fixture.intraday_factor_by_bin,
            day_of_week_factor: seasonality_fixture.day_of_week_factor,
            day_of_year_factor: seasonality_fixture.day_of_year_factor,
            timestamp_utc_nanos: seasonality_fixture.timestamp_utc_nanos,
            normalized_combined_factor_by_event: seasonality_fixture
                .normalized_combined_factor_by_event,
        };
        let option_pricing_input = JointRefinementStageOptionPricingInput {
            valuation_time_utc_nanos: option_pricing_fixture.valuation_time_utc_nanos,
            pricing_interval_schedule_policy: option_pricing_fixture
                .pricing_interval_schedule_policy,
            carry_input: option_pricing_fixture.carry_input,
            current_state_estimation_input: JointRefinementStageCurrentStateEstimationInput {
                state_time_utc_nanos: option_pricing_fixture
                    .current_state_estimation_input
                    .state_time_utc_nanos,
                observed_spot: option_pricing_fixture
                    .current_state_estimation_input
                    .observed_spot,
                observed_log_price: option_pricing_fixture
                    .current_state_estimation_input
                    .observed_log_price,
                observation_timestamp_utc_nanos: option_pricing_fixture
                    .current_state_estimation_input
                    .observation_timestamp_utc_nanos,
                observation_trade_prices: option_pricing_fixture
                    .current_state_estimation_input
                    .observation_trade_prices,
                observation_trade_sides: option_pricing_fixture
                    .current_state_estimation_input
                    .observation_trade_sides
                    .into_iter()
                    .map(|side| match side.as_str() {
                        "buy" => HawkesEventSide::Buy,
                        "sell" => HawkesEventSide::Sell,
                        other => panic!("unsupported event side fixture: {other}"),
                    })
                    .collect(),
            },
            option_snapshot_nodes: option_pricing_fixture
                .option_snapshot_nodes
                .into_iter()
                .map(|node| JointRefinementStageOptionSnapshotNode {
                    quote_timestamp_utc_nanos: node.quote_timestamp_utc_nanos,
                    expiry_timestamp_utc_nanos: node.expiry_timestamp_utc_nanos,
                    strike: node.strike,
                    payoff_type: match node.payoff_type.as_str() {
                        "call" => VanillaOptionPayoffType::Call,
                        "put" => VanillaOptionPayoffType::Put,
                        other => panic!("unsupported payoff type fixture: {other}"),
                    },
                    bid_price: node.bid_price,
                    ask_price: node.ask_price,
                    observed_mid_price: node.observed_mid_price,
                })
                .collect(),
        };
        let hawkes_payload = HawkesStageFittedPayload {
            schema_version: "debug_fixture",
            selected_candidate_id: "debug_fixture".to_string(),
            fitted_parameters: hawkes_fitted_parameters.clone(),
            candidate_diagnostics: Vec::new(),
            seasonality_input_transport: CalibrationStageHandoffTransport::InMemoryOutput,
            seasonality_dataset_id: None,
            seasonality_artifact_reference: None,
        };
        let return_components_payload = ReturnComponentsStageFittedPayload {
            schema_version: "debug_fixture",
            selected_candidate_id: "debug_fixture".to_string(),
            fitted_parameters: return_components_fitted_parameters.clone(),
            candidate_diagnostics: Vec::new(),
            candidate_generation_policy: "debug_fixture",
            objective_weighting_policy: "debug_fixture",
            objective_metric_bucket_policy: "debug_fixture",
            candidate_family_counts:
                crate::stages::return_components::ReturnComponentsStageCandidateFamilyCounts {
                    nig_candidate_count: 1,
                    known_news_arrival_family_count: 1,
                    surprise_news_arrival_family_count: 1,
                    return_jump_mark_scale_family_count: 1,
                    activity_coupling_family_count: 1,
                },
            activity_input_transport: CalibrationStageHandoffTransport::InMemoryOutput,
            activity_selected_hawkes_candidate_id: None,
            activity_artifact_reference: None,
        };
        let mean_reversion_payload = MeanReversionStageFittedPayload {
            schema_version: "debug_fixture",
            fitted_parameters: mean_reversion_fitted_parameters,
            diagnostics: mean_reversion_diagnostics,
            return_components_input_transport: CalibrationStageHandoffTransport::InMemoryOutput,
            return_components_selected_candidate_id: None,
            return_components_artifact_reference: None,
        };

        let horizon = derive_pricing_interval_schedule(
            option_pricing_input.valuation_time_utc_nanos,
            &option_pricing_input
                .option_snapshot_nodes
                .iter()
                .map(|node| node.expiry_timestamp_utc_nanos)
                .collect::<Vec<_>>(),
            &option_pricing_input.pricing_interval_schedule_policy,
        )
        .expect("runtime interval schedule should be valid")
        .schedule;
        let current_state_estimation_output = estimate_option_pricing_current_state(
            &option_pricing_input,
            &hawkes_payload,
            &mean_reversion_payload,
        )
        .expect("current-state estimation should succeed");
        let hourly_coverage_start_utc_nanos = janiform_foundations::time::floor_to_hour_utc_nanos(
            option_pricing_input.valuation_time_utc_nanos,
        );
        let hourly_coverage_end_utc_nanos = janiform_foundations::time::ceil_to_hour_utc_nanos(
            *horizon
                .interval_end_timestamps_utc_nanos
                .last()
                .expect("schedule should contain intervals"),
        );
        let calendar_registry = debug_xcme_calendar_registry();
        let runtime_hourly_seasonality_factors =
            build_runtime_hourly_seasonality_factors_from_components(
                &calendar_registry,
                run_spec.data_inputs.exchange_code.as_str(),
                seasonality_factors_input.intraday_minutes_per_bin,
                seasonality_factors_input.intraday_factor_by_bin.as_slice(),
                seasonality_factors_input.day_of_week_factor.as_slice(),
                seasonality_factors_input.day_of_year_factor.as_slice(),
                hourly_coverage_start_utc_nanos,
                hourly_coverage_end_utc_nanos,
                ONE_HOUR_NANOS,
            )
            .expect("runtime seasonality materialization should succeed");
        let hourly_coverage_interval_count = ((hourly_coverage_end_utc_nanos
            - hourly_coverage_start_utc_nanos)
            / ONE_HOUR_NANOS) as usize;
        let surprise_deterministic_time_multiplier_by_interval_index =
            build_surprise_multipliers_for_interval_grid(
                chrono::DateTime::<chrono::Utc>::from_timestamp_nanos(
                    hourly_coverage_start_utc_nanos,
                ),
                hourly_coverage_interval_count,
                ONE_HOUR_NANOS,
                return_components_payload
                    .fitted_parameters
                    .surprise_deterministic_intensity_profile
                    .weekly_hour_utc_multipliers
                    .as_slice(),
            );
        let hourly_factor_contract = load_hourly_factor_contract_for_calibration(
            &calendar_registry,
            HourlyFactorProviderRequest {
                exchange_code: run_spec.data_inputs.exchange_code.as_str(),
                range_start_utc: chrono::DateTime::<chrono::Utc>::from_timestamp_nanos(
                    hourly_coverage_start_utc_nanos,
                ),
                range_end_utc: chrono::DateTime::<chrono::Utc>::from_timestamp_nanos(
                    hourly_coverage_end_utc_nanos,
                ),
                interval_duration_nanos: ONE_HOUR_NANOS,
                seasonality_source: Some(HourlySeasonalityFactorSource::Explicit {
                    hourly_factors: runtime_hourly_seasonality_factors.as_slice(),
                    materialization_method:
                        "runtime_compact_seasonality_parameters_mean_renormalized.v1",
                }),
                news_intensity_source: Some(HourlyNewsIntensityFactorSource::Materialized {
                    known_scheduled_event_timestamps_utc_nanos: return_components_payload
                        .fitted_parameters
                        .news_process_configuration
                        .arrivals
                        .known
                        .scheduled_event_timestamps_utc_nanos
                        .as_slice(),
                    surprise_baseline_intensity: return_components_payload
                        .fitted_parameters
                        .news_process_configuration
                        .arrivals
                        .surprise
                        .baseline_intensity,
                    surprise_deterministic_time_multiplier_by_interval_index:
                        surprise_deterministic_time_multiplier_by_interval_index.as_slice(),
                }),
            },
        )
        .expect("hourly factor contract should materialize");
        let interval_factors =
            materialize_schedule_interval_factors(&horizon, &hourly_factor_contract)
                .expect("interval factors should materialize");
        let zero_seasonality_interval_count = interval_factors
            .iter()
            .filter(|factors| factors.s_h == 0.0)
            .count();
        let option_spot =
            validate_option_spot(&option_pricing_input).expect("option spot should validate");
        let boundary_input: AnchoredVanillaOptionPricingBoundaryInput =
            build_option_boundary_input(
                &mean_reversion_payload,
                &current_state_estimation_output,
                &horizon,
            )
            .expect("boundary input should build");
        let local_measure_uses_nested = boundary_input
            .local_martingale_deviation_adjustment()
            .expect("boundary input should yield local martingale deviation")
            == 0.0;
        let (grid_batch_pricing_request, _, _, _) =
            build_option_grid_batch_pricing_request(option_spot, &option_pricing_input, &horizon)
                .expect("grid batch pricing request should build");

        let execution_input = VanillaOptionGridBatchPricingExecutionInput {
            request: grid_batch_pricing_request,
            boundary_input: boundary_input.clone(),
            model_parameters: VanillaOptionPricingModelParameters::new(
                hawkes_fitted_parameters.kernel_parameters,
                return_components_fitted_parameters
                    .news_process_configuration
                    .clone(),
                pricing_nig_configuration(
                    &janiform_calibration_artifacts::JointRefinementStageFittedParameters {
                        hawkes_fitted_parameters,
                        return_components_fitted_parameters,
                        pricing_block_fitted_parameters: None,
                    },
                ),
            ),
            runtime_interval_schedule: horizon,
            interval_delta: boundary_input.interval_delta,
            interval_factors,
            optional_output_selection: VanillaOptionPricingOptionalOutputSelection {
                include_transform_diagnostics: false,
                include_integration_quality_diagnostics: false,
                include_calibration_hooks: false,
            },
            cpu_count: Some(1),
        };

        let expected_severity_mark = execution_input
            .model_parameters
            .news_process_configuration
            .shared_mark_law
            .gamma_severity_marks
            .expected_severity_mark()
            .expect("expected severity mark should exist");
        let known_activity_coupling_scale = execution_input
            .model_parameters
            .news_process_configuration
            .known_component_mark_scales
            .activity_coupling_scale;
        let surprise_activity_coupling_scale = execution_input
            .model_parameters
            .news_process_configuration
            .surprise_component_mark_scales
            .activity_coupling_scale;

        let mut first_nonfinite_loading = None;
        let mut max_closed_form_loading = f64::NEG_INFINITY;
        let mut max_closed_form_loading_interval_index = 0usize;
        let mut max_closed_form_loading_delta = 0.0;
        let mut max_closed_form_loading_known = 0.0;
        let mut max_closed_form_loading_surprise = 0.0;

        for (interval_index, interval_factor) in execution_input.interval_factors.iter().enumerate()
        {
            let interval_delta = execution_input
                .runtime_interval_schedule
                .interval_delta_hours(interval_index)
                .expect("runtime interval delta should exist");
            let expected_known_loading = interval_factor.nu_h_known
                * interval_delta
                * known_activity_coupling_scale
                * expected_severity_mark;
            let expected_surprise_loading = interval_factor.nu_h_surprise
                * interval_delta
                * surprise_activity_coupling_scale
                * expected_severity_mark;
            let total_expected_loading = expected_known_loading + expected_surprise_loading;

            if !expected_known_loading.is_finite()
                || !expected_surprise_loading.is_finite()
                || !total_expected_loading.is_finite()
            {
                first_nonfinite_loading = Some((
                    interval_index,
                    interval_delta,
                    interval_factor.nu_h_known,
                    interval_factor.nu_h_surprise,
                    expected_known_loading,
                    expected_surprise_loading,
                    total_expected_loading,
                ));
                break;
            }

            if total_expected_loading > max_closed_form_loading {
                max_closed_form_loading = total_expected_loading;
                max_closed_form_loading_interval_index = interval_index;
                max_closed_form_loading_delta = interval_delta;
                max_closed_form_loading_known = expected_known_loading;
                max_closed_form_loading_surprise = expected_surprise_loading;
            }
        }

        let mut first_failed_node = None;

        for (node_index, snapshot_node) in option_pricing_input
            .option_snapshot_nodes
            .iter()
            .cloned()
            .enumerate()
        {
            let single_node_input = JointRefinementStageOptionPricingInput {
                valuation_time_utc_nanos: option_pricing_input.valuation_time_utc_nanos,
                pricing_interval_schedule_policy: option_pricing_input
                    .pricing_interval_schedule_policy
                    .clone(),
                carry_input: option_pricing_input.carry_input.clone(),
                current_state_estimation_input: option_pricing_input
                    .current_state_estimation_input
                    .clone(),
                option_snapshot_nodes: vec![snapshot_node.clone()],
            };

            let (single_node_request, _, _, _) = build_option_grid_batch_pricing_request(
                option_spot,
                &single_node_input,
                &execution_input.runtime_interval_schedule,
            )
            .expect("single-node pricing request should build");

            let single_node_execution_input = VanillaOptionGridBatchPricingExecutionInput {
                request: single_node_request,
                boundary_input: execution_input.boundary_input.clone(),
                model_parameters: execution_input.model_parameters.clone(),
                runtime_interval_schedule: execution_input.runtime_interval_schedule.clone(),
                interval_delta: execution_input.interval_delta,
                interval_factors: execution_input.interval_factors.clone(),
                optional_output_selection: execution_input.optional_output_selection,
                cpu_count: Some(1),
            };

            let single_node_result =
                run_vanilla_option_grid_batch_pricing(single_node_execution_input)
                    .expect("single-node pricing run should complete");

            let grid = single_node_result
                .grids
                .first()
                .expect("single-node run should return one grid");
            let price = grid
                .price_grid
                .first()
                .and_then(|row| row.first())
                .copied()
                .flatten();

            if price.is_none() {
                first_failed_node = Some((
                    node_index,
                    snapshot_node.payoff_type,
                    snapshot_node.strike,
                    snapshot_node.expiry_timestamp_utc_nanos,
                    grid.pricing_failure_stage_grid
                        .first()
                        .and_then(|row| row.first())
                        .and_then(|stage| *stage),
                    grid.pricing_failure_reason_grid
                        .first()
                        .and_then(|row| row.first())
                        .and_then(|reason| reason.clone()),
                ));
                break;
            }
        }

        panic!(
            "local_measure_uses_nested={local_measure_uses_nested} zero_seasonality_interval_count={zero_seasonality_interval_count} expected_severity_mark={expected_severity_mark} known_activity_coupling_scale={known_activity_coupling_scale} surprise_activity_coupling_scale={surprise_activity_coupling_scale} first_nonfinite_loading={first_nonfinite_loading:?} max_closed_form_loading_interval_index={max_closed_form_loading_interval_index} max_closed_form_loading_delta={max_closed_form_loading_delta} max_closed_form_loading_known={max_closed_form_loading_known} max_closed_form_loading_surprise={max_closed_form_loading_surprise} max_closed_form_loading={max_closed_form_loading} request_grid_count={} runtime_interval_count={} current_state_hawkes={:?} current_state_boundary={:?} first_failed_node={first_failed_node:?}",
            execution_input.request.grids.len(),
            execution_input
                .runtime_interval_schedule
                .interval_deltas_hours
                .len(),
            current_state_estimation_output.hawkes_excitation_state,
            current_state_estimation_output.mean_reversion_boundary_state,
        );
    }
}
