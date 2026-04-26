use std::path::Path;

use serde_json::Value;
use serde_json::json;

use super::preparation::{PreparedFitPreview, fit_warnings};

pub(crate) fn plan_json(
    prepared: &PreparedFitPreview,
    staged_artifact_path: Option<&Path>,
) -> Value {
    json!({
        "schema_version": "cli.fit_plan.v1",
        "command_id": "fit.plan",
        "run_id": prepared.request.run_id,
        "workflow_scope": prepared.request.workflow_scope,
        "input_files": input_files_json(prepared),
        "column_mapping": prepared.request.column_mapping,
        "normalization_policy": prepared.request.normalization_policy,
        "session_resolution": prepared.plan.session_resolution,
        "diagnostics": prepared.plan.diagnostics,
        "provenance": prepared.plan.provenance,
        "outputs": outputs_json(prepared, staged_artifact_path),
        "warnings": fit_warnings(prepared),
        "staged_artifacts_written": staged_artifact_path.is_some(),
    })
}

pub(crate) fn inspect_json(prepared: &PreparedFitPreview) -> Value {
    json!({
        "schema_version": "cli.fit_inspect.v1",
        "command_id": "fit.inspect",
        "run_id": prepared.request.run_id,
        "explicit_user_inputs": pairs_json(&prepared.explicit_values),
        "inferred_values": pairs_json(&prepared.inferred_values),
        "preset_defaults": pairs_json(&prepared.preset_defaults),
        "recommended_defaults": pairs_json(&prepared.recommended_defaults),
        "derived_values": pairs_json(&prepared.derived_values),
        "market_profile": {
            "source": "not_selected"
        },
        "session_resolution": prepared.plan.session_resolution,
        "tick_source_details": prepared.plan.provenance.tick_data_summary,
        "output_directory": prepared.paths.output_directory,
        "model_path": prepared.paths.model_path,
        "report_path": prepared.paths.report_path,
        "warnings": fit_warnings(prepared),
    })
}

pub(crate) fn staged_plan_json(
    prepared: &PreparedFitPreview,
    staged_artifact_path: Option<&Path>,
) -> Value {
    json!({
        "schema_version": "cli.fit_staged_plan.v1",
        "command_id": "fit.staged.plan",
        "run_id": prepared.request.run_id,
        "preparation_request": prepared.request,
        "staged_execution_input": prepared.plan.execution_input,
        "diagnostics": prepared.plan.diagnostics,
        "provenance": prepared.plan.provenance,
        "session_resolution": prepared.plan.session_resolution,
        "outputs": outputs_json(prepared, staged_artifact_path),
        "staged_artifacts_written": staged_artifact_path.is_some(),
    })
}

fn input_files_json(prepared: &PreparedFitPreview) -> Value {
    json!({
        "ticks": prepared.request.tick_data_source,
        "options": prepared.request.option_surface_source,
        "known_news": prepared.request.known_news_inputs.as_ref().map(|input| &input.calendar_source),
        "session_definition": prepared.inputs.session_definition_path,
    })
}

fn outputs_json(prepared: &PreparedFitPreview, staged_artifact_path: Option<&Path>) -> Value {
    json!({
        "run_directory": prepared.paths.output_directory,
        "model": prepared.paths.model_path,
        "report": prepared.paths.report_path,
        "checkpoint_directory": prepared.paths.checkpoint_directory,
        "staged_execution_input": staged_artifact_path,
    })
}

fn pairs_json(values: &[(String, String)]) -> Value {
    values
        .iter()
        .map(|(label, value)| json!({"label": label, "value": value}))
        .collect()
}
