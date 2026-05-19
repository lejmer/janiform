use std::collections::{BTreeMap, BTreeSet};

use crate::contracts::{
    SpectralPricingDiagnostics, VanillaOptionPayoffConfiguration, VanillaOptionPayoffType,
    VanillaOptionPricingMarketInputs, VanillaOptionPricingRequest,
};
use crate::grids::{
    SpectralThetaWarmStartState, VanillaOptionGridBatchPricingExecutionInput,
    VanillaOptionGridBatchPricingRunError, VanillaOptionGridBatchPricingRunResult,
    VanillaOptionGridPricingResult, VanillaOptionGridRequest,
};
use crate::implied_volatility::VanillaOptionImpliedVolatilityInversionStatus;
use crate::progress::PricingProgressEvent;
use crate::runner::{
    VanillaOptionPricingExecutionInput, VanillaOptionPricingRunError,
    build_pricing_sufficient_statistics_neutral_anchor_materialization,
};
use crate::thread_pool::build_pricing_thread_pool;

#[cfg(test)]
use super::asset_reference_log_price_from_observed_spot;
#[cfg(test)]
use super::domain::{
    SpectralDomain, observed_log_domain_half_width,
    spectral_collocation_node_count_from_domain_points,
};
use super::domain::{build_maturity_summary, build_spectral_domain};
use super::interval_operator::{
    CachedIntervalOperatorBuildInput, SolvedMaturityLaw, build_cached_interval_operators,
    solve_maturity_law,
};
#[cfg(test)]
use super::payoff::{
    cos_price_from_terminal_characteristic_function, payoff_cos_coefficient,
    strike_log_boundary_from_reference_log,
};
#[cfg(test)]
use super::row_frequency::{
    IntervalRowFrequencyBatchAdaptiveSolveInput, solve_interval_row_frequency_batches_adaptively,
};
use super::strike::{StrikeRowPricingInput, price_strike_row};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::spectral::pricer) struct CarryGroupKey {
    pub(in crate::spectral::pricer) risk_free_rate_bits: u64,
    pub(in crate::spectral::pricer) dividend_yield_bits: u64,
}

impl CarryGroupKey {
    fn from_grid(grid: &VanillaOptionGridRequest) -> Self {
        Self {
            risk_free_rate_bits: grid.risk_free_rate.to_bits(),
            dividend_yield_bits: grid.dividend_yield.to_bits(),
        }
    }
}

#[derive(Debug, Clone)]
struct CarryGroupSolveContext {
    maturity_laws: BTreeMap<usize, SolvedMaturityLaw>,
}

#[derive(Debug, Clone)]
pub(crate) struct SingleOptionSpectralOutcome {
    pub(crate) price: f64,
    pub(crate) implied_volatility: Option<f64>,
    pub(crate) implied_volatility_inversion_status: VanillaOptionImpliedVolatilityInversionStatus,
    pub(crate) representative_transform:
        Option<janiform_affine_oracle::affine_oracle::ExponentialAffineTransformIntegrationResult>,
    pub(crate) spectral_diagnostics: SpectralPricingDiagnostics,
}

pub fn run_vanilla_option_grid_batch_pricing_with_observer_and_theta_warm_start_state(
    execution_input: VanillaOptionGridBatchPricingExecutionInput,
    mut observer: Option<&mut dyn FnMut(PricingProgressEvent)>,
    theta_warm_start_state: Option<&mut SpectralThetaWarmStartState>,
) -> Result<VanillaOptionGridBatchPricingRunResult, VanillaOptionGridBatchPricingRunError> {
    execution_input.validate().map_err(|source| {
        VanillaOptionGridBatchPricingRunError::InvalidExecutionInput {
            source: Box::new(source),
        }
    })?;

    let pricing_thread_pool = build_pricing_thread_pool(execution_input.cpu_count);

    let mut carry_groups = prepare_carry_group_contexts(
        &execution_input,
        pricing_thread_pool.as_ref(),
        execution_input.cpu_count,
        &mut observer,
        theta_warm_start_state,
    )
    .map_err(|message| VanillaOptionGridBatchPricingRunError::NodePricingFailure { message })?;

    let total_grids = execution_input.request.grids.len();

    emit_pricing_progress(
        &mut observer,
        PricingProgressEvent::group_progress(0, total_grids, Some(0)),
    );

    let mut grid_results = Vec::with_capacity(total_grids);

    for (grid_index, grid) in execution_input.request.grids.iter().enumerate() {
        let carry_group_key = CarryGroupKey::from_grid(grid);

        let carry_context = carry_groups.get_mut(&carry_group_key).ok_or_else(|| {
            VanillaOptionGridBatchPricingRunError::NodePricingFailure {
                message: "missing carry-group spectral pricing context".to_string(),
            }
        })?;

        grid_results.push(build_grid_result(
            &execution_input,
            grid,
            carry_context,
            pricing_thread_pool.as_ref(),
            execution_input.cpu_count,
            &mut observer,
        )?);

        let next_group_index = if grid_index + 1 < total_grids {
            Some(grid_index + 1)
        } else {
            None
        };

        emit_pricing_progress(
            &mut observer,
            PricingProgressEvent::group_progress(grid_index + 1, total_grids, next_group_index),
        );
    }

    Ok(VanillaOptionGridBatchPricingRunResult {
        spot: execution_input.request.spot,
        grids: grid_results,
        runtime_interval_schedule: execution_input.runtime_interval_schedule,
    })
}

pub(crate) fn run_single_option_pricing(
    execution_input: &VanillaOptionPricingExecutionInput,
) -> Result<SingleOptionSpectralOutcome, VanillaOptionPricingRunError> {
    let request = &execution_input.request;
    let market_inputs = &request.market_inputs;

    let strike = market_inputs.single_strike().map_err(|source| {
        VanillaOptionPricingRunError::InvalidExecutionInput {
            source: Box::new(
                crate::runner::VanillaOptionPricingExecutionInputError::InvalidRequest {
                    source:
                        crate::contracts::VanillaOptionPricingRequestError::InvalidMarketInputs {
                            source,
                        },
                },
            ),
        }
    })?;

    let maturity_interval = market_inputs.single_maturity_interval().map_err(|source| {
        VanillaOptionPricingRunError::InvalidExecutionInput {
            source: Box::new(
                crate::runner::VanillaOptionPricingExecutionInputError::InvalidRequest {
                    source:
                        crate::contracts::VanillaOptionPricingRequestError::InvalidMarketInputs {
                            source,
                        },
                },
            ),
        }
    })?;

    let grid_execution_input = VanillaOptionGridBatchPricingExecutionInput {
        request: crate::grids::VanillaOptionGridBatchPricingRequest {
            spot: market_inputs.spot,
            grids: vec![VanillaOptionGridRequest {
                payoff_type: request.payoff.payoff_type,
                risk_free_rate: market_inputs.risk_free_rate,
                dividend_yield: market_inputs.dividend_yield,
                strikes: vec![strike],
                maturity_intervals: vec![maturity_interval],
            }],
        },
        boundary_input: request.boundary_input.clone(),
        model_parameters: request.model_parameters.clone(),
        runtime_interval_schedule: execution_input.runtime_interval_schedule.clone(),
        interval_delta: request.interval_delta,
        interval_factors: execution_input.interval_factors.clone(),
        optional_output_selection: execution_input.optional_output_selection,
        spectral_collocation_node_count_override: None,
        cpu_count: None,
    };

    let run_result =
        run_vanilla_option_grid_batch_pricing_with_observer_and_theta_warm_start_state(
            grid_execution_input,
            None,
            None,
        )
        .map_err(|source| match source {
            VanillaOptionGridBatchPricingRunError::InvalidExecutionInput { source } => {
                VanillaOptionPricingRunError::InternalNumericsFailure {
                    message: format!(
                        "spectral single-option grid execution input became invalid: {source}"
                    ),
                }
            }
            VanillaOptionGridBatchPricingRunError::InvalidDriverParameters { source } => {
                VanillaOptionPricingRunError::InvalidDriverParameters { source }
            }
            VanillaOptionGridBatchPricingRunError::NodePricingFailure { message } => {
                VanillaOptionPricingRunError::InternalNumericsFailure { message }
            }
        })?;

    let grid = run_result.grids.first().ok_or_else(|| {
        VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "spectral single-option pricing returned no grid".to_string(),
        }
    })?;

    let price = grid
        .price_grid
        .first()
        .and_then(|row| row.first())
        .and_then(|value| *value)
        .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "spectral single-option pricing returned no price".to_string(),
        })?;

    let implied_volatility = grid
        .implied_volatility_grid
        .first()
        .and_then(|row| row.first())
        .and_then(|value| *value);

    let implied_volatility_inversion_status = grid
        .implied_volatility_inversion_status_grid
        .first()
        .and_then(|row| row.first())
        .and_then(|value| *value)
        .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "spectral single-option pricing returned no IV status".to_string(),
        })?;

    let maturity_summary = grid
        .maturity_summaries
        .first()
        .and_then(|summary| summary.clone())
        .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "spectral single-option pricing returned no maturity summary".to_string(),
        })?;

    let spectral_diagnostics = SpectralPricingDiagnostics::try_new(
        maturity_summary.deviation_domain_lower_bound,
        maturity_summary.deviation_domain_upper_bound,
        maturity_summary.collocation_node_count,
        maturity_summary.interval_summaries.clone(),
    )
    .map_err(
        |source| VanillaOptionPricingRunError::InvalidSpectralDiagnostics {
            source: Box::new(source),
        },
    )?;

    Ok(SingleOptionSpectralOutcome {
        price,
        implied_volatility,
        implied_volatility_inversion_status,
        representative_transform: maturity_summary.representative_transform,
        spectral_diagnostics,
    })
}

fn prepare_carry_group_contexts(
    execution_input: &VanillaOptionGridBatchPricingExecutionInput,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
    observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
    mut theta_warm_start_state: Option<&mut SpectralThetaWarmStartState>,
) -> Result<BTreeMap<CarryGroupKey, CarryGroupSolveContext>, String> {
    let mut grouped_strikes = BTreeMap::<CarryGroupKey, BTreeSet<u64>>::new();

    for grid in &execution_input.request.grids {
        let carry_group_key = CarryGroupKey::from_grid(grid);
        let strike_bits = grouped_strikes.entry(carry_group_key).or_default();

        for strike in &grid.strikes {
            strike_bits.insert(strike.to_bits());
        }
    }

    let mut carry_groups = BTreeMap::new();

    for (carry_group_key, strike_bits) in grouped_strikes {
        let union_strikes = strike_bits
            .into_iter()
            .map(f64::from_bits)
            .collect::<Vec<_>>();

        let first_grid = execution_input
            .request
            .grids
            .iter()
            .find(|grid| CarryGroupKey::from_grid(grid) == carry_group_key)
            .ok_or_else(|| "missing representative grid for carry group".to_string())?;

        let max_maturity_interval = execution_input
            .request
            .grids
            .iter()
            .filter(|grid| CarryGroupKey::from_grid(grid) == carry_group_key)
            .flat_map(|grid| grid.maturity_intervals.iter().copied())
            .max_by(|left, right| left.total_cmp(right))
            .ok_or_else(|| "carry group requires at least one maturity".to_string())?;

        let dummy_strike = union_strikes
            .first()
            .copied()
            .ok_or_else(|| "carry group requires at least one strike".to_string())?;

        let base_request = VanillaOptionPricingRequest::try_new(
            VanillaOptionPricingMarketInputs::new_single_node(
                execution_input.request.spot,
                dummy_strike,
                max_maturity_interval,
                first_grid.risk_free_rate,
                first_grid.dividend_yield,
            ),
            VanillaOptionPayoffConfiguration {
                payoff_type: VanillaOptionPayoffType::Call,
            },
            execution_input.boundary_input.clone(),
            execution_input.model_parameters.clone(),
            execution_input.interval_delta,
        )
        .map_err(|source| format!("failed to construct spectral carry-group request: {source}"))?;

        let driver_parameters = execution_input
            .model_parameters
            .nig_shared_plus_shift_configuration
            .derive_driver_parameters()
            .map_err(|source| format!("failed to derive pricing driver parameters: {source}"))?;

        let mut no_observer = None;

        let materialization = build_pricing_sufficient_statistics_neutral_anchor_materialization(
            &base_request,
            &execution_input.runtime_interval_schedule,
            execution_input.interval_factors.as_slice(),
            driver_parameters,
            &mut no_observer,
        )
        .map_err(|source| format!("failed to materialize spectral pricing anchors: {source}"))?;

        let unique_maturity_counts = execution_input
            .request
            .grids
            .iter()
            .filter(|grid| CarryGroupKey::from_grid(grid) == carry_group_key)
            .flat_map(|grid| {
                grid.maturity_intervals.iter().map(|maturity_interval| {
                    execution_input
                        .runtime_interval_schedule
                        .terminal_interval_index_for_maturity_interval(*maturity_interval)
                        .map(|terminal_index| terminal_index + 1)
                        .map_err(|source| source.to_string())
                })
            })
            .collect::<Result<BTreeSet<_>, _>>()?;

        let maximum_maturity_count = unique_maturity_counts
            .iter()
            .next_back()
            .copied()
            .ok_or_else(|| "carry group requires at least one resolved maturity".to_string())?;

        let spectral_domain = build_spectral_domain(
            &base_request,
            &materialization,
            maximum_maturity_count,
            max_maturity_interval,
            union_strikes.as_slice(),
            execution_input.spectral_collocation_node_count_override,
        )
        .map_err(|source| format!("failed to construct spectral domain: {source}"))?;

        let cached_interval_operators =
            build_cached_interval_operators(CachedIntervalOperatorBuildInput {
                carry_group_key,
                pricing_request: &base_request,
                include_node_price_sensitivities: execution_input
                    .optional_output_selection
                    .include_node_price_sensitivities,
                interval_factors: execution_input.interval_factors.as_slice(),
                materialization: &materialization,
                maximum_maturity_count,
                spectral_domain,
                driver_parameters,
                pricing_thread_pool,
                cpu_count,
                observer,
                theta_warm_start_state: theta_warm_start_state.as_deref_mut(),
            })
            .map_err(|source| format!("spectral interval-operator assembly failed: {source}"))?;

        let mut maturity_laws = BTreeMap::new();

        let total_maturity_solves = unique_maturity_counts.len();
        if total_maturity_solves > 0 {
            emit_pricing_progress(
                observer,
                PricingProgressEvent::maturity_progress(
                    0,
                    total_maturity_solves,
                    Some(0),
                    total_maturity_solves,
                ),
            );
        }

        for (maturity_position, maturity_count) in unique_maturity_counts.into_iter().enumerate() {
            let solved_maturity_law = solve_maturity_law(
                &base_request,
                &materialization,
                maturity_count,
                spectral_domain,
                cached_interval_operators.as_slice(),
            )
            .map_err(|source| format!("spectral maturity solve failed: {source}"))?;

            maturity_laws.insert(maturity_count, solved_maturity_law);

            let next_maturity_index = if maturity_position + 1 < total_maturity_solves {
                Some(maturity_position + 1)
            } else {
                None
            };

            emit_pricing_progress(
                observer,
                PricingProgressEvent::maturity_progress(
                    maturity_position + 1,
                    total_maturity_solves,
                    next_maturity_index,
                    total_maturity_solves,
                ),
            );
        }

        carry_groups.insert(carry_group_key, CarryGroupSolveContext { maturity_laws });
    }

    Ok(carry_groups)
}

fn build_grid_result(
    execution_input: &VanillaOptionGridBatchPricingExecutionInput,
    grid: &VanillaOptionGridRequest,
    carry_context: &CarryGroupSolveContext,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
    observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<VanillaOptionGridPricingResult, VanillaOptionGridBatchPricingRunError> {
    let mut price_grid = Vec::with_capacity(grid.maturity_intervals.len());

    let mut implied_volatility_grid = Vec::with_capacity(grid.maturity_intervals.len());
    let mut implied_volatility_status_grid = Vec::with_capacity(grid.maturity_intervals.len());

    let mut pricing_status_grid = Vec::with_capacity(grid.maturity_intervals.len());
    let mut pricing_failure_stage_grid = Vec::with_capacity(grid.maturity_intervals.len());
    let mut pricing_failure_reason_grid = Vec::with_capacity(grid.maturity_intervals.len());

    let mut forward_price_grid = Vec::with_capacity(grid.maturity_intervals.len());
    let mut discount_factor_grid = Vec::with_capacity(grid.maturity_intervals.len());
    let mut boundary_state_norm_grid = Vec::with_capacity(grid.maturity_intervals.len());
    let mut node_price_sensitivity_grid = execution_input
        .optional_output_selection
        .include_node_price_sensitivities
        .then(|| Vec::with_capacity(grid.maturity_intervals.len()));

    let mut maturity_summaries = Vec::with_capacity(grid.maturity_intervals.len());
    let mut maturity_terminal_interval_indices = Vec::with_capacity(grid.maturity_intervals.len());

    let total_maturities = grid.maturity_intervals.len();
    if total_maturities > 0 {
        emit_pricing_progress(
            observer,
            PricingProgressEvent::maturity_progress(0, total_maturities, Some(0), total_maturities),
        );
    }

    for (maturity_index, maturity_interval) in grid.maturity_intervals.iter().copied().enumerate() {
        let maturity_count = execution_input
            .runtime_interval_schedule
            .terminal_interval_index_for_maturity_interval(maturity_interval)
            .map(|terminal_index| terminal_index + 1)
            .map_err(
                |message| VanillaOptionGridBatchPricingRunError::NodePricingFailure {
                    message: message.to_string(),
                },
            )?;

        maturity_terminal_interval_indices.push(maturity_count - 1);

        let solved_maturity = carry_context
            .maturity_laws
            .get(&maturity_count)
            .ok_or_else(
                || VanillaOptionGridBatchPricingRunError::NodePricingFailure {
                    message: format!(
                        "missing spectral maturity law for maturity_count={maturity_count}"
                    ),
                },
            )?;

        let priced_row = price_strike_row(StrikeRowPricingInput {
            spot: execution_input.request.spot,
            grid,
            solved_maturity,
            boundary_state: execution_input.boundary_input.hawkes_boundary_state,
            pricing_thread_pool,
            cpu_count,
            maturity_index,
            total_maturities,
            observer,
        });

        let representative_price = priced_row.price_row.iter().flatten().copied().next();

        price_grid.push(priced_row.price_row);

        implied_volatility_grid.push(priced_row.implied_volatility_row);
        implied_volatility_status_grid.push(priced_row.implied_volatility_status_row);

        pricing_status_grid.push(priced_row.pricing_status_row);
        pricing_failure_stage_grid.push(priced_row.pricing_failure_stage_row);
        pricing_failure_reason_grid.push(priced_row.pricing_failure_reason_row);

        forward_price_grid.push(priced_row.forward_price_row);
        discount_factor_grid.push(priced_row.discount_factor_row);
        boundary_state_norm_grid.push(priced_row.boundary_state_norm_row);

        if let Some(node_price_sensitivity_grid) = node_price_sensitivity_grid.as_mut() {
            node_price_sensitivity_grid.push(priced_row.node_price_sensitivity_row);
        }

        maturity_summaries.push(Some(build_maturity_summary(
            solved_maturity,
            execution_input.optional_output_selection,
            representative_price,
        )?));

        let next_maturity_index = if maturity_index + 1 < total_maturities {
            Some(maturity_index + 1)
        } else {
            None
        };

        emit_pricing_progress(
            observer,
            PricingProgressEvent::maturity_progress(
                maturity_index + 1,
                total_maturities,
                next_maturity_index,
                total_maturities,
            ),
        );
    }

    Ok(VanillaOptionGridPricingResult {
        payoff_type: grid.payoff_type,
        risk_free_rate: grid.risk_free_rate,
        dividend_yield: grid.dividend_yield,
        strikes: grid.strikes.clone(),
        maturity_intervals: grid.maturity_intervals.clone(),
        maturity_terminal_interval_indices,
        price_grid,
        implied_volatility_grid,
        implied_volatility_inversion_status_grid: implied_volatility_status_grid,
        pricing_status_grid,
        pricing_failure_stage_grid,
        pricing_failure_reason_grid,
        forward_price_grid,
        discount_factor_grid,
        boundary_state_norm_grid,
        node_price_sensitivity_grid,
        maturity_summaries,
    })
}

pub(in crate::spectral::pricer) fn emit_pricing_progress(
    observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
    event: PricingProgressEvent,
) {
    if let Some(observer) = observer.as_deref_mut() {
        observer(event);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        IntervalRowFrequencyBatchAdaptiveSolveInput, SolvedMaturityLaw, SpectralDomain,
        asset_reference_log_price_from_observed_spot, build_spectral_domain,
        cos_price_from_terminal_characteristic_function, observed_log_domain_half_width,
        payoff_cos_coefficient, solve_interval_row_frequency_batches_adaptively,
        spectral_collocation_node_count_from_domain_points, strike_log_boundary_from_reference_log,
    };
    use crate::constants::MINIMUM_SPECTRAL_COSINE_COLLOCATION_NODE_COUNT;
    use crate::contracts::{
        AnchoredVanillaOptionPricingBoundaryInput, VanillaOptionPayoffConfiguration,
        VanillaOptionPayoffType, VanillaOptionPricingMarketInputs, VanillaOptionPricingRequest,
    };
    use crate::runner::{
        LocalTransitionRequestContext, VanillaOptionPricingIntervalFactors,
        build_pricing_sufficient_statistics_neutral_anchor_materialization,
        certify_local_transition_theta_for_row_with_parallelism,
    };
    use crate::tests::support::{build_uniform_runtime_interval_schedule, valid_model_parameters};
    use janiform_foundations::complex_math::{ComplexScalar, complex_exponential};
    use janiform_model_kernels::hawkes::HawkesExcitationState;
    use janiform_model_kernels::mean_reversion::{
        MeanReversionAnchorModel, MeanReversionBoundaryState, MeanReversionFittedParameters,
    };

    #[test]
    fn spectral_domain_half_width_is_derived_from_envelope_points() {
        let half_width = observed_log_domain_half_width(0.2, &[0.2, -0.1, 0.35, 0.05])
            .expect("non-degenerate envelope should resolve");

        assert!((half_width - 0.3).abs() <= f64::EPSILON);
    }

    #[test]
    fn spectral_domain_half_width_rejects_degenerate_envelope() {
        let half_width = observed_log_domain_half_width(0.2, &[0.2, 0.2]);

        assert_eq!(half_width, None);
    }

    #[test]
    fn default_spectral_collocation_node_count_tracks_distinct_envelope_points() {
        let node_count =
            spectral_collocation_node_count_from_domain_points(&[0.0, 0.2, 0.2, -0.1, 0.5]);

        assert_eq!(node_count, 4);
        assert_eq!(
            spectral_collocation_node_count_from_domain_points(&[0.0]),
            MINIMUM_SPECTRAL_COSINE_COLLOCATION_NODE_COUNT
        );
    }

    #[test]
    fn cos_payoff_coefficients_use_reference_price_scale() {
        let base_reference_price = 100.0_f64;
        let base_strike = 100.0_f64;
        let shifted_spot = 200.0_f64;
        let shifted_strike = 200.0_f64;

        let observed_log_price = shifted_spot.ln() - base_reference_price.ln();

        let shifted_reference_log_price =
            asset_reference_log_price_from_observed_spot(shifted_spot, observed_log_price)
                .expect("reference log price should resolve");

        let shifted_reference_price = shifted_reference_log_price.exp();

        let log_shift = shifted_strike.ln() - base_strike.ln();

        let reference_price_tolerance = f64::EPSILON.sqrt() * base_reference_price;
        assert!(
            (shifted_reference_price - base_reference_price).abs() <= reference_price_tolerance,
            "shifted reference price {shifted_reference_price} should resolve to base reference price {base_reference_price}",
        );

        let base_strike_boundary =
            strike_log_boundary_from_reference_log(base_strike, base_reference_price.ln())
                .expect("base strike boundary should resolve");
        let shifted_strike_boundary =
            strike_log_boundary_from_reference_log(shifted_strike, shifted_reference_log_price)
                .expect("shifted strike boundary should resolve");

        assert_eq!(base_strike_boundary, 0.0);
        assert_eq!(shifted_strike_boundary, log_shift);

        for frequency_index in 0..16 {
            let base_coefficient = payoff_cos_coefficient(
                VanillaOptionPayoffType::Call,
                base_reference_price,
                base_strike,
                -1.0,
                1.0,
                base_strike_boundary,
                frequency_index,
            )
            .expect("base payoff coefficient should resolve");

            let shifted_coefficient = payoff_cos_coefficient(
                VanillaOptionPayoffType::Call,
                shifted_reference_price,
                shifted_strike,
                log_shift - 1.0,
                log_shift + 1.0,
                shifted_strike_boundary,
                frequency_index,
            )
            .expect("shifted payoff coefficient should resolve");

            let expected_shifted_coefficient = 2.0 * base_coefficient;

            let tolerance = f64::EPSILON.sqrt()
                * expected_shifted_coefficient
                    .abs()
                    .max(shifted_coefficient.abs())
                    .max(1.0);

            assert!(
                (shifted_coefficient - expected_shifted_coefficient).abs() <= tolerance,
                "shifted coefficient {shifted_coefficient} should match scaled base coefficient {expected_shifted_coefficient}",
            );
        }
    }

    #[test]
    fn cos_price_uses_observed_log_characteristic_phase() {
        let asset_reference_price = 100.0_f64;
        let terminal_anchor_value = 0.7_f64;
        let terminal_deviation = 0.2_f64;
        let observed_log_price = terminal_anchor_value + terminal_deviation;
        let spot = asset_reference_price * observed_log_price.exp();
        let strike = 0.1 * spot;
        let spectral_domain = SpectralDomain {
            deviation_lower_bound: -1.0,
            deviation_upper_bound: 1.0,
            node_count: 4097,
        };
        let characteristic_function_values = (0..spectral_domain.node_count)
            .map(|frequency_index| {
                complex_exponential(ComplexScalar::new(
                    0.0,
                    spectral_domain.basis_frequency(frequency_index) * observed_log_price,
                ))
            })
            .collect::<Vec<_>>();
        let solved_maturity = SolvedMaturityLaw {
            maturity_interval: 1.0,
            terminal_anchor_value,
            deviation_lower_bound: spectral_domain.deviation_lower_bound,
            deviation_upper_bound: spectral_domain.deviation_upper_bound,
            node_count: spectral_domain.node_count,
            observed_log_price,
            representative_transform: None,
            characteristic_function_values,
            characteristic_function_sensitivities: None,
            interval_summaries: Vec::new(),
        };

        let price = cos_price_from_terminal_characteristic_function(
            spot,
            strike,
            VanillaOptionPayoffType::Call,
            0.0,
            &solved_maturity,
        )
        .expect("degenerate COS price should resolve");
        let expected_price = spot - strike;
        let spectral_resolution_tolerance = expected_price.abs().max(1.0)
            / (spectral_domain.node_count * spectral_domain.node_count) as f64;

        assert!(
            (price - expected_price).abs() <= spectral_resolution_tolerance,
            "degenerate COS price {price} should match intrinsic value {expected_price}",
        );
    }

    #[test]
    fn adaptive_interval_row_solver_returns_certified_thetas_for_nonzero_kappa_interval() {
        let pricing_request = VanillaOptionPricingRequest::try_new(
            VanillaOptionPricingMarketInputs::try_new_single_node(101.0, 100.0, 1.0, 0.02, 0.01)
                .expect("market inputs should construct"),
            VanillaOptionPayoffConfiguration {
                payoff_type: VanillaOptionPayoffType::Call,
            },
            AnchoredVanillaOptionPricingBoundaryInput::try_new(
                0,
                HawkesExcitationState {
                    y_plus_plus: 0.2,
                    y_plus_minus: 0.1,
                    y_minus_plus: 0.3,
                    y_minus_minus: 0.4,
                },
                MeanReversionFittedParameters::try_new(
                    0.5,
                    MeanReversionAnchorModel::zero_gain_endogenous_core_pressure_level(),
                )
                .expect("mean-reversion fitted parameters should construct"),
                1.0,
                MeanReversionBoundaryState::try_new(0.2, 0.0, 0.0, 0.2)
                    .expect("observed boundary state should construct"),
            )
            .expect("boundary input should construct"),
            valid_model_parameters(),
            1.0,
        )
        .expect("pricing request should construct");

        let interval_factors = vec![
            VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
                .expect("interval factors should construct"),
        ];

        let runtime_interval_schedule = build_uniform_runtime_interval_schedule(1, 1.0);

        let driver_parameters = pricing_request
            .model_parameters
            .nig_shared_plus_shift_configuration
            .derive_driver_parameters()
            .expect("driver parameters should derive");

        let mut no_observer = None;

        let materialization = build_pricing_sufficient_statistics_neutral_anchor_materialization(
            &pricing_request,
            &runtime_interval_schedule,
            interval_factors.as_slice(),
            driver_parameters,
            &mut no_observer,
        )
        .expect("risk-neutral anchor materialization should succeed");

        let spectral_domain =
            build_spectral_domain(&pricing_request, &materialization, 1, 1.0, &[100.0], None)
                .expect("spectral domain should construct");

        let collocation_nodes = spectral_domain.collocation_nodes();

        let base_context = LocalTransitionRequestContext::from_interval_state(
            &pricing_request,
            interval_factors[0],
            0,
            materialization.expected_hawkes_state_path[0],
            materialization.runtime_interval_anchors.start_anchor_value,
            materialization
                .runtime_interval_anchors
                .start_pressure_state,
            materialization.runtime_interval_anchors.interval_deltas[0],
        )
        .expect("transition context should construct");

        let row_states = collocation_nodes
            .iter()
            .copied()
            .map(|deviation| base_context.build_row_state(deviation))
            .collect::<Result<Vec<_>, _>>()
            .expect("row states should construct");

        assert!(
            row_states
                .iter()
                .any(|row_state| !row_state.uses_nested_transition_measure()),
            "test fixture should require true adaptive midpoint certification"
        );

        let theta_hints = vec![None; collocation_nodes.len()];

        let mut row_progress_counts = Vec::new();

        let (solved_rows, _) = {
            let mut row_progress_observer = |completed_row_count| {
                row_progress_counts.push(completed_row_count);
            };

            solve_interval_row_frequency_batches_adaptively(
                IntervalRowFrequencyBatchAdaptiveSolveInput {
                    base_context: &base_context,
                    collocation_nodes: collocation_nodes.as_slice(),
                    row_states: row_states.as_slice(),
                    theta_hints: theta_hints.as_slice(),
                    next_anchor_value: materialization.runtime_interval_anchors.end_anchor_values
                        [0],
                    spectral_domain,
                    driver_parameters,
                    collect_theta_warm_start_sensitivities: false,
                    pricing_thread_pool: None,
                    cpu_count: Some(1),
                    progress_observer: Some(&mut row_progress_observer),
                },
            )
            .expect("adaptive interval operator solve should succeed")
        };

        assert_eq!(solved_rows.len(), collocation_nodes.len());
        assert_eq!(row_progress_counts.last(), Some(&collocation_nodes.len()));
        assert!(
            row_progress_counts
                .windows(2)
                .all(|window| window[0] <= window[1]),
            "row progress should never move backwards"
        );

        for (row_state, solved_row) in row_states.iter().zip(solved_rows.iter()) {
            assert!(
                certify_local_transition_theta_for_row_with_parallelism(
                    &base_context,
                    row_state,
                    driver_parameters,
                    solved_row.theta,
                    None,
                    Some(1),
                )
                .expect("solved row theta should certify"),
                "adaptive row theta should satisfy the local tilt equation"
            );
        }
    }
}
