use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};

use crate::contracts::{
    SpectralPricingDiagnostics, SpectralPricingIntervalDiagnostics,
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingFailureStage,
    VanillaOptionPricingMarketInputs, VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest, VanillaOptionPricingStatus,
};
use crate::grids::{
    SpectralThetaWarmStartDriverConfiguration, SpectralThetaWarmStartRowState,
    SpectralThetaWarmStartSensitivity, SpectralThetaWarmStartState,
    VanillaOptionGridBatchPricingExecutionInput, VanillaOptionGridBatchPricingRunError,
    VanillaOptionGridBatchPricingRunResult, VanillaOptionGridPricingResult,
    VanillaOptionGridRequest, VanillaOptionMaturitySummary, VanillaOptionNodePriceSensitivity,
};
use crate::implied_volatility::{
    VanillaOptionImpliedVolatilityInversionOutcome, VanillaOptionImpliedVolatilityInversionStatus,
    invert_vanilla_option_implied_volatility,
};
use crate::intraday_black_scholes_no_arbitrage_price_bounds;
use crate::progress::PricingProgressEvent;
use crate::runner::{
    CoreLocalTiltImplicitThetaSharedPlusShiftSensitivity,
    CorePureImaginaryLogMomentSharedPlusShiftSensitivity, LocalTransitionRequestContext,
    LocalTransitionRowState, PricingCharacteristicFunctionSharedPlusShiftSensitivity,
    RiskNeutralAnchorMaterialization, VanillaOptionPricingExecutionInput,
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingRunError,
    build_pricing_sufficient_statistics_neutral_anchor_materialization,
    build_spectral_compatibility_integration_diagnostics,
    certify_local_transition_theta_for_row_with_parallelism,
    differentiate_core_local_tilt_theta_for_tilt,
    differentiate_core_pure_imaginary_log_moment_exponent_for_tilt,
    evaluate_pricing_log_return_characteristic_function_batch_for_context,
    evaluate_pricing_log_return_characteristic_function_batch_shared_plus_shift_sensitivities_for_context,
    pricing_ratio_derivative, resolve_local_transition_theta_for_row_with_parallelism_and_hint,
};
use crate::thread_pool::{
    build_pricing_thread_pool, install_pricing_parallelism, resolved_pricing_cpu_count,
};
use crate::time_scaling::intraday_year_fraction;
use janiform_foundations::complex_math::{ComplexScalar, complex_exponential, complex_multiply};

const MINIMUM_SPECTRAL_COLLOCATION_NODE_COUNT: usize = 65;
const MINIMUM_SPECTRAL_DEVIATION_DOMAIN_HALF_WIDTH: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CarryGroupKey {
    risk_free_rate_bits: u64,
    dividend_yield_bits: u64,
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
struct CachedIntervalOperator {
    interval_summary: SpectralPricingIntervalDiagnostics,
    row_operators: Vec<RowFrequencyBatchSolution>,
    row_operator_sensitivities: Option<Vec<RowFrequencyBatchSolutionSharedPlusShiftSensitivity>>,
}

#[derive(Debug, Clone)]
struct SolvedMaturityLaw {
    maturity_interval: f64,
    terminal_anchor_value: f64,
    deviation_lower_bound: f64,
    deviation_upper_bound: f64,
    node_count: usize,
    observed_log_price: f64,
    representative_transform:
        Option<janiform_affine_oracle::affine_oracle::ExponentialAffineTransformIntegrationResult>,
    characteristic_function_values: Vec<ComplexScalar>,
    characteristic_function_sensitivities: Option<Vec<NodePriceSharedPlusShiftComplexSensitivity>>,
    interval_summaries: Vec<SpectralPricingIntervalDiagnostics>,
}

#[derive(Debug, Clone, Copy)]
struct SpectralDomain {
    deviation_lower_bound: f64,
    deviation_upper_bound: f64,
    node_count: usize,
}

impl SpectralDomain {
    fn width(self) -> f64 {
        self.deviation_upper_bound - self.deviation_lower_bound
    }

    fn collocation_nodes(self) -> Vec<f64> {
        let width = self.width();

        (0..self.node_count)
            .map(|node_index| {
                self.deviation_lower_bound
                    + (node_index as f64 + 0.5) * width / self.node_count as f64
            })
            .collect()
    }

    fn basis_frequency(self, basis_index: usize) -> f64 {
        basis_index as f64 * std::f64::consts::PI / self.width()
    }
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
        VanillaOptionGridBatchPricingRunError::InvalidExecutionInput { source }
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
            source: crate::runner::VanillaOptionPricingExecutionInputError::InvalidRequest {
                source: crate::contracts::VanillaOptionPricingRequestError::InvalidMarketInputs {
                    source,
                },
            },
        }
    })?;

    let maturity_interval = market_inputs.single_maturity_interval().map_err(|source| {
        VanillaOptionPricingRunError::InvalidExecutionInput {
            source: crate::runner::VanillaOptionPricingExecutionInputError::InvalidRequest {
                source: crate::contracts::VanillaOptionPricingRequestError::InvalidMarketInputs {
                    source,
                },
            },
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
    .map_err(|source| VanillaOptionPricingRunError::InvalidSpectralDiagnostics { source })?;

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

        let cached_interval_operators = build_cached_interval_operators(
            carry_group_key,
            &base_request,
            execution_input
                .optional_output_selection
                .include_node_price_sensitivities,
            execution_input.interval_factors.as_slice(),
            &materialization,
            maximum_maturity_count,
            spectral_domain,
            driver_parameters,
            pricing_thread_pool,
            cpu_count,
            observer,
            theta_warm_start_state.as_deref_mut(),
        )
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

        let priced_row = price_strike_row(
            execution_input.request.spot,
            grid,
            solved_maturity,
            execution_input.boundary_input.hawkes_boundary_state,
            pricing_thread_pool,
            cpu_count,
            maturity_index,
            total_maturities,
            observer,
        );

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

struct PricedStrikeRow {
    price_row: Vec<Option<f64>>,
    implied_volatility_row: Vec<Option<f64>>,
    implied_volatility_status_row: Vec<Option<VanillaOptionImpliedVolatilityInversionStatus>>,
    pricing_status_row: Vec<VanillaOptionPricingStatus>,
    pricing_failure_stage_row: Vec<Option<VanillaOptionPricingFailureStage>>,
    pricing_failure_reason_row: Vec<Option<String>>,
    forward_price_row: Vec<Option<f64>>,
    discount_factor_row: Vec<Option<f64>>,
    boundary_state_norm_row: Vec<Option<f64>>,
    node_price_sensitivity_row: Vec<Option<VanillaOptionNodePriceSensitivity>>,
}

fn price_strike_row(
    spot: f64,
    grid: &VanillaOptionGridRequest,
    solved_maturity: &SolvedMaturityLaw,
    boundary_state: janiform_model_kernels::hawkes::HawkesExcitationState,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
    maturity_index: usize,
    total_maturities: usize,
    observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> PricedStrikeRow {
    let strike_count = grid.strikes.len();

    let maturity_year_fraction = intraday_year_fraction(solved_maturity.maturity_interval);

    let forward_price =
        spot * ((grid.risk_free_rate - grid.dividend_yield) * maturity_year_fraction).exp();

    let discount_factor = (-grid.risk_free_rate * maturity_year_fraction).exp();

    let boundary_state_norm_row_value = Some(
        (boundary_state.y_plus_plus * boundary_state.y_plus_plus
            + boundary_state.y_plus_minus * boundary_state.y_plus_minus
            + boundary_state.y_minus_plus * boundary_state.y_minus_plus
            + boundary_state.y_minus_minus * boundary_state.y_minus_minus)
            .sqrt(),
    );

    if strike_count > 0 {
        emit_pricing_progress(
            observer,
            PricingProgressEvent::strike_pricing_progress(
                0,
                strike_count,
                Some(maturity_index),
                Some(total_maturities),
                Some(0),
                strike_count,
            ),
        );
    }

    let price_strike = |strike: f64| {
        let model_price = cos_price_from_terminal_characteristic_function(
            spot,
            strike,
            grid.payoff_type,
            grid.risk_free_rate,
            grid.dividend_yield,
            solved_maturity,
        );

        match model_price {
            Some(price) if price.is_finite() => {
                let market_inputs = VanillaOptionPricingMarketInputs::new_single_node(
                    spot,
                    strike,
                    solved_maturity.maturity_interval,
                    grid.risk_free_rate,
                    grid.dividend_yield,
                );

                let (lower_bound, upper_bound) = intraday_black_scholes_no_arbitrage_price_bounds(
                    &market_inputs,
                    grid.payoff_type,
                );

                let bounded_price = price.clamp(lower_bound, upper_bound);
                let bounded_price_sensitivity = match solved_maturity
                    .characteristic_function_sensitivities
                    .as_ref()
                {
                    Some(_) => match cos_price_sensitivity_from_terminal_characteristic_function(
                        spot,
                        strike,
                        grid.payoff_type,
                        grid.risk_free_rate,
                        grid.dividend_yield,
                        solved_maturity,
                    ) {
                        Ok(raw_price_sensitivity) => Some(resolve_bounded_node_price_sensitivity(
                            raw_price_sensitivity,
                            price,
                            bounded_price,
                        )),
                        Err(_) => None,
                    },
                    None => None,
                };

                let iv_outcome = invert_spectral_implied_volatility(
                    &market_inputs,
                    grid.payoff_type,
                    bounded_price,
                );

                (
                    Some(bounded_price),
                    iv_outcome.implied_volatility,
                    Some(iv_outcome.inversion_status),
                    VanillaOptionPricingStatus::Succeeded,
                    None,
                    None,
                    Some(forward_price),
                    Some(discount_factor),
                    boundary_state_norm_row_value,
                    bounded_price_sensitivity,
                )
            }
            _ => (
                None,
                None,
                None,
                VanillaOptionPricingStatus::Failed,
                Some(VanillaOptionPricingFailureStage::FourierPricing),
                Some("spectral strike valuation returned a non-finite price".to_string()),
                None,
                None,
                None,
                None,
            ),
        }
    };

    let priced_nodes = if resolved_pricing_cpu_count(cpu_count) > 1 && strike_count > 1 {
        install_pricing_parallelism(pricing_thread_pool, || {
            grid.strikes
                .par_iter()
                .copied()
                .enumerate()
                .map(|(_, strike)| price_strike(strike))
                .collect::<Vec<_>>()
        })
    } else {
        let mut priced_nodes = Vec::with_capacity(strike_count);

        for (strike_index, strike) in grid.strikes.iter().copied().enumerate() {
            priced_nodes.push(price_strike(strike));

            emit_pricing_progress(
                observer,
                PricingProgressEvent::strike_pricing_progress(
                    strike_index + 1,
                    strike_count,
                    Some(maturity_index),
                    Some(total_maturities),
                    Some(strike_index),
                    strike_count,
                ),
            );
        }

        priced_nodes
    };

    emit_pricing_progress(
        observer,
        PricingProgressEvent::strike_pricing_progress(
            strike_count,
            strike_count,
            Some(maturity_index),
            Some(total_maturities),
            strike_count.checked_sub(1),
            strike_count,
        ),
    );

    PricedStrikeRow {
        price_row: priced_nodes.iter().map(|node| node.0).collect(),
        implied_volatility_row: priced_nodes.iter().map(|node| node.1).collect(),
        implied_volatility_status_row: priced_nodes.iter().map(|node| node.2).collect(),
        pricing_status_row: priced_nodes.iter().map(|node| node.3).collect(),
        pricing_failure_stage_row: priced_nodes.iter().map(|node| node.4).collect(),
        pricing_failure_reason_row: priced_nodes.iter().map(|node| node.5.clone()).collect(),
        forward_price_row: vec![Some(forward_price); strike_count],
        discount_factor_row: vec![Some(discount_factor); strike_count],
        boundary_state_norm_row: vec![boundary_state_norm_row_value; strike_count],
        node_price_sensitivity_row: priced_nodes.iter().map(|node| node.9).collect(),
    }
}

fn invert_spectral_implied_volatility(
    market_inputs: &VanillaOptionPricingMarketInputs,
    payoff_type: VanillaOptionPayoffType,
    option_price: f64,
) -> VanillaOptionImpliedVolatilityInversionOutcome {
    let (lower_bound, upper_bound) =
        intraday_black_scholes_no_arbitrage_price_bounds(market_inputs, payoff_type);

    let bounded_price = if option_price == upper_bound {
        upper_bound.next_down()
    } else if option_price == lower_bound {
        lower_bound.next_up()
    } else {
        option_price
    };

    invert_vanilla_option_implied_volatility(market_inputs, payoff_type, bounded_price)
}

fn build_cached_interval_operators(
    carry_group_key: CarryGroupKey,
    pricing_request: &VanillaOptionPricingRequest,
    include_node_price_sensitivities: bool,
    interval_factors: &[VanillaOptionPricingIntervalFactors],
    materialization: &RiskNeutralAnchorMaterialization,
    maximum_maturity_count: usize,
    spectral_domain: SpectralDomain,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
    observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
    mut theta_warm_start_state: Option<&mut SpectralThetaWarmStartState>,
) -> Result<Vec<CachedIntervalOperator>, VanillaOptionPricingRunError> {
    let collocation_nodes = spectral_domain.collocation_nodes();
    let runtime_anchors = &materialization.runtime_interval_anchors;

    let mut cached_interval_operators = Vec::with_capacity(maximum_maturity_count);

    for interval_offset in 0..maximum_maturity_count {
        emit_pricing_progress(
            observer,
            PricingProgressEvent::interval_progress(
                interval_offset,
                maximum_maturity_count,
                Some(interval_offset),
                maximum_maturity_count,
            ),
        );

        let current_anchor_value = if interval_offset == 0 {
            runtime_anchors.start_anchor_value
        } else {
            runtime_anchors.end_anchor_values[interval_offset - 1]
        };

        let current_pressure_state = if interval_offset == 0 {
            runtime_anchors.start_pressure_state
        } else {
            runtime_anchors.end_pressure_states[interval_offset - 1]
        };

        let interval_delta = runtime_anchors.interval_deltas[interval_offset];

        let expected_hawkes_state_before =
            materialization.expected_hawkes_state_path[interval_offset];
        let expected_hawkes_state_after =
            materialization.expected_hawkes_state_path[interval_offset + 1];

        let interval_factor = interval_factors[interval_offset];
        let current_driver_configuration = theta_warm_start_driver_configuration(driver_parameters);

        let base_context = LocalTransitionRequestContext::from_interval_state(
            pricing_request,
            interval_factor,
            pricing_request.boundary_input.interval_index + interval_offset,
            expected_hawkes_state_before,
            current_anchor_value,
            current_pressure_state,
            interval_delta,
        )?;

        emit_pricing_progress(
            observer,
            PricingProgressEvent::transition_row_progress(
                0,
                collocation_nodes.len(),
                Some(interval_offset),
                Some(maximum_maturity_count),
            ),
        );

        let theta_hints = theta_warm_start_state
            .as_deref_mut()
            .map(|state| {
                state.interval_theta_hints(
                    (
                        carry_group_key.risk_free_rate_bits,
                        carry_group_key.dividend_yield_bits,
                    ),
                    interval_offset,
                    collocation_nodes.len(),
                    current_driver_configuration,
                )
            })
            .unwrap_or_else(|| vec![None; collocation_nodes.len()]);

        let row_states = collocation_nodes
            .iter()
            .copied()
            .map(|deviation| base_context.build_row_state(deviation))
            .collect::<Result<Vec<_>, _>>()?;

        let (row_operators, row_theta_states, row_solve_diagnostics) =
            solve_interval_row_frequency_batches_adaptively(
                &base_context,
                collocation_nodes.as_slice(),
                row_states.as_slice(),
                theta_hints.as_slice(),
                spectral_domain,
                driver_parameters,
                observer.as_ref().is_some(),
                theta_warm_start_state.is_some(),
                pricing_thread_pool,
                cpu_count,
            )?;

        let row_operator_sensitivities = if include_node_price_sensitivities {
            Some(build_row_operator_sensitivities(
                &base_context,
                row_states.as_slice(),
                row_operators.as_slice(),
                spectral_domain,
                driver_parameters,
            )?)
        } else {
            None
        };

        if let Some(state) = theta_warm_start_state.as_deref_mut() {
            state.record_interval_theta_states(
                (
                    carry_group_key.risk_free_rate_bits,
                    carry_group_key.dividend_yield_bits,
                ),
                interval_offset,
                current_driver_configuration,
                row_theta_states,
            );
        }

        emit_pricing_progress(
            observer,
            PricingProgressEvent::transition_row_progress(
                collocation_nodes.len(),
                collocation_nodes.len(),
                Some(interval_offset),
                Some(maximum_maturity_count),
            )
            .with_transition_row_solve_diagnostics(
                row_solve_diagnostics.seed_exact_transition_row_count,
                row_solve_diagnostics.shared_theta_transition_row_count,
                row_solve_diagnostics.certified_transition_row_count,
                row_solve_diagnostics.fallback_exact_transition_row_count,
                row_solve_diagnostics.theta_search_elapsed_millis,
                row_solve_diagnostics.theta_certification_elapsed_millis,
                row_solve_diagnostics.row_batch_elapsed_millis,
                row_solve_diagnostics.average_buy_shift_configuration_gradient_norm(),
                row_solve_diagnostics.average_sell_shift_configuration_gradient_norm(),
                row_solve_diagnostics.average_core_log_moment_shift_configuration_gradient_norm(),
            ),
        );

        cached_interval_operators.push(CachedIntervalOperator {
            interval_summary: SpectralPricingIntervalDiagnostics {
                interval_index: pricing_request.boundary_input.interval_index + interval_offset,
                current_anchor_value,
                next_anchor_value: runtime_anchors.end_anchor_values[interval_offset],
                interval_delta,
                expected_hawkes_boundary_state_before: expected_hawkes_state_before,
                expected_hawkes_boundary_state_after: expected_hawkes_state_after,
                minimum_solved_theta: row_operators
                    .iter()
                    .map(|row_operator| row_operator.theta)
                    .fold(f64::INFINITY, f64::min),
                maximum_solved_theta: row_operators
                    .iter()
                    .map(|row_operator| row_operator.theta)
                    .fold(f64::NEG_INFINITY, f64::max),
            },
            row_operators,
            row_operator_sensitivities,
        });
    }

    if maximum_maturity_count > 0 {
        emit_pricing_progress(
            observer,
            PricingProgressEvent::interval_progress(
                maximum_maturity_count,
                maximum_maturity_count,
                None,
                maximum_maturity_count,
            ),
        );
    }

    Ok(cached_interval_operators)
}

fn solve_maturity_law(
    pricing_request: &VanillaOptionPricingRequest,
    materialization: &RiskNeutralAnchorMaterialization,
    maturity_count: usize,
    spectral_domain: SpectralDomain,
    cached_interval_operators: &[CachedIntervalOperator],
) -> Result<SolvedMaturityLaw, VanillaOptionPricingRunError> {
    let maturity_interval = materialization.runtime_interval_anchors.interval_deltas
        [..maturity_count]
        .iter()
        .sum::<f64>();

    let basis_size = spectral_domain.node_count;
    let frequency_count = basis_size;

    let mut continuation_coefficients =
        vec![vec![ComplexScalar::new(0.0, 0.0); basis_size]; frequency_count];
    let mut continuation_coefficient_sensitivities = cached_interval_operators
        .first()
        .and_then(|interval_operator| interval_operator.row_operator_sensitivities.as_ref())
        .map(|_| {
            vec![
                vec![NodePriceSharedPlusShiftComplexSensitivity::zero(); basis_size];
                frequency_count
            ]
        });

    for frequency_index in 0..frequency_count {
        continuation_coefficients[frequency_index][0] = ComplexScalar::new(1.0, 0.0);
    }

    for cached_interval_operator in cached_interval_operators[..maturity_count].iter().rev() {
        let (updated_coefficients, updated_sensitivities) = apply_cached_interval_operator(
            continuation_coefficients,
            continuation_coefficient_sensitivities,
            cached_interval_operator,
            spectral_domain,
        );
        continuation_coefficients = updated_coefficients;
        continuation_coefficient_sensitivities = updated_sensitivities;
    }

    let characteristic_function_values = continuation_coefficients
        .iter()
        .enumerate()
        .map(|(frequency_index, coefficients)| {
            evaluate_cosine_series(
                coefficients.as_slice(),
                spectral_domain,
                pricing_request
                    .boundary_input
                    .observed_boundary_state
                    .deviation,
                frequency_index,
            )
        })
        .collect::<Vec<_>>();

    let characteristic_function_sensitivities =
        continuation_coefficient_sensitivities.map(|coefficient_sensitivities| {
            coefficient_sensitivities
                .iter()
                .enumerate()
                .map(|(frequency_index, sensitivities)| {
                    evaluate_cosine_series_sensitivity(
                        sensitivities.as_slice(),
                        spectral_domain,
                        pricing_request
                            .boundary_input
                            .observed_boundary_state
                            .deviation,
                        frequency_index,
                    )
                })
                .collect::<Vec<_>>()
        });

    Ok(SolvedMaturityLaw {
        maturity_interval,
        terminal_anchor_value: materialization.runtime_interval_anchors.end_anchor_values
            [maturity_count - 1],
        deviation_lower_bound: spectral_domain.deviation_lower_bound,
        deviation_upper_bound: spectral_domain.deviation_upper_bound,
        node_count: spectral_domain.node_count,
        observed_log_price: pricing_request
            .boundary_input
            .observed_boundary_state
            .observed_log_price,
        representative_transform: cached_interval_operators[..maturity_count]
            .iter()
            .flat_map(|interval_operator| interval_operator.row_operators.iter())
            .find_map(|row_operator| row_operator.representative_transform),
        characteristic_function_values,
        characteristic_function_sensitivities,
        interval_summaries: cached_interval_operators[..maturity_count]
            .iter()
            .map(|interval_operator| interval_operator.interval_summary.clone())
            .collect(),
    })
}

fn apply_cached_interval_operator(
    continuation_coefficients: Vec<Vec<ComplexScalar>>,
    continuation_coefficient_sensitivities: Option<
        Vec<Vec<NodePriceSharedPlusShiftComplexSensitivity>>,
    >,
    cached_interval_operator: &CachedIntervalOperator,
    spectral_domain: SpectralDomain,
) -> (
    Vec<Vec<ComplexScalar>>,
    Option<Vec<Vec<NodePriceSharedPlusShiftComplexSensitivity>>>,
) {
    let frequency_count = spectral_domain.node_count;

    let mut interval_values =
        vec![vec![ComplexScalar::new(0.0, 0.0); spectral_domain.node_count]; frequency_count];
    let mut interval_value_sensitivities = continuation_coefficient_sensitivities
        .as_ref()
        .map(|_| {
            vec![
                vec![NodePriceSharedPlusShiftComplexSensitivity::zero(); spectral_domain.node_count];
                frequency_count
            ]
        });

    for (row_index, row_operator) in cached_interval_operator.row_operators.iter().enumerate() {
        let row_operator_sensitivity = cached_interval_operator
            .row_operator_sensitivities
            .as_ref()
            .and_then(|row_operator_sensitivities| row_operator_sensitivities.get(row_index));

        for frequency_index in 0..frequency_count {
            let mut row_value = ComplexScalar::new(0.0, 0.0);
            let mut row_value_sensitivity = NodePriceSharedPlusShiftComplexSensitivity::zero();

            for basis_index in 0..spectral_domain.node_count {
                let difference_value = if frequency_index >= basis_index {
                    row_operator.positive_frequency_values[frequency_index - basis_index]
                } else {
                    row_operator.positive_frequency_values[basis_index - frequency_index]
                        .conjugate()
                };

                let expectation = complex_multiply(
                    row_operator.basis_forward_phases[basis_index],
                    row_operator.positive_frequency_values[frequency_index + basis_index],
                )
                .add(complex_multiply(
                    row_operator.basis_backward_phases[basis_index],
                    difference_value,
                ))
                .scale(0.5);

                let expectation_sensitivity = row_operator_sensitivity.map_or(
                    NodePriceSharedPlusShiftComplexSensitivity::zero(),
                    |row_operator_sensitivity| {
                        let positive_frequency_sensitivity = row_operator_sensitivity
                            .positive_frequency_value_sensitivities[frequency_index + basis_index];

                        let difference_sensitivity = if frequency_index >= basis_index {
                            row_operator_sensitivity.positive_frequency_value_sensitivities
                                [frequency_index - basis_index]
                        } else {
                            row_operator_sensitivity.positive_frequency_value_sensitivities
                                [basis_index - frequency_index]
                                .conjugate()
                        };

                        complex_expectation_sensitivity(
                            row_operator.basis_forward_phases[basis_index],
                            row_operator_sensitivity.basis_forward_phase_sensitivities[basis_index],
                            row_operator.positive_frequency_values[frequency_index + basis_index],
                            positive_frequency_sensitivity,
                            row_operator.basis_backward_phases[basis_index],
                            row_operator_sensitivity.basis_backward_phase_sensitivities
                                [basis_index],
                            difference_value,
                            difference_sensitivity,
                        )
                    },
                );

                let continuation_sensitivity = continuation_coefficient_sensitivities
                    .as_ref()
                    .map(|coefficient_sensitivities| {
                        coefficient_sensitivities[frequency_index][basis_index]
                    })
                    .unwrap_or_else(NodePriceSharedPlusShiftComplexSensitivity::zero);

                row_value = row_value.add(complex_multiply(
                    continuation_coefficients[frequency_index][basis_index],
                    expectation,
                ));

                row_value_sensitivity = row_value_sensitivity
                    .add(complex_scale_sensitivity(
                        expectation,
                        continuation_sensitivity,
                    ))
                    .add(complex_scale_sensitivity(
                        continuation_coefficients[frequency_index][basis_index],
                        expectation_sensitivity,
                    ));
            }

            interval_values[frequency_index][row_index] = complex_multiply(
                row_operator.row_frequency_phases[frequency_index],
                row_value,
            );

            if let Some(interval_value_sensitivities) = interval_value_sensitivities.as_mut() {
                interval_value_sensitivities[frequency_index][row_index] =
                    complex_scale_sensitivity(
                        row_operator.row_frequency_phases[frequency_index],
                        row_value_sensitivity,
                    );
            }
        }
    }

    let coefficients = values_to_cosine_coefficients(interval_values);
    let coefficient_sensitivities =
        interval_value_sensitivities.map(values_to_cosine_coefficients_sensitivities);

    (coefficients, coefficient_sensitivities)
}

fn emit_pricing_progress(
    observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
    event: PricingProgressEvent,
) {
    if let Some(observer) = observer.as_deref_mut() {
        observer(event);
    }
}

#[derive(Debug, Clone)]
struct RowFrequencyBatchSolution {
    theta: f64,
    positive_frequency_values: Vec<ComplexScalar>,
    basis_forward_phases: Vec<ComplexScalar>,
    basis_backward_phases: Vec<ComplexScalar>,
    row_frequency_phases: Vec<ComplexScalar>,
    representative_transform:
        Option<janiform_affine_oracle::affine_oracle::ExponentialAffineTransformIntegrationResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct NodePriceSharedPlusShiftSensitivity {
    alpha_derivative: f64,
    beta_0_derivative: f64,
    delta_derivative: f64,
    delta_beta_derivative: f64,
}

impl NodePriceSharedPlusShiftSensitivity {
    fn zero() -> Self {
        Self::default()
    }

    fn add(self, other: Self) -> Self {
        Self {
            alpha_derivative: self.alpha_derivative + other.alpha_derivative,
            beta_0_derivative: self.beta_0_derivative + other.beta_0_derivative,
            delta_derivative: self.delta_derivative + other.delta_derivative,
            delta_beta_derivative: self.delta_beta_derivative + other.delta_beta_derivative,
        }
    }

    fn into_output(self) -> VanillaOptionNodePriceSensitivity {
        VanillaOptionNodePriceSensitivity {
            alpha_derivative: self.alpha_derivative,
            beta_0_derivative: self.beta_0_derivative,
            delta_derivative: self.delta_derivative,
            delta_beta_derivative: self.delta_beta_derivative,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct NodePriceSharedPlusShiftComplexSensitivity {
    alpha_derivative: ComplexScalar,
    beta_0_derivative: ComplexScalar,
    delta_derivative: ComplexScalar,
    delta_beta_derivative: ComplexScalar,
}

impl NodePriceSharedPlusShiftComplexSensitivity {
    fn zero() -> Self {
        Self {
            alpha_derivative: ComplexScalar::new(0.0, 0.0),
            beta_0_derivative: ComplexScalar::new(0.0, 0.0),
            delta_derivative: ComplexScalar::new(0.0, 0.0),
            delta_beta_derivative: ComplexScalar::new(0.0, 0.0),
        }
    }

    fn scale(self, factor: f64) -> Self {
        Self {
            alpha_derivative: self.alpha_derivative.scale(factor),
            beta_0_derivative: self.beta_0_derivative.scale(factor),
            delta_derivative: self.delta_derivative.scale(factor),
            delta_beta_derivative: self.delta_beta_derivative.scale(factor),
        }
    }

    fn add(self, other: Self) -> Self {
        Self {
            alpha_derivative: self.alpha_derivative.add(other.alpha_derivative),
            beta_0_derivative: self.beta_0_derivative.add(other.beta_0_derivative),
            delta_derivative: self.delta_derivative.add(other.delta_derivative),
            delta_beta_derivative: self.delta_beta_derivative.add(other.delta_beta_derivative),
        }
    }

    fn conjugate(self) -> Self {
        Self {
            alpha_derivative: self.alpha_derivative.conjugate(),
            beta_0_derivative: self.beta_0_derivative.conjugate(),
            delta_derivative: self.delta_derivative.conjugate(),
            delta_beta_derivative: self.delta_beta_derivative.conjugate(),
        }
    }
}

#[derive(Debug, Clone)]
struct SharedRowFrequencyBatchKernelSensitivity {
    positive_frequency_value_sensitivities: Vec<NodePriceSharedPlusShiftComplexSensitivity>,
    local_expectation_sensitivity: NodePriceSharedPlusShiftSensitivity,
}

#[derive(Debug, Clone)]
struct RowFrequencyBatchSolutionSharedPlusShiftSensitivity {
    positive_frequency_value_sensitivities: Vec<NodePriceSharedPlusShiftComplexSensitivity>,
    basis_forward_phase_sensitivities: Vec<NodePriceSharedPlusShiftComplexSensitivity>,
    basis_backward_phase_sensitivities: Vec<NodePriceSharedPlusShiftComplexSensitivity>,
}

#[derive(Debug, Clone)]
struct SharedRowFrequencyBatchKernel {
    theta: f64,
    positive_frequency_values: Vec<ComplexScalar>,
    representative_transform:
        Option<janiform_affine_oracle::affine_oracle::ExponentialAffineTransformIntegrationResult>,
    local_expectations: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdaptiveRowSolveMode {
    SeedExact,
    CertifiedPredicted,
    FallbackExact,
}

#[derive(Debug, Clone)]
struct TimedRowFrequencyBatchSolution {
    solution: RowFrequencyBatchSolution,
    solve_mode: AdaptiveRowSolveMode,
    theta_search_elapsed_millis: u64,
    theta_certification_elapsed_millis: u64,
    row_batch_elapsed_millis: u64,
    theta_warm_start_state: SpectralThetaWarmStartRowState,
    buy_shift_configuration_gradient_norm: f64,
    sell_shift_configuration_gradient_norm: f64,
    core_log_moment_shift_configuration_gradient_norm: f64,
}

#[derive(Debug, Clone, Copy)]
struct AdaptiveRowSolveTask {
    left_row_index: usize,
    midpoint_row_index: usize,
    right_row_index: usize,
    predicted_theta: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct IntervalRowSolveDiagnostics {
    seed_exact_transition_row_count: usize,
    shared_theta_transition_row_count: usize,
    certified_transition_row_count: usize,
    fallback_exact_transition_row_count: usize,
    theta_search_elapsed_millis: u64,
    theta_certification_elapsed_millis: u64,
    row_batch_elapsed_millis: u64,
    total_buy_shift_configuration_gradient_norm: f64,
    total_sell_shift_configuration_gradient_norm: f64,
    total_core_log_moment_shift_configuration_gradient_norm: f64,
    gradient_norm_sample_count: usize,
}

impl IntervalRowSolveDiagnostics {
    fn record(&mut self, solution: &TimedRowFrequencyBatchSolution) {
        match solution.solve_mode {
            AdaptiveRowSolveMode::SeedExact => {
                self.seed_exact_transition_row_count += 1;
            }
            AdaptiveRowSolveMode::CertifiedPredicted => {
                self.certified_transition_row_count += 1;
            }
            AdaptiveRowSolveMode::FallbackExact => {
                self.fallback_exact_transition_row_count += 1;
            }
        }

        self.theta_search_elapsed_millis += solution.theta_search_elapsed_millis;
        self.theta_certification_elapsed_millis += solution.theta_certification_elapsed_millis;
        self.row_batch_elapsed_millis += solution.row_batch_elapsed_millis;
        self.total_buy_shift_configuration_gradient_norm +=
            solution.buy_shift_configuration_gradient_norm;
        self.total_sell_shift_configuration_gradient_norm +=
            solution.sell_shift_configuration_gradient_norm;
        self.total_core_log_moment_shift_configuration_gradient_norm +=
            solution.core_log_moment_shift_configuration_gradient_norm;
        self.gradient_norm_sample_count += 1;
    }

    fn average_buy_shift_configuration_gradient_norm(self) -> f64 {
        if self.gradient_norm_sample_count == 0 {
            return 0.0;
        }

        self.total_buy_shift_configuration_gradient_norm / self.gradient_norm_sample_count as f64
    }

    fn average_sell_shift_configuration_gradient_norm(self) -> f64 {
        if self.gradient_norm_sample_count == 0 {
            return 0.0;
        }

        self.total_sell_shift_configuration_gradient_norm / self.gradient_norm_sample_count as f64
    }

    fn average_core_log_moment_shift_configuration_gradient_norm(self) -> f64 {
        if self.gradient_norm_sample_count == 0 {
            return 0.0;
        }

        self.total_core_log_moment_shift_configuration_gradient_norm
            / self.gradient_norm_sample_count as f64
    }
}

fn shared_plus_shift_configuration_gradient_norm(
    alpha_derivative: ComplexScalar,
    beta_0_derivative: ComplexScalar,
    delta_derivative: ComplexScalar,
    delta_beta_derivative: ComplexScalar,
) -> f64 {
    let alpha_norm = alpha_derivative.norm();
    let beta_0_norm = beta_0_derivative.norm();
    let delta_norm = delta_derivative.norm();
    let delta_beta_norm = delta_beta_derivative.norm();

    (alpha_norm * alpha_norm
        + beta_0_norm * beta_0_norm
        + delta_norm * delta_norm
        + delta_beta_norm * delta_beta_norm)
        .sqrt()
}

fn core_log_moment_shift_configuration_gradient_norm(
    sensitivity: &CorePureImaginaryLogMomentSharedPlusShiftSensitivity,
) -> f64 {
    (sensitivity.alpha_derivative * sensitivity.alpha_derivative
        + sensitivity.beta_0_derivative * sensitivity.beta_0_derivative
        + sensitivity.delta_derivative * sensitivity.delta_derivative
        + sensitivity.delta_beta_derivative * sensitivity.delta_beta_derivative)
        .sqrt()
}

fn theta_warm_start_driver_configuration(
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
) -> SpectralThetaWarmStartDriverConfiguration {
    SpectralThetaWarmStartDriverConfiguration {
        alpha: driver_parameters.buy_side_parameters.alpha,
        beta_0: driver_parameters.buy_side_parameters.beta,
        delta: driver_parameters.buy_side_parameters.delta,
        mu: driver_parameters.buy_side_parameters.mu,
        delta_beta: driver_parameters.sell_side_parameters.beta
            - driver_parameters.buy_side_parameters.beta,
    }
}

fn try_resolve_theta_warm_start_sensitivity(
    sensitivity: &CorePureImaginaryLogMomentSharedPlusShiftSensitivity,
) -> Option<SpectralThetaWarmStartSensitivity> {
    // The pricing result is already determined by the exact theta solve. If the
    // derivative-based predictor cannot be evaluated cleanly, preserve the
    // canonical solved theta and skip only the advisory warm-start sensitivity.
    if !sensitivity.theta_derivative.is_finite() || sensitivity.theta_derivative == 0.0 {
        return None;
    }

    let inverse_theta_derivative = -1.0 / sensitivity.theta_derivative;

    Some(SpectralThetaWarmStartSensitivity {
        alpha_derivative: sensitivity.alpha_derivative * inverse_theta_derivative,
        beta_0_derivative: sensitivity.beta_0_derivative * inverse_theta_derivative,
        delta_derivative: sensitivity.delta_derivative * inverse_theta_derivative,
        mu_derivative: sensitivity.mu_derivative * inverse_theta_derivative,
        delta_beta_derivative: sensitivity.delta_beta_derivative * inverse_theta_derivative,
    })
}

fn resolve_shift_configuration_gradient_norms(
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    theta: f64,
) -> Result<(f64, f64), VanillaOptionPricingRunError> {
    let shifted_argument = ComplexScalar::new(0.0, -theta);

    let buy_shift_sensitivity = driver_parameters
        .differentiate_buy_side_characteristic_exponent_for_shared_plus_shift_configuration(
            shifted_argument,
        )
        .map_err(|source| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: format!(
                "buy-side shared-plus-shift characteristic-exponent sensitivity failed at theta={theta}: {source}"
            ),
        })?;

    let sell_shift_sensitivity = driver_parameters
        .differentiate_sell_side_characteristic_exponent_for_return_composition_for_shared_plus_shift_configuration(
            shifted_argument,
        )
        .map_err(|source| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: format!(
                "sell-side shared-plus-shift characteristic-exponent sensitivity failed at theta={theta}: {source}"
            ),
        })?;

    Ok((
        shared_plus_shift_configuration_gradient_norm(
            buy_shift_sensitivity.alpha_derivative,
            buy_shift_sensitivity.beta_0_derivative,
            buy_shift_sensitivity.delta_derivative,
            buy_shift_sensitivity.delta_beta_derivative,
        ),
        shared_plus_shift_configuration_gradient_norm(
            sell_shift_sensitivity.alpha_derivative,
            sell_shift_sensitivity.beta_0_derivative,
            sell_shift_sensitivity.delta_derivative,
            sell_shift_sensitivity.delta_beta_derivative,
        ),
    ))
}

fn solve_row_frequency_batch(
    base_context: &LocalTransitionRequestContext,
    transition_row: &LocalTransitionRowState,
    spectral_domain: SpectralDomain,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    theta: f64,
) -> Result<RowFrequencyBatchSolution, VanillaOptionPricingRunError> {
    let shared_kernel = solve_shared_row_frequency_batch_kernel(
        base_context,
        spectral_domain,
        driver_parameters,
        theta,
    )?;

    build_row_frequency_batch_from_shared_kernel(
        base_context,
        transition_row,
        spectral_domain,
        &shared_kernel,
    )
}

fn solve_shared_row_frequency_batch_kernel(
    base_context: &LocalTransitionRequestContext,
    spectral_domain: SpectralDomain,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    theta: f64,
) -> Result<SharedRowFrequencyBatchKernel, VanillaOptionPricingRunError> {
    let frequency_count = spectral_domain.node_count;

    let mut batch_arguments = Vec::with_capacity(2 * frequency_count - 1);

    for frequency_index in 0..(2 * frequency_count - 1) {
        batch_arguments.push(ComplexScalar::new(
            spectral_domain.basis_frequency(frequency_index),
            -theta,
        ));
    }

    let batch_evaluations = evaluate_pricing_log_return_characteristic_function_batch_for_context(
        base_context,
        driver_parameters,
        batch_arguments.as_slice(),
    )?;

    let denominator = batch_evaluations[0].value;
    let representative_transform = Some(batch_evaluations[0].transform_diagnostics);

    let mut positive_frequency_values = Vec::with_capacity(batch_evaluations.len());

    for evaluation in &batch_evaluations {
        let value = evaluation.value.divide(denominator).ok_or(
            VanillaOptionPricingRunError::InvalidRiskNeutralCharacteristicFunction {
                numerator: evaluation.value,
                denominator,
            },
        )?;

        positive_frequency_values.push(value);
    }

    let local_expectations = derive_expected_increment_from_frequency_values(
        positive_frequency_values.as_slice(),
        spectral_domain,
    )?;

    Ok(SharedRowFrequencyBatchKernel {
        theta,
        positive_frequency_values,
        representative_transform,
        local_expectations,
    })
}

fn build_row_frequency_batch_from_shared_kernel(
    base_context: &LocalTransitionRequestContext,
    transition_row: &LocalTransitionRowState,
    spectral_domain: SpectralDomain,
    shared_kernel: &SharedRowFrequencyBatchKernel,
) -> Result<RowFrequencyBatchSolution, VanillaOptionPricingRunError> {
    let frequency_count = spectral_domain.node_count;
    let deviation = transition_row.boundary_state.deviation;
    let shift_center =
        base_context.deviation_decay_factor * deviation - shared_kernel.local_expectations;
    let shift_phase_offset = shift_center - spectral_domain.deviation_lower_bound;

    let mut basis_forward_phases = Vec::with_capacity(spectral_domain.node_count);
    let mut basis_backward_phases = Vec::with_capacity(spectral_domain.node_count);

    for basis_index in 0..spectral_domain.node_count {
        let forward_phase = complex_exponential(ComplexScalar::new(
            0.0,
            spectral_domain.basis_frequency(basis_index) * shift_phase_offset,
        ));

        basis_forward_phases.push(forward_phase);
        basis_backward_phases.push(forward_phase.conjugate());
    }

    let mut row_frequency_phases = Vec::with_capacity(frequency_count);
    for frequency_index in 0..frequency_count {
        row_frequency_phases.push(complex_exponential(ComplexScalar::new(
            0.0,
            spectral_domain.basis_frequency(frequency_index)
                * (base_context.deviation_decay_factor - 1.0)
                * deviation,
        )));
    }

    Ok(RowFrequencyBatchSolution {
        theta: shared_kernel.theta,
        positive_frequency_values: shared_kernel.positive_frequency_values.clone(),
        basis_forward_phases,
        basis_backward_phases,
        row_frequency_phases,
        representative_transform: shared_kernel.representative_transform.clone(),
    })
}

fn build_row_operator_sensitivities(
    base_context: &LocalTransitionRequestContext,
    row_states: &[LocalTransitionRowState],
    row_operators: &[RowFrequencyBatchSolution],
    spectral_domain: SpectralDomain,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
) -> Result<Vec<RowFrequencyBatchSolutionSharedPlusShiftSensitivity>, VanillaOptionPricingRunError>
{
    if row_states.len() != row_operators.len() {
        return Err(VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "row operator sensitivity build requires aligned row states and operators"
                .to_string(),
        });
    }

    if row_states.is_empty() {
        return Ok(Vec::new());
    }

    if row_states
        .iter()
        .all(LocalTransitionRowState::uses_nested_transition_measure)
    {
        let shared_theta = row_operators[0].theta;
        let shared_kernel_sensitivity = build_shared_row_frequency_batch_kernel_sensitivity(
            base_context,
            spectral_domain,
            driver_parameters,
            shared_theta,
            NodePriceSharedPlusShiftSensitivity::zero(),
        )?;

        return row_states
            .iter()
            .zip(row_operators.iter())
            .map(|(row_state, row_operator)| {
                build_row_frequency_batch_sensitivity_from_shared_kernel(
                    row_state,
                    row_operator,
                    spectral_domain,
                    &shared_kernel_sensitivity,
                )
            })
            .collect();
    }

    row_states
        .iter()
        .zip(row_operators.iter())
        .map(|(row_state, row_operator)| {
            let theta_sensitivity = node_price_shared_plus_shift_theta_sensitivity(
                differentiate_core_local_tilt_theta_for_tilt(
                    base_context,
                    driver_parameters,
                    row_operator.theta,
                )?,
            );
            let shared_kernel_sensitivity = build_shared_row_frequency_batch_kernel_sensitivity(
                base_context,
                spectral_domain,
                driver_parameters,
                row_operator.theta,
                theta_sensitivity,
            )?;

            build_row_frequency_batch_sensitivity_from_shared_kernel(
                row_state,
                row_operator,
                spectral_domain,
                &shared_kernel_sensitivity,
            )
        })
        .collect()
}

fn node_price_shared_plus_shift_theta_sensitivity(
    theta_sensitivity: CoreLocalTiltImplicitThetaSharedPlusShiftSensitivity,
) -> NodePriceSharedPlusShiftSensitivity {
    NodePriceSharedPlusShiftSensitivity {
        alpha_derivative: theta_sensitivity.alpha_derivative,
        beta_0_derivative: theta_sensitivity.beta_0_derivative,
        delta_derivative: theta_sensitivity.delta_derivative,
        delta_beta_derivative: theta_sensitivity.delta_beta_derivative,
    }
}

fn node_price_shared_plus_shift_sensitivity_from_characteristic(
    sensitivity: PricingCharacteristicFunctionSharedPlusShiftSensitivity,
    theta_sensitivity: NodePriceSharedPlusShiftSensitivity,
) -> NodePriceSharedPlusShiftComplexSensitivity {
    NodePriceSharedPlusShiftComplexSensitivity {
        alpha_derivative: sensitivity.alpha_derivative.add(
            sensitivity
                .theta_derivative
                .scale(theta_sensitivity.alpha_derivative),
        ),
        beta_0_derivative: sensitivity.beta_0_derivative.add(
            sensitivity
                .theta_derivative
                .scale(theta_sensitivity.beta_0_derivative),
        ),
        delta_derivative: sensitivity.delta_derivative.add(
            sensitivity
                .theta_derivative
                .scale(theta_sensitivity.delta_derivative),
        ),
        delta_beta_derivative: sensitivity.delta_beta_derivative.add(
            sensitivity
                .theta_derivative
                .scale(theta_sensitivity.delta_beta_derivative),
        ),
    }
}

fn build_shared_row_frequency_batch_kernel_sensitivity(
    base_context: &LocalTransitionRequestContext,
    spectral_domain: SpectralDomain,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    theta: f64,
    theta_sensitivity: NodePriceSharedPlusShiftSensitivity,
) -> Result<SharedRowFrequencyBatchKernelSensitivity, VanillaOptionPricingRunError> {
    let frequency_count = spectral_domain.node_count;
    let mut batch_arguments = Vec::with_capacity(2 * frequency_count - 1);

    for frequency_index in 0..(2 * frequency_count - 1) {
        batch_arguments.push(ComplexScalar::new(
            spectral_domain.basis_frequency(frequency_index),
            -theta,
        ));
    }

    let batch_sensitivities =
        evaluate_pricing_log_return_characteristic_function_batch_shared_plus_shift_sensitivities_for_context(
            base_context,
            driver_parameters,
            batch_arguments.as_slice(),
        )?;

    let denominator_sensitivity = batch_sensitivities[0];
    let denominator = denominator_sensitivity.value;

    let positive_frequency_value_sensitivities = batch_sensitivities
        .into_iter()
        .map(|sensitivity| {
            Ok(node_price_shared_plus_shift_sensitivity_from_characteristic(
                PricingCharacteristicFunctionSharedPlusShiftSensitivity {
                    value: sensitivity
                        .value
                        .divide(denominator)
                        .ok_or(
                            VanillaOptionPricingRunError::InvalidRiskNeutralCharacteristicFunction {
                                numerator: sensitivity.value,
                                denominator,
                            },
                        )?,
                    alpha_derivative: pricing_ratio_derivative(
                        sensitivity.value,
                        sensitivity
                            .alpha_derivative
                            .add(sensitivity.theta_derivative.scale(theta_sensitivity.alpha_derivative)),
                        denominator_sensitivity.value,
                        denominator_sensitivity.alpha_derivative.add(
                            denominator_sensitivity
                                .theta_derivative
                                .scale(theta_sensitivity.alpha_derivative),
                        ),
                    )?,
                    beta_0_derivative: pricing_ratio_derivative(
                        sensitivity.value,
                        sensitivity.beta_0_derivative.add(
                            sensitivity.theta_derivative.scale(theta_sensitivity.beta_0_derivative),
                        ),
                        denominator_sensitivity.value,
                        denominator_sensitivity.beta_0_derivative.add(
                            denominator_sensitivity
                                .theta_derivative
                                .scale(theta_sensitivity.beta_0_derivative),
                        ),
                    )?,
                    delta_derivative: pricing_ratio_derivative(
                        sensitivity.value,
                        sensitivity
                            .delta_derivative
                            .add(sensitivity.theta_derivative.scale(theta_sensitivity.delta_derivative)),
                        denominator_sensitivity.value,
                        denominator_sensitivity.delta_derivative.add(
                            denominator_sensitivity
                                .theta_derivative
                                .scale(theta_sensitivity.delta_derivative),
                        ),
                    )?,
                    mu_derivative: ComplexScalar::new(0.0, 0.0),
                    delta_beta_derivative: pricing_ratio_derivative(
                        sensitivity.value,
                        sensitivity.delta_beta_derivative.add(
                            sensitivity
                                .theta_derivative
                                .scale(theta_sensitivity.delta_beta_derivative),
                        ),
                        denominator_sensitivity.value,
                        denominator_sensitivity.delta_beta_derivative.add(
                            denominator_sensitivity
                                .theta_derivative
                                .scale(theta_sensitivity.delta_beta_derivative),
                        ),
                    )?,
                    theta_derivative: ComplexScalar::new(0.0, 0.0),
                },
                NodePriceSharedPlusShiftSensitivity::zero(),
            ))
        })
        .collect::<Result<Vec<_>, VanillaOptionPricingRunError>>()?;

    let local_expectation_sensitivity =
        derive_expected_increment_sensitivity_from_frequency_values(
            positive_frequency_value_sensitivities.as_slice(),
            spectral_domain,
        )?;

    Ok(SharedRowFrequencyBatchKernelSensitivity {
        positive_frequency_value_sensitivities,
        local_expectation_sensitivity,
    })
}

fn derive_expected_increment_sensitivity_from_frequency_values(
    positive_frequency_value_sensitivities: &[NodePriceSharedPlusShiftComplexSensitivity],
    spectral_domain: SpectralDomain,
) -> Result<NodePriceSharedPlusShiftSensitivity, VanillaOptionPricingRunError> {
    let first_frequency = spectral_domain.basis_frequency(1);
    let first_sensitivity = positive_frequency_value_sensitivities
        .get(1)
        .copied()
        .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
            message:
                "spectral expected-increment sensitivity recovery requires at least two frequencies"
                    .to_string(),
        })?;

    Ok(NodePriceSharedPlusShiftSensitivity {
        alpha_derivative: first_sensitivity.alpha_derivative.imaginary / first_frequency,
        beta_0_derivative: first_sensitivity.beta_0_derivative.imaginary / first_frequency,
        delta_derivative: first_sensitivity.delta_derivative.imaginary / first_frequency,
        delta_beta_derivative: first_sensitivity.delta_beta_derivative.imaginary / first_frequency,
    })
}

fn build_row_frequency_batch_sensitivity_from_shared_kernel(
    _transition_row: &LocalTransitionRowState,
    row_operator: &RowFrequencyBatchSolution,
    spectral_domain: SpectralDomain,
    shared_kernel_sensitivity: &SharedRowFrequencyBatchKernelSensitivity,
) -> Result<RowFrequencyBatchSolutionSharedPlusShiftSensitivity, VanillaOptionPricingRunError> {
    let basis_forward_phase_sensitivities = row_operator
        .basis_forward_phases
        .iter()
        .copied()
        .enumerate()
        .map(|(basis_index, phase)| {
            basis_phase_sensitivity(
                phase,
                spectral_domain.basis_frequency(basis_index),
                shared_kernel_sensitivity.local_expectation_sensitivity,
            )
        })
        .collect::<Vec<_>>();

    let basis_backward_phase_sensitivities = basis_forward_phase_sensitivities
        .iter()
        .copied()
        .map(NodePriceSharedPlusShiftComplexSensitivity::conjugate)
        .collect::<Vec<_>>();

    Ok(RowFrequencyBatchSolutionSharedPlusShiftSensitivity {
        positive_frequency_value_sensitivities: shared_kernel_sensitivity
            .positive_frequency_value_sensitivities
            .clone(),
        basis_forward_phase_sensitivities,
        basis_backward_phase_sensitivities,
    })
}

fn basis_phase_sensitivity(
    phase: ComplexScalar,
    basis_frequency: f64,
    local_expectation_sensitivity: NodePriceSharedPlusShiftSensitivity,
) -> NodePriceSharedPlusShiftComplexSensitivity {
    let scaling_factor = ComplexScalar::new(0.0, -basis_frequency);

    NodePriceSharedPlusShiftComplexSensitivity {
        alpha_derivative: complex_multiply(
            phase,
            scaling_factor.scale(local_expectation_sensitivity.alpha_derivative),
        ),
        beta_0_derivative: complex_multiply(
            phase,
            scaling_factor.scale(local_expectation_sensitivity.beta_0_derivative),
        ),
        delta_derivative: complex_multiply(
            phase,
            scaling_factor.scale(local_expectation_sensitivity.delta_derivative),
        ),
        delta_beta_derivative: complex_multiply(
            phase,
            scaling_factor.scale(local_expectation_sensitivity.delta_beta_derivative),
        ),
    }
}

fn solve_interval_row_frequency_batches_adaptively(
    base_context: &LocalTransitionRequestContext,
    collocation_nodes: &[f64],
    row_states: &[LocalTransitionRowState],
    theta_hints: &[Option<f64>],
    spectral_domain: SpectralDomain,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    collect_extended_gradient_diagnostics: bool,
    collect_theta_warm_start_sensitivities: bool,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
) -> Result<
    (
        Vec<RowFrequencyBatchSolution>,
        Vec<SpectralThetaWarmStartRowState>,
        IntervalRowSolveDiagnostics,
    ),
    VanillaOptionPricingRunError,
> {
    let row_count = collocation_nodes.len();
    if row_count == 0 {
        return Ok((
            Vec::new(),
            Vec::new(),
            IntervalRowSolveDiagnostics::default(),
        ));
    }

    if row_states
        .iter()
        .all(LocalTransitionRowState::uses_nested_transition_measure)
    {
        return solve_nested_interval_row_frequency_batches_with_shared_theta(
            base_context,
            row_states,
            theta_hints,
            spectral_domain,
            driver_parameters,
            collect_extended_gradient_diagnostics,
            collect_theta_warm_start_sensitivities,
            pricing_thread_pool,
            cpu_count,
        );
    }

    let mut solved_rows = vec![None; row_count];
    let mut solved_theta_warm_start_states = vec![None; row_count];
    let mut row_solve_diagnostics = IntervalRowSolveDiagnostics::default();

    let mut seeded_row_indices =
        derive_parallel_adaptive_seed_row_indices(row_count, resolved_pricing_cpu_count(cpu_count));

    for (row_index, row_state) in row_states.iter().enumerate() {
        if row_state.uses_nested_transition_measure() {
            seeded_row_indices.push(row_index);
        }
    }

    seeded_row_indices.sort_unstable();
    seeded_row_indices.dedup();

    let seeded_row_solutions =
        if resolved_pricing_cpu_count(cpu_count) > 1 && seeded_row_indices.len() > 1 {
            install_pricing_parallelism(pricing_thread_pool, || {
                seeded_row_indices
                    .par_iter()
                    .map(|&row_index| {
                        solve_exact_row_frequency_batch(
                            base_context,
                            &row_states[row_index],
                            theta_hints[row_index],
                            spectral_domain,
                            driver_parameters,
                            collect_extended_gradient_diagnostics,
                            collect_theta_warm_start_sensitivities,
                            pricing_thread_pool,
                            cpu_count,
                        )
                        .map(|solution| (row_index, solution))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })?
        } else {
            let mut seeded_solutions = Vec::with_capacity(seeded_row_indices.len());
            for &row_index in &seeded_row_indices {
                seeded_solutions.push((
                    row_index,
                    solve_exact_row_frequency_batch(
                        base_context,
                        &row_states[row_index],
                        theta_hints[row_index],
                        spectral_domain,
                        driver_parameters,
                        collect_extended_gradient_diagnostics,
                        collect_theta_warm_start_sensitivities,
                        pricing_thread_pool,
                        cpu_count,
                    )?,
                ));
            }
            seeded_solutions
        };

    for (row_index, solution) in seeded_row_solutions {
        row_solve_diagnostics.record(&solution);
        solved_theta_warm_start_states[row_index] = Some(solution.theta_warm_start_state);
        solved_rows[row_index] = Some(solution.solution);
    }

    let mut active_segments = seeded_row_indices
        .windows(2)
        .filter_map(|window| {
            let left = window[0];
            let right = window[1];
            (right > left + 1).then_some((left, right))
        })
        .collect::<Vec<_>>();

    while !active_segments.is_empty() {
        let tasks = active_segments
            .iter()
            .map(|(left_row_index, right_row_index)| {
                let left_theta = solved_rows[*left_row_index]
                    .as_ref()
                    .expect("adaptive left row must be solved")
                    .theta;
                let right_theta = solved_rows[*right_row_index]
                    .as_ref()
                    .expect("adaptive right row must be solved")
                    .theta;

                let midpoint_row_index = (left_row_index + right_row_index) / 2;

                let predicted_theta = interpolate_theta_across_deviation_rows(
                    collocation_nodes[*left_row_index],
                    left_theta,
                    collocation_nodes[*right_row_index],
                    right_theta,
                    collocation_nodes[midpoint_row_index],
                );

                AdaptiveRowSolveTask {
                    left_row_index: *left_row_index,
                    midpoint_row_index,
                    right_row_index: *right_row_index,
                    predicted_theta,
                }
            })
            .collect::<Vec<_>>();

        let solved_midpoints = if resolved_pricing_cpu_count(cpu_count) > 1 && tasks.len() > 1 {
            install_pricing_parallelism(pricing_thread_pool, || {
                tasks
                    .par_iter()
                    .map(|task| {
                        solve_adaptive_midpoint_row_frequency_batch(
                            base_context,
                            &row_states[task.midpoint_row_index],
                            theta_hints[task.midpoint_row_index].or(Some(task.predicted_theta)),
                            task.predicted_theta,
                            spectral_domain,
                            driver_parameters,
                            collect_extended_gradient_diagnostics,
                            collect_theta_warm_start_sensitivities,
                            pricing_thread_pool,
                            cpu_count,
                        )
                        .map(|solution| (task.midpoint_row_index, solution))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })?
        } else {
            let mut midpoint_solutions = Vec::with_capacity(tasks.len());
            for task in &tasks {
                midpoint_solutions.push((
                    task.midpoint_row_index,
                    solve_adaptive_midpoint_row_frequency_batch(
                        base_context,
                        &row_states[task.midpoint_row_index],
                        theta_hints[task.midpoint_row_index].or(Some(task.predicted_theta)),
                        task.predicted_theta,
                        spectral_domain,
                        driver_parameters,
                        collect_extended_gradient_diagnostics,
                        collect_theta_warm_start_sensitivities,
                        pricing_thread_pool,
                        cpu_count,
                    )?,
                ));
            }

            midpoint_solutions
        };

        for (midpoint_row_index, solution) in solved_midpoints {
            row_solve_diagnostics.record(&solution);

            solved_theta_warm_start_states[midpoint_row_index] =
                Some(solution.theta_warm_start_state);

            solved_rows[midpoint_row_index] = Some(solution.solution);
        }

        let mut next_segments = Vec::new();
        for task in tasks {
            if task.midpoint_row_index > task.left_row_index + 1 {
                next_segments.push((task.left_row_index, task.midpoint_row_index));
            }
            if task.right_row_index > task.midpoint_row_index + 1 {
                next_segments.push((task.midpoint_row_index, task.right_row_index));
            }
        }

        active_segments = next_segments;
    }

    let row_operators = solved_rows
        .into_iter()
        .enumerate()
        .map(|(row_index, solution)| {
            solution.ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
                message: format!(
                    "adaptive spectral interval operator left row {row_index} unsolved"
                ),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let theta_warm_start_states = solved_theta_warm_start_states
        .into_iter()
        .enumerate()
        .map(|(row_index, row_state)| {
            row_state.ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
                message: format!(
                    "adaptive spectral theta warm-start state left row {row_index} unsolved"
                ),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok((
        row_operators,
        theta_warm_start_states,
        row_solve_diagnostics,
    ))
}

fn derive_parallel_adaptive_seed_row_indices(
    row_count: usize,
    resolved_cpu_count: usize,
) -> Vec<usize> {
    if row_count <= 1 {
        return vec![0];
    }

    let last_row_index = row_count - 1;

    // Keep at least one unsolved interior row inside every initial span so the
    // adaptive midpoint certification step has meaningful work to do.
    let target_segment_count = resolved_cpu_count.min(last_row_index / 2).max(1);
    let mut seeded_row_indices = Vec::with_capacity(target_segment_count + 1);

    for segment_boundary_index in 0..=target_segment_count {
        seeded_row_indices.push(segment_boundary_index * last_row_index / target_segment_count);
    }

    seeded_row_indices
}

fn solve_nested_interval_row_frequency_batches_with_shared_theta(
    base_context: &LocalTransitionRequestContext,
    row_states: &[LocalTransitionRowState],
    theta_hints: &[Option<f64>],
    spectral_domain: SpectralDomain,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    collect_extended_gradient_diagnostics: bool,
    _collect_theta_warm_start_sensitivities: bool,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
) -> Result<
    (
        Vec<RowFrequencyBatchSolution>,
        Vec<SpectralThetaWarmStartRowState>,
        IntervalRowSolveDiagnostics,
    ),
    VanillaOptionPricingRunError,
> {
    let representative_row_index = row_states
        .iter()
        .enumerate()
        .min_by(|(_, left_row_state), (_, right_row_state)| {
            left_row_state
                .boundary_state
                .deviation
                .abs()
                .total_cmp(&right_row_state.boundary_state.deviation.abs())
        })
        .map(|(row_index, _)| row_index)
        .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "nested spectral interval solve expected at least one row".to_string(),
        })?;

    let theta_started_at = Instant::now();
    let (theta, _) = resolve_local_transition_theta_for_row_with_parallelism_and_hint(
        base_context,
        &row_states[representative_row_index],
        driver_parameters,
        theta_hints[representative_row_index],
        pricing_thread_pool,
        cpu_count,
    )?;

    let shared_batch_started_at = Instant::now();
    let shared_kernel = solve_shared_row_frequency_batch_kernel(
        base_context,
        spectral_domain,
        driver_parameters,
        theta,
    )?;
    let shared_batch_elapsed_millis = elapsed_millis(shared_batch_started_at);

    let row_materialization_started_at = Instant::now();
    let row_operators = if resolved_pricing_cpu_count(cpu_count) > 1 && row_states.len() > 1 {
        install_pricing_parallelism(pricing_thread_pool, || {
            row_states
                .par_iter()
                .map(|row_state| {
                    build_row_frequency_batch_from_shared_kernel(
                        base_context,
                        row_state,
                        spectral_domain,
                        &shared_kernel,
                    )
                })
                .collect::<Result<Vec<_>, _>>()
        })?
    } else {
        let mut row_operators = Vec::with_capacity(row_states.len());

        for row_state in row_states {
            row_operators.push(build_row_frequency_batch_from_shared_kernel(
                base_context,
                row_state,
                spectral_domain,
                &shared_kernel,
            )?);
        }

        row_operators
    };

    let (buy_shift_configuration_gradient_norm, sell_shift_configuration_gradient_norm) =
        resolve_shift_configuration_gradient_norms(driver_parameters, theta)?;

    let core_log_moment_sensitivity = if collect_extended_gradient_diagnostics {
        Some(
            differentiate_core_pure_imaginary_log_moment_exponent_for_tilt(
                base_context,
                driver_parameters,
                theta,
            )?,
        )
    } else {
        None
    };

    let core_log_moment_shift_configuration_gradient_norm =
        if let Some(core_log_moment_sensitivity) = core_log_moment_sensitivity.as_ref() {
            if collect_extended_gradient_diagnostics {
                core_log_moment_shift_configuration_gradient_norm(core_log_moment_sensitivity)
            } else {
                0.0
            }
        } else {
            0.0
        };

    Ok((
        row_operators,
        vec![SpectralThetaWarmStartRowState::from_theta_hint(theta); row_states.len()],
        IntervalRowSolveDiagnostics {
            seed_exact_transition_row_count: 1,
            shared_theta_transition_row_count: row_states.len().saturating_sub(1),
            certified_transition_row_count: 0,
            fallback_exact_transition_row_count: 0,
            theta_search_elapsed_millis: elapsed_millis(theta_started_at),
            theta_certification_elapsed_millis: 0,
            row_batch_elapsed_millis: shared_batch_elapsed_millis
                + elapsed_millis(row_materialization_started_at),
            total_buy_shift_configuration_gradient_norm: buy_shift_configuration_gradient_norm
                * row_states.len() as f64,
            total_sell_shift_configuration_gradient_norm: sell_shift_configuration_gradient_norm
                * row_states.len() as f64,
            total_core_log_moment_shift_configuration_gradient_norm:
                core_log_moment_shift_configuration_gradient_norm * row_states.len() as f64,
            gradient_norm_sample_count: row_states.len(),
        },
    ))
}

fn solve_exact_row_frequency_batch(
    base_context: &LocalTransitionRequestContext,
    transition_row: &LocalTransitionRowState,
    initial_theta_hint: Option<f64>,
    spectral_domain: SpectralDomain,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    collect_extended_gradient_diagnostics: bool,
    collect_theta_warm_start_sensitivities: bool,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
) -> Result<TimedRowFrequencyBatchSolution, VanillaOptionPricingRunError> {
    let theta_started_at = Instant::now();
    let (theta, _) = resolve_local_transition_theta_for_row_with_parallelism_and_hint(
        base_context,
        transition_row,
        driver_parameters,
        initial_theta_hint,
        pricing_thread_pool,
        cpu_count,
    )?;

    let row_batch_started_at = Instant::now();
    let solution = solve_row_frequency_batch(
        base_context,
        transition_row,
        spectral_domain,
        driver_parameters,
        theta,
    )?;

    let (buy_shift_configuration_gradient_norm, sell_shift_configuration_gradient_norm) =
        resolve_shift_configuration_gradient_norms(driver_parameters, theta)?;

    let core_log_moment_sensitivity =
        if collect_extended_gradient_diagnostics || collect_theta_warm_start_sensitivities {
            Some(
                differentiate_core_pure_imaginary_log_moment_exponent_for_tilt(
                    base_context,
                    driver_parameters,
                    theta,
                )?,
            )
        } else {
            None
        };

    let core_log_moment_shift_configuration_gradient_norm =
        if let Some(core_log_moment_sensitivity) = core_log_moment_sensitivity.as_ref() {
            if collect_extended_gradient_diagnostics {
                core_log_moment_shift_configuration_gradient_norm(core_log_moment_sensitivity)
            } else {
                0.0
            }
        } else {
            0.0
        };

    let theta_warm_start_state = if collect_theta_warm_start_sensitivities {
        if let Some(theta_warm_start_sensitivity) = core_log_moment_sensitivity
            .as_ref()
            .and_then(try_resolve_theta_warm_start_sensitivity)
        {
            SpectralThetaWarmStartRowState::from_theta_hint(theta)
                .with_local_theta_sensitivity(theta_warm_start_sensitivity)
        } else {
            SpectralThetaWarmStartRowState::from_theta_hint(theta)
        }
    } else {
        SpectralThetaWarmStartRowState::from_theta_hint(theta)
    };

    Ok(TimedRowFrequencyBatchSolution {
        solution,
        solve_mode: AdaptiveRowSolveMode::SeedExact,
        theta_search_elapsed_millis: elapsed_millis(theta_started_at),
        theta_certification_elapsed_millis: 0,
        row_batch_elapsed_millis: elapsed_millis(row_batch_started_at),
        theta_warm_start_state,
        buy_shift_configuration_gradient_norm,
        sell_shift_configuration_gradient_norm,
        core_log_moment_shift_configuration_gradient_norm,
    })
}

fn solve_adaptive_midpoint_row_frequency_batch(
    base_context: &LocalTransitionRequestContext,
    transition_row: &LocalTransitionRowState,
    initial_theta_hint: Option<f64>,
    predicted_theta: f64,
    spectral_domain: SpectralDomain,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    collect_extended_gradient_diagnostics: bool,
    collect_theta_warm_start_sensitivities: bool,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
) -> Result<TimedRowFrequencyBatchSolution, VanillaOptionPricingRunError> {
    let certification_started_at = Instant::now();

    if certify_local_transition_theta_for_row_with_parallelism(
        base_context,
        transition_row,
        driver_parameters,
        predicted_theta,
        pricing_thread_pool,
        cpu_count,
    )? {
        let row_batch_started_at = Instant::now();

        let solution = solve_row_frequency_batch(
            base_context,
            transition_row,
            spectral_domain,
            driver_parameters,
            predicted_theta,
        )?;

        let (buy_shift_configuration_gradient_norm, sell_shift_configuration_gradient_norm) =
            resolve_shift_configuration_gradient_norms(driver_parameters, predicted_theta)?;

        let core_log_moment_shift_configuration_gradient_norm =
            if collect_extended_gradient_diagnostics {
                let core_log_moment_sensitivity =
                    differentiate_core_pure_imaginary_log_moment_exponent_for_tilt(
                        base_context,
                        driver_parameters,
                        predicted_theta,
                    )?;
                core_log_moment_shift_configuration_gradient_norm(&core_log_moment_sensitivity)
            } else {
                0.0
            };

        return Ok(TimedRowFrequencyBatchSolution {
            solution,
            solve_mode: AdaptiveRowSolveMode::CertifiedPredicted,
            theta_search_elapsed_millis: 0,
            theta_certification_elapsed_millis: elapsed_millis(certification_started_at),
            row_batch_elapsed_millis: elapsed_millis(row_batch_started_at),
            theta_warm_start_state: SpectralThetaWarmStartRowState::from_theta_hint(
                predicted_theta,
            ),
            buy_shift_configuration_gradient_norm,
            sell_shift_configuration_gradient_norm,
            core_log_moment_shift_configuration_gradient_norm,
        });
    }

    let exact_solution = solve_exact_row_frequency_batch(
        base_context,
        transition_row,
        initial_theta_hint,
        spectral_domain,
        driver_parameters,
        collect_extended_gradient_diagnostics,
        collect_theta_warm_start_sensitivities,
        pricing_thread_pool,
        cpu_count,
    )?;

    Ok(TimedRowFrequencyBatchSolution {
        solution: exact_solution.solution,
        solve_mode: AdaptiveRowSolveMode::FallbackExact,
        theta_search_elapsed_millis: exact_solution.theta_search_elapsed_millis,
        theta_certification_elapsed_millis: elapsed_millis(certification_started_at),
        row_batch_elapsed_millis: exact_solution.row_batch_elapsed_millis,
        theta_warm_start_state: exact_solution.theta_warm_start_state,
        buy_shift_configuration_gradient_norm: exact_solution.buy_shift_configuration_gradient_norm,
        sell_shift_configuration_gradient_norm: exact_solution
            .sell_shift_configuration_gradient_norm,
        core_log_moment_shift_configuration_gradient_norm: exact_solution
            .core_log_moment_shift_configuration_gradient_norm,
    })
}

fn interpolate_theta_across_deviation_rows(
    left_deviation: f64,
    left_theta: f64,
    right_deviation: f64,
    right_theta: f64,
    target_deviation: f64,
) -> f64 {
    let deviation_span = right_deviation - left_deviation;
    if deviation_span == 0.0 {
        return left_theta;
    }

    let interpolation_weight = (target_deviation - left_deviation) / deviation_span;

    left_theta + interpolation_weight * (right_theta - left_theta)
}

fn elapsed_millis(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn derive_expected_increment_from_frequency_values(
    positive_frequency_values: &[ComplexScalar],
    spectral_domain: SpectralDomain,
) -> Result<f64, VanillaOptionPricingRunError> {
    let first_frequency = spectral_domain.basis_frequency(1);
    let first_value = positive_frequency_values.get(1).copied().ok_or_else(|| {
        VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "spectral expected-increment recovery requires at least two frequencies"
                .to_string(),
        }
    })?;

    Ok(first_value.imaginary / first_frequency)
}

fn values_to_cosine_coefficients(
    values_by_frequency: Vec<Vec<ComplexScalar>>,
) -> Vec<Vec<ComplexScalar>> {
    let frequency_count = values_by_frequency.len();
    let node_count = values_by_frequency.first().map_or(0, Vec::len);

    let mut coefficients = vec![vec![ComplexScalar::new(0.0, 0.0); node_count]; frequency_count];

    for frequency_index in 0..frequency_count {
        for basis_index in 0..node_count {
            let scale = if basis_index == 0 {
                1.0 / node_count as f64
            } else {
                2.0 / node_count as f64
            };

            let mut coefficient = ComplexScalar::new(0.0, 0.0);

            for node_index in 0..node_count {
                let angle = std::f64::consts::PI * basis_index as f64 * (node_index as f64 + 0.5)
                    / node_count as f64;
                coefficient = coefficient
                    .add(values_by_frequency[frequency_index][node_index].scale(angle.cos()));
            }

            coefficients[frequency_index][basis_index] = coefficient.scale(scale);
        }
    }

    coefficients
}

fn values_to_cosine_coefficients_sensitivities(
    values_by_frequency: Vec<Vec<NodePriceSharedPlusShiftComplexSensitivity>>,
) -> Vec<Vec<NodePriceSharedPlusShiftComplexSensitivity>> {
    let frequency_count = values_by_frequency.len();
    let node_count = values_by_frequency.first().map_or(0, Vec::len);

    let mut coefficients =
        vec![vec![NodePriceSharedPlusShiftComplexSensitivity::zero(); node_count]; frequency_count];

    for frequency_index in 0..frequency_count {
        for basis_index in 0..node_count {
            let scale = if basis_index == 0 {
                1.0 / node_count as f64
            } else {
                2.0 / node_count as f64
            };

            let mut coefficient = NodePriceSharedPlusShiftComplexSensitivity::zero();

            for node_index in 0..node_count {
                let angle = std::f64::consts::PI * basis_index as f64 * (node_index as f64 + 0.5)
                    / node_count as f64;
                coefficient = coefficient
                    .add(values_by_frequency[frequency_index][node_index].scale(angle.cos()));
            }

            coefficients[frequency_index][basis_index] = coefficient.scale(scale);
        }
    }

    coefficients
}

fn evaluate_cosine_series(
    coefficients: &[ComplexScalar],
    spectral_domain: SpectralDomain,
    evaluation_point: f64,
    _frequency_index: usize,
) -> ComplexScalar {
    coefficients.iter().enumerate().fold(
        ComplexScalar::new(0.0, 0.0),
        |accum, (basis_index, coefficient)| {
            let angle = spectral_domain.basis_frequency(basis_index)
                * (evaluation_point - spectral_domain.deviation_lower_bound);
            accum.add(coefficient.scale(angle.cos()))
        },
    )
}

fn evaluate_cosine_series_sensitivity(
    coefficient_sensitivities: &[NodePriceSharedPlusShiftComplexSensitivity],
    spectral_domain: SpectralDomain,
    evaluation_point: f64,
    _frequency_index: usize,
) -> NodePriceSharedPlusShiftComplexSensitivity {
    coefficient_sensitivities.iter().enumerate().fold(
        NodePriceSharedPlusShiftComplexSensitivity::zero(),
        |accumulator, (basis_index, coefficient_sensitivity)| {
            let angle = spectral_domain.basis_frequency(basis_index)
                * (evaluation_point - spectral_domain.deviation_lower_bound);
            accumulator.add(coefficient_sensitivity.scale(angle.cos()))
        },
    )
}

fn complex_scale_sensitivity(
    value: ComplexScalar,
    sensitivity: NodePriceSharedPlusShiftComplexSensitivity,
) -> NodePriceSharedPlusShiftComplexSensitivity {
    NodePriceSharedPlusShiftComplexSensitivity {
        alpha_derivative: complex_multiply(value, sensitivity.alpha_derivative),
        beta_0_derivative: complex_multiply(value, sensitivity.beta_0_derivative),
        delta_derivative: complex_multiply(value, sensitivity.delta_derivative),
        delta_beta_derivative: complex_multiply(value, sensitivity.delta_beta_derivative),
    }
}

fn complex_expectation_sensitivity(
    basis_forward_phase: ComplexScalar,
    basis_forward_phase_sensitivity: NodePriceSharedPlusShiftComplexSensitivity,
    forward_value: ComplexScalar,
    forward_value_sensitivity: NodePriceSharedPlusShiftComplexSensitivity,
    basis_backward_phase: ComplexScalar,
    basis_backward_phase_sensitivity: NodePriceSharedPlusShiftComplexSensitivity,
    backward_value: ComplexScalar,
    backward_value_sensitivity: NodePriceSharedPlusShiftComplexSensitivity,
) -> NodePriceSharedPlusShiftComplexSensitivity {
    complex_scale_sensitivity(forward_value, basis_forward_phase_sensitivity)
        .add(complex_scale_sensitivity(
            basis_forward_phase,
            forward_value_sensitivity,
        ))
        .add(complex_scale_sensitivity(
            backward_value,
            basis_backward_phase_sensitivity,
        ))
        .add(complex_scale_sensitivity(
            basis_backward_phase,
            backward_value_sensitivity,
        ))
        .scale(0.5)
}

fn build_spectral_domain(
    pricing_request: &VanillaOptionPricingRequest,
    materialization: &RiskNeutralAnchorMaterialization,
    maturity_count: usize,
    _maturity_interval: f64,
    _strikes: &[f64],
    collocation_node_count_override: Option<usize>,
) -> Result<SpectralDomain, VanillaOptionPricingRunError> {
    let terminal_anchor =
        materialization.runtime_interval_anchors.end_anchor_values[maturity_count - 1];

    let current_observed_log_price = pricing_request
        .boundary_input
        .observed_boundary_state
        .observed_log_price;

    let mut anchor_min = pricing_request
        .boundary_input
        .current_anchor_value()
        .map_err(
            |source| VanillaOptionPricingRunError::InvalidExecutionInput {
                source: crate::runner::VanillaOptionPricingExecutionInputError::InvalidRequest {
                    source:
                        crate::contracts::VanillaOptionPricingRequestError::InvalidBoundaryInput {
                            source,
                        },
                },
            },
        )?;

    let mut anchor_max = anchor_min;

    for anchor_value in
        &materialization.runtime_interval_anchors.end_anchor_values[..maturity_count]
    {
        anchor_min = anchor_min.min(*anchor_value);
        anchor_max = anchor_max.max(*anchor_value);
    }

    let observed_log_half_width = (current_observed_log_price.abs() + (anchor_max - anchor_min))
        .max(MINIMUM_SPECTRAL_DEVIATION_DOMAIN_HALF_WIDTH);
    let observed_log_center = current_observed_log_price;

    let node_count =
        collocation_node_count_override.unwrap_or(MINIMUM_SPECTRAL_COLLOCATION_NODE_COUNT);

    Ok(SpectralDomain {
        deviation_lower_bound: observed_log_center - observed_log_half_width - terminal_anchor,
        deviation_upper_bound: observed_log_center + observed_log_half_width - terminal_anchor,
        node_count,
    })
}

fn build_maturity_summary(
    solved_maturity: &SolvedMaturityLaw,
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    representative_price: Option<f64>,
) -> Result<VanillaOptionMaturitySummary, VanillaOptionGridBatchPricingRunError> {
    let spectral_diagnostics = if optional_output_selection.include_integration_quality_diagnostics
    {
        Some(
            SpectralPricingDiagnostics::try_new(
                solved_maturity.deviation_lower_bound,
                solved_maturity.deviation_upper_bound,
                solved_maturity.node_count,
                solved_maturity.interval_summaries.clone(),
            )
            .map_err(|source| {
                VanillaOptionGridBatchPricingRunError::NodePricingFailure {
                    message: format!("spectral maturity diagnostics are invalid: {source}"),
                }
            })?,
        )
    } else {
        None
    };

    let integration_quality =
        if let (Some(price), Some(diagnostics)) = (representative_price, spectral_diagnostics) {
            Some(build_spectral_compatibility_integration_diagnostics(
                price,
                solved_maturity.maturity_interval,
                &diagnostics,
            ))
        } else {
            None
        };

    Ok(VanillaOptionMaturitySummary {
        maturity_interval: solved_maturity.maturity_interval,
        terminal_anchor_value: solved_maturity.terminal_anchor_value,
        deviation_domain_lower_bound: solved_maturity.deviation_lower_bound,
        deviation_domain_upper_bound: solved_maturity.deviation_upper_bound,
        collocation_node_count: solved_maturity.node_count,
        collocation_nodes: SpectralDomain {
            deviation_lower_bound: solved_maturity.deviation_lower_bound,
            deviation_upper_bound: solved_maturity.deviation_upper_bound,
            node_count: solved_maturity.node_count,
        }
        .collocation_nodes(),
        interval_summaries: solved_maturity.interval_summaries.clone(),
        representative_transform: if optional_output_selection.include_transform_diagnostics
            || optional_output_selection.include_calibration_hooks
        {
            solved_maturity.representative_transform
        } else {
            None
        },
        integration_quality,
    })
}

fn cos_price_from_terminal_characteristic_function(
    spot: f64,
    strike: f64,
    payoff_type: VanillaOptionPayoffType,
    risk_free_rate: f64,
    _dividend_yield: f64,
    solved_maturity: &SolvedMaturityLaw,
) -> Option<f64> {
    let maturity_year_fraction = intraday_year_fraction(solved_maturity.maturity_interval);

    let y_lower_bound =
        solved_maturity.terminal_anchor_value + solved_maturity.deviation_lower_bound;
    let y_upper_bound =
        solved_maturity.terminal_anchor_value + solved_maturity.deviation_upper_bound;

    let width = y_upper_bound - y_lower_bound;
    if width <= 0.0 || !width.is_finite() {
        return None;
    }

    let strike_boundary = solved_maturity.observed_log_price + (strike / spot).ln();
    let discount_factor = (-risk_free_rate * maturity_year_fraction).exp();

    let mut price = 0.0;

    for (frequency_index, phi_value) in solved_maturity
        .characteristic_function_values
        .iter()
        .enumerate()
    {
        let coefficient = payoff_cos_coefficient(
            payoff_type,
            spot,
            strike,
            y_lower_bound,
            y_upper_bound,
            strike_boundary,
            frequency_index,
        )?;

        let phase = complex_exponential(ComplexScalar::new(
            0.0,
            -(frequency_index as f64 * std::f64::consts::PI / width) * y_lower_bound,
        ));

        let weight = if frequency_index == 0 { 0.5 } else { 1.0 };

        price += weight * complex_multiply(*phi_value, phase).real * coefficient;
    }

    Some((discount_factor * price).max(0.0))
}

fn cos_price_sensitivity_from_terminal_characteristic_function(
    spot: f64,
    strike: f64,
    payoff_type: VanillaOptionPayoffType,
    risk_free_rate: f64,
    _dividend_yield: f64,
    solved_maturity: &SolvedMaturityLaw,
) -> Result<NodePriceSharedPlusShiftSensitivity, VanillaOptionPricingRunError> {
    let Some(characteristic_function_sensitivities) = solved_maturity
        .characteristic_function_sensitivities
        .as_ref()
    else {
        return Ok(NodePriceSharedPlusShiftSensitivity::zero());
    };

    let maturity_year_fraction = intraday_year_fraction(solved_maturity.maturity_interval);

    let y_lower_bound =
        solved_maturity.terminal_anchor_value + solved_maturity.deviation_lower_bound;
    let y_upper_bound =
        solved_maturity.terminal_anchor_value + solved_maturity.deviation_upper_bound;

    let width = y_upper_bound - y_lower_bound;
    if width <= 0.0 || !width.is_finite() {
        return Err(VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "COS price sensitivity requires a positive finite pricing domain width"
                .to_string(),
        });
    }

    let strike_boundary = solved_maturity.observed_log_price + (strike / spot).ln();
    let discount_factor = (-risk_free_rate * maturity_year_fraction).exp();

    let mut price_sensitivity = NodePriceSharedPlusShiftSensitivity::zero();

    for (frequency_index, phi_sensitivity) in
        characteristic_function_sensitivities.iter().enumerate()
    {
        let coefficient = payoff_cos_coefficient(
            payoff_type,
            spot,
            strike,
            y_lower_bound,
            y_upper_bound,
            strike_boundary,
            frequency_index,
        )
        .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: format!(
                "failed to resolve payoff COS coefficient for frequency_index={frequency_index}"
            ),
        })?;

        let phase = complex_exponential(ComplexScalar::new(
            0.0,
            -(frequency_index as f64 * std::f64::consts::PI / width) * y_lower_bound,
        ));
        let weight = if frequency_index == 0 { 0.5 } else { 1.0 };
        let scaled_weight = discount_factor * weight * coefficient;

        price_sensitivity = price_sensitivity.add(NodePriceSharedPlusShiftSensitivity {
            alpha_derivative: scaled_weight
                * complex_multiply(phi_sensitivity.alpha_derivative, phase).real,
            beta_0_derivative: scaled_weight
                * complex_multiply(phi_sensitivity.beta_0_derivative, phase).real,
            delta_derivative: scaled_weight
                * complex_multiply(phi_sensitivity.delta_derivative, phase).real,
            delta_beta_derivative: scaled_weight
                * complex_multiply(phi_sensitivity.delta_beta_derivative, phase).real,
        });
    }

    Ok(price_sensitivity)
}

fn resolve_bounded_node_price_sensitivity(
    raw_price_sensitivity: NodePriceSharedPlusShiftSensitivity,
    raw_price: f64,
    bounded_price: f64,
) -> VanillaOptionNodePriceSensitivity {
    if raw_price == bounded_price {
        raw_price_sensitivity.into_output()
    } else {
        // The reported node price is the clipped no-arbitrage output. Away from the exact
        // kink, the active bound is constant with respect to the pricing parameters, so the
        // local derivative of the clipped output is zero.
        NodePriceSharedPlusShiftSensitivity::zero().into_output()
    }
}

fn payoff_cos_coefficient(
    payoff_type: VanillaOptionPayoffType,
    spot: f64,
    strike: f64,
    domain_lower_bound: f64,
    domain_upper_bound: f64,
    strike_boundary: f64,
    frequency_index: usize,
) -> Option<f64> {
    let width = domain_upper_bound - domain_lower_bound;
    let angular_frequency = frequency_index as f64 * std::f64::consts::PI / width;

    match payoff_type {
        VanillaOptionPayoffType::Call => {
            if strike_boundary >= domain_upper_bound {
                return Some(0.0);
            }

            let lower = strike_boundary.max(domain_lower_bound);
            let upper = domain_upper_bound;

            Some(
                2.0 / width
                    * (spot * chi_coefficient(lower, upper, domain_lower_bound, angular_frequency)
                        - strike
                            * psi_coefficient(lower, upper, domain_lower_bound, angular_frequency)),
            )
        }
        VanillaOptionPayoffType::Put => {
            if strike_boundary <= domain_lower_bound {
                return Some(0.0);
            }

            let lower = domain_lower_bound;
            let upper = strike_boundary.min(domain_upper_bound);

            Some(
                2.0 / width
                    * (strike
                        * psi_coefficient(lower, upper, domain_lower_bound, angular_frequency)
                        - spot
                            * chi_coefficient(lower, upper, domain_lower_bound, angular_frequency)),
            )
        }
    }
}

fn chi_coefficient(lower: f64, upper: f64, domain_lower_bound: f64, angular_frequency: f64) -> f64 {
    if angular_frequency == 0.0 {
        return upper.exp() - lower.exp();
    }

    let upper_shift = upper - domain_lower_bound;
    let lower_shift = lower - domain_lower_bound;

    let denominator = 1.0 + angular_frequency * angular_frequency;

    ((upper.exp() * (angular_frequency * upper_shift.sin() + upper_shift.cos()))
        - (lower.exp() * (angular_frequency * lower_shift.sin() + lower_shift.cos())))
        / denominator
}

fn psi_coefficient(lower: f64, upper: f64, domain_lower_bound: f64, angular_frequency: f64) -> f64 {
    if angular_frequency == 0.0 {
        return upper - lower;
    }

    let upper_shift = upper - domain_lower_bound;
    let lower_shift = lower - domain_lower_bound;

    (upper_shift.sin() - lower_shift.sin()) / angular_frequency
}

#[cfg(test)]
mod tests {
    use super::{build_spectral_domain, solve_interval_row_frequency_batches_adaptively};
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
    use janiform_model_kernels::hawkes::HawkesExcitationState;
    use janiform_model_kernels::mean_reversion::{
        MeanReversionAnchorModel, MeanReversionBoundaryState, MeanReversionFittedParameters,
    };

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

        let (solved_rows, _, _) = solve_interval_row_frequency_batches_adaptively(
            &base_context,
            collocation_nodes.as_slice(),
            row_states.as_slice(),
            &vec![None; collocation_nodes.len()],
            spectral_domain,
            driver_parameters,
            false,
            false,
            None,
            Some(1),
        )
        .expect("adaptive interval operator solve should succeed");

        assert_eq!(solved_rows.len(), collocation_nodes.len());

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
