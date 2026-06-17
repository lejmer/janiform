use janiform_foundations::complex_math::ComplexScalar;
use janiform_model_kernels::no_move_event::NoMoveEventModel;

use super::contracts::ExponentialAffineTransformActivityLoadings;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MovingActivityLoadings {
    pub(crate) buy_baseline: ComplexScalar,
    pub(crate) buy_endogenous: ComplexScalar,
    pub(crate) sell_baseline: ComplexScalar,
    pub(crate) sell_endogenous: ComplexScalar,
}

impl MovingActivityLoadings {
    pub(crate) fn from_total_activity_loadings(
        no_move_event_model: NoMoveEventModel,
        activity_loadings: ExponentialAffineTransformActivityLoadings,
    ) -> Self {
        Self {
            buy_baseline: activity_loadings
                .u_plus_baseline
                .scale(no_move_event_model.buy_baseline_move_probability()),
            buy_endogenous: activity_loadings
                .u_plus_endogenous
                .scale(no_move_event_model.buy_endogenous_move_probability()),
            sell_baseline: activity_loadings
                .u_minus_baseline
                .scale(no_move_event_model.sell_baseline_move_probability()),
            sell_endogenous: activity_loadings
                .u_minus_endogenous
                .scale(no_move_event_model.sell_endogenous_move_probability()),
        }
    }
}
