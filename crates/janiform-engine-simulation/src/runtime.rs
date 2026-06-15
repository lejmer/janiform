pub(crate) mod buffers;
mod debug_trace;
mod endogenous;
mod entrypoints;
mod market_state;
mod metadata;
mod news;
mod observed_state;
mod results;
mod simulation;
mod timestamp_observation;
mod timing_diagnostics;
mod trading_time;
mod updates;
mod validation;

pub use entrypoints::{
    run_seeded_tick_event_simulation, run_seeded_tick_event_simulation_with_observer,
    run_seeded_tick_event_simulation_with_randomness_factory,
    run_seeded_tick_event_simulation_with_randomness_factory_and_observer,
    stream_seeded_tick_event_simulation, stream_seeded_tick_event_simulation_with_observer,
    stream_seeded_tick_event_simulation_with_randomness_factory,
    stream_seeded_tick_event_simulation_with_randomness_factory_and_observer,
};
pub use observed_state::project_core_increment_sequence_to_tick_grid;
pub use results::estimate_seeded_result_capacities;

pub(crate) use market_state::SeededSimulationMarketStateResolver;
#[cfg(test)]
pub(crate) use news::{
    IntervalNewsEventInputBuildInput, build_interval_news_event_inputs,
    derive_interval_news_arrival_decomposition,
};
pub(crate) use news::{
    build_timestamped_news_event_input, derive_known_news_interval_slice_bounds,
    sample_count_from_expectation, sample_interval_timestamp_utc_nanos,
    validate_known_scheduled_news_timestamps,
};
pub(crate) use observed_state::SeededObservedState;
pub(crate) use trading_time::{
    build_interval_open_market_segments, integrate_open_market_elapsed_hours,
};
pub(crate) use validation::{
    compute_max_effective_endogenous_branching_ratio, is_valid_observed_price_tick_size,
    validate_interval_factor_grid_field_length, validate_interval_factor_grid_interval_coverage,
};
