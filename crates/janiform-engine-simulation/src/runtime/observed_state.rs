use janiform_model_kernels::mean_reversion::MeanReversionBoundaryState;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SeededObservedState {
    pub(crate) cumulative_core_log_return: f64,
    pub(crate) observed_log_price: f64,
    pub(crate) anchor_value: f64,
    pub(crate) pressure_state: f64,
    pub(crate) deviation_from_anchor: f64,
}

impl SeededObservedState {
    pub(crate) fn from_boundary_state(boundary_state: &MeanReversionBoundaryState) -> Self {
        Self {
            cumulative_core_log_return: 0.0,
            observed_log_price: boundary_state.observed_log_price,
            anchor_value: boundary_state.anchor_value,
            pressure_state: boundary_state.pressure_state,
            deviation_from_anchor: boundary_state.deviation,
        }
    }

    pub(crate) fn apply_core_increment(
        self,
        kappa: f64,
        elapsed_hours_since_previous_update: f64,
        core_total_return_increment: f64,
    ) -> Self {
        let deviation_from_anchor = (-kappa * elapsed_hours_since_previous_update).exp()
            * self.deviation_from_anchor
            + core_total_return_increment;

        Self {
            cumulative_core_log_return: self.cumulative_core_log_return
                + core_total_return_increment,
            observed_log_price: self.anchor_value + deviation_from_anchor,
            anchor_value: self.anchor_value,
            pressure_state: self.pressure_state,
            deviation_from_anchor,
        }
    }

    pub(crate) fn apply_anchor_reset(
        self,
        next_anchor_value: f64,
        next_pressure_state: f64,
    ) -> Self {
        let deviation_from_anchor =
            self.deviation_from_anchor + self.anchor_value - next_anchor_value;

        Self {
            cumulative_core_log_return: self.cumulative_core_log_return,
            observed_log_price: next_anchor_value + deviation_from_anchor,
            anchor_value: next_anchor_value,
            pressure_state: next_pressure_state,
            deviation_from_anchor,
        }
    }
}
