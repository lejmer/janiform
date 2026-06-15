//! Shared market-data progress adapters for workflow frontends.

use std::time::Instant;

use janiform::calibration::market_data::{
    CalibrationTimestampBatchingObservationDiagnostic, MarketDataLoadingProgressEvent,
    MarketDataPreparationProgressPhase,
};
use janiform::model::timestamp_observation::TimestampBatchingObservationMode;
use janiform_event_log::{
    EventDeliveryPriority, ProgressMeasure as EventProgressMeasure, RunEvent, RunEventPayload,
    RunMetricUpdateEvent, RunPhaseRef, RunProgressEvent, WorkflowRef,
};
use janiform_terminal_ui::{ProgressContractError, ProgressMeasure};

use crate::progress::VolatileProgressGate;

/// Time-gated loading progress policy shared by terminal and machine outputs.
#[derive(Debug, Default)]
pub struct MarketDataLoadingProgressGate {
    progress_gate: VolatileProgressGate<(&'static str, &'static str)>,
}

impl MarketDataLoadingProgressGate {
    /// Return whether the loading event should be emitted now.
    pub fn should_emit(&mut self, event: &MarketDataLoadingProgressEvent) -> bool {
        self.progress_gate.should_emit(
            Instant::now(),
            (event.source.source_id(), event.phase.phase_id()),
            event.completed_records,
            event.total_records.unwrap_or(0),
        )
    }
}

/// Convert a market-data loading event into a terminal progress measure.
pub fn market_data_loading_progress_measure(
    event: &MarketDataLoadingProgressEvent,
) -> Result<ProgressMeasure, ProgressContractError> {
    match event.total_records {
        Some(total_records) if total_records > 0 => {
            ProgressMeasure::bounded(event.completed_records.min(total_records), total_records)
        }
        Some(_) | None => Ok(ProgressMeasure::Unbounded),
    }
}

/// Return the latest signal for a market-data loading event.
pub fn market_data_loading_progress_signal(event: &MarketDataLoadingProgressEvent) -> String {
    event.latest_signal()
}

/// Return the latest signal for a market-data preparation phase.
pub fn market_data_preparation_progress_signal(
    phase: MarketDataPreparationProgressPhase,
) -> String {
    phase.latest_signal().to_string()
}

pub(crate) fn should_emit_market_data_loading_progress(
    progress_gate: &mut MarketDataLoadingProgressGate,
    event: &MarketDataLoadingProgressEvent,
) -> bool {
    progress_gate.should_emit(event)
}

pub(crate) fn market_data_loading_run_event(
    run_id: &str,
    workflow: WorkflowRef,
    event: &MarketDataLoadingProgressEvent,
) -> RunEvent {
    RunEvent::new(
        run_id,
        workflow,
        EventDeliveryPriority::DroppableVolatile,
        RunEventPayload::Progress(RunProgressEvent {
            stage: None,
            phase: Some(RunPhaseRef::labeled(
                "market_data_loading",
                "Loading market data",
            )),
            subphase: Some(RunPhaseRef::labeled(
                format!("{}.{}", event.source.source_id(), event.phase.phase_id()),
                event.phase_signal(),
            )),
            measures: vec![market_data_loading_event_measure(event)],
        }),
    )
}

pub(crate) fn market_data_preparation_run_event(
    run_id: &str,
    workflow: WorkflowRef,
    phase: MarketDataPreparationProgressPhase,
) -> RunEvent {
    RunEvent::new(
        run_id,
        workflow,
        EventDeliveryPriority::DroppableVolatile,
        RunEventPayload::Progress(RunProgressEvent {
            stage: None,
            phase: Some(RunPhaseRef::labeled(
                "market_data_preparation",
                "Preparing market data",
            )),
            subphase: Some(RunPhaseRef::labeled(phase.phase_id(), phase.phase_label())),
            measures: Vec::new(),
        }),
    )
}

pub(crate) fn market_data_timestamp_batching_observation_run_event(
    run_id: &str,
    workflow: WorkflowRef,
    diagnostic: &CalibrationTimestampBatchingObservationDiagnostic,
) -> RunEvent {
    RunEvent::new(
        run_id,
        workflow,
        EventDeliveryPriority::Required,
        RunEventPayload::MetricUpdate(RunMetricUpdateEvent {
            stage: None,
            phase: Some(RunPhaseRef::labeled(
                "market_data_preparation",
                "Preparing market data",
            )),
            measures: timestamp_batching_observation_measures(diagnostic),
        }),
    )
}

fn market_data_loading_event_measure(
    event: &MarketDataLoadingProgressEvent,
) -> EventProgressMeasure {
    match event.total_records {
        Some(total_records) if total_records > 0 => EventProgressMeasure::BoundedCount {
            metric_id: "market_data_records".to_string(),
            label: Some("market data records".to_string()),
            completed_units: event.completed_records.min(total_records),
            total_units: total_records,
        },
        Some(total_records) => EventProgressMeasure::EvaluationCount {
            metric_id: "market_data_records".to_string(),
            label: Some("market data records".to_string()),
            completed_evaluations: event.completed_records.min(total_records),
            evaluation_budget: Some(total_records),
        },
        None => EventProgressMeasure::EvaluationCount {
            metric_id: "market_data_records".to_string(),
            label: Some("market data records".to_string()),
            completed_evaluations: event.completed_records,
            evaluation_budget: None,
        },
    }
}

fn timestamp_batching_observation_measures(
    diagnostic: &CalibrationTimestampBatchingObservationDiagnostic,
) -> Vec<EventProgressMeasure> {
    let mut measures = vec![
        EventProgressMeasure::Text {
            metric_id: "market_data.timestamp_batching_observation.state".to_string(),
            label: Some("timestamp batching state".to_string()),
            value: diagnostic.state.state_id().to_string(),
        },
        EventProgressMeasure::Text {
            metric_id: "market_data.timestamp_batching_observation.mode".to_string(),
            label: Some("timestamp batching mode".to_string()),
            value: diagnostic.observation_model.mode.mode_id().to_string(),
        },
        EventProgressMeasure::Text {
            metric_id: "market_data.timestamp_batching_observation.inference_policy".to_string(),
            label: Some("timestamp batching inference policy".to_string()),
            value: diagnostic.observation_model.inference_policy.to_string(),
        },
        EventProgressMeasure::Text {
            metric_id: "market_data.timestamp_batching_observation.observation_policy".to_string(),
            label: Some("timestamp batching observation policy".to_string()),
            value: diagnostic.observation_model.observation_policy.to_string(),
        },
        EventProgressMeasure::Text {
            metric_id: "market_data.timestamp_batching_observation.diagnostic_policy".to_string(),
            label: Some("timestamp batching diagnostic policy".to_string()),
            value: diagnostic.diagnostic_policy.to_string(),
        },
        EventProgressMeasure::Text {
            metric_id: "market_data.timestamp_batching_observation.message".to_string(),
            label: Some("timestamp batching observation".to_string()),
            value: diagnostic.message.to_string(),
        },
    ];

    if diagnostic.observation_model.mode == TimestampBatchingObservationMode::FixedBucketFloor
        && let Some(bucket_width_nanos) = diagnostic.observation_model.bucket_width_nanos
    {
        measures.push(EventProgressMeasure::Text {
            metric_id: "market_data.timestamp_batching_observation.bucket_width_nanos".to_string(),
            label: Some("timestamp batching bucket width nanos".to_string()),
            value: bucket_width_nanos.to_string(),
        });

        if let Some(same_timestamp_continuation_count) = diagnostic
            .observation_model
            .same_timestamp_continuation_count
        {
            measures.push(EventProgressMeasure::Text {
                metric_id:
                    "market_data.timestamp_batching_observation.same_timestamp_continuation_count"
                        .to_string(),
                label: Some("timestamp batching same timestamp continuation count".to_string()),
                value: same_timestamp_continuation_count.to_string(),
            });
        }

        if let Some(same_session_observation_transition_count) = diagnostic
            .observation_model
            .same_session_observation_transition_count
        {
            measures.push(EventProgressMeasure::Text {
                metric_id: "market_data.timestamp_batching_observation.same_session_observation_transition_count"
                        .to_string(),
                label: Some("timestamp batching same-session transition count".to_string()),
                value: same_session_observation_transition_count.to_string(),
            });
        }

        if let Some(continuation_probability) = diagnostic
            .observation_model
            .same_timestamp_continuation_probability()
        {
            measures.push(EventProgressMeasure::Scalar {
                metric_id: "market_data.timestamp_batching_observation.continuation_probability"
                    .to_string(),
                label: Some("timestamp batching continuation probability".to_string()),
                value: continuation_probability,
            });
        }
    }

    measures
}

#[cfg(test)]
mod tests {
    use janiform::calibration::market_data::{
        CALIBRATION_TIMESTAMP_BATCHING_OBSERVATION_DIAGNOSTIC_POLICY,
        CALIBRATION_TIMESTAMP_BATCHING_OBSERVATION_DIAGNOSTIC_SCHEMA_VERSION,
        CalibrationTimestampBatchingObservationDiagnostic,
        CalibrationTimestampBatchingObservationDiagnosticState, MarketDataLoadingProgressEvent,
        MarketDataLoadingProgressPhase, MarketDataLoadingSource,
        MarketDataPreparationProgressPhase,
    };
    use janiform::model::timestamp_observation::TimestampBatchingObservationModel;
    use janiform_event_log::RunEventPayload;

    use super::{
        market_data_loading_progress_measure, market_data_loading_progress_signal,
        market_data_preparation_progress_signal,
        market_data_timestamp_batching_observation_run_event,
    };

    #[test]
    fn market_data_loading_progress_uses_bounded_measure_when_total_is_known() {
        let event = MarketDataLoadingProgressEvent::new(
            MarketDataLoadingSource::TickData,
            MarketDataLoadingProgressPhase::ReadingRecords,
            4,
            Some(10),
        );

        assert!(market_data_loading_progress_measure(&event).is_ok());
        assert_eq!(
            market_data_loading_progress_signal(&event),
            "tick data: reading records 4/10 records"
        );
    }

    #[test]
    fn market_data_loading_progress_uses_unbounded_measure_without_total() {
        let event = MarketDataLoadingProgressEvent::new(
            MarketDataLoadingSource::TickData,
            MarketDataLoadingProgressPhase::ReadingRecords,
            4,
            None,
        );

        assert!(market_data_loading_progress_measure(&event).is_ok());
        assert_eq!(
            market_data_loading_progress_signal(&event),
            "tick data: reading records 4 records"
        );
    }

    #[test]
    fn market_data_loading_progress_reports_active_timestamp_batching_inference() {
        let event = MarketDataLoadingProgressEvent::new(
            MarketDataLoadingSource::TickData,
            MarketDataLoadingProgressPhase::InferringTimestampBatchingObservation,
            0,
            None,
        );

        assert!(market_data_loading_progress_measure(&event).is_ok());
        assert_eq!(
            market_data_loading_progress_signal(&event),
            "tick data: inferring observed timestamp batching model"
        );
    }

    #[test]
    fn market_data_preparation_progress_uses_phase_signal() {
        assert_eq!(
            market_data_preparation_progress_signal(
                MarketDataPreparationProgressPhase::BuildingExecutionInput
            ),
            "materializing staged execution input"
        );
    }

    #[test]
    fn timestamp_batching_observation_progress_event_is_required_metric_update() {
        let diagnostic = CalibrationTimestampBatchingObservationDiagnostic {
            schema_version: CALIBRATION_TIMESTAMP_BATCHING_OBSERVATION_DIAGNOSTIC_SCHEMA_VERSION
                .to_string(),
            diagnostic_policy: CALIBRATION_TIMESTAMP_BATCHING_OBSERVATION_DIAGNOSTIC_POLICY
                .to_string(),
            state:
                CalibrationTimestampBatchingObservationDiagnosticState::DisabledNoDetectableBatching,
            return_components_stage_selected: true,
            observation_model: TimestampBatchingObservationModel::disabled(),
            message: "timestamp batching observation evaluated".to_string(),
        };

        let event = market_data_timestamp_batching_observation_run_event(
            "run-1",
            janiform_event_log::WorkflowRef::labeled("fit", "Fit"),
            &diagnostic,
        );

        let RunEventPayload::MetricUpdate(metric_update) = event.payload else {
            panic!("timestamp batching diagnostic should be emitted as metric update");
        };

        assert!(metric_update.measures.iter().any(|measure| matches!(
            measure,
            janiform_event_log::ProgressMeasure::Text { metric_id, value, .. }
                if metric_id == "market_data.timestamp_batching_observation.state"
                    && value == "disabled_no_detectable_batching"
        )));
    }

    #[test]
    fn timestamp_batching_observation_progress_event_reports_fitted_continuation_metrics() {
        let observation_model =
            TimestampBatchingObservationModel::fixed_bucket_floor(1_000, "test_source", 2, 10)
                .expect("timestamp batching fixture should validate");

        let diagnostic = CalibrationTimestampBatchingObservationDiagnostic {
            schema_version: CALIBRATION_TIMESTAMP_BATCHING_OBSERVATION_DIAGNOSTIC_SCHEMA_VERSION
                .to_string(),
            diagnostic_policy: CALIBRATION_TIMESTAMP_BATCHING_OBSERVATION_DIAGNOSTIC_POLICY
                .to_string(),
            state: CalibrationTimestampBatchingObservationDiagnosticState::InferredFixedBucket,
            return_components_stage_selected: true,
            observation_model,
            message: "timestamp batching observation inferred".to_string(),
        };

        let event = market_data_timestamp_batching_observation_run_event(
            "run-1",
            janiform_event_log::WorkflowRef::labeled("fit", "Fit"),
            &diagnostic,
        );

        let RunEventPayload::MetricUpdate(metric_update) = event.payload else {
            panic!("timestamp batching diagnostic should be emitted as metric update");
        };

        assert!(metric_update.measures.iter().any(|measure| matches!(
            measure,
            janiform_event_log::ProgressMeasure::Text { metric_id, value, .. }
                if metric_id
                    == "market_data.timestamp_batching_observation.same_timestamp_continuation_count"
                    && value == "2"
        )));

        assert!(metric_update.measures.iter().any(|measure| matches!(
            measure,
            janiform_event_log::ProgressMeasure::Text { metric_id, value, .. }
                if metric_id
                    == "market_data.timestamp_batching_observation.same_session_observation_transition_count"
                    && value == "10"
        )));

        assert!(metric_update.measures.iter().any(|measure| matches!(
            measure,
            janiform_event_log::ProgressMeasure::Scalar { metric_id, value, .. }
                if metric_id
                    == "market_data.timestamp_batching_observation.continuation_probability"
                    && *value == 2.0 / 10.0
        )));
    }
}
