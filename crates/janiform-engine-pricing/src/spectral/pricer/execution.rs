use std::collections::{BTreeMap, BTreeSet};

use crate::contracts::{
    SpectralPricingDiagnostics, VanillaOptionPayoffConfiguration, VanillaOptionPayoffType,
    VanillaOptionPricingMarketInputs, VanillaOptionPricingRequest,
};
use crate::grids::{
    SpectralThetaWarmStartKey, SpectralThetaWarmStartState,
    VanillaOptionGridBatchPricingExecutionInput, VanillaOptionGridBatchPricingRunError,
    VanillaOptionGridBatchPricingRunResult, VanillaOptionGridPricingResult,
    VanillaOptionGridRequest,
};
use crate::implied_volatility::VanillaOptionImpliedVolatilityInversionStatus;
use crate::progress::PricingProgressEvent;
use crate::runner::{VanillaOptionPricingExecutionInput, VanillaOptionPricingRunError};
use crate::thread_pool::build_pricing_thread_pool;

use super::domain::build_maturity_summary;
use super::maturity_law::SolvedMaturityLaw;
use super::strike::{StrikeRowPricingInput, price_strike_row};
use super::terminal_affine::{
    TerminalAffineMaturitySolveInput, TerminalAffineMeasurePolicy,
    TerminalAffineSpectralDomainInput, solve_terminal_affine_maturity_law,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::spectral::pricer) struct PricingGroupKey {
    pub(in crate::spectral::pricer) risk_free_rate_bits: u64,
    pub(in crate::spectral::pricer) dividend_yield_bits: u64,
}

impl PricingGroupKey {
    fn from_grid(grid: &VanillaOptionGridRequest) -> Self {
        Self {
            risk_free_rate_bits: grid.risk_free_rate.to_bits(),
            dividend_yield_bits: grid.dividend_yield.to_bits(),
        }
    }

    pub(in crate::spectral::pricer) const fn theta_warm_start_key(
        self,
        maturity_count: usize,
    ) -> SpectralThetaWarmStartKey {
        SpectralThetaWarmStartKey::new(
            self.risk_free_rate_bits,
            self.dividend_yield_bits,
            maturity_count,
        )
    }
}

#[derive(Debug, Clone, Default)]
struct PricingGroupMembers {
    strike_bits: BTreeSet<u64>,
    payoff_types: BTreeSet<VanillaOptionPayoffType>,
}

#[derive(Debug, Clone)]
struct PricingGroupSolveContext {
    maturity_laws: BTreeMap<usize, SolvedMaturityLaw>,
}

struct TerminalAffineGroupSolveInput<'a> {
    execution_input: &'a VanillaOptionGridBatchPricingExecutionInput,
    base_request: &'a VanillaOptionPricingRequest,
    unique_maturity_counts: BTreeSet<usize>,
    strikes: &'a [f64],
    payoff_types: &'a [VanillaOptionPayoffType],
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    pricing_thread_pool: Option<&'a rayon::ThreadPool>,
    pricing_group_key: PricingGroupKey,
}

#[derive(Debug, Clone)]
pub(crate) struct SingleOptionSpectralOutcome {
    pub(crate) price: f64,
    pub(crate) implied_volatility: Option<f64>,
    pub(crate) implied_volatility_inversion_status: VanillaOptionImpliedVolatilityInversionStatus,
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

    let mut pricing_groups = prepare_pricing_group_contexts(
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
        let pricing_group_key = PricingGroupKey::from_grid(grid);

        let pricing_context = pricing_groups.get_mut(&pricing_group_key).ok_or_else(|| {
            VanillaOptionGridBatchPricingRunError::NodePricingFailure {
                message: "missing spectral pricing group context".to_string(),
            }
        })?;

        grid_results.push(build_grid_result(
            &execution_input,
            grid,
            pricing_context,
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
        interval_factor_grids: execution_input.interval_factor_grids.clone(),
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
        maturity_summary.terminal_affine_measure_change,
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
        spectral_diagnostics,
    })
}

fn prepare_pricing_group_contexts(
    execution_input: &VanillaOptionGridBatchPricingExecutionInput,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    _cpu_count: Option<usize>,
    observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
    mut theta_warm_start_state: Option<&mut SpectralThetaWarmStartState>,
) -> Result<BTreeMap<PricingGroupKey, PricingGroupSolveContext>, String> {
    let mut grouped_members = BTreeMap::<PricingGroupKey, PricingGroupMembers>::new();

    for grid in &execution_input.request.grids {
        let pricing_group_key = PricingGroupKey::from_grid(grid);
        let members = grouped_members.entry(pricing_group_key).or_default();

        members.payoff_types.insert(grid.payoff_type);

        for strike in &grid.strikes {
            members.strike_bits.insert(strike.to_bits());
        }
    }

    let mut pricing_groups = BTreeMap::new();

    for (pricing_group_key, members) in grouped_members {
        let union_strikes = members
            .strike_bits
            .into_iter()
            .map(f64::from_bits)
            .collect::<Vec<_>>();

        let payoff_types = members.payoff_types.into_iter().collect::<Vec<_>>();

        let first_grid = execution_input
            .request
            .grids
            .iter()
            .find(|grid| PricingGroupKey::from_grid(grid) == pricing_group_key)
            .ok_or_else(|| "missing representative grid for pricing group".to_string())?;

        let max_maturity_interval = execution_input
            .request
            .grids
            .iter()
            .filter(|grid| PricingGroupKey::from_grid(grid) == pricing_group_key)
            .flat_map(|grid| grid.maturity_intervals.iter().copied())
            .max_by(|left, right| left.total_cmp(right))
            .ok_or_else(|| "pricing group requires at least one maturity".to_string())?;

        let dummy_strike = union_strikes
            .first()
            .copied()
            .ok_or_else(|| "pricing group requires at least one strike".to_string())?;

        let base_request = VanillaOptionPricingRequest::try_new(
            VanillaOptionPricingMarketInputs::new_single_node(
                execution_input.request.spot,
                dummy_strike,
                max_maturity_interval,
                first_grid.risk_free_rate,
                first_grid.dividend_yield,
            ),
            VanillaOptionPayoffConfiguration {
                payoff_type: first_grid.payoff_type,
            },
            execution_input.boundary_input.clone(),
            execution_input.model_parameters.clone(),
            execution_input.interval_delta,
        )
        .map_err(|source| {
            format!("failed to construct spectral pricing-group request: {source}")
        })?;

        let driver_parameters = execution_input
            .model_parameters
            .nig_shared_plus_shift_configuration
            .derive_driver_parameters()
            .map_err(|source| format!("failed to derive pricing driver parameters: {source}"))?;

        let unique_maturity_counts =
            resolve_unique_maturity_counts(execution_input, pricing_group_key)?;

        let maturity_laws = solve_terminal_affine_maturity_laws(
            TerminalAffineGroupSolveInput {
                execution_input,
                base_request: &base_request,
                unique_maturity_counts,
                strikes: union_strikes.as_slice(),
                payoff_types: payoff_types.as_slice(),
                driver_parameters,
                pricing_thread_pool,
                pricing_group_key,
            },
            observer,
            theta_warm_start_state.as_deref_mut(),
        )?;

        pricing_groups.insert(
            pricing_group_key,
            PricingGroupSolveContext { maturity_laws },
        );
    }

    Ok(pricing_groups)
}

fn resolve_unique_maturity_counts(
    execution_input: &VanillaOptionGridBatchPricingExecutionInput,
    pricing_group_key: PricingGroupKey,
) -> Result<BTreeSet<usize>, String> {
    execution_input
        .request
        .grids
        .iter()
        .filter(|grid| PricingGroupKey::from_grid(grid) == pricing_group_key)
        .flat_map(|grid| {
            grid.maturity_intervals.iter().map(|maturity_interval| {
                execution_input
                    .runtime_interval_schedule
                    .terminal_interval_index_for_maturity_interval(*maturity_interval)
                    .map(|terminal_index| terminal_index + 1)
                    .map_err(|source| source.to_string())
            })
        })
        .collect::<Result<BTreeSet<_>, _>>()
}

fn solve_terminal_affine_maturity_laws(
    solve_input: TerminalAffineGroupSolveInput<'_>,
    observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
    mut theta_warm_start_state: Option<&mut SpectralThetaWarmStartState>,
) -> Result<BTreeMap<usize, SolvedMaturityLaw>, String> {
    let total_maturity_solves = solve_input.unique_maturity_counts.len();
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

    let mut maturity_laws = BTreeMap::new();

    for (maturity_position, maturity_count) in
        solve_input.unique_maturity_counts.into_iter().enumerate()
    {
        let maturity_interval = solve_input
            .execution_input
            .runtime_interval_schedule
            .maturity_interval_hours(maturity_count - 1)
            .map_err(|source| source.to_string())?;

        let solved_maturity_law = solve_terminal_affine_maturity_law(
            TerminalAffineMaturitySolveInput {
                pricing_request: solve_input.base_request,
                maturity_count,
                maturity_interval,
                spectral_domain_input: TerminalAffineSpectralDomainInput::PayoffAnchored {
                    strikes: solve_input.strikes,
                    payoff_types: solve_input.payoff_types,
                    collocation_node_count_override: solve_input
                        .execution_input
                        .spectral_collocation_node_count_override,
                },
                interval_deltas: solve_input
                    .execution_input
                    .runtime_interval_schedule
                    .interval_deltas_hours
                    .as_slice(),
                interval_factor_grids: solve_input.execution_input.interval_factor_grids.as_slice(),
                driver_parameters: solve_input.driver_parameters,
                measure_policy: TerminalAffineMeasurePolicy::RiskNeutralMartingale,
                include_node_price_sensitivities: solve_input
                    .execution_input
                    .optional_output_selection
                    .include_node_price_sensitivities,
                pricing_thread_pool: solve_input.pricing_thread_pool,
                theta_warm_start_key: solve_input
                    .pricing_group_key
                    .theta_warm_start_key(maturity_count),
            },
            theta_warm_start_state.as_deref_mut(),
            observer,
        )
        .map_err(|source| format!("terminal affine maturity solve failed: {source}"))?;

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

    Ok(maturity_laws)
}

fn build_grid_result(
    execution_input: &VanillaOptionGridBatchPricingExecutionInput,
    grid: &VanillaOptionGridRequest,
    pricing_context: &PricingGroupSolveContext,
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

        let solved_maturity = pricing_context
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
    use super::super::asset_reference_log_price_from_observed_spot;
    use super::super::domain::SpectralDomain;
    use super::super::payoff::{
        cos_price_from_terminal_characteristic_function, payoff_cos_coefficient,
        strike_log_boundary_from_reference_log,
    };
    use super::{PricingGroupKey, SolvedMaturityLaw};
    use crate::contracts::VanillaOptionPayoffType;
    use janiform_foundations::complex_math::{ComplexScalar, complex_exponential};
    use janiform_foundations::numeric::square_root_machine_epsilon;

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

        let reference_price_tolerance = square_root_machine_epsilon() * base_reference_price;
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

            let tolerance = square_root_machine_epsilon()
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
            expected_log_return: 0.0,
            characteristic_function_values,
            characteristic_function_sensitivities: None,
            terminal_affine_measure_change: None,
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
    fn spectral_pricing_group_key_reuses_operator_cache_across_payoff_types() {
        let call_grid = crate::grids::VanillaOptionGridRequest {
            payoff_type: VanillaOptionPayoffType::Call,
            risk_free_rate: 0.02,
            dividend_yield: 0.01,
            strikes: vec![99.0, 101.0],
            maturity_intervals: vec![1.0],
        };

        let put_grid = crate::grids::VanillaOptionGridRequest {
            payoff_type: VanillaOptionPayoffType::Put,
            ..call_grid.clone()
        };

        let call_key = PricingGroupKey::from_grid(&call_grid);
        let put_key = PricingGroupKey::from_grid(&put_grid);

        assert_eq!(call_key, put_key);

        assert_eq!(
            call_key.theta_warm_start_key(7),
            put_key.theta_warm_start_key(7),
        );
    }
}
