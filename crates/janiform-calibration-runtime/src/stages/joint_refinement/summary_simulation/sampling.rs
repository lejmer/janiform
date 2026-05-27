use janiform_engine_simulation::SeededTickSimulationRunError;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct BoundedTickShapeReturnSampler {
    retained_returns: Vec<f64>,
    target_count: usize,
    retention_stride: usize,
    seen_return_count: usize,
}

impl BoundedTickShapeReturnSampler {
    pub(super) fn new(target_count: usize) -> Self {
        Self {
            retained_returns: Vec::new(),
            target_count,
            retention_stride: 1,
            seen_return_count: 0,
        }
    }

    pub(super) fn retained_returns(&self) -> &[f64] {
        self.retained_returns.as_slice()
    }

    pub(super) fn reserve_retention_capacity(
        &mut self,
        estimated_event_capacity: usize,
    ) -> Result<(), SeededTickSimulationRunError> {
        self.retained_returns.clear();

        self.retention_stride =
            retained_trade_return_initial_stride(estimated_event_capacity, self.target_count);

        self.seen_return_count = 0;

        let additional_capacity = self
            .target_count
            .saturating_sub(self.retained_returns.capacity());
        if additional_capacity > 0 {
            self.retained_returns
                .try_reserve(additional_capacity)
                .map_err(
                    |source| SeededTickSimulationRunError::BufferAllocationFailed {
                        buffer_name: "joint_refinement.retained_trade_returns",
                        requested_capacity: self.target_count,
                        detail: source.to_string(),
                    },
                )?;
        }

        Ok(())
    }

    pub(super) fn retain_return(&mut self, trade_return: f64) {
        if self.target_count == 0 {
            return;
        }

        let return_position = self.seen_return_count;
        self.seen_return_count = self.seen_return_count.saturating_add(1);

        if !return_position.is_multiple_of(self.retention_stride) {
            return;
        }

        if self.retained_returns.len() >= self.target_count {
            self.compact_retained_returns();
        }

        self.retained_returns.push(trade_return);
    }

    fn compact_retained_returns(&mut self) {
        // Binary compaction preserves chronological coverage while keeping the
        // retained tick-shape sample bounded by the observed shape target.
        if self.target_count <= 1 {
            self.retained_returns.clear();
            self.retention_stride = self.retention_stride.saturating_mul(2);

            return;
        }

        let mut retained_position = 0usize;

        self.retained_returns.retain(|_| {
            let keep = retained_position.is_multiple_of(2);
            retained_position = retained_position.saturating_add(1);
            keep
        });

        self.retention_stride = self.retention_stride.saturating_mul(2);
    }
}

pub(super) fn retained_trade_return_initial_stride(
    estimated_event_capacity: usize,
    target_count: usize,
) -> usize {
    if target_count == 0 {
        return 1;
    }

    estimated_event_capacity.div_ceil(target_count).max(1)
}

#[cfg(test)]
mod tests {
    use super::{BoundedTickShapeReturnSampler, retained_trade_return_initial_stride};

    #[test]
    fn retained_trade_return_initial_stride_tracks_estimated_event_capacity() {
        assert_eq!(retained_trade_return_initial_stride(16, 4), 4);
        assert_eq!(retained_trade_return_initial_stride(3, 4), 1);
        assert_eq!(retained_trade_return_initial_stride(16, 0), 1);
    }

    #[test]
    fn sampler_retains_time_spread_tick_shape_sample() {
        let mut sampler = BoundedTickShapeReturnSampler::new(4);
        sampler
            .reserve_retention_capacity(16)
            .expect("sampler should reserve bounded retention capacity");

        for value in 0..16 {
            sampler.retain_return(f64::from(value));
        }

        assert_eq!(sampler.retained_returns(), &[0.0, 4.0, 8.0, 12.0]);
    }

    #[test]
    fn sampler_compacts_when_realized_count_exceeds_estimate() {
        let mut sampler = BoundedTickShapeReturnSampler::new(4);
        sampler
            .reserve_retention_capacity(4)
            .expect("sampler should reserve bounded retention capacity");

        for value in 0..10 {
            sampler.retain_return(f64::from(value));
        }

        assert!(sampler.retained_returns().len() <= 4);
        assert_eq!(sampler.retained_returns(), &[0.0, 4.0, 8.0]);
    }
}
