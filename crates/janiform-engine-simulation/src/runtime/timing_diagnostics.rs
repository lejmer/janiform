use crate::artifacts::{
    SeededTickSimulationInterarrivalDiagnostics, SeededTickSimulationTimingDiagnostics,
};
use crate::config::SeededTickSimulationRunConfig;
use crate::constants::{
    SEEDED_TICK_EVENT_LATENT_INTERVAL_OWNERSHIP_POLICY,
    SEEDED_TICK_EVENT_OBSERVED_TIMESTAMP_INTERVAL_OWNERSHIP_POLICY,
    SEEDED_TICK_EVENT_TIMING_DIAGNOSTICS_SCHEMA_VERSION,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SeededTickSimulationTimingDiagnosticsAccumulator {
    latent_event_interarrivals: SeededTickSimulationInterarrivalAccumulator,
    observed_timestamp_interarrivals: SeededTickSimulationInterarrivalAccumulator,
}

impl SeededTickSimulationTimingDiagnosticsAccumulator {
    pub(crate) fn record_event(
        &mut self,
        latent_event_time_utc_nanos: i64,
        observed_timestamp_utc_nanos: i64,
    ) {
        self.latent_event_interarrivals
            .record_time(latent_event_time_utc_nanos);

        self.observed_timestamp_interarrivals
            .record_time(observed_timestamp_utc_nanos);
    }

    pub(crate) fn finish(
        self,
        run_config: &SeededTickSimulationRunConfig,
    ) -> SeededTickSimulationTimingDiagnostics {
        // Diagnostics are serialized independently from the run config.
        let timestamp_batching_observation_model = run_config
            .model_inputs
            .timestamp_batching_observation_model
            .clone();

        SeededTickSimulationTimingDiagnostics {
            schema_version: SEEDED_TICK_EVENT_TIMING_DIAGNOSTICS_SCHEMA_VERSION,
            // The diagnostics repeat the compatibility policy as a top-level comparison hint.
            coarse_interval_compatibility_policy: timestamp_batching_observation_model
                .coarse_interval_compatibility_policy
                .clone(),
            timestamp_batching_observation_model,
            latent_event_interarrivals: self.latent_event_interarrivals.finish(),
            observed_timestamp_interarrivals: self.observed_timestamp_interarrivals.finish(),
            latent_interval_ownership_policy: SEEDED_TICK_EVENT_LATENT_INTERVAL_OWNERSHIP_POLICY,
            observed_timestamp_interval_ownership_policy:
                SEEDED_TICK_EVENT_OBSERVED_TIMESTAMP_INTERVAL_OWNERSHIP_POLICY,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SeededTickSimulationInterarrivalAccumulator {
    previous_time_utc_nanos: Option<i64>,
    interarrival_count: usize,
    zero_interarrival_count: usize,
    min_positive_interarrival_nanos: Option<i64>,
}

impl SeededTickSimulationInterarrivalAccumulator {
    fn record_time(&mut self, time_utc_nanos: i64) {
        let Some(previous_time_utc_nanos) = self.previous_time_utc_nanos else {
            self.previous_time_utc_nanos = Some(time_utc_nanos);
            return;
        };

        let interarrival_nanos = time_utc_nanos - previous_time_utc_nanos;

        self.previous_time_utc_nanos = Some(time_utc_nanos);

        if interarrival_nanos < 0 {
            return;
        }

        self.interarrival_count += 1;

        if interarrival_nanos == 0 {
            self.zero_interarrival_count += 1;
            return;
        }

        self.min_positive_interarrival_nanos = Some(
            self.min_positive_interarrival_nanos
                .map(|current_minimum| current_minimum.min(interarrival_nanos))
                .unwrap_or(interarrival_nanos),
        );
    }

    fn finish(self) -> SeededTickSimulationInterarrivalDiagnostics {
        SeededTickSimulationInterarrivalDiagnostics {
            interarrival_count: self.interarrival_count,
            zero_interarrival_count: self.zero_interarrival_count,
            min_positive_interarrival_nanos: self.min_positive_interarrival_nanos,
        }
    }
}
