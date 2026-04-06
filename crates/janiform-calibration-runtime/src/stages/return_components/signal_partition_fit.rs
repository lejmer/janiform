use janiform_foundations::numeric::{median_abs, median_abs_non_zero};

use super::constants::RETURN_COMPONENTS_STAGE_CONTINUOUS_RETURN_FILTER_MULTIPLIER;
use super::contracts::ReturnComponentsObservedOpenSessionReturn;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FittedSignalPartitionLayer {
    pub(crate) continuous_return_filter_multiplier: f64,
    pub(crate) continuous_return_filter_threshold: f64,
}

pub(crate) fn fit_signal_partition_layer(
    observed_open_session_returns: &[ReturnComponentsObservedOpenSessionReturn],
) -> FittedSignalPartitionLayer {
    let observed_open_return_values: Vec<f64> = observed_open_session_returns
        .iter()
        .map(|observation| observation.log_return)
        .collect();
    let continuous_return_filter_threshold =
        (median_abs_non_zero(observed_open_return_values.as_slice())
            .unwrap_or_else(|| median_abs(observed_open_return_values.as_slice()))
            * RETURN_COMPONENTS_STAGE_CONTINUOUS_RETURN_FILTER_MULTIPLIER)
            .max(1.0e-8);

    FittedSignalPartitionLayer {
        continuous_return_filter_multiplier:
            RETURN_COMPONENTS_STAGE_CONTINUOUS_RETURN_FILTER_MULTIPLIER,
        continuous_return_filter_threshold,
    }
}
