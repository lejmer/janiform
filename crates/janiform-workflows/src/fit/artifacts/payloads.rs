use janiform_calibration_runtime::{CalibrationStagedRunReport, CalibrationStagedRunStageReport};
use serde::Serialize;
use serde_json::Value;
use serde_json::json;

use crate::error::WorkflowError;

pub(super) fn stage_report_payloads(
    report: &CalibrationStagedRunReport,
) -> Result<Vec<Value>, WorkflowError> {
    report
        .stage_reports
        .iter()
        .map(stage_report_payload)
        .collect()
}

pub(super) fn seasonality_parameter_payload(
    payload: &janiform_calibration_runtime::SeasonalityStageNormalizedPayload,
) -> Value {
    json!({
        "schema_version": payload.composition_schema_version,
        "exchange_code": payload.exchange_code,
        "intraday_minutes_per_bin": payload.intraday_minutes_per_bin,
        "intraday_factor_by_bin": payload.intraday_factor_by_bin,
        "day_of_week_factor": payload.day_of_week_factor,
        "hour_of_week_factor": payload.hour_of_week_factor,
        "day_of_year_factor": payload.day_of_year_factor,
    })
}

fn stage_report_payload(
    stage_report: &CalibrationStagedRunStageReport,
) -> Result<Value, WorkflowError> {
    match stage_report {
        CalibrationStagedRunStageReport::Seasonality(report) => {
            seasonality_stage_report_payload(report)
        }
        CalibrationStagedRunStageReport::Hawkes(report) => {
            stage_report_payload_from_report("hawkes", report)
        }
        CalibrationStagedRunStageReport::ReturnComponents(report) => {
            stage_report_payload_from_report("return_components", report)
        }
        CalibrationStagedRunStageReport::LongVariance(report) => {
            stage_report_payload_from_report("long_variance", report)
        }
        CalibrationStagedRunStageReport::MeanReversion(report) => {
            stage_report_payload_from_report("mean_reversion", report)
        }
        CalibrationStagedRunStageReport::JointRefinement(report) => {
            stage_report_payload_from_report("joint_refinement", report)
        }
        CalibrationStagedRunStageReport::OptionRefinement(report) => {
            stage_report_payload_from_report("option_refinement", report)
        }
        CalibrationStagedRunStageReport::Finalization(report) => {
            stage_report_payload_from_report("finalization", report)
        }
    }
}

fn seasonality_stage_report_payload(
    report: &janiform_calibration_runtime::SeasonalityStageExecutionReport,
) -> Result<Value, WorkflowError> {
    let normalized_stage_payload = report
        .normalized_stage_payload
        .as_ref()
        .map(seasonality_parameter_payload)
        .unwrap_or(Value::Null);

    Ok(json!({
        "stage_report_kind": "seasonality",
        "schema_version": report.schema_version,
        "stage": report.stage,
        "status": report.status,
        "run_id": report.run_id,
        "execution_mode": report.execution_mode,
        "handoff_policy": report.handoff_policy,
        "handoffs": report.handoffs,
        "normalized_stage_payload": normalized_stage_payload,
        "metadata": report.metadata,
        "failure": report.failure,
    }))
}

fn stage_report_payload_from_report<T>(
    stage_report_kind: &str,
    report: &T,
) -> Result<Value, WorkflowError>
where
    T: Serialize,
{
    let mut payload =
        serde_json::to_value(report).map_err(|source| WorkflowError::OutputSerialize { source })?;
    let fields = payload
        .as_object_mut()
        .ok_or_else(|| WorkflowError::ModelValidationFailed {
            field: "fit_report.staged_response.stage_reports",
            message: "serialized stage report must be a JSON object".to_string(),
        })?;

    fields.insert(
        "stage_report_kind".to_string(),
        Value::String(stage_report_kind.to_string()),
    );

    Ok(payload)
}
