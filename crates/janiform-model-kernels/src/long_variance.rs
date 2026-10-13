//! Persistent variance parameters, state transitions and integrated moments.

mod dynamics;
mod errors;
mod moments;

pub use dynamics::{
    LongVarianceParameters, LongVarianceState, return_energy_increment_from_log_return,
    stationary_return_energy_rate_per_hour_from_return_generator,
};
pub use errors::{
    LongVarianceParametersError, LongVarianceStateError, LongVarianceStateTransitionError,
    LongVarianceStationaryLevelError, LongVarianceStationaryLevelForHawkesKernelError,
};
pub use moments::integrated_cir_covariance_factor;
