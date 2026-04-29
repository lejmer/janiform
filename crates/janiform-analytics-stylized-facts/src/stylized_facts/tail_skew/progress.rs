use std::sync::mpsc;
use std::thread;

use super::contracts::{
    TailSkewDatasetRole, TailSkewDiagnosticsError, TailSkewDiagnosticsProgressEvent,
    TailSkewDiagnosticsReport, TailSkewDiagnosticsRequest,
};
use super::core::run_tail_skew_diagnostics_internal;
use super::metrics::{TAIL_SKEW_DATASET_METRIC_TASK_COUNT, TailSkewMetricSpecification};

/// Run tail/skew diagnostics and observe completed dataset metric tasks.
pub fn run_tail_skew_diagnostics_with_observer(
    request: TailSkewDiagnosticsRequest,
    observer: &mut dyn FnMut(TailSkewDiagnosticsProgressEvent),
) -> Result<TailSkewDiagnosticsReport, TailSkewDiagnosticsError> {
    let total_units = diagnostics_progress_total_units(request.horizon_buckets.len())?;
    let (sender, receiver) = mpsc::channel();

    let worker = thread::spawn(move || {
        let result = run_tail_skew_diagnostics_internal(request, Some(sender.clone()));

        let _ = sender.send(TailSkewDiagnosticsWorkerMessage::Completed(result));
    });

    let mut completed_units = 0usize;
    let mut diagnostics_result = None;

    for message in receiver {
        match message {
            TailSkewDiagnosticsWorkerMessage::DatasetMetricCompleted(payload) => {
                completed_units += 1;

                observer(TailSkewDiagnosticsProgressEvent {
                    bucket_label: payload.bucket_label,
                    dataset_role: payload.dataset_role,
                    metric_id: payload.metric_id,
                    metric_name: payload.metric_name,
                    completed_units,
                    total_units,
                });
            }
            TailSkewDiagnosticsWorkerMessage::Completed(result) => {
                diagnostics_result = Some(result);
                break;
            }
        }
    }

    worker.join().map_err(
        |_| TailSkewDiagnosticsError::InvalidParallelismConfiguration {
            details: "tail/skew diagnostics worker thread panicked".to_string(),
        },
    )?;

    diagnostics_result.ok_or_else(
        || TailSkewDiagnosticsError::InvalidParallelismConfiguration {
            details: "tail/skew diagnostics worker exited without a result".to_string(),
        },
    )?
}

#[derive(Debug)]
pub(super) enum TailSkewDiagnosticsWorkerMessage {
    DatasetMetricCompleted(TailSkewDatasetMetricProgressPayload),
    Completed(Result<TailSkewDiagnosticsReport, TailSkewDiagnosticsError>),
}

#[derive(Debug)]
pub(super) struct TailSkewDatasetMetricProgressPayload {
    bucket_label: String,
    dataset_role: TailSkewDatasetRole,
    metric_id: &'static str,
    metric_name: &'static str,
}

pub(super) fn emit_dataset_metric_completed(
    sender: Option<&mpsc::Sender<TailSkewDiagnosticsWorkerMessage>>,
    horizon_label: &str,
    dataset_role: TailSkewDatasetRole,
    metric_specification: TailSkewMetricSpecification,
) {
    let Some(sender) = sender else {
        return;
    };

    let _ = sender.send(TailSkewDiagnosticsWorkerMessage::DatasetMetricCompleted(
        TailSkewDatasetMetricProgressPayload {
            bucket_label: horizon_label.to_string(),
            dataset_role,
            metric_id: metric_specification.metric_id,
            metric_name: metric_specification.metric_name,
        },
    ));
}

fn diagnostics_progress_total_units(
    horizon_count: usize,
) -> Result<usize, TailSkewDiagnosticsError> {
    horizon_count
        .checked_mul(TAIL_SKEW_DATASET_METRIC_TASK_COUNT)
        .ok_or_else(
            || TailSkewDiagnosticsError::InvalidParallelismConfiguration {
                details: "tail/skew progress unit count does not fit in usize".to_string(),
            },
        )
}
