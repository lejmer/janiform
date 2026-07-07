use crate::affine_oracle::ExponentialAffineTransformEquationSpecification;
use janiform_foundations::numeric::u64_to_f64_exact;
use janiform_foundations::randomness::{
    SimulationRandomnessFactory, SimulationRandomnessStream, SimulationRandomnessSubstreamDomain,
};
use janiform_model_kernels::hawkes::{
    HawkesEventSide, HawkesIntervalTerminalStateContribution,
    hawkes_endogenous_event_terminal_state_contribution_for_remaining_time,
    resolve_hawkes_interval_terminal_state,
};
use janiform_model_kernels::market_pressure::{
    MarketPressureIntervalTransitionInput, sample_market_pressure_interval_transition,
};

use super::contracts::{
    SufficientStatisticsSampledEventCounts, SufficientStatisticsSampledIntervalTransition,
    SufficientStatisticsTransformSliceContext,
};
use super::errors::{
    SufficientStatisticsTransformSliceSamplingError, SufficientStatisticsTransformSlicesError,
};

/// Sample hidden interval placements for sampled sufficient-statistics event counts.
///
/// This is the canonical transition policy for count-based sufficient-statistics paths. Counts are
/// sampled from interval expectations, while hidden placements only determine interval-end state
/// variables and the integrated market-pressure anchor increment.
pub fn sample_interval_endogenous_transition_from_sampled_counts_with_randomness_factory<F>(
    seed: u64,
    interval_index: usize,
    equation_specification: &ExponentialAffineTransformEquationSpecification,
    context: SufficientStatisticsTransformSliceContext,
    sampled_counts: SufficientStatisticsSampledEventCounts,
    randomness_factory: &F,
) -> Result<
    SufficientStatisticsSampledIntervalTransition,
    SufficientStatisticsTransformSliceSamplingError,
>
where
    F: SimulationRandomnessFactory,
{
    let substream_index = derive_transform_slice_sampling_substream_index(interval_index as u64, 0);

    sample_interval_endogenous_transition_from_sampled_counts(
        seed,
        substream_index,
        equation_specification,
        &context,
        sampled_counts,
        randomness_factory,
    )
}

pub(crate) fn sample_interval_endogenous_transition_from_sampled_counts<F>(
    seed: u64,
    substream_index: u64,
    equation_specification: &ExponentialAffineTransformEquationSpecification,
    context: &SufficientStatisticsTransformSliceContext,
    sampled_counts: SufficientStatisticsSampledEventCounts,
    randomness_factory: &F,
) -> Result<
    SufficientStatisticsSampledIntervalTransition,
    SufficientStatisticsTransformSliceSamplingError,
>
where
    F: SimulationRandomnessFactory,
{
    context.validate().map_err(|source| {
        SufficientStatisticsTransformSliceSamplingError::InvalidSlices {
            source: SufficientStatisticsTransformSlicesError::InvalidContext { source },
        }
    })?;

    let hawkes_terminal_state_contribution = sample_hidden_interval_hawkes_contribution(
        seed,
        substream_index,
        equation_specification,
        context,
        sampled_counts,
        randomness_factory,
    )?;

    let terminal_excitation_state = resolve_hawkes_interval_terminal_state(
        context.initial_excitation_state,
        equation_specification.hawkes_kernel_parameters,
        context.delta,
        hawkes_terminal_state_contribution,
    )
    .map_err(|source| {
        SufficientStatisticsTransformSliceSamplingError::HawkesTransitionFailure {
            interval_index: context.interval_index,
            source,
        }
    })?;

    let mut market_pressure_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::MarketPressureDiffusion,
        substream_index,
    );
    let signed_balance_exposure_scale =
        sampled_event_count_exposure_scale(context.interval_index, sampled_counts)?;

    let market_pressure_transition = sample_market_pressure_interval_transition(
        MarketPressureIntervalTransitionInput {
            parameters: &equation_specification.market_pressure_parameters,
            initial_state: &context.initial_market_pressure_state,
            interval_delta: context.delta,
            signed_balance_exposure_scale,
        },
        &mut market_pressure_stream,
    )
    .map_err(|source| {
        SufficientStatisticsTransformSliceSamplingError::MarketPressureTransitionFailure {
            interval_index: context.interval_index,
            source,
        }
    })?;

    Ok(SufficientStatisticsSampledIntervalTransition {
        terminal_excitation_state,
        terminal_market_pressure_state: market_pressure_transition.terminal_state,
        market_pressure_anchor_increment: market_pressure_transition.anchor_increment,
    })
}

fn sampled_event_count_exposure_scale(
    interval_index: usize,
    sampled_counts: SufficientStatisticsSampledEventCounts,
) -> Result<f64, SufficientStatisticsTransformSliceSamplingError> {
    let sampled_event_count = sampled_counts
        .n_plus
        .checked_add(sampled_counts.n_minus)
        .ok_or(
            SufficientStatisticsTransformSliceSamplingError::SampledEventCountOverflow {
                interval_index,
            },
        )?;

    u64_to_f64_exact(sampled_event_count, "sampled_event_count_exposure").map_err(|source| {
        SufficientStatisticsTransformSliceSamplingError::InvalidSampledEventCountExposure {
            interval_index,
            source,
        }
    })
}

pub(crate) fn derive_transform_slice_sampling_substream_index(
    interval_index: u64,
    batch_position: u64,
) -> u64 {
    let interval_component = interval_index.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    interval_component ^ batch_position
}

fn sample_hidden_interval_hawkes_contribution<F>(
    seed: u64,
    substream_index: u64,
    equation_specification: &ExponentialAffineTransformEquationSpecification,
    context: &SufficientStatisticsTransformSliceContext,
    sampled_counts: SufficientStatisticsSampledEventCounts,
    randomness_factory: &F,
) -> Result<HawkesIntervalTerminalStateContribution, SufficientStatisticsTransformSliceSamplingError>
where
    F: SimulationRandomnessFactory,
{
    let mut buy_timestamp_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousBuyEventTimestamp,
        substream_index,
    );

    let mut sell_timestamp_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousSellEventTimestamp,
        substream_index,
    );

    let mut terminal_state_contribution = HawkesIntervalTerminalStateContribution::empty();

    accumulate_hidden_side_event_contributions(
        &mut terminal_state_contribution,
        equation_specification,
        context,
        HawkesEventSide::Buy,
        sampled_counts.n_plus,
        &mut buy_timestamp_stream,
    )?;

    accumulate_hidden_side_event_contributions(
        &mut terminal_state_contribution,
        equation_specification,
        context,
        HawkesEventSide::Sell,
        sampled_counts.n_minus,
        &mut sell_timestamp_stream,
    )?;

    Ok(terminal_state_contribution)
}

fn accumulate_hidden_side_event_contributions<R>(
    terminal_state_contribution: &mut HawkesIntervalTerminalStateContribution,
    equation_specification: &ExponentialAffineTransformEquationSpecification,
    context: &SufficientStatisticsTransformSliceContext,
    hawkes_event_side: HawkesEventSide,
    event_count: u64,
    timestamp_stream: &mut R,
) -> Result<(), SufficientStatisticsTransformSliceSamplingError>
where
    R: SimulationRandomnessStream,
{
    for _event_position in 0..event_count {
        let remaining_interval_delta =
            sample_hidden_event_remaining_interval_delta(context.delta, timestamp_stream);

        let hawkes_contribution =
            hawkes_endogenous_event_terminal_state_contribution_for_remaining_time(
                equation_specification.hawkes_kernel_parameters,
                hawkes_event_side,
                remaining_interval_delta,
            )
            .map_err(|source| {
                SufficientStatisticsTransformSliceSamplingError::HawkesTransitionFailure {
                    interval_index: context.interval_index,
                    source,
                }
            })?;

        *terminal_state_contribution = terminal_state_contribution
            .plus(hawkes_contribution)
            .map_err(|source| {
                SufficientStatisticsTransformSliceSamplingError::HawkesTransitionFailure {
                    interval_index: context.interval_index,
                    source,
                }
            })?;
    }

    Ok(())
}

fn sample_hidden_event_remaining_interval_delta<R>(
    interval_delta: f64,
    timestamp_stream: &mut R,
) -> f64
where
    R: SimulationRandomnessStream,
{
    let elapsed_fraction = timestamp_stream.next_unit_interval_f64();

    interval_delta * (1.0 - elapsed_fraction)
}

#[cfg(test)]
mod tests {
    use janiform_foundations::randomness::SplitMix64SimulationRandomnessFactory;
    use janiform_model_kernels::hawkes::{
        HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters, HawkesTransitionStep,
        evolve_hawkes_state,
    };
    use janiform_model_kernels::long_variance::LongVarianceParameters;
    use janiform_model_kernels::market_pressure::{MarketPressureParameters, MarketPressureState};
    use janiform_model_kernels::news::GammaSeverityMarkConfiguration;

    use super::sample_interval_endogenous_transition_from_sampled_counts_with_randomness_factory;
    use crate::affine_oracle::{
        ExponentialAffineTransformEquationSpecification,
        ExponentialAffineTransformNewsIntensityInput,
    };
    use crate::sufficient_statistics::{
        SufficientStatisticsSampledEventCounts, SufficientStatisticsTransformSliceContext,
    };

    #[test]
    fn hidden_transition_sampler_preserves_no_event_boundary_evolution() {
        let kernel_parameters = HawkesKernelParameters::try_new(
            1.0,
            2.0,
            HawkesKernelLoadings {
                plus_plus: 0.1,
                plus_minus: 0.04,
                minus_plus: 0.03,
                minus_minus: 0.08,
            },
            HawkesKernelLoadings {
                plus_plus: 0.02,
                plus_minus: 0.01,
                minus_plus: 0.015,
                minus_minus: 0.025,
            },
        )
        .expect("kernel parameters should construct");

        let market_pressure_parameters = MarketPressureParameters::no_anchor_impact();

        let equation_specification = ExponentialAffineTransformEquationSpecification::try_new(
            kernel_parameters,
            GammaSeverityMarkConfiguration::try_new(1.0)
                .expect("gamma severity marks should construct"),
            LongVarianceParameters::neutral(),
            market_pressure_parameters,
        )
        .expect("equation specification should construct");

        let initial_excitation_state = HawkesExcitationState {
            y_plus_plus: 0.3,
            y_plus_minus: 0.2,
            y_minus_plus: 0.4,
            y_minus_minus: 0.1,
        };

        let initial_market_pressure_state = MarketPressureState::neutral();

        let interval_delta = 0.5;

        let context = SufficientStatisticsTransformSliceContext::try_new(
            4,
            1.0,
            ExponentialAffineTransformNewsIntensityInput::try_new(0.0, 0.0)
                .expect("news intensity should construct"),
            interval_delta,
            initial_excitation_state,
            initial_market_pressure_state,
        )
        .expect("slice context should construct");

        let sampled_transition =
            sample_interval_endogenous_transition_from_sampled_counts_with_randomness_factory(
                17,
                4,
                &equation_specification,
                context.clone(),
                SufficientStatisticsSampledEventCounts::new(0, 0),
                &SplitMix64SimulationRandomnessFactory,
            )
            .expect("hidden transition should sample");
        let repeated_transition =
            sample_interval_endogenous_transition_from_sampled_counts_with_randomness_factory(
                17,
                4,
                &equation_specification,
                context,
                SufficientStatisticsSampledEventCounts::new(0, 0),
                &SplitMix64SimulationRandomnessFactory,
            )
            .expect("hidden transition should sample deterministically");

        let expected_excitation_state = evolve_hawkes_state(
            initial_excitation_state,
            kernel_parameters,
            HawkesTransitionStep::new(interval_delta, None, None),
        )
        .expect("Hawkes no-event drift should resolve");

        assert_eq!(
            sampled_transition.terminal_excitation_state,
            expected_excitation_state
        );
        assert_eq!(sampled_transition, repeated_transition);
        sampled_transition
            .terminal_market_pressure_state
            .validate()
            .expect("sampled market-pressure state should validate");
        assert!(
            sampled_transition
                .market_pressure_anchor_increment
                .is_finite()
        );
    }
}
