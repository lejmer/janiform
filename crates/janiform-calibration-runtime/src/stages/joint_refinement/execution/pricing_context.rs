use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::mpsc::Sender;

use chrono::{DateTime, Utc};
use janiform_engine_pricing::internal::{
    SpectralThetaWarmStartState,
    run_vanilla_option_grid_batch_pricing_with_observer_and_theta_warm_start_state,
};
use janiform_engine_pricing::{
    AnchoredVanillaOptionPricingBoundaryInput, PricingIntervalSchedule,
    PricingIntervalSchedulePolicy, PricingProgressEvent, PricingProgressPhase,
    VanillaOptionGridBatchPricingExecutionInput, VanillaOptionGridBatchPricingRequest,
    VanillaOptionGridBatchPricingRunResult, VanillaOptionGridRequest,
    VanillaOptionNodePriceSensitivity, VanillaOptionPayoffType,
    VanillaOptionPricingModelParameters, VanillaOptionPricingOptionalOutputSelection,
    derive_activity_compressed_pricing_interval_schedule, derive_pricing_interval_schedule,
    materialize_schedule_interval_factors_with_cpu_count,
};
use janiform_foundations::time::{
    NANOS_PER_HOUR, ceil_to_hour_utc_nanos, checked_interval_count_between_utc_nanos,
    floor_to_hour_utc_nanos,
};
use janiform_market_time::hourly_factors::{
    HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource, HourlySeasonalityFactorSource,
};

use crate::adapters::load_hourly_factor_contract_for_calibration;
use crate::current_state::{
    CurrentStateEstimationInput, estimate_current_state_from_observed_data,
};
use crate::runtime_seasonality::RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD;
use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageOptionPricingInput,
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate, progress_percent,
};
use crate::stages::joint_refinement::option_surface_tiering::{
    JointRefinementCanonicalOptionQuote, JointRefinementOptionSurfaceTier,
    JointRefinementOptionSurfaceTierKind, build_option_surface_tiers, clean_option_pricing_input,
};
use crate::stages::seasonality::{
    RuntimeHourlySeasonalityFactorsBuildInput,
    build_runtime_hourly_seasonality_factors_from_components,
};
use janiform_calibration_artifacts::{
    CalibrationObjectivePolicy, JointRefinementStageOptionAggregateDiagnostic,
    JointRefinementStageOptionCalibrationDiagnostics,
    JointRefinementStageOptionMaturityScoreDiagnostic, JointRefinementStageOptionNodeDiagnostic,
    JointRefinementStageOptionQuoteUniverseDiagnostics, JointRefinementStageOptionSnapshotMetadata,
    JointRefinementStageOptionTierConstructionDiagnostic,
    JointRefinementStageOptionTierConstructionDiagnostics,
    JointRefinementStageOptionTierMaturityDiagnostic,
    JointRefinementStageOptionTierPricingDiagnostics,
};
use janiform_market_time::calendar::ExchangeCalendarRegistry;

use super::super::objective::root_mean_square;
use super::super::setup::build_surprise_multipliers_for_interval_grid;
use super::option_objective_gradient::{
    JointRefinementOptionNodeResidualSensitivity, differentiate_option_tier_root_mean_square_score,
};
use super::option_reduced_model::{
    build_option_reduced_model_artifact, populate_reduced_model_runtime_estimates,
    reduced_model_diagnostics_from_artifact,
};
use super::option_refinement::option_pricing_block_evaluation_budget;
use super::state::{
    JointRefinementContext, JointRefinementOptionMaturityScoreArtifact,
    JointRefinementOptionNodeDefinition, JointRefinementOptionNodeLocation,
    JointRefinementOptionPricingContext, JointRefinementOptionTierPricingArtifact,
    JointRefinementOptionTierPricingInputSet, JointRefinementOptionTierPricingInputs,
    JointRefinementOptionTierScore,
};
use crate::objective::CalibrationLossBuilder;

#[cfg(test)]
type OptionGridBatchPricingRequestBuildOutput = (
    VanillaOptionGridBatchPricingRequest,
    Vec<JointRefinementOptionNodeDefinition>,
    Vec<f64>,
    Vec<usize>,
);

struct OptionPricingHorizon {
    hourly_coverage_start_utc_nanos: i64,
    hourly_coverage_end_utc_nanos: i64,
    hourly_coverage_interval_count: usize,
}

pub(crate) struct OptionPricingProgressInput<'a> {
    pub(crate) progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    pub(crate) progress_phase: Option<JointRefinementStageProgressPhase>,
    pub(crate) completed_units: Option<usize>,
    pub(crate) total_units: Option<usize>,
    pub(crate) current_restart_id: Option<&'a str>,
    pub(crate) best_restart_id: Option<&'a str>,
    pub(crate) best_weighted_objective: Option<f64>,
    pub(crate) active_restart_count: Option<usize>,
    pub(crate) current_evaluation_count: Option<usize>,
    pub(crate) total_evaluation_budget: Option<usize>,
    pub(crate) evaluation_progress_percent: Option<usize>,
    pub(crate) current_weighted_objective: Option<f64>,
    pub(crate) progress_snapshot_source: Option<&'a dyn OptionPricingProgressSnapshotSource>,
}

impl<'a> OptionPricingProgressInput<'a> {
    pub(crate) const fn none() -> Self {
        Self {
            progress_sender: None,
            progress_phase: None,
            completed_units: None,
            total_units: None,
            current_restart_id: None,
            best_restart_id: None,
            best_weighted_objective: None,
            active_restart_count: None,
            current_evaluation_count: None,
            total_evaluation_budget: None,
            evaluation_progress_percent: None,
            current_weighted_objective: None,
            progress_snapshot_source: None,
        }
    }
}

pub(crate) struct OptionPricingProgressSnapshot {
    pub(crate) completed_units: usize,
    pub(crate) aggregate_progress_percent: Option<usize>,
    pub(crate) active_restart_count: Option<usize>,
    pub(crate) current_evaluation_count: Option<usize>,
    pub(crate) total_evaluation_budget: Option<usize>,
    pub(crate) evaluation_progress_percent: Option<usize>,
    pub(crate) current_weighted_objective: Option<f64>,
    pub(crate) best_restart_id: Option<String>,
    pub(crate) best_weighted_objective: Option<f64>,
}

pub(crate) trait OptionPricingProgressSnapshotSource {
    fn option_pricing_progress_snapshot(
        &self,
        pricing_event_progress_percent: Option<usize>,
    ) -> OptionPricingProgressSnapshot;
}

struct OptionGridBatchPricingRunInput<'a, 'theta> {
    context: &'a JointRefinementContext,
    fitted_parameters: &'a janiform_calibration_artifacts::JointRefinementStageFittedParameters,
    tier_inputs: &'a JointRefinementOptionTierPricingInputs,
    include_node_price_sensitivities: bool,
    theta_warm_start_state: Option<&'theta mut SpectralThetaWarmStartState>,
    progress: OptionPricingProgressInput<'a>,
}

pub(crate) struct OptionSurfaceTierPricingArtifactResolutionInput<'a, 'theta> {
    pub(crate) context: &'a JointRefinementContext,
    pub(crate) fitted_parameters:
        &'a janiform_calibration_artifacts::JointRefinementStageFittedParameters,
    pub(crate) tier_kind: JointRefinementOptionSurfaceTierKind,
    pub(crate) source_dynamics_parameter_bits: &'a [u64],
    pub(crate) pricing_parameter_bits: &'a [u64],
    pub(crate) include_node_price_sensitivities: bool,
    pub(crate) theta_warm_start_state: Option<&'theta mut SpectralThetaWarmStartState>,
    pub(crate) progress: OptionPricingProgressInput<'a>,
}

#[cfg(test)]
struct OptionBucketDefinition {
    payoff_type: VanillaOptionPayoffType,
    maturity_interval: f64,
    strikes: Vec<f64>,
}

#[cfg(test)]
struct ResolvedOptionNode<'a> {
    node: &'a crate::stages::joint_refinement::JointRefinementStageOptionSnapshotNode,
    maturity_interval: f64,
}

#[cfg(test)]
struct ResolvedOptionMaturityBucket<'a> {
    definition: OptionBucketDefinition,
    nodes: Vec<ResolvedOptionNode<'a>>,
}

struct OptionMaturityResidualBucket {
    maturity_weight: usize,
    residuals: Vec<f64>,
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

    let cleaned_option_pricing_input = clean_option_pricing_input(option_pricing_input)?;

    let horizon = resolve_option_pricing_horizon(&cleaned_option_pricing_input)?;

    let current_state_estimation_output = estimate_option_pricing_current_state(
        &cleaned_option_pricing_input,
        hawkes_payload,
        mean_reversion_payload,
    )?;

    let option_spot = validate_option_spot(&cleaned_option_pricing_input)?;

    let hourly_factor_contract = build_option_hourly_factor_contract(
        run_spec,
        calendar_registry,
        stage_input,
        return_components_payload,
        &horizon,
    )?;

    let runtime_interval_schedule = resolve_option_runtime_interval_schedule(
        &cleaned_option_pricing_input,
        &hourly_factor_contract,
        false,
    )?;

    let interval_factors = materialize_schedule_interval_factors_with_cpu_count(
        &runtime_interval_schedule,
        &hourly_factor_contract,
        Some(cpu_count.max(1)),
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
        &runtime_interval_schedule,
    )?;

    let audit_runtime_interval_schedule = resolve_option_runtime_interval_schedule(
        &cleaned_option_pricing_input,
        &hourly_factor_contract,
        true,
    )?;

    let audit_interval_factors = if audit_runtime_interval_schedule == runtime_interval_schedule {
        interval_factors.clone()
    } else {
        materialize_schedule_interval_factors_with_cpu_count(
            &audit_runtime_interval_schedule,
            &hourly_factor_contract,
            Some(cpu_count.max(1)),
        )
        .map_err(|source| {
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: format!(
                    "joint option calibration audit interval factor materialization failed: {source}"
                ),
            }
        })?
    };

    let (snapshot_maturity_intervals, snapshot_maturity_terminal_interval_indices) =
        snapshot_maturity_terminal_mapping(
            &cleaned_option_pricing_input,
            &runtime_interval_schedule,
        )?;

    let option_surface_tiers = build_option_surface_tiers(
        option_pricing_input,
        &cleaned_option_pricing_input,
        option_spot,
        &runtime_interval_schedule,
    )?;

    let tier_pricing_inputs = build_option_surface_tier_pricing_input_set(
        option_spot,
        &cleaned_option_pricing_input.carry_input,
        &option_surface_tiers,
        &runtime_interval_schedule,
        interval_factors.as_slice(),
        &audit_runtime_interval_schedule,
        audit_interval_factors.as_slice(),
    )
    .map_err(|error_message| {
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput { error_message }
    })?;

    Ok(Some(JointRefinementOptionPricingContext {
        valuation_time_utc_nanos: cleaned_option_pricing_input.valuation_time_utc_nanos,
        boundary_input,
        snapshot_maturity_intervals,
        snapshot_maturity_terminal_interval_indices,
        option_surface_tiers,
        tier_pricing_inputs,
        carry_input: cleaned_option_pricing_input.carry_input.clone(),
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

    let hourly_coverage_start_utc_nanos =
        floor_to_hour_utc_nanos(option_pricing_input.valuation_time_utc_nanos);
    let hourly_coverage_end_utc_nanos = ceil_to_hour_utc_nanos(max_expiry_timestamp_utc_nanos);

    let hourly_coverage_interval_count = checked_interval_count_between_utc_nanos(
        hourly_coverage_start_utc_nanos,
        hourly_coverage_end_utc_nanos,
        NANOS_PER_HOUR,
    )
    .ok_or_else(
        || JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
            error_message: "joint option calibration requires a valid hourly option coverage grid"
                .to_string(),
        },
    )?;

    Ok(OptionPricingHorizon {
        hourly_coverage_start_utc_nanos,
        hourly_coverage_end_utc_nanos,
        hourly_coverage_interval_count,
    })
}

fn resolve_option_runtime_interval_schedule(
    option_pricing_input: &JointRefinementStageOptionPricingInput,
    hourly_factor_contract: &janiform_market_time::hourly_factors::HourlyFactorContractOutput,
    force_exact_sparse: bool,
) -> Result<PricingIntervalSchedule, JointRefinementStageExecutionFailure> {
    let expiry_timestamps_utc_nanos = option_pricing_input
        .option_snapshot_nodes
        .iter()
        .map(|node| node.expiry_timestamp_utc_nanos)
        .collect::<Vec<_>>();

    let resolved_schedule = if force_exact_sparse {
        derive_pricing_interval_schedule(
            option_pricing_input.valuation_time_utc_nanos,
            expiry_timestamps_utc_nanos.as_slice(),
            &PricingIntervalSchedulePolicy::AutoExactSparse,
        )
    } else {
        match &option_pricing_input.pricing_interval_schedule_policy {
            PricingIntervalSchedulePolicy::AutoActivityCompressed => {
                derive_activity_compressed_pricing_interval_schedule(
                    option_pricing_input.valuation_time_utc_nanos,
                    expiry_timestamps_utc_nanos.as_slice(),
                    hourly_factor_contract,
                )
            }
            policy => derive_pricing_interval_schedule(
                option_pricing_input.valuation_time_utc_nanos,
                expiry_timestamps_utc_nanos.as_slice(),
                policy,
            ),
        }
    };

    resolved_schedule
        .map(|derived_schedule| derived_schedule.schedule)
        .map_err(
            |source| JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: format!(
                    "joint option calibration runtime interval schedule is invalid: {source}"
                ),
            },
        )
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
            RuntimeHourlySeasonalityFactorsBuildInput {
                calendar_registry,
                exchange_code: run_spec.data_inputs.exchange_code.as_str(),
                intraday_minutes_per_bin: stage_input
                    .seasonality_factors_input
                    .intraday_minutes_per_bin,
                intraday_factor_by_bin: stage_input
                    .seasonality_factors_input
                    .intraday_factor_by_bin
                    .as_slice(),
                day_of_week_factor: stage_input
                    .seasonality_factors_input
                    .day_of_week_factor
                    .as_slice(),
                day_of_year_factor: stage_input
                    .seasonality_factors_input
                    .day_of_year_factor
                    .as_slice(),
                range_start_utc_nanos: horizon.hourly_coverage_start_utc_nanos,
                range_end_utc_nanos: horizon.hourly_coverage_end_utc_nanos,
                interval_duration_nanos: NANOS_PER_HOUR,
            },
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
            NANOS_PER_HOUR,
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
            interval_duration_nanos: NANOS_PER_HOUR,
            seasonality_source: Some(HourlySeasonalityFactorSource::Explicit {
                hourly_factors: runtime_hourly_seasonality_factors.as_slice(),
                materialization_method: RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
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

#[cfg(test)]
fn build_option_grid_batch_pricing_request(
    spot: f64,
    option_pricing_input: &JointRefinementStageOptionPricingInput,
    runtime_interval_schedule: &PricingIntervalSchedule,
) -> Result<OptionGridBatchPricingRequestBuildOutput, JointRefinementStageExecutionFailure> {
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

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
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
                maturity_weight: 1,
                location: JointRefinementOptionNodeLocation {
                    grid_index,
                    maturity_index: 0,
                    strike_index,
                },
            })
        })
        .collect()
}

#[cfg(test)]
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
    input: OptionGridBatchPricingRunInput<'_, '_>,
) -> Result<VanillaOptionGridBatchPricingRunResult, String> {
    let OptionGridBatchPricingRunInput {
        context,
        fitted_parameters,
        tier_inputs,
        include_node_price_sensitivities,
        theta_warm_start_state,
        progress,
    } = input;

    let Some(option_pricing_context) = context.option_pricing_context.as_ref() else {
        return Err("joint option pricing context is unavailable".to_string());
    };

    let started_at = std::time::Instant::now();

    let mut boundary_input = option_pricing_context.boundary_input.clone();
    boundary_input.interval_delta = tier_inputs.runtime_interval_schedule.interval_deltas_hours[0];

    let execution_input = VanillaOptionGridBatchPricingExecutionInput {
        request: tier_inputs.grid_batch_pricing_request.clone(),
        boundary_input,
        model_parameters: VanillaOptionPricingModelParameters::new(
            fitted_parameters.hawkes_fitted_parameters.kernel_parameters,
            fitted_parameters
                .return_components_fitted_parameters
                .news_process_configuration
                .clone(),
            pricing_nig_configuration(fitted_parameters),
        ),
        runtime_interval_schedule: tier_inputs.runtime_interval_schedule.clone(),
        interval_delta: tier_inputs.runtime_interval_schedule.interval_deltas_hours[0],
        interval_factors: tier_inputs.interval_factors.clone(),
        optional_output_selection: VanillaOptionPricingOptionalOutputSelection {
            include_transform_diagnostics: false,
            include_integration_quality_diagnostics: false,
            include_calibration_hooks: false,
            include_node_price_sensitivities,
        },
        spectral_collocation_node_count_override: tier_inputs
            .spectral_collocation_node_count_override,
        cpu_count: Some(option_pricing_context.cpu_count),
    };

    let pricing_result = if let (
        Some(progress_sender),
        Some(progress_phase),
        Some(completed_units),
        Some(total_units),
    ) = (
        progress.progress_sender,
        progress.progress_phase,
        progress.completed_units,
        progress.total_units,
    ) {
        let mut pricing_progress_observer = |event: PricingProgressEvent| {
            let snapshot = option_pricing_progress_snapshot(&progress, &event, completed_units);

            let _ = progress_sender.send(JointRefinementStageProgressUpdate {
                aggregate_progress_percent: snapshot.aggregate_progress_percent,
                current_item_label: Some(format_option_pricing_work_item(&progress, &event)),
                current_restart_id: progress.current_restart_id.map(str::to_string),
                active_restart_count: snapshot.active_restart_count,
                current_evaluation_count: snapshot.current_evaluation_count,
                total_evaluation_budget: snapshot.total_evaluation_budget,
                evaluation_progress_percent: snapshot.evaluation_progress_percent,
                current_global_loss: snapshot.current_weighted_objective,
                current_pricing_interval_index: event.current_interval_index,
                total_pricing_interval_count: event.total_intervals,
                completed_pricing_row_count: pricing_progress_completed_row_count(&event),
                total_pricing_row_count: pricing_progress_total_row_count(&event),
                best_restart_id: snapshot.best_restart_id,
                best_global_loss: snapshot.best_weighted_objective,
                ..JointRefinementStageProgressUpdate::new(
                    progress_phase,
                    snapshot.completed_units,
                    total_units,
                )
            });
        };

        run_vanilla_option_grid_batch_pricing_with_observer_and_theta_warm_start_state(
            execution_input,
            Some(&mut pricing_progress_observer),
            theta_warm_start_state,
        )
    } else {
        run_vanilla_option_grid_batch_pricing_with_observer_and_theta_warm_start_state(
            execution_input,
            None,
            theta_warm_start_state,
        )
    };

    context.profiling_state.record_option_pricing_run_duration(
        started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    );

    pricing_result.map_err(|source| format!("joint option pricing evaluation failed: {source}"))
}

fn option_pricing_progress_snapshot(
    progress: &OptionPricingProgressInput<'_>,
    event: &PricingProgressEvent,
    completed_units: usize,
) -> OptionPricingProgressSnapshot {
    match progress.progress_snapshot_source {
        Some(source) => {
            source.option_pricing_progress_snapshot(pricing_event_progress_percent(event))
        }
        None => OptionPricingProgressSnapshot {
            completed_units,
            aggregate_progress_percent: None,
            active_restart_count: progress.active_restart_count,
            current_evaluation_count: progress.current_evaluation_count,
            total_evaluation_budget: progress.total_evaluation_budget,
            evaluation_progress_percent: progress.evaluation_progress_percent,
            current_weighted_objective: progress.current_weighted_objective,
            best_restart_id: progress.best_restart_id.map(str::to_string),
            best_weighted_objective: progress.best_weighted_objective,
        },
    }
}

fn pricing_event_progress_percent(event: &PricingProgressEvent) -> Option<usize> {
    match event.phase {
        PricingProgressPhase::TransitionRowProgress => {
            let interval_index = event.current_interval_index?;
            let total_intervals = event.total_intervals?;
            if total_intervals == 0 || event.total_units == 0 {
                return None;
            }

            let completed_row_count = event.completed_units.min(event.total_units);
            let completed_progress_units = interval_index
                .min(total_intervals)
                .saturating_mul(event.total_units)
                .saturating_add(completed_row_count);
            let total_progress_units = total_intervals.saturating_mul(event.total_units);

            Some(progress_percent(
                completed_progress_units,
                total_progress_units,
            ))
        }
        _ => Some(progress_percent(event.completed_units, event.total_units)),
    }
}

pub(crate) fn resolve_option_surface_tier_pricing_artifact(
    input: OptionSurfaceTierPricingArtifactResolutionInput<'_, '_>,
) -> Result<Arc<JointRefinementOptionTierPricingArtifact>, String> {
    let OptionSurfaceTierPricingArtifactResolutionInput {
        context,
        fitted_parameters,
        tier_kind,
        source_dynamics_parameter_bits,
        pricing_parameter_bits,
        include_node_price_sensitivities,
        theta_warm_start_state,
        progress,
    } = input;

    let cache_key = super::JointRefinementOptionPricingCacheKey {
        tier_kind,
        include_node_price_sensitivities,
        source_dynamics_parameter_bits: source_dynamics_parameter_bits.to_vec(),
        pricing_parameter_bits: pricing_parameter_bits.to_vec(),
    };

    if let Ok(cache_guard) = context.option_pricing_cache.lock()
        && let Some(cached_value) = cache_guard.get(&cache_key).cloned()
    {
        context.profiling_state.record_option_pricing_cache_hit();
        return cached_value;
    }

    context.profiling_state.record_option_pricing_cache_miss();

    let Some(option_pricing_context) = context.option_pricing_context.as_ref() else {
        return Err("joint option pricing context is unavailable".to_string());
    };

    let valuation_time_utc_nanos = option_pricing_context.valuation_time_utc_nanos;
    let tier_inputs = option_pricing_context.tier_pricing_inputs.tier(tier_kind);

    let pricing_result = run_option_grid_batch_pricing(OptionGridBatchPricingRunInput {
        context,
        fitted_parameters,
        tier_inputs,
        include_node_price_sensitivities,
        theta_warm_start_state,
        progress,
    });

    let resolved_artifact = pricing_result.and_then(|pricing_result| {
        build_option_surface_tier_pricing_artifact(
            cache_key.clone(),
            &pricing_result,
            context.objective_policy,
            valuation_time_utc_nanos,
            tier_inputs,
        )
        .map(Arc::new)
    });

    if let Ok(mut cache_guard) = context.option_pricing_cache.lock() {
        cache_guard.insert(cache_key, resolved_artifact.clone());
    }

    resolved_artifact
}

fn build_option_surface_tier_pricing_input_set(
    spot: f64,
    carry_input: &janiform_calibration_artifacts::JointRefinementStageCarryInput,
    option_surface_tiers: &crate::stages::joint_refinement::option_surface_tiering::JointRefinementOptionSurfaceTierSet,
    primary_runtime_interval_schedule: &PricingIntervalSchedule,
    primary_interval_factors: &[janiform_engine_pricing::VanillaOptionPricingIntervalFactors],
    audit_runtime_interval_schedule: &PricingIntervalSchedule,
    audit_interval_factors: &[janiform_engine_pricing::VanillaOptionPricingIntervalFactors],
) -> Result<JointRefinementOptionTierPricingInputSet, String> {
    Ok(JointRefinementOptionTierPricingInputSet {
        search: build_option_surface_tier_pricing_inputs(
            spot,
            carry_input,
            option_surface_tiers,
            JointRefinementOptionSurfaceTierKind::Search,
            primary_runtime_interval_schedule,
            primary_interval_factors,
        )?,
        selection: build_option_surface_tier_pricing_inputs(
            spot,
            carry_input,
            option_surface_tiers,
            JointRefinementOptionSurfaceTierKind::Selection,
            primary_runtime_interval_schedule,
            primary_interval_factors,
        )?,
        audit: build_option_surface_tier_pricing_inputs(
            spot,
            carry_input,
            option_surface_tiers,
            JointRefinementOptionSurfaceTierKind::Audit,
            audit_runtime_interval_schedule,
            audit_interval_factors,
        )?,
    })
}

fn build_option_surface_tier_pricing_inputs(
    spot: f64,
    carry_input: &janiform_calibration_artifacts::JointRefinementStageCarryInput,
    option_surface_tiers: &crate::stages::joint_refinement::option_surface_tiering::JointRefinementOptionSurfaceTierSet,
    tier_kind: JointRefinementOptionSurfaceTierKind,
    runtime_interval_schedule: &PricingIntervalSchedule,
    interval_factors: &[janiform_engine_pricing::VanillaOptionPricingIntervalFactors],
) -> Result<JointRefinementOptionTierPricingInputs, String> {
    let tier = option_surface_tier_by_kind(option_surface_tiers, tier_kind);

    let risk_free_rate = carry_input.discount_rate;
    let dividend_yield = carry_input.discount_rate - carry_input.carry_rate;

    let mut grids = Vec::new();
    let mut observed_nodes = Vec::new();

    for payoff_type in [VanillaOptionPayoffType::Call, VanillaOptionPayoffType::Put] {
        for representative in &tier.maturity_representatives {
            let mut quotes = representative
                .quote_indices
                .iter()
                .filter_map(|quote_index| option_surface_tiers.canonical_quotes.get(*quote_index))
                .filter(|quote| quote.payoff_type == payoff_type)
                .collect::<Vec<_>>();

            quotes.sort_by(|left, right| {
                left.strike
                    .total_cmp(&right.strike)
                    .then_with(|| left.source_position.cmp(&right.source_position))
            });

            if quotes.is_empty() {
                continue;
            }

            let grid_index = grids.len();
            let strikes = quotes.iter().map(|quote| quote.strike).collect::<Vec<_>>();

            observed_nodes.extend(build_observed_tier_option_nodes(
                grid_index,
                representative.maturity_weight,
                quotes.as_slice(),
                strikes.as_slice(),
            )?);

            grids.push(VanillaOptionGridRequest {
                payoff_type,
                risk_free_rate,
                dividend_yield,
                strikes,
                maturity_intervals: vec![representative.maturity_interval],
            });
        }
    }

    if observed_nodes.is_empty() {
        return Err(format!(
            "joint option pricing tier `{tier_kind:?}` contains no priceable option quotes"
        ));
    }

    let maturity_intervals = tier
        .maturity_representatives
        .iter()
        .map(|representative| representative.maturity_interval)
        .collect::<Vec<_>>();

    let maturity_terminal_interval_indices = tier
        .maturity_representatives
        .iter()
        .map(|representative| {
            runtime_interval_schedule
                .terminal_interval_index_for_timestamp(representative.expiry_timestamp_utc_nanos)
                .map_err(|source| format!("{source}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(JointRefinementOptionTierPricingInputs {
        runtime_interval_schedule: runtime_interval_schedule.clone(),
        interval_factors: interval_factors.to_vec(),
        grid_batch_pricing_request: VanillaOptionGridBatchPricingRequest { spot, grids },
        spectral_collocation_node_count_override: None,
        observed_nodes,
        maturity_intervals,
        maturity_terminal_interval_indices,
    })
}

fn option_surface_tier_by_kind(
    option_surface_tiers: &crate::stages::joint_refinement::option_surface_tiering::JointRefinementOptionSurfaceTierSet,
    tier_kind: JointRefinementOptionSurfaceTierKind,
) -> &JointRefinementOptionSurfaceTier {
    match tier_kind {
        JointRefinementOptionSurfaceTierKind::Search => &option_surface_tiers.search,
        JointRefinementOptionSurfaceTierKind::Selection => &option_surface_tiers.selection,
        JointRefinementOptionSurfaceTierKind::Audit => &option_surface_tiers.audit,
    }
}

fn build_observed_tier_option_nodes(
    grid_index: usize,
    maturity_weight: usize,
    quotes: &[&JointRefinementCanonicalOptionQuote],
    strikes: &[f64],
) -> Result<Vec<JointRefinementOptionNodeDefinition>, String> {
    quotes
        .iter()
        .map(|quote| {
            let strike_index = locate_option_grid_coordinate(strikes, quote.strike, "strike")
                .map_err(|source| format!("{source:?}"))?;

            Ok(JointRefinementOptionNodeDefinition {
                quote_timestamp_utc_nanos: quote.quote_timestamp_utc_nanos,
                expiry_timestamp_utc_nanos: quote.expiry_timestamp_utc_nanos,
                strike: quote.strike,
                payoff_type: quote.payoff_type,
                bid_price: quote.bid_price,
                ask_price: quote.ask_price,
                observed_mid_price: quote.observed_mid_price,
                maturity_weight,
                location: JointRefinementOptionNodeLocation {
                    grid_index,
                    maturity_index: 0,
                    strike_index,
                },
            })
        })
        .collect()
}

fn build_option_surface_tier_pricing_artifact(
    cache_key: super::JointRefinementOptionPricingCacheKey,
    pricing_result: &VanillaOptionGridBatchPricingRunResult,
    objective_policy: CalibrationObjectivePolicy,
    valuation_time_utc_nanos: i64,
    tier_inputs: &JointRefinementOptionTierPricingInputs,
) -> Result<JointRefinementOptionTierPricingArtifact, String> {
    let mut node_diagnostics = Vec::with_capacity(tier_inputs.observed_nodes.len());
    let mut loss_builder = CalibrationLossBuilder::new(objective_policy);

    let quote_scale_floor =
        minimum_positive_option_quote_spread(tier_inputs.observed_nodes.as_slice())?;

    let mut maturity_bucket_residuals = BTreeMap::<String, Vec<f64>>::new();
    let mut strike_bucket_residuals = BTreeMap::<String, Vec<f64>>::new();
    let mut residuals_by_maturity = BTreeMap::<i64, OptionMaturityResidualBucket>::new();

    let mut node_sensitivities: Option<Vec<JointRefinementOptionNodeResidualSensitivity>> =
        pricing_result
            .grids
            .iter()
            .any(|grid_result| grid_result.node_price_sensitivity_grid.is_some())
            .then(Vec::new);

    for observed_node in &tier_inputs.observed_nodes {
        let model_price = resolve_model_option_price(
            pricing_result,
            observed_node,
            "joint option pricing artifact returned a grid inconsistent with the selected option tier",
        )?;

        let quote_residual_scale = option_quote_residual_scale(observed_node, quote_scale_floor)?;

        let spread_normalized_residual =
            (model_price - observed_node.observed_mid_price) / quote_residual_scale;

        loss_builder.add_option_quote_term(
            format!(
                "options.quote.{}.{}.{}",
                observed_node.expiry_timestamp_utc_nanos,
                observed_node.strike,
                match observed_node.payoff_type {
                    VanillaOptionPayoffType::Call => "call",
                    VanillaOptionPayoffType::Put => "put",
                }
            ),
            observed_node.observed_mid_price,
            model_price,
            quote_residual_scale,
        );

        if node_sensitivities.is_some() {
            match resolve_model_option_price_sensitivity(pricing_result, observed_node)? {
                Some(model_price_sensitivity) => {
                    if let Some(node_sensitivities) = node_sensitivities.as_mut() {
                        node_sensitivities.push(JointRefinementOptionNodeResidualSensitivity {
                            expiry_timestamp_utc_nanos: observed_node.expiry_timestamp_utc_nanos,
                            maturity_weight: observed_node.maturity_weight,
                            spread_normalized_residual,
                            spread_normalized_residual_gradient:
                                spread_normalized_residual_gradient(
                                    model_price_sensitivity,
                                    observed_node,
                                    quote_scale_floor,
                                ),
                        });
                    }
                }
                None => node_sensitivities = None,
            }
        }

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
                (observed_node.expiry_timestamp_utc_nanos - valuation_time_utc_nanos) as f64
                    / NANOS_PER_HOUR as f64
            ))
            .or_default()
            .push(spread_normalized_residual);

        strike_bucket_residuals
            .entry(format!("{}", observed_node.strike))
            .or_default()
            .push(spread_normalized_residual);

        if let Some(bucket) =
            residuals_by_maturity.get_mut(&observed_node.expiry_timestamp_utc_nanos)
        {
            if bucket.maturity_weight != observed_node.maturity_weight {
                return Err(
                    "joint option pricing artifact encountered inconsistent maturity weights"
                        .to_string(),
                );
            }

            bucket.residuals.push(spread_normalized_residual);
        } else {
            residuals_by_maturity.insert(
                observed_node.expiry_timestamp_utc_nanos,
                OptionMaturityResidualBucket {
                    maturity_weight: observed_node.maturity_weight,
                    residuals: vec![spread_normalized_residual],
                },
            );
        }
    }

    let mut weighted_squared_maturity_fit_sum = 0.0;
    let mut total_maturity_weight = 0usize;
    let mut maturity_scores = Vec::with_capacity(residuals_by_maturity.len());

    for (expiry_timestamp_utc_nanos, bucket) in residuals_by_maturity {
        let maturity_fit = root_mean_square(bucket.residuals.as_slice());

        weighted_squared_maturity_fit_sum +=
            bucket.maturity_weight as f64 * maturity_fit * maturity_fit;
        total_maturity_weight += bucket.maturity_weight;

        maturity_scores.push(JointRefinementOptionMaturityScoreArtifact {
            expiry_timestamp_utc_nanos,
            maturity_weight: bucket.maturity_weight,
            quote_count: bucket.residuals.len(),
            root_mean_square_spread_normalized_residual: maturity_fit,
        });
    }

    if total_maturity_weight == 0 {
        return Err("joint option pricing tier contains no weighted maturities".to_string());
    }

    let score = JointRefinementOptionTierScore {
        root_mean_square_spread_normalized_residual: (weighted_squared_maturity_fit_sum
            / total_maturity_weight as f64)
            .sqrt(),
    };

    let loss_evaluation = loss_builder.build();

    let aggregate_diagnostics =
        build_option_aggregate_diagnostics(maturity_bucket_residuals, strike_bucket_residuals);

    let tier_kind = cache_key.tier_kind;

    let score_gradient_artifact = node_sensitivities
        .as_ref()
        .filter(|node_sensitivities| !node_sensitivities.is_empty())
        .and_then(|node_sensitivities| {
            differentiate_option_tier_root_mean_square_score(node_sensitivities.as_slice()).ok()
        });

    let reduced_model_artifact = node_sensitivities
        .as_ref()
        .filter(|node_sensitivities| !node_sensitivities.is_empty())
        .and_then(|node_sensitivities| {
            build_option_reduced_model_artifact(node_sensitivities.as_slice()).ok()
        });

    Ok(JointRefinementOptionTierPricingArtifact {
        cache_key,
        tier_kind,
        runtime_interval_schedule: tier_inputs.runtime_interval_schedule.clone(),
        spectral_collocation_node_count: resolve_pricing_result_collocation_node_count(
            pricing_result,
        ),
        score,
        loss_evaluation,
        maturity_scores,
        node_diagnostics,
        aggregate_diagnostics,
        score_gradient_artifact,
        reduced_model_artifact,
        maturity_intervals: tier_inputs.maturity_intervals.clone(),
        maturity_terminal_interval_indices: tier_inputs.maturity_terminal_interval_indices.clone(),
    })
}

fn resolve_pricing_result_collocation_node_count(
    pricing_result: &VanillaOptionGridBatchPricingRunResult,
) -> Option<usize> {
    let mut node_counts = BTreeSet::new();

    for grid in &pricing_result.grids {
        for maturity_summary in &grid.maturity_summaries {
            let summary = maturity_summary.as_ref()?;
            node_counts.insert(summary.collocation_node_count);
        }
    }

    if node_counts.len() == 1 {
        node_counts.first().copied()
    } else {
        None
    }
}

pub(crate) fn build_option_calibration_diagnostics(
    context: &JointRefinementContext,
    selected_search_tier_pricing_artifact: Option<&JointRefinementOptionTierPricingArtifact>,
    selected_selection_tier_pricing_artifact: Option<&JointRefinementOptionTierPricingArtifact>,
    audit_tier_pricing_artifact: Option<&JointRefinementOptionTierPricingArtifact>,
) -> Result<Option<JointRefinementStageOptionCalibrationDiagnostics>, String> {
    let Some(option_pricing_context) = context.option_pricing_context.as_ref() else {
        return Ok(None);
    };

    debug_assert_eq!(
        option_pricing_context
            .option_surface_tiers
            .audit
            .maturity_count,
        option_pricing_context.snapshot_maturity_intervals.len()
    );

    debug_assert_eq!(
        option_pricing_context
            .snapshot_maturity_terminal_interval_indices
            .len(),
        option_pricing_context.snapshot_maturity_intervals.len()
    );

    let selection_artifact = selected_selection_tier_pricing_artifact.ok_or_else(|| {
        "joint option calibration diagnostics require the selected selection-tier pricing artifact"
            .to_string()
    })?;

    validate_option_tier_pricing_artifact(
        selection_artifact,
        JointRefinementOptionSurfaceTierKind::Selection,
    )?;

    if let Some(audit_artifact) = audit_tier_pricing_artifact {
        validate_option_tier_pricing_artifact(
            audit_artifact,
            JointRefinementOptionSurfaceTierKind::Audit,
        )?;
    }

    let reference_oracle_call_wall_clock_millis = context
        .profiling_state
        .mean_option_pricing_run_wall_clock_millis();

    let mut reduced_model = selection_artifact
        .reduced_model_artifact
        .as_ref()
        .map(|reduced_model_artifact| {
            reduced_model_diagnostics_from_artifact(
                reduced_model_artifact,
                option_pricing_block_evaluation_budget(),
            )
        })
        .or_else(|| {
            selected_search_tier_pricing_artifact
                .and_then(|artifact| artifact.reduced_model_artifact.as_ref())
                .map(|reduced_model_artifact| {
                    reduced_model_diagnostics_from_artifact(
                        reduced_model_artifact,
                        option_pricing_block_evaluation_budget(),
                    )
                })
        });

    if let Some(reduced_model_diagnostics) = reduced_model.as_mut() {
        populate_reduced_model_runtime_estimates(
            reduced_model_diagnostics,
            reference_oracle_call_wall_clock_millis,
        );
    }

    Ok(Some(JointRefinementStageOptionCalibrationDiagnostics {
        carry_input: option_pricing_context.carry_input.clone(),
        quote_universe: JointRefinementStageOptionQuoteUniverseDiagnostics {
            input_quote_count: option_pricing_context
                .option_surface_tiers
                .valid_input_quote_count
                + option_pricing_context
                    .option_surface_tiers
                    .dropped_input_quote_count,
            canonical_quote_count: option_pricing_context
                .option_surface_tiers
                .canonical_quotes
                .len(),
            dropped_quote_count: option_pricing_context
                .option_surface_tiers
                .dropped_input_quote_count,
        },
        tier_construction: build_option_tier_construction_diagnostics(option_pricing_context),
        selection_tier_pricing: build_option_tier_pricing_diagnostics(
            option_pricing_context,
            selection_artifact,
        ),
        audit_tier_pricing: audit_tier_pricing_artifact.map(|audit_artifact| {
            build_option_tier_pricing_diagnostics(option_pricing_context, audit_artifact)
        }),
        reduced_model,
    }))
}

fn validate_option_tier_pricing_artifact(
    artifact: &JointRefinementOptionTierPricingArtifact,
    expected_tier_kind: JointRefinementOptionSurfaceTierKind,
) -> Result<(), String> {
    if artifact.tier_kind != expected_tier_kind
        || artifact.cache_key.tier_kind != expected_tier_kind
    {
        return Err(format!(
            "joint option calibration diagnostics require a `{expected_tier_kind:?}` pricing artifact"
        ));
    }

    for maturity_score in &artifact.maturity_scores {
        if maturity_score.maturity_weight == 0
            || maturity_score.quote_count == 0
            || !maturity_score
                .root_mean_square_spread_normalized_residual
                .is_finite()
        {
            return Err(
                "joint option calibration diagnostics received an invalid maturity score artifact"
                    .to_string(),
            );
        }

        if !artifact.node_diagnostics.iter().any(|node| {
            node.expiry_timestamp_utc_nanos == maturity_score.expiry_timestamp_utc_nanos
        }) {
            return Err(
                "joint option calibration diagnostics received a maturity score without node coverage"
                    .to_string(),
            );
        }
    }

    Ok(())
}

fn build_option_tier_construction_diagnostics(
    option_pricing_context: &JointRefinementOptionPricingContext,
) -> JointRefinementStageOptionTierConstructionDiagnostics {
    JointRefinementStageOptionTierConstructionDiagnostics {
        search: build_option_tier_construction_diagnostic(
            &option_pricing_context.option_surface_tiers.search,
        ),
        selection: build_option_tier_construction_diagnostic(
            &option_pricing_context.option_surface_tiers.selection,
        ),
        audit: build_option_tier_construction_diagnostic(
            &option_pricing_context.option_surface_tiers.audit,
        ),
    }
}

fn build_option_tier_construction_diagnostic(
    tier: &JointRefinementOptionSurfaceTier,
) -> JointRefinementStageOptionTierConstructionDiagnostic {
    JointRefinementStageOptionTierConstructionDiagnostic {
        tier_kind: option_surface_tier_kind_label(tier.tier_kind).to_string(),
        maturity_count: tier.maturity_count,
        representative_maturities: tier
            .maturity_representatives
            .iter()
            .map(
                |representative| JointRefinementStageOptionTierMaturityDiagnostic {
                    terminal_interval_index: representative.terminal_interval_index,
                    maturity_interval: representative.maturity_interval,
                    expiry_timestamp_utc_nanos: representative.expiry_timestamp_utc_nanos,
                    maturity_weight: representative.maturity_weight,
                    selected_quote_count: representative.quote_indices.len(),
                },
            )
            .collect(),
    }
}

fn build_option_tier_pricing_diagnostics(
    option_pricing_context: &JointRefinementOptionPricingContext,
    artifact: &JointRefinementOptionTierPricingArtifact,
) -> JointRefinementStageOptionTierPricingDiagnostics {
    JointRefinementStageOptionTierPricingDiagnostics {
        tier_kind: option_surface_tier_kind_label(artifact.tier_kind).to_string(),
        snapshot_metadata: JointRefinementStageOptionSnapshotMetadata {
            valuation_time_utc_nanos: option_pricing_context.valuation_time_utc_nanos,
            node_count: artifact.node_diagnostics.len(),
            spectral_collocation_node_count: artifact.spectral_collocation_node_count,
            pricing_interval_schedule: artifact.runtime_interval_schedule.clone(),
            maturity_intervals: artifact.maturity_intervals.clone(),
            maturity_terminal_interval_indices: artifact.maturity_terminal_interval_indices.clone(),
        },
        root_mean_square_spread_normalized_residual: artifact
            .score
            .root_mean_square_spread_normalized_residual,
        maturity_scores: artifact
            .maturity_scores
            .iter()
            .map(
                |maturity_score| JointRefinementStageOptionMaturityScoreDiagnostic {
                    expiry_timestamp_utc_nanos: maturity_score.expiry_timestamp_utc_nanos,
                    maturity_weight: maturity_score.maturity_weight,
                    quote_count: maturity_score.quote_count,
                    root_mean_square_spread_normalized_residual: maturity_score
                        .root_mean_square_spread_normalized_residual,
                },
            )
            .collect(),
        node_diagnostics: artifact.node_diagnostics.clone(),
        aggregate_diagnostics: artifact.aggregate_diagnostics.clone(),
    }
}

fn option_surface_tier_kind_label(tier_kind: JointRefinementOptionSurfaceTierKind) -> &'static str {
    match tier_kind {
        JointRefinementOptionSurfaceTierKind::Search => "search",
        JointRefinementOptionSurfaceTierKind::Selection => "selection",
        JointRefinementOptionSurfaceTierKind::Audit => "audit",
    }
}

fn format_option_pricing_work_item(
    progress: &OptionPricingProgressInput<'_>,
    event: &PricingProgressEvent,
) -> String {
    progress
        .current_restart_id
        .map(str::to_string)
        .or_else(|| {
            progress
                .progress_phase
                .map(option_pricing_phase_label)
                .map(str::to_string)
        })
        .unwrap_or_else(|| format_option_pricing_progress_item(event))
}

fn option_pricing_phase_label(phase: JointRefinementStageProgressPhase) -> &'static str {
    match phase {
        JointRefinementStageProgressPhase::OptionSearchTierRefinement => "option search",
        JointRefinementStageProgressPhase::OptionSelectionTierEvaluation => "option selection",
        JointRefinementStageProgressPhase::OptionAuditTierEvaluation => "option audit",
        _ => "option pricing",
    }
}

fn pricing_progress_completed_row_count(event: &PricingProgressEvent) -> Option<usize> {
    match event.phase {
        PricingProgressPhase::TransitionRowProgress => Some(event.completed_units),
        _ => None,
    }
}

fn pricing_progress_total_row_count(event: &PricingProgressEvent) -> Option<usize> {
    match event.phase {
        PricingProgressPhase::TransitionRowProgress => Some(event.total_units),
        _ => None,
    }
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
        PricingProgressPhase::TransitionRowProgress => match event.total_intervals {
            Some(total_intervals) => format!(
                "interval {}/{}",
                display_position(event.current_interval_index, event.completed_units),
                total_intervals,
            ),
            None => "interval".to_string(),
        },
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

fn format_progress_counts(label: &str, total_units: usize, current_index: Option<usize>) -> String {
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

fn resolve_model_option_price_sensitivity(
    pricing_result: &VanillaOptionGridBatchPricingRunResult,
    observed_node: &JointRefinementOptionNodeDefinition,
) -> Result<Option<VanillaOptionNodePriceSensitivity>, String> {
    let Some(grid) = pricing_result.grids.get(observed_node.location.grid_index) else {
        return Err(
            "joint option pricing artifact returned a sensitivity grid inconsistent with the selected option tier"
                .to_string(),
        );
    };

    let Some(node_price_sensitivity_grid) = grid.node_price_sensitivity_grid.as_ref() else {
        return Ok(None);
    };

    Ok(node_price_sensitivity_grid
        .get(observed_node.location.maturity_index)
        .and_then(|row| row.get(observed_node.location.strike_index))
        .copied()
        .flatten())
}

fn minimum_positive_option_quote_spread(
    observed_nodes: &[JointRefinementOptionNodeDefinition],
) -> Result<f64, String> {
    observed_nodes
        .iter()
        .map(|observed_node| observed_node.ask_price - observed_node.bid_price)
        .filter(|spread| spread.is_finite() && *spread > 0.0)
        .min_by(f64::total_cmp)
        .ok_or_else(|| {
            "joint option pricing tier contains no positive finite quote spread".to_string()
        })
}

fn option_quote_residual_scale(
    observed_node: &JointRefinementOptionNodeDefinition,
    minimum_positive_spread: f64,
) -> Result<f64, String> {
    let quote_spread = observed_node.ask_price - observed_node.bid_price;
    if !quote_spread.is_finite() || quote_spread <= 0.0 {
        return Err("joint option pricing node contains an invalid quote spread".to_string());
    }

    if !minimum_positive_spread.is_finite() || minimum_positive_spread <= 0.0 {
        return Err(
            "joint option pricing tier contains an invalid minimum quote spread".to_string(),
        );
    }

    Ok((0.5 * quote_spread).max(minimum_positive_spread))
}

fn spread_normalized_residual_gradient(
    model_price_sensitivity: VanillaOptionNodePriceSensitivity,
    observed_node: &JointRefinementOptionNodeDefinition,
    quote_scale_floor: f64,
) -> Vec<f64> {
    let spread_inverse = 1.0
        / option_quote_residual_scale(observed_node, quote_scale_floor)
            .unwrap_or(quote_scale_floor);

    vec![
        model_price_sensitivity.alpha_derivative * spread_inverse,
        model_price_sensitivity.beta_0_derivative * spread_inverse,
        model_price_sensitivity.delta_derivative * spread_inverse,
        model_price_sensitivity.delta_beta_derivative * spread_inverse,
    ]
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use janiform_calibration_artifacts::JointRefinementStageCarryInput;
    use janiform_calibration_artifacts::{
        CalibrationObjectivePolicy, CalibrationRunSpec, HawkesStageFittedParameters,
        MeanReversionStageDiagnostics, ReturnComponentsStageFittedParameters,
    };
    use janiform_engine_pricing::{
        AnchoredVanillaOptionPricingBoundaryInput, PricingIntervalSchedule,
        PricingIntervalSchedulePolicy, PricingProgressEvent, PricingProgressPhase,
        VanillaOptionGridBatchPricingExecutionInput, VanillaOptionGridBatchPricingRequest,
        VanillaOptionGridBatchPricingRunResult, VanillaOptionGridPricingResult,
        VanillaOptionGridRequest, VanillaOptionMaturitySummary, VanillaOptionPayoffType,
        VanillaOptionPricingModelParameters, VanillaOptionPricingOptionalOutputSelection,
        VanillaOptionPricingStatus, materialize_schedule_interval_factors,
        run_vanilla_option_grid_batch_pricing,
    };
    use janiform_foundations::time::NANOS_PER_HOUR;
    use janiform_market_time::calendar::ExchangeCalendarRegistry;
    use janiform_market_time::calendar::ExchangeCalendarRegistryLoader;
    use janiform_model_kernels::hawkes::HawkesEventSide;
    use serde::Deserialize;
    use serde_json::Value;

    use super::{
        build_option_boundary_input, build_option_grid_batch_pricing_request,
        build_option_surface_tier_pricing_artifact, build_surprise_multipliers_for_interval_grid,
        derive_pricing_interval_schedule, estimate_option_pricing_current_state,
        format_option_pricing_progress_item, minimum_positive_option_quote_spread,
        option_quote_residual_scale, pricing_event_progress_percent, pricing_nig_configuration,
        resolve_option_runtime_interval_schedule, validate_option_spot,
    };
    use crate::adapters::load_hourly_factor_contract_for_calibration;
    use crate::stages::hawkes::HawkesStageFittedPayload;
    use crate::stages::joint_refinement::contracts::{
        JointRefinementStageCurrentStateEstimationInput, JointRefinementStageOptionPricingInput,
        JointRefinementStageOptionSnapshotNode, JointRefinementStageSeasonalityFactorsInput,
    };
    use crate::stages::joint_refinement::execution::state::{
        JointRefinementOptionNodeDefinition, JointRefinementOptionNodeLocation,
        JointRefinementOptionPricingCacheKey, JointRefinementOptionTierPricingInputs,
    };
    use crate::stages::joint_refinement::option_surface_tiering::JointRefinementOptionSurfaceTierKind;
    use crate::stages::mean_reversion::MeanReversionStageFittedPayload;
    use crate::stages::return_components::ReturnComponentsStageFittedPayload;
    use crate::stages::seasonality::{
        RuntimeHourlySeasonalityFactorsBuildInput,
        build_runtime_hourly_seasonality_factors_from_components,
    };
    use janiform_calibration_artifacts::CalibrationStageHandoffTransport;
    use janiform_market_time::hourly_factors::{
        HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION, HourlyFactorContractDiagnostics,
        HourlyFactorContractOutput, HourlyFactorMaterializationMetadata,
        HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource,
        HourlySeasonalityFactorSource,
    };

    fn sample_option_pricing_input() -> JointRefinementStageOptionPricingInput {
        let valuation_time_utc_nanos = 0;
        let first_expiry_utc_nanos = NANOS_PER_HOUR;
        let second_expiry_utc_nanos = 2 * NANOS_PER_HOUR;
        let third_expiry_utc_nanos = 3 * NANOS_PER_HOUR;

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

    fn sample_option_pricing_input_with_schedule_policy(
        pricing_interval_schedule_policy: PricingIntervalSchedulePolicy,
    ) -> JointRefinementStageOptionPricingInput {
        let mut option_pricing_input = sample_option_pricing_input();
        option_pricing_input.pricing_interval_schedule_policy = pricing_interval_schedule_policy;

        option_pricing_input
    }

    #[test]
    fn transition_row_progress_lists_interval_before_row_detail() {
        let event = PricingProgressEvent {
            phase: PricingProgressPhase::TransitionRowProgress,
            completed_units: 0,
            total_units: 65,
            current_group_index: None,
            current_maturity_index: None,
            total_maturities: None,
            current_interval_index: Some(424),
            total_intervals: Some(1890),
            current_strike_index: None,
            total_strikes: None,
        };

        assert_eq!(
            format_option_pricing_progress_item(&event),
            "interval 425/1890"
        );
    }

    #[test]
    fn transition_row_progress_percent_includes_interval_and_row_position() {
        let event = PricingProgressEvent {
            phase: PricingProgressPhase::TransitionRowProgress,
            completed_units: 32,
            total_units: 64,
            current_group_index: None,
            current_maturity_index: None,
            total_maturities: None,
            current_interval_index: Some(4),
            total_intervals: Some(10),
            current_strike_index: None,
            total_strikes: None,
        };

        assert_eq!(pricing_event_progress_percent(&event), Some(45));
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

    fn build_test_hourly_factor_contract(
        request_start_utc_nanos: i64,
        open_hour_flags: &[bool],
    ) -> HourlyFactorContractOutput {
        let interval_count = open_hour_flags.len();

        let interval_start_utc_nanos = (0..interval_count)
            .map(|interval_index| request_start_utc_nanos + interval_index as i64 * NANOS_PER_HOUR)
            .collect::<Vec<_>>();

        let interval_end_utc_nanos = interval_start_utc_nanos
            .iter()
            .map(|interval_start_utc_nanos| interval_start_utc_nanos + NANOS_PER_HOUR)
            .collect::<Vec<_>>();

        let last_interval_end_utc_nanos = interval_end_utc_nanos[interval_end_utc_nanos.len() - 1];

        HourlyFactorContractOutput {
            schema_version: HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
            exchange_code: "XCME".to_string(),
            exchange_timezone_name: "UTC".to_string(),
            request_start_utc_nanos,
            request_end_utc_nanos: last_interval_end_utc_nanos,
            interval_duration_nanos: NANOS_PER_HOUR,
            interval_indices: (0..interval_count as u64).collect(),
            interval_start_utc_nanos,
            interval_end_utc_nanos,
            interval_start_exchange_local: vec![String::new(); interval_count],
            interval_end_exchange_local: vec![String::new(); interval_count],
            s_h: open_hour_flags
                .iter()
                .map(|is_open| if *is_open { 1.0 } else { 0.0 })
                .collect(),
            known_component_nu_h: vec![0.0; interval_count],
            surprise_component_nu_h: vec![0.0; interval_count],
            nu_h: vec![0.0; interval_count],
            metadata: HourlyFactorMaterializationMetadata {
                seasonality_materialization_method: "explicit".to_string(),
                news_intensity_materialization_method: "explicit".to_string(),
                interval_boundary_policy: "x",
                interval_alignment_policy: "x",
                timezone_transition_policy: "x",
                spring_forward_policy: "x",
                fall_back_policy: "x",
            },
            diagnostics: HourlyFactorContractDiagnostics {
                interval_count,
                interval_duration_nanos: NANOS_PER_HOUR,
                first_interval_start_utc_nanos: request_start_utc_nanos,
                last_interval_end_utc_nanos,
                skipped_local_hour_interval_count: 0,
                repeated_local_hour_interval_count: 0,
            },
        }
    }

    #[test]
    fn audit_runtime_schedule_stays_exact_when_primary_policy_is_activity_compressed() {
        let option_pricing_input = sample_option_pricing_input_with_schedule_policy(
            PricingIntervalSchedulePolicy::AutoActivityCompressed,
        );

        let hourly_factor_contract = build_test_hourly_factor_contract(
            option_pricing_input.valuation_time_utc_nanos,
            &[true, true, true],
        );

        let compressed_schedule = resolve_option_runtime_interval_schedule(
            &option_pricing_input,
            &hourly_factor_contract,
            false,
        )
        .expect("compressed runtime schedule should resolve");

        let exact_schedule = resolve_option_runtime_interval_schedule(
            &option_pricing_input,
            &hourly_factor_contract,
            true,
        )
        .expect("audit runtime schedule should resolve");

        assert_eq!(
            compressed_schedule.metadata.policy_id,
            "auto_activity_compressed.v1"
        );
        assert_eq!(exact_schedule.metadata.policy_id, "auto_exact_sparse.v1");
        assert!(compressed_schedule.interval_count() <= exact_schedule.interval_count());
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

    #[test]
    fn option_tier_pricing_artifact_preserves_node_and_maturity_scores() {
        let runtime_interval_schedule =
            sample_option_runtime_interval_schedule(&sample_option_pricing_input());

        let cache_key = JointRefinementOptionPricingCacheKey {
            tier_kind: JointRefinementOptionSurfaceTierKind::Selection,
            include_node_price_sensitivities: false,
            source_dynamics_parameter_bits: vec![1, 2, 3],
            pricing_parameter_bits: vec![4, 5],
        };

        let tier_inputs = JointRefinementOptionTierPricingInputs {
            runtime_interval_schedule: runtime_interval_schedule.clone(),
            interval_factors: Vec::new(),
            grid_batch_pricing_request: VanillaOptionGridBatchPricingRequest {
                spot: 100.0,
                grids: vec![
                    VanillaOptionGridRequest {
                        payoff_type: VanillaOptionPayoffType::Call,
                        risk_free_rate: 0.01,
                        dividend_yield: 0.005,
                        strikes: vec![100.0, 110.0],
                        maturity_intervals: vec![1.0],
                    },
                    VanillaOptionGridRequest {
                        payoff_type: VanillaOptionPayoffType::Put,
                        risk_free_rate: 0.01,
                        dividend_yield: 0.005,
                        strikes: vec![95.0],
                        maturity_intervals: vec![2.0],
                    },
                ],
            },
            spectral_collocation_node_count_override: None,
            observed_nodes: vec![
                artifact_observed_node(ArtifactObservedNodeInput {
                    expiry_timestamp_utc_nanos: NANOS_PER_HOUR,
                    strike: 100.0,
                    payoff_type: VanillaOptionPayoffType::Call,
                    grid_index: 0,
                    maturity_index: 0,
                    strike_index: 0,
                    maturity_weight: 2,
                    bid_price: 0.9,
                    ask_price: 1.1,
                }),
                artifact_observed_node(ArtifactObservedNodeInput {
                    expiry_timestamp_utc_nanos: NANOS_PER_HOUR,
                    strike: 110.0,
                    payoff_type: VanillaOptionPayoffType::Call,
                    grid_index: 0,
                    maturity_index: 0,
                    strike_index: 1,
                    maturity_weight: 2,
                    bid_price: 1.25,
                    ask_price: 1.75,
                }),
                artifact_observed_node(ArtifactObservedNodeInput {
                    expiry_timestamp_utc_nanos: 2 * NANOS_PER_HOUR,
                    strike: 95.0,
                    payoff_type: VanillaOptionPayoffType::Put,
                    grid_index: 1,
                    maturity_index: 0,
                    strike_index: 0,
                    maturity_weight: 1,
                    bid_price: 1.05,
                    ask_price: 1.35,
                }),
            ],
            maturity_intervals: vec![1.0, 2.0],
            maturity_terminal_interval_indices: vec![0, 1],
        };

        let pricing_result = VanillaOptionGridBatchPricingRunResult {
            spot: 100.0,
            grids: vec![
                artifact_grid_result(
                    VanillaOptionPayoffType::Call,
                    vec![100.0, 110.0],
                    1.0,
                    0,
                    vec![1.4, 2.0],
                ),
                artifact_grid_result(VanillaOptionPayoffType::Put, vec![95.0], 2.0, 1, vec![0.9]),
            ],
            runtime_interval_schedule,
        };

        let artifact = build_option_surface_tier_pricing_artifact(
            cache_key.clone(),
            &pricing_result,
            CalibrationObjectivePolicy::default(),
            0,
            &tier_inputs,
        )
        .expect("artifact should build from complete grid prices");
        assert_eq!(artifact.cache_key, cache_key);
        assert_eq!(
            artifact.tier_kind,
            JointRefinementOptionSurfaceTierKind::Selection
        );
        assert_eq!(artifact.node_diagnostics.len(), 3);
        assert_eq!(artifact.node_diagnostics[0].model_price, 1.4);

        let quote_scale_floor =
            minimum_positive_option_quote_spread(tier_inputs.observed_nodes.as_slice())
                .expect("fixture quote spreads should include a positive floor");
        let second_observed_node = &tier_inputs.observed_nodes[1];
        let second_quote_scale =
            option_quote_residual_scale(second_observed_node, quote_scale_floor)
                .expect("fixture quote spread should be valid");
        assert_eq!(
            artifact.node_diagnostics[1].spread_normalized_residual,
            (artifact.node_diagnostics[1].model_price - second_observed_node.observed_mid_price)
                / second_quote_scale
        );

        let third_observed_node = &tier_inputs.observed_nodes[2];
        let third_quote_scale = option_quote_residual_scale(third_observed_node, quote_scale_floor)
            .expect("fixture quote spread should be valid");
        assert_eq!(
            artifact.node_diagnostics[2].spread_normalized_residual,
            (0.9 - third_observed_node.observed_mid_price) / third_quote_scale
        );
        assert_eq!(artifact.loss_evaluation.family_losses.len(), 1);
        assert_eq!(
            artifact.loss_evaluation.family_losses[0].family_id,
            "options"
        );
        assert_eq!(artifact.maturity_scores.len(), 2);
        assert_eq!(
            artifact.maturity_scores[0].expiry_timestamp_utc_nanos,
            NANOS_PER_HOUR
        );
        assert_eq!(artifact.maturity_scores[0].maturity_weight, 2);
        assert_eq!(artifact.maturity_scores[0].quote_count, 2);
        assert_eq!(
            artifact.maturity_scores[1].expiry_timestamp_utc_nanos,
            2 * NANOS_PER_HOUR
        );
        assert_eq!(artifact.maturity_scores[1].maturity_weight, 1);
        assert_eq!(artifact.aggregate_diagnostics.len(), 5);
        assert_eq!(artifact.spectral_collocation_node_count, Some(7));

        let total_maturity_weight = artifact
            .maturity_scores
            .iter()
            .map(|maturity_score| maturity_score.maturity_weight)
            .sum::<usize>();
        let weighted_squared_maturity_fit_sum = artifact
            .maturity_scores
            .iter()
            .map(|maturity_score| {
                maturity_score.maturity_weight as f64
                    * maturity_score.root_mean_square_spread_normalized_residual
                    * maturity_score.root_mean_square_spread_normalized_residual
            })
            .sum::<f64>();
        assert_eq!(
            artifact.score.root_mean_square_spread_normalized_residual,
            (weighted_squared_maturity_fit_sum / total_maturity_weight as f64).sqrt()
        );
    }

    #[test]
    fn option_pricing_cache_key_separates_tier_dynamics_and_pricing_bits() {
        let base_key = JointRefinementOptionPricingCacheKey {
            tier_kind: JointRefinementOptionSurfaceTierKind::Search,
            include_node_price_sensitivities: false,
            source_dynamics_parameter_bits: vec![1, 2],
            pricing_parameter_bits: vec![3, 4],
        };

        assert_ne!(
            base_key,
            JointRefinementOptionPricingCacheKey {
                tier_kind: JointRefinementOptionSurfaceTierKind::Selection,
                include_node_price_sensitivities: false,
                source_dynamics_parameter_bits: vec![1, 2],
                pricing_parameter_bits: vec![3, 4],
            }
        );
        assert_ne!(
            base_key,
            JointRefinementOptionPricingCacheKey {
                tier_kind: JointRefinementOptionSurfaceTierKind::Search,
                include_node_price_sensitivities: false,
                source_dynamics_parameter_bits: vec![1, 5],
                pricing_parameter_bits: vec![3, 4],
            }
        );
        assert_ne!(
            base_key,
            JointRefinementOptionPricingCacheKey {
                tier_kind: JointRefinementOptionSurfaceTierKind::Search,
                include_node_price_sensitivities: false,
                source_dynamics_parameter_bits: vec![1, 2],
                pricing_parameter_bits: vec![3, 6],
            }
        );
    }

    struct ArtifactObservedNodeInput {
        expiry_timestamp_utc_nanos: i64,
        strike: f64,
        payoff_type: VanillaOptionPayoffType,
        grid_index: usize,
        maturity_index: usize,
        strike_index: usize,
        maturity_weight: usize,
        bid_price: f64,
        ask_price: f64,
    }

    fn artifact_observed_node(
        input: ArtifactObservedNodeInput,
    ) -> JointRefinementOptionNodeDefinition {
        let ArtifactObservedNodeInput {
            expiry_timestamp_utc_nanos,
            strike,
            payoff_type,
            grid_index,
            maturity_index,
            strike_index,
            maturity_weight,
            bid_price,
            ask_price,
        } = input;

        JointRefinementOptionNodeDefinition {
            quote_timestamp_utc_nanos: 0,
            expiry_timestamp_utc_nanos,
            strike,
            payoff_type,
            bid_price,
            ask_price,
            observed_mid_price: 0.5 * (bid_price + ask_price),
            maturity_weight,
            location: JointRefinementOptionNodeLocation {
                grid_index,
                maturity_index,
                strike_index,
            },
        }
    }

    fn artifact_grid_result(
        payoff_type: VanillaOptionPayoffType,
        strikes: Vec<f64>,
        maturity_interval: f64,
        terminal_interval_index: usize,
        prices: Vec<f64>,
    ) -> VanillaOptionGridPricingResult {
        let strike_count = strikes.len();

        VanillaOptionGridPricingResult {
            payoff_type,
            risk_free_rate: 0.01,
            dividend_yield: 0.005,
            strikes,
            maturity_intervals: vec![maturity_interval],
            maturity_terminal_interval_indices: vec![terminal_interval_index],
            price_grid: vec![prices.into_iter().map(Some).collect()],
            implied_volatility_grid: vec![vec![None; strike_count]],
            implied_volatility_inversion_status_grid: vec![vec![None; strike_count]],
            pricing_status_grid: vec![vec![VanillaOptionPricingStatus::Succeeded; strike_count]],
            pricing_failure_stage_grid: vec![vec![None; strike_count]],
            pricing_failure_reason_grid: vec![vec![None; strike_count]],
            forward_price_grid: vec![vec![None; strike_count]],
            discount_factor_grid: vec![vec![None; strike_count]],
            boundary_state_norm_grid: vec![vec![None; strike_count]],
            node_price_sensitivity_grid: None,
            maturity_summaries: vec![Some(VanillaOptionMaturitySummary {
                maturity_interval,
                terminal_anchor_value: 0.0,
                deviation_domain_lower_bound: -1.0,
                deviation_domain_upper_bound: 1.0,
                collocation_node_count: 7,
                collocation_nodes: Vec::new(),
                interval_summaries: Vec::new(),
                representative_transform: None,
                integration_quality: None,
            })],
        }
    }

    #[derive(Debug, Deserialize)]
    struct SeasonalityFactorsFixture {
        intraday_minutes_per_bin: u16,
        intraday_factor_by_bin: Vec<f64>,
        day_of_week_factor: Vec<f64>,
        hour_of_week_factor: Vec<f64>,
        day_of_year_factor: Vec<f64>,
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
            hour_of_week_factor: seasonality_fixture.hour_of_week_factor,
            day_of_year_factor: seasonality_fixture.day_of_year_factor,
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
                RuntimeHourlySeasonalityFactorsBuildInput {
                    calendar_registry: &calendar_registry,
                    exchange_code: run_spec.data_inputs.exchange_code.as_str(),
                    intraday_minutes_per_bin: seasonality_factors_input.intraday_minutes_per_bin,
                    intraday_factor_by_bin: seasonality_factors_input
                        .intraday_factor_by_bin
                        .as_slice(),
                    day_of_week_factor: seasonality_factors_input.day_of_week_factor.as_slice(),
                    day_of_year_factor: seasonality_factors_input.day_of_year_factor.as_slice(),
                    range_start_utc_nanos: hourly_coverage_start_utc_nanos,
                    range_end_utc_nanos: hourly_coverage_end_utc_nanos,
                    interval_duration_nanos: NANOS_PER_HOUR,
                },
            )
            .expect("runtime seasonality materialization should succeed");

        let hourly_coverage_interval_count =
            janiform_foundations::time::checked_interval_count_between_utc_nanos(
                hourly_coverage_start_utc_nanos,
                hourly_coverage_end_utc_nanos,
                NANOS_PER_HOUR,
            )
            .expect("test hourly coverage grid should be valid");

        let surprise_deterministic_time_multiplier_by_interval_index =
            build_surprise_multipliers_for_interval_grid(
                chrono::DateTime::<chrono::Utc>::from_timestamp_nanos(
                    hourly_coverage_start_utc_nanos,
                ),
                hourly_coverage_interval_count,
                NANOS_PER_HOUR,
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
                interval_duration_nanos: NANOS_PER_HOUR,
                seasonality_source: Some(HourlySeasonalityFactorSource::Explicit {
                    hourly_factors: runtime_hourly_seasonality_factors.as_slice(),
                    materialization_method: crate::runtime_seasonality::RUNTIME_COMPACT_SEASONALITY_MATERIALIZATION_METHOD,
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
                include_node_price_sensitivities: false,
            },
            spectral_collocation_node_count_override: None,
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
                spectral_collocation_node_count_override: None,
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
