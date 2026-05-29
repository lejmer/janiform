use janiform_event_log::RunStateSnapshot;
use janiform_terminal_ui::KeyValueRow;
use janiform_terminal_ui::StartSummary;
use janiform_terminal_ui::TerminalRenderer;
use janiform_terminal_ui::WarningBlock;
use janiform_terminal_ui::format_elapsed_label;
use janiform_terminal_ui::format_optional_path_relative_to;
use janiform_terminal_ui::format_path_relative_to;
use janiform_terminal_ui::format_token_as_words;
use serde_json::Value;

use crate::commands::shared::terminal::ArtifactListItem;
use crate::commands::shared::terminal::render_artifact_list;
use janiform_foundations::json::display::compact_value_string;
use janiform_foundations::json::query::nested_string;
use janiform_foundations::json::query::nested_value;
use janiform_foundations::paths::resolve_path_relative_to;
use janiform_workflows::fit::stage_label_from_token;

use super::FitReportContext;
use super::timing::stage_duration;

pub(super) fn render_calibration_outcomes(
    report: &Value,
    model: Option<&Value>,
    run_state: Option<&RunStateSnapshot>,
    renderer: &TerminalRenderer,
) -> String {
    let mut rows = stage_status_rows(report, model, run_state);

    if rows.is_empty() {
        rows.push(KeyValueRow::new("Stages", "not recorded"));
    }

    StartSummary {
        title: "Calibration outcomes".to_string(),
        rows,
    }
    .render(renderer)
}

pub(super) fn render_selected_candidates(
    model: Option<&Value>,
    renderer: &TerminalRenderer,
) -> Option<String> {
    let selected = model
        .and_then(|model| {
            nested_value(
                model,
                &[
                    "metadata",
                    "fitted_parameter_summary",
                    "selected_candidate_ids",
                ],
            )
        })
        .and_then(Value::as_object)?;

    if selected.is_empty() {
        return None;
    }

    let rows = selected
        .iter()
        .map(|(stage, candidate)| {
            KeyValueRow::new(
                stage_label_from_token(stage),
                compact_value_string(candidate),
            )
        })
        .collect();

    Some(
        StartSummary {
            title: "Selected candidates".to_string(),
            rows,
        }
        .render(renderer),
    )
}

pub(super) fn render_diagnostics(
    report: &Value,
    report_context: &FitReportContext,
    renderer: &TerminalRenderer,
) -> Option<String> {
    let references = nested_value(
        report,
        &["staged_response", "diagnostic_artifact_references"],
    )
    .and_then(Value::as_array)?;

    if references.is_empty() {
        return None;
    }

    let rows = references
        .iter()
        .filter_map(|reference| {
            let family = reference.get("artifact_family").and_then(Value::as_str)?;
            let relative_path = reference.get("relative_path").and_then(Value::as_str);
            let value = relative_path
                .and_then(|relative_path| {
                    report_context
                        .run_directory
                        .as_deref()
                        .map(|run_directory| {
                            format_path_relative_to(
                                run_directory,
                                &resolve_path_relative_to(run_directory, relative_path),
                            )
                        })
                })
                .unwrap_or_else(|| relative_path.unwrap_or("path not recorded").to_string());

            Some(KeyValueRow::new(format_token_as_words(family), value))
        })
        .collect::<Vec<_>>();

    if rows.is_empty() {
        return None;
    }

    Some(
        StartSummary {
            title: "Diagnostics".to_string(),
            rows,
        }
        .render(renderer),
    )
}

pub(super) fn render_artifacts(
    report_context: &FitReportContext,
    renderer: &TerminalRenderer,
) -> Option<String> {
    let run_directory = report_context.run_directory.as_deref();

    let mut artifacts = Vec::new();

    let report_path = match run_directory {
        Some(run_directory) => format_path_relative_to(run_directory, &report_context.report_path),
        None => report_context.report_path.display().to_string(),
    };

    artifacts.push(ArtifactListItem::new("report", report_path));

    if let Some(model_path) = &report_context.model_path
        && model_path.is_file()
    {
        let path = match run_directory {
            Some(run_directory) => format_path_relative_to(run_directory, model_path),
            None => model_path.display().to_string(),
        };

        artifacts.push(ArtifactListItem::new("model", path));
    }

    render_artifact_list("Artifacts", artifacts, renderer)
}

pub(super) fn render_resume(
    report: &Value,
    run_state: Option<&RunStateSnapshot>,
    report_context: &FitReportContext,
    renderer: &TerminalRenderer,
) -> Option<String> {
    let checkpoint_path = run_state
        .and_then(|state| state.checkpoints.latest_path.as_deref())
        .and_then(|path| {
            report_context
                .run_directory
                .as_deref()
                .and_then(|run_directory| {
                    format_optional_path_relative_to(run_directory, Some(path))
                })
                .or_else(|| Some(path.to_string()))
        });

    let checkpoint_reference = latest_checkpoint_reference(report).or_else(|| {
        nested_string(
            report,
            &["staged_response", "resumed_from_checkpoint_reference"],
        )
        .map(str::to_string)
    });

    if checkpoint_path.is_none() && checkpoint_reference.is_none() {
        return None;
    }

    let mut rows = Vec::new();

    if let Some(checkpoint_path) = checkpoint_path {
        rows.push(KeyValueRow::new("Checkpoint", checkpoint_path));
    }

    if let Some(checkpoint_reference) = checkpoint_reference {
        rows.push(KeyValueRow::new("Reference", checkpoint_reference));
    }

    Some(
        StartSummary {
            title: "Resume".to_string(),
            rows,
        }
        .render(renderer),
    )
}

pub(super) fn render_warnings(
    run_state: Option<&RunStateSnapshot>,
    report: &Value,
) -> Option<WarningBlock> {
    let mut warnings = run_state
        .map(|state| {
            state
                .warnings
                .iter()
                .map(|warning| warning.reason.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if let Some(diagnostics) = nested_value(
        report,
        &["preparation", "resolved_configuration", "diagnostics"],
    )
    .and_then(Value::as_array)
    {
        warnings.extend(diagnostics.iter().filter_map(|diagnostic| {
            let severity = diagnostic.get("severity").and_then(Value::as_str)?;
            if severity == "info" {
                return None;
            }

            diagnostic
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
        }));
    }

    if let Some(preparation_warnings) =
        nested_value(report, &["preparation", "warnings"]).and_then(Value::as_array)
    {
        warnings.extend(
            preparation_warnings
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string),
        );
    }

    if warnings.is_empty() {
        return None;
    }

    Some(WarningBlock {
        title: format!("Warnings: {}", warnings.len()),
        warnings,
    })
}

fn stage_status_rows(
    report: &Value,
    model: Option<&Value>,
    run_state: Option<&RunStateSnapshot>,
) -> Vec<KeyValueRow> {
    let statuses = nested_value(report, &["summary", "stage_statuses"])
        .and_then(Value::as_array)
        .or_else(|| {
            nested_value(report, &["staged_response", "stage_reports"]).and_then(Value::as_array)
        });

    let Some(statuses) = statuses else {
        return Vec::new();
    };

    statuses
        .iter()
        .filter_map(|stage| {
            let stage_id = stage.get("stage").and_then(Value::as_str)?;

            let status = stage
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");

            let duration = run_state.and_then(|state| stage_duration(state, stage_id));

            let selected_candidate = selected_candidate_for_stage(model, stage_id);

            let mut details = vec![format_token_as_words(status)];

            if let Some(duration) = duration {
                details.push(format_elapsed_label(duration));
            }

            if let Some(selected_candidate) = selected_candidate {
                details.push(format!("selected {selected_candidate}"));
            }

            Some(KeyValueRow::new(
                stage_label_from_token(stage_id),
                details.join(", "),
            ))
        })
        .collect()
}

fn selected_candidate_for_stage(model: Option<&Value>, stage_id: &str) -> Option<String> {
    model
        .and_then(|model| {
            nested_value(
                model,
                &[
                    "metadata",
                    "fitted_parameter_summary",
                    "selected_candidate_ids",
                    stage_id,
                ],
            )
        })
        .map(compact_value_string)
}

fn latest_checkpoint_reference(report: &Value) -> Option<String> {
    nested_value(report, &["staged_response", "emitted_checkpoints"])
        .and_then(Value::as_array)
        .and_then(|checkpoints| checkpoints.last())
        .and_then(|checkpoint| checkpoint.get("checkpoint_reference"))
        .and_then(Value::as_str)
        .map(str::to_string)
}
