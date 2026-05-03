//! Top-level re-export for the sufficient-statistics runtime engine.

pub use janiform_engine_sufficient_statistics::{
    SufficientStatisticsIntervalFactors, SufficientStatisticsIntervalFactorsError,
    SufficientStatisticsModelInputs, SufficientStatisticsModelInputsError,
    SufficientStatisticsRunRequest, SufficientStatisticsRunRequestError,
    solve_exponential_affine_transform_for_sufficient_statistics_sampling,
    solve_exponential_affine_transform_for_sufficient_statistics_sampling_with_settings,
    solve_sufficient_statistics_transform_slices, stream_interval_statistics_from_transform_slices,
};
