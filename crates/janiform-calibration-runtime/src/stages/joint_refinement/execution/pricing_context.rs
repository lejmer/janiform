use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use janiform_engine_pricing::{
    AnchoredVanillaOptionPricingBoundaryInput, VanillaOptionGroupedBatchGroupRequest,
    VanillaOptionGroupedBatchPricingExecutionInput, VanillaOptionGroupedBatchPricingRequest,
    VanillaOptionGroupedBatchPricingRunResult, VanillaOptionPayoffType,
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection, run_vanilla_option_grouped_batch_pricing,
};
use janiform_foundations::time::ONE_HOUR_NANOS;
use janiform_market_time::hourly_factors::{
    HourlyFactorProviderRequest, HourlyNewsIntensityFactorSource, HourlySeasonalityFactorSource,
};

use crate::adapters::load_hourly_factor_contract_for_calibration;
use crate::calendar::ExchangeCalendarRegistry;
use crate::current_state::{
    CurrentStateEstimationInput, estimate_current_state_from_observed_data,
};
use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageOptionPricingInput,
};
use crate::stages::seasonality::build_runtime_hourly_seasonality_factors_from_components;
use janiform_calibration_artifacts::{
    JointRefinementStageOptionAggregateDiagnostic,
    JointRefinementStageOptionCalibrationDiagnostics, JointRefinementStageOptionNodeDiagnostic,
    JointRefinementStageOptionSnapshotMetadata,
};

use super::super::objective::root_mean_square;
use super::super::setup::build_surprise_multipliers_for_interval_grid;
use super::state::{
    JointRefinementContext, JointRefinementOptionNodeDefinition, JointRefinementOptionNodeLocation,
    JointRefinementOptionPricingContext,
};

struct OptionPricingHorizon {
    valuation_time_utc: DateTime<Utc>,
    max_expiry_timestamp_utc_nanos: i64,
    interval_count: usize,
}

struct OptionBucketDefinition {
    payoff_type: VanillaOptionPayoffType,
    strikes: Vec<f64>,
    maturity_intervals: Vec<f64>,
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
        option_pricing_input,
        return_components_payload,
        &horizon,
    )?;

    let interval_factors = build_option_interval_factors(&hourly_factor_contract)?;

    let boundary_input =
        build_option_boundary_input(mean_reversion_payload, &current_state_estimation_output)?;

    let (grouped_pricing_request, observed_nodes) =
        build_option_grouped_pricing_request(option_spot, option_pricing_input)?;

    Ok(Some(JointRefinementOptionPricingContext {
        valuation_time_utc_nanos: option_pricing_input.valuation_time_utc_nanos,
        grouped_pricing_request,
        boundary_input,
        interval_factors,
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
    let max_expiry_timestamp_utc_nanos = option_pricing_input
        .option_snapshot_nodes
        .iter()
        .map(|node| node.expiry_timestamp_utc_nanos)
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

    let option_horizon_nanos =
        max_expiry_timestamp_utc_nanos - option_pricing_input.valuation_time_utc_nanos;
    if option_horizon_nanos % ONE_HOUR_NANOS != 0 {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option calibration requires option expiries aligned to the 1h pricing grid"
                        .to_string(),
            },
        );
    }

    Ok(OptionPricingHorizon {
        valuation_time_utc: DateTime::<Utc>::from_timestamp_nanos(
            option_pricing_input.valuation_time_utc_nanos,
        ),
        max_expiry_timestamp_utc_nanos,
        interval_count: option_horizon_nanos.div_euclid(ONE_HOUR_NANOS) as usize,
    })
}

fn build_option_hourly_factor_contract(
    run_spec: &janiform_calibration_artifacts::CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: &crate::stages::joint_refinement::JointRefinementStageExecutionInput,
    option_pricing_input: &JointRefinementStageOptionPricingInput,
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
            option_pricing_input.valuation_time_utc_nanos,
            horizon.max_expiry_timestamp_utc_nanos,
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
            horizon.valuation_time_utc,
            horizon.interval_count,
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
            range_start_utc: horizon.valuation_time_utc,
            range_end_utc: DateTime::<Utc>::from_timestamp_nanos(
                horizon.max_expiry_timestamp_utc_nanos,
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
    .map_err(|source| JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
        error_message: format!(
            "joint option calibration hourly factor materialization failed: {source}"
        ),
    })
}

fn build_option_interval_factors(
    hourly_factor_contract: &janiform_market_time::hourly_factors::HourlyFactorContractOutput,
) -> Result<Vec<VanillaOptionPricingIntervalFactors>, JointRefinementStageExecutionFailure> {
    hourly_factor_contract
        .s_h
        .iter()
        .zip(hourly_factor_contract.known_component_nu_h.iter())
        .zip(hourly_factor_contract.surprise_component_nu_h.iter())
        .map(|((&s_h, &nu_h_known), &nu_h_surprise)| {
            VanillaOptionPricingIntervalFactors::try_new(s_h, nu_h_known, nu_h_surprise).map_err(
                |source| JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                    error_message: format!(
                        "joint option calibration interval factors are invalid: {source}"
                    ),
                },
            )
        })
        .collect()
}

fn build_option_boundary_input(
    mean_reversion_payload: &crate::stages::mean_reversion::MeanReversionStageFittedPayload,
    current_state_estimation_output: &crate::current_state::CurrentStateEstimationOutput,
) -> Result<AnchoredVanillaOptionPricingBoundaryInput, JointRefinementStageExecutionFailure> {
    AnchoredVanillaOptionPricingBoundaryInput::try_new(
        0,
        current_state_estimation_output.hawkes_excitation_state,
        // Intentional clone so the shared fitted payload remains available for later reporting.
        mean_reversion_payload.fitted_parameters.clone(),
        1.0,
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

fn build_option_grouped_pricing_request(
    spot: f64,
    option_pricing_input: &JointRefinementStageOptionPricingInput,
) -> Result<
    (
        VanillaOptionGroupedBatchPricingRequest,
        Vec<JointRefinementOptionNodeDefinition>,
    ),
    JointRefinementStageExecutionFailure,
> {
    let risk_free_rate = option_pricing_input.carry_input.discount_rate;
    let dividend_yield = option_pricing_input.carry_input.discount_rate
        - option_pricing_input.carry_input.carry_rate;

    let mut groups = Vec::new();
    let mut observed_nodes = Vec::new();

    for payoff_type in [VanillaOptionPayoffType::Call, VanillaOptionPayoffType::Put] {
        let payoff_nodes = option_nodes_for_payoff_type(option_pricing_input, payoff_type);
        if payoff_nodes.is_empty() {
            continue;
        }

        let bucket_definition =
            build_option_bucket_definition(&payoff_nodes, option_pricing_input)?;

        let group_index = groups.len();

        observed_nodes.extend(build_observed_option_nodes(
            group_index,
            &bucket_definition,
            &payoff_nodes,
            option_pricing_input,
        )?);

        groups.push(VanillaOptionGroupedBatchGroupRequest {
            payoff_type: bucket_definition.payoff_type,
            risk_free_rate,
            dividend_yield,
            strikes: bucket_definition.strikes,
            maturity_intervals: bucket_definition.maturity_intervals,
        });
    }

    Ok((
        VanillaOptionGroupedBatchPricingRequest { spot, groups },
        observed_nodes,
    ))
}

fn option_nodes_for_payoff_type<'a>(
    option_pricing_input: &'a JointRefinementStageOptionPricingInput,
    payoff_type: VanillaOptionPayoffType,
) -> Vec<&'a crate::stages::joint_refinement::JointRefinementStageOptionSnapshotNode> {
    option_pricing_input
        .option_snapshot_nodes
        .iter()
        .filter(|node| node.payoff_type == payoff_type)
        .collect::<Vec<_>>()
}

fn build_option_bucket_definition(
    payoff_nodes: &[&crate::stages::joint_refinement::JointRefinementStageOptionSnapshotNode],
    option_pricing_input: &JointRefinementStageOptionPricingInput,
) -> Result<OptionBucketDefinition, JointRefinementStageExecutionFailure> {
    let mut strikes = payoff_nodes
        .iter()
        .map(|node| node.strike)
        .collect::<Vec<_>>();

    strikes.sort_by(|left, right| left.total_cmp(right));
    strikes.dedup_by(|left, right| left.total_cmp(right).is_eq());

    let mut maturity_intervals = payoff_nodes
        .iter()
        .map(|node| maturity_interval_hours(node.expiry_timestamp_utc_nanos, option_pricing_input))
        .collect::<Result<Vec<_>, _>>()?;

    maturity_intervals.sort_by(|left, right| left.total_cmp(right));
    maturity_intervals.dedup_by(|left, right| left.total_cmp(right).is_eq());

    Ok(OptionBucketDefinition {
        payoff_type: payoff_nodes[0].payoff_type,
        strikes,
        maturity_intervals,
    })
}

fn build_observed_option_nodes(
    group_index: usize,
    bucket_definition: &OptionBucketDefinition,
    payoff_nodes: &[&crate::stages::joint_refinement::JointRefinementStageOptionSnapshotNode],
    option_pricing_input: &JointRefinementStageOptionPricingInput,
) -> Result<Vec<JointRefinementOptionNodeDefinition>, JointRefinementStageExecutionFailure> {
    payoff_nodes
        .iter()
        .map(|node| {
            let maturity_interval =
                maturity_interval_hours(node.expiry_timestamp_utc_nanos, option_pricing_input)?;
            let strike_index =
                locate_option_grid_coordinate(&bucket_definition.strikes, node.strike, "strike")?;
            let maturity_index = locate_option_grid_coordinate(
                &bucket_definition.maturity_intervals,
                maturity_interval,
                "maturity",
            )?;

            Ok(JointRefinementOptionNodeDefinition {
                quote_timestamp_utc_nanos: node.quote_timestamp_utc_nanos,
                expiry_timestamp_utc_nanos: node.expiry_timestamp_utc_nanos,
                strike: node.strike,
                payoff_type: node.payoff_type,
                bid_price: node.bid_price,
                ask_price: node.ask_price,
                observed_mid_price: node.observed_mid_price,
                location: JointRefinementOptionNodeLocation {
                    group_index,
                    maturity_index,
                    strike_index,
                },
            })
        })
        .collect()
}

fn maturity_interval_hours(
    expiry_timestamp_utc_nanos: i64,
    option_pricing_input: &JointRefinementStageOptionPricingInput,
) -> Result<f64, JointRefinementStageExecutionFailure> {
    let maturity_nanos = expiry_timestamp_utc_nanos - option_pricing_input.valuation_time_utc_nanos;
    if maturity_nanos <= 0 || maturity_nanos % ONE_HOUR_NANOS != 0 {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option calibration requires expiries aligned to positive 1h maturities"
                        .to_string(),
            },
        );
    }

    Ok(maturity_nanos as f64 / ONE_HOUR_NANOS as f64)
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

fn run_option_grouped_pricing(
    context: &JointRefinementContext,
    fitted_parameters: &janiform_calibration_artifacts::JointRefinementStageFittedParameters,
) -> Result<VanillaOptionGroupedBatchPricingRunResult, String> {
    let Some(option_pricing_context) = context.option_pricing_context.as_ref() else {
        return Err("joint option pricing context is unavailable".to_string());
    };

    run_vanilla_option_grouped_batch_pricing(VanillaOptionGroupedBatchPricingExecutionInput {
        request: option_pricing_context.grouped_pricing_request.clone(),
        boundary_input: option_pricing_context.boundary_input.clone(),
        model_parameters: VanillaOptionPricingModelParameters::new(
            fitted_parameters.hawkes_fitted_parameters.kernel_parameters,
            fitted_parameters
                .return_components_fitted_parameters
                .news_process_configuration
                .clone(),
            pricing_nig_configuration(fitted_parameters),
        ),
        interval_delta: 1.0,
        fourier_inversion_settings: None,
        interval_factors: option_pricing_context.interval_factors.clone(),
        optional_output_selection: VanillaOptionPricingOptionalOutputSelection {
            include_transform_diagnostics: false,
            include_integration_quality_diagnostics: false,
            include_calibration_hooks: false,
        },
        cpu_count: Some(option_pricing_context.cpu_count),
    })
    .map_err(|source| format!("joint option pricing evaluation failed: {source}"))
}

pub(crate) fn option_spread_normalized_price_fit(
    context: &JointRefinementContext,
    fitted_parameters: &janiform_calibration_artifacts::JointRefinementStageFittedParameters,
) -> Result<f64, String> {
    let Some(option_pricing_context) = context.option_pricing_context.as_ref() else {
        return Ok(0.0);
    };

    let pricing_result = run_option_grouped_pricing(context, fitted_parameters)?;

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

    let pricing_result = run_option_grouped_pricing(context, fitted_parameters)?;

    let (node_diagnostics, maturity_bucket_residuals, strike_bucket_residuals) =
        build_option_node_diagnostics(&pricing_result, option_pricing_context)?;
    let aggregate_diagnostics =
        build_option_aggregate_diagnostics(maturity_bucket_residuals, strike_bucket_residuals);

    Ok(Some(JointRefinementStageOptionCalibrationDiagnostics {
        snapshot_metadata: JointRefinementStageOptionSnapshotMetadata {
            valuation_time_utc_nanos: option_pricing_context.valuation_time_utc_nanos,
            node_count: node_diagnostics.len(),
        },
        carry_input: option_pricing_context.carry_input.clone(),
        node_diagnostics,
        aggregate_diagnostics,
    }))
}

type ResidualBuckets = BTreeMap<String, Vec<f64>>;

fn build_option_node_diagnostics(
    pricing_result: &VanillaOptionGroupedBatchPricingRunResult,
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
    pricing_result: &VanillaOptionGroupedBatchPricingRunResult,
    observed_node: &JointRefinementOptionNodeDefinition,
    error_message: &str,
) -> Result<f64, String> {
    let group = pricing_result
        .groups
        .get(observed_node.location.group_index)
        .ok_or_else(|| error_message.to_string())?;
    let model_price = group
        .price_grid
        .get(observed_node.location.maturity_index)
        .and_then(|row| row.get(observed_node.location.strike_index))
        .copied()
        .ok_or_else(|| error_message.to_string())?;

    if let Some(model_price) = model_price {
        return Ok(model_price);
    }

    let failure_stage = group
        .pricing_failure_stage_grid
        .get(observed_node.location.maturity_index)
        .and_then(|row| row.get(observed_node.location.strike_index))
        .and_then(|stage| *stage)
        .map(|stage| format!("{stage:?}"))
        .unwrap_or_else(|| "unknown".to_string());
    let failure_reason = group
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
