use janiform_model_kernels::mean_reversion::MeanReversionBoundaryState;

use crate::error::SeededTickSimulationRunError;

use super::is_valid_observed_price_tick_size;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SeededObservedState {
    pub(crate) cumulative_core_log_return: f64,
    pub(crate) observed_log_price: f64,
    pub(crate) anchor_value: f64,
    pub(crate) deviation_from_anchor: f64,
    tick_grid_origin_log_price: f64,
    tick_grid_residual_log_return: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SeededProjectedCoreIncrement {
    pub(crate) observed_core_increment: f64,
    pub(crate) tick_grid_residual_log_return: f64,
}

impl SeededObservedState {
    pub(crate) fn from_boundary_state(boundary_state: &MeanReversionBoundaryState) -> Self {
        Self {
            cumulative_core_log_return: 0.0,
            observed_log_price: boundary_state.observed_log_price,
            anchor_value: boundary_state.anchor_value,
            deviation_from_anchor: boundary_state.deviation,
            tick_grid_origin_log_price: boundary_state.observed_log_price,
            tick_grid_residual_log_return: 0.0,
        }
    }

    pub(crate) fn project_core_increment(
        self,
        kappa: f64,
        elapsed_hours_since_previous_update: f64,
        raw_core_increment: f64,
        observed_price_tick_size: Option<f64>,
        interval_index: usize,
    ) -> Result<SeededProjectedCoreIncrement, SeededTickSimulationRunError> {
        let decay_weight = (-kappa * elapsed_hours_since_previous_update).exp();
        let observed_base_log_price = self.anchor_value + decay_weight * self.deviation_from_anchor;

        let latent_base_log_price = self.anchor_value
            + decay_weight * (self.deviation_from_anchor + self.tick_grid_residual_log_return);

        let latent_target_log_price = latent_base_log_price + raw_core_increment;

        let Some(tick_size) = observed_price_tick_size else {
            return Ok(SeededProjectedCoreIncrement {
                observed_core_increment: latent_target_log_price - observed_base_log_price,
                tick_grid_residual_log_return: 0.0,
            });
        };

        let observed_target_log_price = project_log_price_to_tick_grid(
            latent_target_log_price,
            self.tick_grid_origin_log_price,
            tick_size,
            interval_index,
        )?;

        Ok(SeededProjectedCoreIncrement {
            observed_core_increment: observed_target_log_price - observed_base_log_price,
            tick_grid_residual_log_return: latent_target_log_price - observed_target_log_price,
        })
    }

    pub(crate) fn apply_projected_core_increment(
        self,
        kappa: f64,
        elapsed_hours_since_previous_update: f64,
        projected_core_increment: SeededProjectedCoreIncrement,
    ) -> Self {
        let deviation_from_anchor = (-kappa * elapsed_hours_since_previous_update).exp()
            * self.deviation_from_anchor
            + projected_core_increment.observed_core_increment;

        Self {
            cumulative_core_log_return: self.cumulative_core_log_return
                + projected_core_increment.observed_core_increment,
            observed_log_price: self.anchor_value + deviation_from_anchor,
            anchor_value: self.anchor_value,
            deviation_from_anchor,
            tick_grid_origin_log_price: self.tick_grid_origin_log_price,
            tick_grid_residual_log_return: projected_core_increment.tick_grid_residual_log_return,
        }
    }

    pub(crate) fn apply_anchor_reset(self, next_anchor_value: f64) -> Self {
        let deviation_from_anchor =
            self.deviation_from_anchor + self.anchor_value - next_anchor_value;

        Self {
            cumulative_core_log_return: self.cumulative_core_log_return,
            observed_log_price: next_anchor_value + deviation_from_anchor,
            anchor_value: next_anchor_value,
            deviation_from_anchor,
            tick_grid_origin_log_price: self.tick_grid_origin_log_price,
            tick_grid_residual_log_return: self.tick_grid_residual_log_return,
        }
    }
}

/// Project a sequence of latent core increments to observed core increments using the same
/// residual-preserving tick-grid projection as seeded and coarse simulation.
///
/// The sequence starts from the normalized reference log price `0.0`, matching normalized tick
/// sizes whose reference price is one.
pub fn project_core_increment_sequence_to_tick_grid(
    raw_core_increments: &[f64],
    observed_price_tick_size: Option<f64>,
) -> Result<Vec<f64>, SeededTickSimulationRunError> {
    if let Some(tick_size) = observed_price_tick_size
        && !is_valid_observed_price_tick_size(tick_size)
    {
        return Err(SeededTickSimulationRunError::InvalidObservedPriceTickSize {
            value: tick_size,
        });
    }

    let mut observed_state = SeededObservedState {
        cumulative_core_log_return: 0.0,
        observed_log_price: 0.0,
        anchor_value: 0.0,
        deviation_from_anchor: 0.0,
        tick_grid_origin_log_price: 0.0,
        tick_grid_residual_log_return: 0.0,
    };

    let mut projected_increments = Vec::with_capacity(raw_core_increments.len());

    for (increment_index, raw_core_increment) in raw_core_increments.iter().enumerate() {
        let projected_increment = observed_state.project_core_increment(
            0.0,
            0.0,
            *raw_core_increment,
            observed_price_tick_size,
            increment_index,
        )?;

        observed_state =
            observed_state.apply_projected_core_increment(0.0, 0.0, projected_increment);

        projected_increments.push(projected_increment.observed_core_increment);
    }

    Ok(projected_increments)
}

fn project_log_price_to_tick_grid(
    latent_target_log_price: f64,
    tick_grid_origin_log_price: f64,
    tick_size: f64,
    interval_index: usize,
) -> Result<f64, SeededTickSimulationRunError> {
    let tick_grid_origin_price = tick_grid_origin_log_price.exp();
    let latent_target_price = latent_target_log_price.exp();

    if !tick_grid_origin_price.is_finite()
        || tick_grid_origin_price <= 0.0
        || !latent_target_price.is_finite()
        || latent_target_price <= 0.0
    {
        return Err(SeededTickSimulationRunError::TickGridProjectionFailed {
            interval_index,
            previous_observed_log_price: tick_grid_origin_log_price,
            raw_total_return: latent_target_log_price - tick_grid_origin_log_price,
        });
    }

    let tick_units = ((latent_target_price - tick_grid_origin_price) / tick_size).round();
    let observed_price = tick_grid_origin_price + tick_units * tick_size;

    if !observed_price.is_finite() || observed_price <= 0.0 {
        return Err(SeededTickSimulationRunError::TickGridProjectionFailed {
            interval_index,
            previous_observed_log_price: tick_grid_origin_log_price,
            raw_total_return: latent_target_log_price - tick_grid_origin_log_price,
        });
    }

    Ok(observed_price.ln())
}

#[cfg(test)]
mod tests {
    use super::{SeededObservedState, project_core_increment_sequence_to_tick_grid};
    use janiform_model_kernels::mean_reversion::MeanReversionBoundaryState;

    #[test]
    fn tick_grid_projection_carries_sub_tick_residual_between_updates() {
        let boundary_state = MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0)
            .expect("boundary state should validate");

        let state = SeededObservedState::from_boundary_state(&boundary_state);

        let first_projection = state
            .project_core_increment(0.0, 0.0, 0.004, Some(0.01), 0)
            .expect("first projection should succeed");

        let state_after_first_projection =
            state.apply_projected_core_increment(0.0, 0.0, first_projection);

        let second_projection = state_after_first_projection
            .project_core_increment(0.0, 0.0, 0.004, Some(0.01), 0)
            .expect("second projection should succeed");

        let state_after_second_projection = state_after_first_projection
            .apply_projected_core_increment(0.0, 0.0, second_projection);

        assert_eq!(state_after_first_projection.observed_log_price, 0.0);
        assert_eq!(
            state_after_second_projection.observed_log_price,
            1.01_f64.ln()
        );
    }

    #[test]
    fn tick_grid_sequence_projection_carries_sub_tick_residual_between_increments() {
        let projected_increments =
            project_core_increment_sequence_to_tick_grid(&[0.004, 0.004], Some(0.01))
                .expect("projection should succeed");

        assert_eq!(projected_increments, vec![0.0, 1.01_f64.ln()]);
    }

    #[test]
    fn tick_grid_sequence_projection_preserves_increments_without_tick_size() {
        let raw_increments = vec![0.004, -0.002];

        let projected_increments =
            project_core_increment_sequence_to_tick_grid(raw_increments.as_slice(), None)
                .expect("projection should succeed");

        assert_eq!(projected_increments, raw_increments);
    }

    #[test]
    fn tick_grid_sequence_projection_rejects_invalid_tick_size() {
        let error = project_core_increment_sequence_to_tick_grid(&[0.004], Some(-0.01))
            .expect_err("negative tick size should be rejected");

        assert!(matches!(
            error,
            crate::error::SeededTickSimulationRunError::InvalidObservedPriceTickSize { .. }
        ));
    }
}
