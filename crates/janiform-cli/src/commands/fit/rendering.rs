use std::path::Path;

use janiform::calibration::CalibrationExecutionMode;
use janiform_cli_ui::{
    ArtifactPathGroup, KeyValueRow, StageChecklist, StageChecklistItem, StageChecklistStatus,
    StartSummary, WarningBlock, format_artifact_path, format_path,
};

use super::labels::{
    market_data_file_format_label, optional_label, stage_label, workflow_scope_label,
};
use super::preparation::{PreparedFitPreview, fit_warnings};
use crate::context::CommandContext;

pub(crate) fn render_plan_human(
    prepared: &PreparedFitPreview,
    staged_artifact_path: Option<&Path>,
    context: &CommandContext,
) -> String {
    let renderer = context.stdout_renderer();
    let warnings = fit_warnings(prepared);

    let mut sections = Vec::new();

    if !warnings.is_empty() {
        sections.push(
            WarningBlock {
                title: format!("Warnings: {}", warnings.len()),
                warnings,
            }
            .render(&renderer),
        );
    }

    sections.push(
        StartSummary {
            title: "Janiform fit plan".to_string(),
            rows: vec![
                KeyValueRow::new("Run", prepared.request.run_id.clone()),
                KeyValueRow::new(
                    "Scope",
                    workflow_scope_label(prepared.request.workflow_scope),
                ),
                KeyValueRow::new("Ticks", format_path(&prepared.inputs.tick_data_path)),
                KeyValueRow::new(
                    "Options",
                    prepared
                        .inputs
                        .option_surface_path
                        .as_deref()
                        .map(format_path)
                        .unwrap_or_else(|| "none".to_string()),
                ),
                KeyValueRow::new(
                    "Known news",
                    prepared
                        .inputs
                        .known_news_path
                        .as_deref()
                        .map(format_path)
                        .unwrap_or_else(|| "none".to_string()),
                ),
            ],
        }
        .render(&renderer),
    );

    sections.push(inferred_summary(prepared).render(&renderer));
    sections.push(stage_checklist(prepared).render(&renderer));
    sections.push(output_paths(prepared, staged_artifact_path).render(&renderer));

    sections.join("\n\n")
}

pub(crate) fn render_inspect_human(
    prepared: &PreparedFitPreview,
    context: &CommandContext,
) -> String {
    let renderer = context.stdout_renderer();

    let mut sections = vec![
        StartSummary {
            title: "Janiform fit inspect".to_string(),
            rows: vec![
                KeyValueRow::new("Run", prepared.request.run_id.clone()),
                KeyValueRow::new("Ticks", format_path(&prepared.inputs.tick_data_path)),
            ],
        }
        .render(&renderer),

        value_section("Explicit user inputs", &prepared.explicit_values).render(&renderer),
        value_section("Inferred values", &prepared.inferred_values).render(&renderer),
        value_section("Preset defaults", &prepared.preset_defaults).render(&renderer),
        value_section("Recommended defaults", &prepared.recommended_defaults).render(&renderer),
        value_section("Derived values", &prepared.derived_values).render(&renderer),

        session_summary(prepared).render(&renderer),
    ];

    let warnings = fit_warnings(prepared);
    if !warnings.is_empty() {
        sections.insert(
            1,
            WarningBlock {
                title: format!("Warnings: {}", warnings.len()),
                warnings,
            }
            .render(&renderer),
        );
    }

    sections.join("\n\n")
}

pub(crate) fn render_staged_plan_human(
    prepared: &PreparedFitPreview,
    staged_artifact_path: Option<&Path>,
    context: &CommandContext,
) -> String {
    let renderer = context.stdout_renderer();

    vec![
        StartSummary {
            title: "Janiform fit staged plan".to_string(),
            rows: vec![
                KeyValueRow::new("Run", prepared.request.run_id.clone()),
                KeyValueRow::new(
                    "Scope",
                    workflow_scope_label(prepared.request.workflow_scope),
                ),
                KeyValueRow::new(
                    "Execution input",
                    staged_artifact_path
                        .map(format_path)
                        .unwrap_or_else(|| "not written".to_string()),
                ),
            ],
        }
        .render(&renderer),
        stage_checklist(prepared).render(&renderer),
    ]
    .join("\n\n")
}

fn inferred_summary(prepared: &PreparedFitPreview) -> StartSummary {
    StartSummary {
        title: "Inferred".to_string(),
        rows: vec![
            KeyValueRow::new(
                "Tick format",
                market_data_file_format_label(prepared.request.tick_data_source.file_format),
            ),
            KeyValueRow::new(
                "Timestamp",
                optional_label(
                    prepared
                        .request
                        .column_mapping
                        .tick_data
                        .timestamp_column
                        .as_deref(),
                ),
            ),
            KeyValueRow::new(
                "Price",
                optional_label(
                    prepared
                        .request
                        .column_mapping
                        .tick_data
                        .price_column
                        .as_deref(),
                ),
            ),
            KeyValueRow::new(
                "Trade sign",
                optional_label(
                    prepared
                        .request
                        .column_mapping
                        .tick_data
                        .trade_sign_column
                        .as_deref(),
                ),
            ),
            KeyValueRow::new(
                "Session",
                format!(
                    "{}, {}",
                    prepared.plan.session_resolution.exchange_name,
                    prepared.plan.session_resolution.resolved_timezone_name
                ),
            ),
        ],
    }
}

fn session_summary(prepared: &PreparedFitPreview) -> StartSummary {
    StartSummary {
        title: "Session".to_string(),
        rows: vec![
            KeyValueRow::new(
                "Exchange",
                prepared.plan.session_resolution.exchange_code.clone(),
            ),
            KeyValueRow::new(
                "Timezone",
                prepared
                    .plan
                    .session_resolution
                    .resolved_timezone_name
                    .clone(),
            ),
            KeyValueRow::new(
                "Source",
                prepared.plan.session_resolution.provenance_kind.clone(),
            ),
            KeyValueRow::new(
                "Confidence",
                prepared.plan.session_resolution.confidence_label.clone(),
            ),
        ],
    }
}

fn value_section(title: &str, values: &[(String, String)]) -> StartSummary {
    let rows = if values.is_empty() {
        vec![KeyValueRow::new("None", "none")]
    } else {
        values
            .iter()
            .map(|(label, value)| KeyValueRow::new(label.clone(), value.clone()))
            .collect()
    };

    StartSummary {
        title: title.to_string(),
        rows,
    }
}

fn stage_checklist(prepared: &PreparedFitPreview) -> StageChecklist {
    StageChecklist {
        title: "Stages".to_string(),
        stages: prepared
            .plan
            .diagnostics
            .selected_stages
            .iter()
            .map(|stage| StageChecklistItem {
                name: stage_label(*stage).to_string(),
                status: StageChecklistStatus::Completed,
            })
            .collect(),
    }
}

fn output_paths(
    prepared: &PreparedFitPreview,
    staged_artifact_path: Option<&Path>,
) -> ArtifactPathGroup {
    let mut paths = vec![
        format!(
            "run: {}",
            format_artifact_path(&prepared.paths.output_directory)
        ),
        format!(
            "model: {}",
            format_artifact_path(&prepared.paths.model_path)
        ),
        format!(
            "report: {}",
            format_artifact_path(&prepared.paths.report_path)
        ),
        format!(
            "staged artifacts: {}",
            staged_artifact_path
                .map(format_path)
                .unwrap_or_else(|| "not written".to_string())
        ),
    ];

    if prepared.request.output_preferences.execution_mode == CalibrationExecutionMode::Disk {
        paths.push(format!(
            "checkpoints: {}",
            format_artifact_path(&prepared.paths.checkpoint_directory)
        ));
    }

    ArtifactPathGroup {
        title: "Outputs".to_string(),
        paths,
    }
}
