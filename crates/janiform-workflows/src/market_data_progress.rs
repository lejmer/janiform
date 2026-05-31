//! Shared market-data loading progress adapters for workflow frontends.

use std::time::Instant;

use janiform::calibration::market_data::MarketDataLoadingProgressEvent;
use janiform_event_log::{
    EventDeliveryPriority, ProgressMeasure as EventProgressMeasure, RunEvent, RunEventPayload,
    RunPhaseRef, RunProgressEvent, WorkflowRef,
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
                format!(
                    "{}: {}",
                    event.source.source_label(),
                    event.phase.phase_label()
                ),
            )),
            measures: vec![market_data_loading_event_measure(event)],
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

#[cfg(test)]
mod tests {
    use janiform::calibration::market_data::{
        MarketDataLoadingProgressEvent, MarketDataLoadingProgressPhase, MarketDataLoadingSource,
    };

    use super::{market_data_loading_progress_measure, market_data_loading_progress_signal};

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
}
