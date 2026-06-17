use janiform_model_kernels::no_move_event::NoMoveEventModel;

use crate::stages::return_components::ReturnComponentsObservedOpenSessionReturn;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct WeightedNoMoveEventCounter {
    event_weight: f64,
    zero_return_weight: f64,
}

impl WeightedNoMoveEventCounter {
    fn record(&mut self, event_weight: f64, zero_return: bool) {
        self.event_weight += event_weight;

        if zero_return {
            self.zero_return_weight += event_weight;
        }
    }

    fn probability(self) -> Option<f64> {
        (self.event_weight > 0.0).then_some(self.zero_return_weight / self.event_weight)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct NoMoveEventFitCounters {
    buy_baseline: WeightedNoMoveEventCounter,
    buy_endogenous: WeightedNoMoveEventCounter,
    sell_baseline: WeightedNoMoveEventCounter,
    sell_endogenous: WeightedNoMoveEventCounter,
}

impl NoMoveEventFitCounters {
    fn record(&mut self, observation: &ReturnComponentsObservedOpenSessionReturn) {
        let zero_return = observation.log_return == 0.0;

        self.buy_baseline.record(
            observation.source_activity.buy_activity.baseline_activity,
            zero_return,
        );

        self.buy_endogenous.record(
            observation.source_activity.buy_activity.endogenous_activity,
            zero_return,
        );

        self.sell_baseline.record(
            observation.source_activity.sell_activity.baseline_activity,
            zero_return,
        );

        self.sell_endogenous.record(
            observation
                .source_activity
                .sell_activity
                .endogenous_activity,
            zero_return,
        );
    }

    fn buy_source_total(self, endogenous: bool) -> WeightedNoMoveEventCounter {
        if endogenous {
            self.buy_endogenous
        } else {
            self.buy_baseline
        }
    }

    fn sell_source_total(self, endogenous: bool) -> WeightedNoMoveEventCounter {
        if endogenous {
            self.sell_endogenous
        } else {
            self.sell_baseline
        }
    }

    fn source_total(self, endogenous: bool) -> WeightedNoMoveEventCounter {
        combine_counters(
            self.buy_source_total(endogenous),
            self.sell_source_total(endogenous),
        )
    }

    fn buy_total(self) -> WeightedNoMoveEventCounter {
        combine_counters(self.buy_baseline, self.buy_endogenous)
    }

    fn sell_total(self) -> WeightedNoMoveEventCounter {
        combine_counters(self.sell_baseline, self.sell_endogenous)
    }

    fn global_total(self) -> WeightedNoMoveEventCounter {
        combine_counters(self.buy_total(), self.sell_total())
    }

    fn tick_grid_zero_floor(self) -> f64 {
        [
            self.buy_baseline.probability(),
            self.buy_endogenous.probability(),
            self.sell_baseline.probability(),
            self.sell_endogenous.probability(),
        ]
        .into_iter()
        .flatten()
        .min_by(f64::total_cmp)
        .unwrap_or(0.0)
    }
}

pub(super) fn fit_no_move_event_model_from_open_session_returns(
    observed_returns: &[ReturnComponentsObservedOpenSessionReturn],
) -> NoMoveEventModel {
    let counters = observed_returns.iter().fold(
        NoMoveEventFitCounters::default(),
        |mut counters, observation| {
            counters.record(observation);
            counters
        },
    );

    let tick_grid_zero_floor = counters.tick_grid_zero_floor();

    NoMoveEventModel::new(
        resolve_excess_no_move_probability(
            counters.buy_baseline,
            counters.source_total(false),
            counters.buy_total(),
            counters.global_total(),
            tick_grid_zero_floor,
        ),
        resolve_excess_no_move_probability(
            counters.buy_endogenous,
            counters.source_total(true),
            counters.buy_total(),
            counters.global_total(),
            tick_grid_zero_floor,
        ),
        resolve_excess_no_move_probability(
            counters.sell_baseline,
            counters.source_total(false),
            counters.sell_total(),
            counters.global_total(),
            tick_grid_zero_floor,
        ),
        resolve_excess_no_move_probability(
            counters.sell_endogenous,
            counters.source_total(true),
            counters.sell_total(),
            counters.global_total(),
            tick_grid_zero_floor,
        ),
    )
}

fn resolve_excess_no_move_probability(
    side_source_counter: WeightedNoMoveEventCounter,
    source_counter: WeightedNoMoveEventCounter,
    side_counter: WeightedNoMoveEventCounter,
    global_counter: WeightedNoMoveEventCounter,
    tick_grid_zero_floor: f64,
) -> f64 {
    let observed_zero_probability = side_source_counter
        .probability()
        .or_else(|| source_counter.probability())
        .or_else(|| side_counter.probability())
        .or_else(|| global_counter.probability())
        .unwrap_or(0.0);

    excess_zero_probability(observed_zero_probability, tick_grid_zero_floor)
}

fn excess_zero_probability(observed_zero_probability: f64, tick_grid_zero_floor: f64) -> f64 {
    if tick_grid_zero_floor >= 1.0 {
        return 0.0;
    }

    let excess_probability =
        (observed_zero_probability - tick_grid_zero_floor) / (1.0 - tick_grid_zero_floor);

    if excess_probability <= 0.0 {
        return 0.0;
    }

    if excess_probability >= 1.0 {
        return 1.0;
    }

    excess_probability
}

fn combine_counters(
    left: WeightedNoMoveEventCounter,
    right: WeightedNoMoveEventCounter,
) -> WeightedNoMoveEventCounter {
    WeightedNoMoveEventCounter {
        event_weight: left.event_weight + right.event_weight,
        zero_return_weight: left.zero_return_weight + right.zero_return_weight,
    }
}

#[cfg(test)]
mod tests {
    use janiform_model_kernels::return_clock::{
        IntegratedSideActivityDecomposition, SideSourceActivityDecomposition,
    };

    use super::fit_no_move_event_model_from_open_session_returns;
    use crate::stages::return_components::ReturnComponentsObservedOpenSessionReturn;

    #[test]
    fn no_move_event_fit_separates_baseline_and_endogenous_activity() {
        let observed_returns = [
            buy_baseline_return(1, 0.0),
            buy_baseline_return(2, 0.001),
            buy_endogenous_return(3, 0.0),
            buy_endogenous_return(4, 0.0),
            sell_baseline_return(5, 0.0),
            sell_baseline_return(6, 0.001),
            sell_endogenous_return(7, 0.0),
            sell_endogenous_return(8, 0.001),
        ];

        let model = fit_no_move_event_model_from_open_session_returns(&observed_returns);

        assert_eq!(model.buy_baseline_no_move_probability, 0.0);
        assert_eq!(model.buy_endogenous_no_move_probability, 1.0);
        assert_eq!(model.sell_baseline_no_move_probability, 0.0);
        assert_eq!(model.sell_endogenous_no_move_probability, 0.0);
    }

    #[test]
    fn no_move_event_fit_fills_empty_side_source_bucket_from_source_bucket() {
        let observed_returns = [
            buy_endogenous_return(1, 0.0),
            buy_endogenous_return(2, 0.001),
            sell_baseline_return(3, 0.0),
        ];

        let model = fit_no_move_event_model_from_open_session_returns(&observed_returns);

        assert_eq!(model.buy_baseline_no_move_probability, 1.0);
        assert_eq!(model.buy_endogenous_no_move_probability, 0.0);
        assert_eq!(model.sell_baseline_no_move_probability, 1.0);
        assert_eq!(model.sell_endogenous_no_move_probability, 0.0);
    }

    #[test]
    fn no_move_event_fit_estimates_excess_above_tick_grid_zero_floor() {
        let observed_returns = [
            buy_baseline_return(1, 0.0),
            buy_baseline_return(2, 0.0),
            buy_baseline_return(3, 0.0),
            buy_baseline_return(4, 0.001),
            buy_baseline_return(5, 0.001),
            sell_baseline_return(6, 0.0),
            sell_baseline_return(7, 0.0),
            sell_baseline_return(8, 0.0),
            sell_baseline_return(9, 0.001),
            sell_baseline_return(10, 0.001),
            buy_endogenous_return(11, 0.0),
            buy_endogenous_return(12, 0.0),
            buy_endogenous_return(13, 0.0),
            buy_endogenous_return(14, 0.0),
            buy_endogenous_return(15, 0.001),
            sell_endogenous_return(16, 0.0),
            sell_endogenous_return(17, 0.0),
            sell_endogenous_return(18, 0.0),
            sell_endogenous_return(19, 0.001),
            sell_endogenous_return(20, 0.001),
        ];

        let model = fit_no_move_event_model_from_open_session_returns(&observed_returns);
        let expected_buy_endogenous_no_move_probability = (0.8 - 0.6) / (1.0 - 0.6);

        assert_eq!(model.buy_baseline_no_move_probability, 0.0);
        assert_eq!(
            model.buy_endogenous_no_move_probability,
            expected_buy_endogenous_no_move_probability
        );
        assert_eq!(model.sell_baseline_no_move_probability, 0.0);
        assert_eq!(model.sell_endogenous_no_move_probability, 0.0);
    }

    fn buy_baseline_return(
        timestamp: i64,
        log_return: f64,
    ) -> ReturnComponentsObservedOpenSessionReturn {
        observed_return(
            timestamp,
            log_return,
            0.0,
            SideSourceActivityDecomposition::new(
                IntegratedSideActivityDecomposition::new(1.0, 0.0),
                IntegratedSideActivityDecomposition::new(0.0, 0.0),
            ),
        )
    }

    fn buy_endogenous_return(
        timestamp: i64,
        log_return: f64,
    ) -> ReturnComponentsObservedOpenSessionReturn {
        observed_return(
            timestamp,
            log_return,
            0.0,
            SideSourceActivityDecomposition::new(
                IntegratedSideActivityDecomposition::new(0.0, 1.0),
                IntegratedSideActivityDecomposition::new(0.0, 0.0),
            ),
        )
    }

    fn sell_baseline_return(
        timestamp: i64,
        log_return: f64,
    ) -> ReturnComponentsObservedOpenSessionReturn {
        observed_return(
            timestamp,
            log_return,
            1.0,
            SideSourceActivityDecomposition::new(
                IntegratedSideActivityDecomposition::new(0.0, 0.0),
                IntegratedSideActivityDecomposition::new(1.0, 0.0),
            ),
        )
    }

    fn sell_endogenous_return(
        timestamp: i64,
        log_return: f64,
    ) -> ReturnComponentsObservedOpenSessionReturn {
        observed_return(
            timestamp,
            log_return,
            1.0,
            SideSourceActivityDecomposition::new(
                IntegratedSideActivityDecomposition::new(0.0, 0.0),
                IntegratedSideActivityDecomposition::new(0.0, 1.0),
            ),
        )
    }

    fn observed_return(
        timestamp: i64,
        log_return: f64,
        sell_share_proxy: f64,
        source_activity: SideSourceActivityDecomposition,
    ) -> ReturnComponentsObservedOpenSessionReturn {
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos: timestamp,
            elapsed_same_session_hours: 1.0,
            log_return,
            post_shock_activity_lift: 1.0,
            sell_share_proxy,
            source_activity,
        }
    }
}
