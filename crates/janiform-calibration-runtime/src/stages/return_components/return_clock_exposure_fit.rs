use janiform_model_kernels::no_move_event::NoMoveEventModel;
use janiform_model_kernels::return_clock::{
    IntegratedSideActivityDecomposition, ReturnClockExposureParameters,
};

use super::contracts::ReturnComponentsObservedOpenSessionReturn;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReturnClockSide {
    Buy,
    Sell,
}

pub(crate) fn fit_return_clock_exposure_parameters(
    moving_return_evidence: &[ReturnComponentsObservedOpenSessionReturn],
    no_move_event_model: NoMoveEventModel,
) -> ReturnClockExposureParameters {
    let buy_scale = fit_side_return_clock_scale(
        moving_return_evidence,
        no_move_event_model,
        ReturnClockSide::Buy,
    )
    .unwrap_or(ReturnClockExposureParameters::neutral().buy_excitation_return_clock_scale);

    let sell_scale = fit_side_return_clock_scale(
        moving_return_evidence,
        no_move_event_model,
        ReturnClockSide::Sell,
    )
    .unwrap_or(ReturnClockExposureParameters::neutral().sell_excitation_return_clock_scale);

    ReturnClockExposureParameters::try_new(buy_scale, sell_scale)
        .unwrap_or_else(|_| ReturnClockExposureParameters::neutral())
}

pub(crate) fn moving_return_clock_proxy(
    observation: ReturnComponentsObservedOpenSessionReturn,
    no_move_event_model: NoMoveEventModel,
    return_clock_exposure_parameters: ReturnClockExposureParameters,
    baseline_clock_scale: f64,
) -> f64 {
    side_moving_return_clock(
        observation.source_activity.buy_activity,
        no_move_event_model.buy_baseline_move_probability(),
        no_move_event_model.buy_endogenous_move_probability(),
        return_clock_exposure_parameters.buy_excitation_return_clock_scale,
        baseline_clock_scale,
    ) + side_moving_return_clock(
        observation.source_activity.sell_activity,
        no_move_event_model.sell_baseline_move_probability(),
        no_move_event_model.sell_endogenous_move_probability(),
        return_clock_exposure_parameters.sell_excitation_return_clock_scale,
        baseline_clock_scale,
    )
}

fn fit_side_return_clock_scale(
    moving_return_evidence: &[ReturnComponentsObservedOpenSessionReturn],
    no_move_event_model: NoMoveEventModel,
    side: ReturnClockSide,
) -> Option<f64> {
    let side_second_moment = side_weighted_second_moment(moving_return_evidence, side)?;

    let mut numerator = 0.0;
    let mut denominator = 0.0;

    for observation in moving_return_evidence {
        if observation.log_return == 0.0 {
            continue;
        }

        let activity = side_activity(*observation, side);

        let side_weight = activity.total_activity();
        if side_weight <= 0.0 {
            continue;
        }

        if activity.endogenous_activity <= 0.0 {
            continue;
        }

        let baseline_clock =
            side_baseline_move_probability(no_move_event_model, side) * activity.baseline_activity;

        let endogenous_clock = side_endogenous_move_probability(no_move_event_model, side)
            * activity.endogenous_activity;

        if endogenous_clock <= 0.0 {
            continue;
        }

        let observed_clock_target =
            side_weight * observation.log_return.powi(2) / side_second_moment;

        numerator += endogenous_clock * (observed_clock_target - baseline_clock);
        denominator += endogenous_clock * endogenous_clock;
    }

    if denominator <= 0.0 {
        return None;
    }

    let fitted_scale = numerator / denominator;
    if fitted_scale.is_finite() && fitted_scale > 0.0 {
        Some(fitted_scale)
    } else {
        None
    }
}

fn side_weighted_second_moment(
    moving_return_evidence: &[ReturnComponentsObservedOpenSessionReturn],
    side: ReturnClockSide,
) -> Option<f64> {
    let mut weighted_second_moment_sum = 0.0;
    let mut weight_sum = 0.0;

    for observation in moving_return_evidence {
        if observation.log_return == 0.0 {
            continue;
        }

        let side_weight = side_activity(*observation, side).total_activity();
        if side_weight <= 0.0 {
            continue;
        }

        weighted_second_moment_sum += side_weight * observation.log_return.powi(2);
        weight_sum += side_weight;
    }

    if weight_sum <= 0.0 {
        return None;
    }

    let second_moment = weighted_second_moment_sum / weight_sum;
    if second_moment.is_finite() && second_moment > 0.0 {
        Some(second_moment)
    } else {
        None
    }
}

fn side_activity(
    observation: ReturnComponentsObservedOpenSessionReturn,
    side: ReturnClockSide,
) -> IntegratedSideActivityDecomposition {
    match side {
        ReturnClockSide::Buy => observation.source_activity.buy_activity,
        ReturnClockSide::Sell => observation.source_activity.sell_activity,
    }
}

fn side_moving_return_clock(
    activity: IntegratedSideActivityDecomposition,
    baseline_move_probability: f64,
    endogenous_move_probability: f64,
    excitation_return_clock_scale: f64,
    baseline_clock_scale: f64,
) -> f64 {
    baseline_move_probability * activity.baseline_activity * baseline_clock_scale
        + excitation_return_clock_scale * endogenous_move_probability * activity.endogenous_activity
}

fn side_baseline_move_probability(
    no_move_event_model: NoMoveEventModel,
    side: ReturnClockSide,
) -> f64 {
    match side {
        ReturnClockSide::Buy => no_move_event_model.buy_baseline_move_probability(),
        ReturnClockSide::Sell => no_move_event_model.sell_baseline_move_probability(),
    }
}

fn side_endogenous_move_probability(
    no_move_event_model: NoMoveEventModel,
    side: ReturnClockSide,
) -> f64 {
    match side {
        ReturnClockSide::Buy => no_move_event_model.buy_endogenous_move_probability(),
        ReturnClockSide::Sell => no_move_event_model.sell_endogenous_move_probability(),
    }
}

#[cfg(test)]
mod tests {
    use janiform_model_kernels::no_move_event::NoMoveEventModel;
    use janiform_model_kernels::return_clock::{
        IntegratedSideActivityDecomposition, ReturnClockExposureParameters,
        SideSourceActivityDecomposition,
    };

    use super::{fit_return_clock_exposure_parameters, moving_return_clock_proxy};
    use crate::stages::return_components::ReturnComponentsObservedOpenSessionReturn;

    #[test]
    fn return_clock_exposure_fit_increases_endogenous_scale_for_larger_high_activity_moves() {
        let observations = vec![
            observed_return(1, 0.01, buy_activity(1.0, 0.0)),
            observed_return(2, 0.04, buy_activity(0.0, 1.0)),
            observed_return(3, -0.01, sell_activity(1.0, 0.0)),
            observed_return(4, -0.04, sell_activity(0.0, 1.0)),
        ];

        let fitted = fit_return_clock_exposure_parameters(
            observations.as_slice(),
            NoMoveEventModel::neutral(),
        );

        assert!(fitted.buy_excitation_return_clock_scale > 1.0);
        assert!(fitted.sell_excitation_return_clock_scale > 1.0);
    }

    #[test]
    fn return_clock_exposure_fit_stays_neutral_without_endogenous_moving_evidence() {
        let observations = vec![
            observed_return(1, 0.01, buy_activity(1.0, 0.0)),
            observed_return(2, -0.01, sell_activity(1.0, 0.0)),
        ];

        let fitted = fit_return_clock_exposure_parameters(
            observations.as_slice(),
            NoMoveEventModel::neutral(),
        );

        assert_eq!(fitted, ReturnClockExposureParameters::neutral());
    }

    #[test]
    fn moving_return_clock_proxy_applies_no_move_and_long_variance_to_baseline_clock() {
        let no_move_event_model = NoMoveEventModel::try_new(0.25, 0.5, 0.0, 0.75)
            .expect("no-move probabilities should validate");

        let return_clock_exposure_parameters = ReturnClockExposureParameters::try_new(2.0, 3.0)
            .expect("return clock exposure should validate");

        let observation = observed_return(
            1,
            0.01,
            SideSourceActivityDecomposition::new(
                IntegratedSideActivityDecomposition::new(0.375, 0.375),
                IntegratedSideActivityDecomposition::new(0.125, 0.125),
            ),
        );
        let clock = moving_return_clock_proxy(
            observation,
            no_move_event_model,
            return_clock_exposure_parameters,
            4.0,
        );

        assert_eq!(clock, 2.09375);
    }

    fn observed_return(
        return_timestamp_utc_nanos: i64,
        log_return: f64,
        source_activity: SideSourceActivityDecomposition,
    ) -> ReturnComponentsObservedOpenSessionReturn {
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos,
            elapsed_same_session_hours: 1.0,
            log_return,
            post_shock_activity_lift: 1.0,
            sell_share_proxy: source_activity.sell_activity.total_activity(),
            source_activity,
        }
    }

    fn buy_activity(
        baseline_activity: f64,
        endogenous_activity: f64,
    ) -> SideSourceActivityDecomposition {
        SideSourceActivityDecomposition::new(
            IntegratedSideActivityDecomposition::new(baseline_activity, endogenous_activity),
            IntegratedSideActivityDecomposition::new(0.0, 0.0),
        )
    }

    fn sell_activity(
        baseline_activity: f64,
        endogenous_activity: f64,
    ) -> SideSourceActivityDecomposition {
        SideSourceActivityDecomposition::new(
            IntegratedSideActivityDecomposition::new(0.0, 0.0),
            IntegratedSideActivityDecomposition::new(baseline_activity, endogenous_activity),
        )
    }
}
