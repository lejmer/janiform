use janiform_foundations::cancellation::CancellationToken;
use janiform_foundations::randomness::{
    SimulationRandomnessFactory, SplitMix64SimulationRandomnessFactory,
};

use crate::artifacts::{
    SeededTickSimulationRunResult, SeededTickSimulationStreamCompletion,
    SeededTickSimulationTransitionConsumer,
};
use crate::config::SeededTickSimulationRunConfig;
use crate::error::SeededTickSimulationRunError;
use crate::progress::SimulationProgressEvent;

use super::results::SeededTickSimulationResultBuilder;
use super::simulation::stream_seeded_tick_event_simulation_with_randomness_factory_internal;

/// Run deterministic seeded tick-event simulation using the default splitmix64 randomness policy.
pub fn run_seeded_tick_event_simulation(
    run_config: SeededTickSimulationRunConfig,
    cancellation_token: CancellationToken,
) -> Result<SeededTickSimulationRunResult, SeededTickSimulationRunError> {
    let randomness_factory = SplitMix64SimulationRandomnessFactory;

    run_seeded_tick_event_simulation_with_randomness_factory(
        run_config,
        cancellation_token,
        &randomness_factory,
    )
}

pub fn run_seeded_tick_event_simulation_with_observer(
    run_config: SeededTickSimulationRunConfig,
    cancellation_token: CancellationToken,
    observer: &mut dyn FnMut(SimulationProgressEvent),
) -> Result<SeededTickSimulationRunResult, SeededTickSimulationRunError> {
    let randomness_factory = SplitMix64SimulationRandomnessFactory;

    run_seeded_tick_event_simulation_with_randomness_factory_and_observer(
        run_config,
        cancellation_token,
        &randomness_factory,
        observer,
    )
}

/// Run deterministic seeded tick-event simulation with an injected randomness factory.
///
/// This hook keeps randomness centralized in one abstraction while enabling fixed fixture
/// streams for deterministic testing and reference-sequence generation.
pub fn run_seeded_tick_event_simulation_with_randomness_factory<F>(
    run_config: SeededTickSimulationRunConfig,
    cancellation_token: CancellationToken,
    randomness_factory: &F,
) -> Result<SeededTickSimulationRunResult, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    run_seeded_tick_event_simulation_with_randomness_factory_internal(
        run_config,
        cancellation_token,
        randomness_factory,
        None,
    )
}

pub fn run_seeded_tick_event_simulation_with_randomness_factory_and_observer<F>(
    run_config: SeededTickSimulationRunConfig,
    cancellation_token: CancellationToken,
    randomness_factory: &F,
    observer: &mut dyn FnMut(SimulationProgressEvent),
) -> Result<SeededTickSimulationRunResult, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    run_seeded_tick_event_simulation_with_randomness_factory_internal(
        run_config,
        cancellation_token,
        randomness_factory,
        Some(observer),
    )
}

/// Run deterministic seeded tick-event simulation and stream each transition to a consumer.
pub fn stream_seeded_tick_event_simulation<C>(
    run_config: SeededTickSimulationRunConfig,
    cancellation_token: CancellationToken,
    consumer: &mut C,
) -> Result<SeededTickSimulationStreamCompletion, SeededTickSimulationRunError>
where
    C: SeededTickSimulationTransitionConsumer,
{
    let randomness_factory = SplitMix64SimulationRandomnessFactory;

    stream_seeded_tick_event_simulation_with_randomness_factory(
        run_config,
        cancellation_token,
        &randomness_factory,
        consumer,
    )
}

pub fn stream_seeded_tick_event_simulation_with_observer<C>(
    run_config: SeededTickSimulationRunConfig,
    cancellation_token: CancellationToken,
    consumer: &mut C,
    observer: &mut dyn FnMut(SimulationProgressEvent),
) -> Result<SeededTickSimulationStreamCompletion, SeededTickSimulationRunError>
where
    C: SeededTickSimulationTransitionConsumer,
{
    let randomness_factory = SplitMix64SimulationRandomnessFactory;

    stream_seeded_tick_event_simulation_with_randomness_factory_and_observer(
        run_config,
        cancellation_token,
        &randomness_factory,
        consumer,
        observer,
    )
}

/// Run deterministic seeded tick-event simulation with an injected randomness factory and a
/// streamed transition consumer.
pub fn stream_seeded_tick_event_simulation_with_randomness_factory<F, C>(
    run_config: SeededTickSimulationRunConfig,
    cancellation_token: CancellationToken,
    randomness_factory: &F,
    consumer: &mut C,
) -> Result<SeededTickSimulationStreamCompletion, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
    C: SeededTickSimulationTransitionConsumer,
{
    stream_seeded_tick_event_simulation_with_randomness_factory_internal(
        run_config,
        cancellation_token,
        randomness_factory,
        consumer,
        None,
    )
}

pub fn stream_seeded_tick_event_simulation_with_randomness_factory_and_observer<F, C>(
    run_config: SeededTickSimulationRunConfig,
    cancellation_token: CancellationToken,
    randomness_factory: &F,
    consumer: &mut C,
    observer: &mut dyn FnMut(SimulationProgressEvent),
) -> Result<SeededTickSimulationStreamCompletion, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
    C: SeededTickSimulationTransitionConsumer,
{
    stream_seeded_tick_event_simulation_with_randomness_factory_internal(
        run_config,
        cancellation_token,
        randomness_factory,
        consumer,
        Some(observer),
    )
}

fn run_seeded_tick_event_simulation_with_randomness_factory_internal<F>(
    run_config: SeededTickSimulationRunConfig,
    cancellation_token: CancellationToken,
    randomness_factory: &F,
    observer: Option<&mut dyn FnMut(SimulationProgressEvent)>,
) -> Result<SeededTickSimulationRunResult, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    let mut result_builder = SeededTickSimulationResultBuilder::default();

    let stream_completion = stream_seeded_tick_event_simulation_with_randomness_factory_internal(
        run_config,
        cancellation_token,
        randomness_factory,
        &mut result_builder,
        observer,
    )?;

    Ok(result_builder.finish(stream_completion))
}
