pub(crate) mod buffers;
mod market_state;
mod news;
mod observed_state;
mod trading_time;
pub(crate) mod updates;
mod validation;

pub(crate) use buffers::{push_simulation_buffer_value, try_reserve_simulation_buffer};
pub(crate) use market_state::SeededSimulationMarketStateResolver;
pub(crate) use news::{
    ExogenousIntervalNewsSchedulingInput, accumulate_news_routing_totals,
    build_timestamped_news_event_input, derive_interval_news_arrival_decomposition,
    derive_known_news_interval_slice_bounds, sample_count_from_expectation,
    sample_interval_timestamp_utc_nanos, schedule_exogenous_interval_news_updates,
    validate_known_scheduled_news_timestamps,
};
#[cfg(test)]
pub(crate) use news::{IntervalNewsEventInputBuildInput, build_interval_news_event_inputs};
pub use observed_state::project_core_increment_sequence_to_tick_grid;
pub(crate) use observed_state::{SeededObservedState, SeededProjectedCoreIncrement};
pub(crate) use trading_time::{
    IntervalOpenMarketSegments, TransitionReturnClockInput, build_interval_open_market_segments,
    compute_transition_return_clocks, integrate_open_market_elapsed_hours,
    sample_trading_time_return_from_return_clocks,
};
pub(crate) use updates::{
    IntervalStochasticUpdate, SeededTickSimulationNewsRoutingUpdate,
    SeededTickSimulationTraceNewsTransitionPayload, is_public_seeded_fine_path_record,
    map_seeded_tick_side_to_hawkes_side, map_side_to_transition_kind,
    merge_ordered_interval_updates,
};
pub(crate) use validation::{
    compute_max_effective_endogenous_branching_ratio, is_valid_observed_price_tick_size,
    map_transition_error_to_seeded_simulation_run_error, validate_integrated_intensity,
    validate_interval_factor_grid_field_length, validate_interval_factor_grid_interval_coverage,
};
