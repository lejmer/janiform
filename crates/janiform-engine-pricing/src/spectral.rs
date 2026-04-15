use std::collections::{BTreeMap, BTreeSet};

use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};

use crate::contracts::{
    SpectralPricingDiagnostics, SpectralPricingIntervalDiagnostics,
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingFailureStage,
    VanillaOptionPricingMarketInputs, VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest, VanillaOptionPricingStatus,
};
use crate::grids::{
    VanillaOptionGridBatchPricingExecutionInput, VanillaOptionGridBatchPricingRunError,
    VanillaOptionGridBatchPricingRunResult, VanillaOptionGridPricingResult,
    VanillaOptionGridRequest, VanillaOptionMaturitySummary,
};
use crate::implied_volatility::{
    VanillaOptionImpliedVolatilityInversionOutcome, VanillaOptionImpliedVolatilityInversionStatus,
    invert_vanilla_option_implied_volatility,
};
use crate::intraday_black_scholes_no_arbitrage_price_bounds;
use crate::progress::PricingProgressEvent;
use crate::runner::{
    LocalTransitionRequestContext, RiskNeutralAnchorMaterialization,
    VanillaOptionPricingExecutionInput, VanillaOptionPricingIntervalFactors,
    VanillaOptionPricingRunError, build_pricing_risk_neutral_anchor_materialization,
    evaluate_pricing_log_return_characteristic_function_batch_for_context,
    resolve_local_transition_theta_for_row_with_parallelism,
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

pub(crate) fn run_grid_batch_pricing(
    execution_input: VanillaOptionGridBatchPricingExecutionInput,
    mut observer: Option<&mut dyn FnMut(PricingProgressEvent)>,
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
        cpu_count: None,
    };

    let run_result =
        run_grid_batch_pricing(grid_execution_input, None).map_err(|source| match source {
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
        let materialization = build_pricing_risk_neutral_anchor_materialization(
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
        )
        .map_err(|source| format!("failed to construct spectral domain: {source}"))?;
        let cached_interval_operators = build_cached_interval_operators(
            &base_request,
            execution_input.interval_factors.as_slice(),
            &materialization,
            maximum_maturity_count,
            spectral_domain,
            driver_parameters,
            pricing_thread_pool,
            cpu_count,
            observer,
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

        price_grid.push(priced_row.price_row);
        implied_volatility_grid.push(priced_row.implied_volatility_row);
        implied_volatility_status_grid.push(priced_row.implied_volatility_status_row);
        pricing_status_grid.push(priced_row.pricing_status_row);
        pricing_failure_stage_grid.push(priced_row.pricing_failure_stage_row);
        pricing_failure_reason_grid.push(priced_row.pricing_failure_reason_row);
        forward_price_grid.push(priced_row.forward_price_row);
        discount_factor_grid.push(priced_row.discount_factor_row);
        boundary_state_norm_grid.push(priced_row.boundary_state_norm_row);
        maturity_summaries.push(Some(build_maturity_summary(
            solved_maturity,
            execution_input.optional_output_selection,
        )));
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
        for strike in grid.strikes.iter().copied() {
            priced_nodes.push(price_strike(strike));
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
            None,
            strike_count,
        ),
    );

    PricedStrikeRow {
        price_row: priced_nodes.iter().map(|node| node.0).collect(),
        implied_volatility_row: priced_nodes.iter().map(|node| node.1).collect(),
        implied_volatility_status_row: priced_nodes.iter().map(|node| node.2).collect(),
        pricing_status_row: priced_nodes.iter().map(|node| node.3).collect(),
        pricing_failure_stage_row: priced_nodes.iter().map(|node| node.4).collect(),
        pricing_failure_reason_row: priced_nodes.into_iter().map(|node| node.5).collect(),
        forward_price_row: vec![Some(forward_price); strike_count],
        discount_factor_row: vec![Some(discount_factor); strike_count],
        boundary_state_norm_row: vec![boundary_state_norm_row_value; strike_count],
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
    pricing_request: &VanillaOptionPricingRequest,
    interval_factors: &[VanillaOptionPricingIntervalFactors],
    materialization: &RiskNeutralAnchorMaterialization,
    maximum_maturity_count: usize,
    spectral_domain: SpectralDomain,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
    observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
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
                Some(0),
                collocation_nodes.len(),
            ),
        );

        let row_operators =
            if resolved_pricing_cpu_count(cpu_count) > 1 && collocation_nodes.len() > 1 {
                install_pricing_parallelism(pricing_thread_pool, || {
                    collocation_nodes
                        .par_iter()
                        .copied()
                        .map(|deviation| {
                            solve_row_frequency_batch(
                                &base_context,
                                deviation,
                                spectral_domain,
                                driver_parameters,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()
                })?
            } else {
                let mut row_operators = Vec::with_capacity(collocation_nodes.len());
                for deviation in &collocation_nodes {
                    row_operators.push(solve_row_frequency_batch(
                        &base_context,
                        *deviation,
                        spectral_domain,
                        driver_parameters,
                    )?);
                }
                row_operators
            };

        emit_pricing_progress(
            observer,
            PricingProgressEvent::transition_row_progress(
                collocation_nodes.len(),
                collocation_nodes.len(),
                Some(interval_offset),
                Some(maximum_maturity_count),
                Some(collocation_nodes.len() - 1),
                collocation_nodes.len(),
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
    for frequency_index in 0..frequency_count {
        continuation_coefficients[frequency_index][0] = ComplexScalar::new(1.0, 0.0);
    }

    for cached_interval_operator in cached_interval_operators[..maturity_count].iter().rev() {
        continuation_coefficients = apply_cached_interval_operator(
            continuation_coefficients,
            cached_interval_operator,
            spectral_domain,
        );
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
        interval_summaries: cached_interval_operators[..maturity_count]
            .iter()
            .map(|interval_operator| interval_operator.interval_summary.clone())
            .collect(),
    })
}

fn apply_cached_interval_operator(
    continuation_coefficients: Vec<Vec<ComplexScalar>>,
    cached_interval_operator: &CachedIntervalOperator,
    spectral_domain: SpectralDomain,
) -> Vec<Vec<ComplexScalar>> {
    let frequency_count = spectral_domain.node_count;
    let mut interval_values =
        vec![vec![ComplexScalar::new(0.0, 0.0); spectral_domain.node_count]; frequency_count];

    for (row_index, row_operator) in cached_interval_operator.row_operators.iter().enumerate() {
        for frequency_index in 0..frequency_count {
            let mut row_value = ComplexScalar::new(0.0, 0.0);

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
                row_value = row_value.add(complex_multiply(
                    continuation_coefficients[frequency_index][basis_index],
                    expectation,
                ));
            }

            interval_values[frequency_index][row_index] = complex_multiply(
                row_operator.row_frequency_phases[frequency_index],
                row_value,
            );
        }
    }

    values_to_cosine_coefficients(interval_values)
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

fn solve_row_frequency_batch(
    base_context: &LocalTransitionRequestContext,
    deviation: f64,
    spectral_domain: SpectralDomain,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
) -> Result<RowFrequencyBatchSolution, VanillaOptionPricingRunError> {
    let transition_row = base_context.build_row_state(deviation)?;
    let (theta, _) = resolve_local_transition_theta_for_row_with_parallelism(
        base_context,
        &transition_row,
        driver_parameters,
        None,
        None,
    )?;

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
    let shift_center = base_context.deviation_decay_factor * deviation - local_expectations;
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
        theta,
        positive_frequency_values,
        basis_forward_phases,
        basis_backward_phases,
        row_frequency_phases,
        representative_transform,
    })
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

fn evaluate_cosine_series(
    coefficients: &[ComplexScalar],
    spectral_domain: SpectralDomain,
    evaluation_point: f64,
    frequency_index: usize,
) -> ComplexScalar {
    let _ = frequency_index;
    coefficients.iter().enumerate().fold(
        ComplexScalar::new(0.0, 0.0),
        |accum, (basis_index, coefficient)| {
            let angle = spectral_domain.basis_frequency(basis_index)
                * (evaluation_point - spectral_domain.deviation_lower_bound);
            accum.add(coefficient.scale(angle.cos()))
        },
    )
}

fn build_spectral_domain(
    pricing_request: &VanillaOptionPricingRequest,
    materialization: &RiskNeutralAnchorMaterialization,
    maturity_count: usize,
    _maturity_interval: f64,
    _strikes: &[f64],
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
    let node_count = MINIMUM_SPECTRAL_COLLOCATION_NODE_COUNT;

    Ok(SpectralDomain {
        deviation_lower_bound: observed_log_center - observed_log_half_width - terminal_anchor,
        deviation_upper_bound: observed_log_center + observed_log_half_width - terminal_anchor,
        node_count,
    })
}

fn build_maturity_summary(
    solved_maturity: &SolvedMaturityLaw,
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
) -> VanillaOptionMaturitySummary {
    VanillaOptionMaturitySummary {
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
        integration_quality: None,
    }
}

fn cos_price_from_terminal_characteristic_function(
    spot: f64,
    strike: f64,
    payoff_type: VanillaOptionPayoffType,
    risk_free_rate: f64,
    dividend_yield: f64,
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

    let _ = dividend_yield;
    Some((discount_factor * price).max(0.0))
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
