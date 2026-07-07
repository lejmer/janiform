use janiform_foundations::time::NANOS_PER_WEEK;

use crate::market_data::preparation::CalibrationTickObjectiveDataUsagePolicy;
use crate::stages::return_components::{
    ReturnComponentsObservedOpenSessionReturn, ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageSignalSourceWindow,
};

pub(crate) const DEFAULT_SHORT_HORIZON_TICK_OBJECTIVE_WINDOW_NANOS: i64 = NANOS_PER_WEEK;

pub(crate) fn retained_open_session_returns_for_tick_objectives(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    tick_objective_data_usage_policy: CalibrationTickObjectiveDataUsagePolicy,
) -> Vec<ReturnComponentsObservedOpenSessionReturn> {
    let observed_returns = candidate_generation_input
        .observed_open_session_returns
        .as_slice();
    if tick_objective_data_usage_policy == CalibrationTickObjectiveDataUsagePolicy::AllAvailable {
        return observed_returns.to_vec();
    }

    let Some(last_return) = observed_returns.last() else {
        return Vec::new();
    };

    let Some(window_start) = last_return
        .return_timestamp_utc_nanos
        .checked_sub(DEFAULT_SHORT_HORIZON_TICK_OBJECTIVE_WINDOW_NANOS)
    else {
        return observed_returns.to_vec();
    };

    let retained_returns = observed_returns
        .iter()
        .copied()
        .filter(|observation| observation.return_timestamp_utc_nanos >= window_start)
        .collect::<Vec<_>>();
    if retained_returns.is_empty() {
        return observed_returns.to_vec();
    }

    retained_returns
}

pub(crate) fn source_window_for_retained_tick_objectives(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    observed_returns: &[ReturnComponentsObservedOpenSessionReturn],
) -> ReturnComponentsStageSignalSourceWindow {
    let original_window = candidate_generation_input.known_news_calendar.source_window;

    let Some(first_return) = observed_returns.first() else {
        return original_window;
    };

    let Some(last_return) = observed_returns.last() else {
        return original_window;
    };

    ReturnComponentsStageSignalSourceWindow {
        window_start_utc_nanos: first_return
            .return_timestamp_utc_nanos
            .max(original_window.window_start_utc_nanos),
        window_end_utc_nanos: last_return
            .return_timestamp_utc_nanos
            .saturating_add(1)
            .min(original_window.window_end_utc_nanos),
    }
}
