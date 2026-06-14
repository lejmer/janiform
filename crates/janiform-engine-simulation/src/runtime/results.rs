use janiform_model_kernels::return_decomposition::ReturnDecompositionValidationError;

use crate::artifacts::{
    SeededFinePathRecord, SeededTickSimulationRunResult, SeededTickSimulationStreamCompletion,
    SeededTickSimulationTransitionConsumer, SeededTickSimulationTransitionEmission,
};
use crate::config::SeededTickSimulationRunConfig;
use crate::constants::{MAX_STOCHASTIC_UPDATES_PER_INTERVAL, NANOS_PER_HOUR};
use crate::error::SeededTickSimulationRunError;

use super::buffers::{push_simulation_buffer_value, try_reserve_simulation_buffer};
use super::endogenous::estimate_endogenous_interval_event_capacity;

#[derive(Debug, Default)]
pub(crate) struct SeededTickSimulationResultBuilder {
    fine_path: Vec<SeededFinePathRecord>,
    public_transition_positions: Vec<usize>,
    event_transition_positions: Vec<usize>,
}

impl SeededTickSimulationResultBuilder {
    pub(crate) fn finish(
        self,
        stream_completion: SeededTickSimulationStreamCompletion,
    ) -> SeededTickSimulationRunResult {
        SeededTickSimulationRunResult {
            schema_version: stream_completion.schema_version,
            determinism_policy: stream_completion.determinism_policy,
            event_ordering_policy: stream_completion.event_ordering_policy,
            randomness_stream_policy: stream_completion.randomness_stream_policy,
            randomness_substream_derivation_policy: stream_completion
                .randomness_substream_derivation_policy,
            seed: stream_completion.seed,
            start_time_utc: stream_completion.start_time_utc,
            end_time_utc: stream_completion.end_time_utc,
            simulated_interval_count: stream_completion.simulated_interval_count,
            fine_path_metadata: stream_completion.fine_path_metadata,
            fine_path: self.fine_path,
            public_transition_positions: self.public_transition_positions,
            event_tape_metadata: stream_completion.event_tape_metadata,
            event_transition_positions: self.event_transition_positions,
            timing_diagnostics: stream_completion.timing_diagnostics,
            final_excitation_state: stream_completion.final_excitation_state,
            news_routing_totals: stream_completion.news_routing_totals,
            cumulative_core_log_return: stream_completion.cumulative_core_log_return,
            debug_trace_enabled: stream_completion.debug_trace_enabled,
            debug_trace_output: stream_completion.debug_trace_output,
        }
    }
}

impl SeededTickSimulationTransitionConsumer for SeededTickSimulationResultBuilder {
    fn reserve_transition_capacity(
        &mut self,
        estimated_transition_capacity: usize,
        estimated_event_capacity: usize,
    ) -> Result<(), SeededTickSimulationRunError> {
        self.fine_path = try_reserve_simulation_buffer("fine_path", estimated_transition_capacity)?;

        self.public_transition_positions = try_reserve_simulation_buffer(
            "public_transition_positions",
            estimated_transition_capacity,
        )?;

        self.event_transition_positions =
            try_reserve_simulation_buffer("event_transition_positions", estimated_event_capacity)?;

        Ok(())
    }

    fn consume_transition(
        &mut self,
        record: SeededFinePathRecord,
        emission: SeededTickSimulationTransitionEmission,
    ) -> Result<(), SeededTickSimulationRunError> {
        let transition_position = self.fine_path.len();

        if emission.is_public_fine_path_record {
            push_simulation_buffer_value(
                "public_transition_positions",
                &mut self.public_transition_positions,
                transition_position,
            )?;
        }

        if emission.is_event_projection_record {
            push_simulation_buffer_value(
                "event_transition_positions",
                &mut self.event_transition_positions,
                transition_position,
            )?;
        }

        push_simulation_buffer_value("fine_path", &mut self.fine_path, record)
    }
}

pub fn estimate_seeded_result_capacities(
    run_config: &SeededTickSimulationRunConfig,
) -> (usize, usize) {
    let known_news_count = run_config
        .model_inputs
        .news_process_configuration
        .arrivals
        .known
        .scheduled_event_timestamps_utc_nanos
        .len();

    let base_transition_count = run_config.horizon_interval_count;

    let mut estimated_endogenous_event_count = 0_usize;
    let mut estimated_surprise_news_count = 0_usize;

    for interval_index in 0..run_config.horizon_interval_count {
        let interval_start_utc_nanos = run_config
            .model_inputs
            .interval_factor_grid
            .interval_start_utc_nanos[interval_index];

        let interval_end_utc_nanos = run_config
            .model_inputs
            .interval_factor_grid
            .interval_end_utc_nanos[interval_index];

        let interval_duration_in_hours =
            (interval_end_utc_nanos - interval_start_utc_nanos) as f64 / NANOS_PER_HOUR as f64;

        let seasonality_factor = run_config
            .model_inputs
            .interval_factor_grid
            .seasonality_factor[interval_index];

        let interval_news_intensity =
            run_config.model_inputs.interval_factor_grid.news_intensity[interval_index].max(0.0);

        estimated_endogenous_event_count = estimated_endogenous_event_count.saturating_add(
            estimate_endogenous_interval_event_capacity(
                run_config.model_inputs.kernel_parameters.lambda_bar,
                seasonality_factor,
                interval_duration_in_hours,
            ),
        );

        let surprise_expected_count = interval_news_intensity * interval_duration_in_hours.max(0.0);

        estimated_surprise_news_count = estimated_surprise_news_count.saturating_add(
            surprise_expected_count
                .ceil()
                .min(MAX_STOCHASTIC_UPDATES_PER_INTERVAL as f64) as usize,
        );
    }

    let estimated_transition_capacity = base_transition_count
        .saturating_add(known_news_count)
        .saturating_add(estimated_surprise_news_count)
        .saturating_add(estimated_endogenous_event_count);

    (
        estimated_transition_capacity,
        estimated_endogenous_event_count,
    )
}

pub(crate) fn validate_integrated_intensity(
    field: &'static str,
    value: f64,
    interval_index: usize,
) -> Result<(), SeededTickSimulationRunError> {
    if !value.is_finite() || value < 0.0 {
        return Err(SeededTickSimulationRunError::InvalidReturnDecomposition {
            interval_index,
            source: Box::new(ReturnDecompositionValidationError::DomainViolation {
                field,
                constraint:
                    janiform_model_kernels::return_decomposition::ReturnDecompositionConstraint::FiniteAndNonNegative,
                value,
            }),
        });
    }

    Ok(())
}
