use std::path::Path;

use serde_json::Value;
use serde_json::json;

use super::preparation::{PreparedFitPreview, fit_warnings};

pub(crate) const FIT_PLAN_SCHEMA_VERSION: &str = "cli.fit_plan.v1";
pub(crate) const FIT_PLAN_COMMAND_ID: &str = "fit.plan";
pub(crate) const FIT_INSPECT_SCHEMA_VERSION: &str = "cli.fit_inspect.v1";
pub(crate) const FIT_INSPECT_COMMAND_ID: &str = "fit.inspect";
pub(crate) const FIT_STAGED_PLAN_SCHEMA_VERSION: &str = "cli.fit_staged_plan.v1";
pub(crate) const FIT_STAGED_PLAN_COMMAND_ID: &str = "fit.staged.plan";

pub(crate) fn plan_json(
    prepared: &PreparedFitPreview,
    staged_artifact_path: Option<&Path>,
) -> Value {
    json!({
        "schema_version": FIT_PLAN_SCHEMA_VERSION,
        "command_id": FIT_PLAN_COMMAND_ID,
        "run_id": prepared.request.run_id,
        "workflow_scope": prepared.request.workflow_scope,
        "input_files": input_files_json(prepared),
        "market_profile": market_profile_json(prepared),
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
        "schema_version": FIT_INSPECT_SCHEMA_VERSION,
        "command_id": FIT_INSPECT_COMMAND_ID,
        "run_id": prepared.request.run_id,
        "explicit_user_inputs": pairs_json(&prepared.explicit_values),
        "inferred_values": pairs_json(&prepared.inferred_values),
        "preset_defaults": pairs_json(&prepared.preset_defaults),
        "recommended_defaults": pairs_json(&prepared.recommended_defaults),
        "derived_values": pairs_json(&prepared.derived_values),
        "market_profile": market_profile_json(prepared),
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
        "schema_version": FIT_STAGED_PLAN_SCHEMA_VERSION,
        "command_id": FIT_STAGED_PLAN_COMMAND_ID,
        "run_id": prepared.request.run_id,
        "preparation_request": prepared.request,
        "market_profile": market_profile_json(prepared),
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

fn market_profile_json(prepared: &PreparedFitPreview) -> Value {
    match &prepared.inputs.market_profile {
        Some(market_profile) => json!({
            "source": "bundled_inference",
            "profile_name": market_profile.profile_name,
            "session_preset_name": market_profile.session_preset_name,
            "known_news_preset_name": market_profile.known_news_preset_name,
            "known_news_policy": market_profile.known_news_policy,
        }),
        None => json!({
            "source": "not_selected"
        }),
    }
}

fn pairs_json(values: &[(String, String)]) -> Value {
    values
        .iter()
        .map(|(label, value)| json!({"label": label, "value": value}))
        .collect()
}
