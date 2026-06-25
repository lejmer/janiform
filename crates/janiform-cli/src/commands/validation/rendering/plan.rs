use std::path::Path;

use janiform_foundations::paths::format_path;
use janiform_terminal_ui::{
    KeyValueRow, StageChecklist, StageChecklistItem, StageChecklistStatus, StartSummary,
    TerminalRenderer, format_count,
};
use janiform_workflows::validation::{
    StylizedFactsValidationDecisionMode, StylizedFactsValidationExecutionMode,
    StylizedFactsValidationPlan, StylizedFactsValidationProgressPhase,
    stylized_facts_validation_phase_label,
};

use crate::commands::shared::output::write_json_payload;
use crate::commands::shared::terminal::write_terminal_output;
use crate::context::CommandContext;
use crate::context::OutputMode;
use crate::error::CliError;

pub(crate) fn emit_stylized_facts_validation_plan(
    context: &CommandContext,
    plan: &StylizedFactsValidationPlan,
) -> Result<(), CliError> {
    match context.output_mode() {
        OutputMode::Json => write_json_payload(plan),
        OutputMode::Terminal | OutputMode::Quiet => write_terminal_output(
            context,
            &format!("{}\n", render_plan_human(plan, &context.stdout_renderer())),
        ),
        OutputMode::Silent => Ok(()),
    }
}

pub(crate) fn render_plan_human(
    plan: &StylizedFactsValidationPlan,
    renderer: &TerminalRenderer,
) -> String {
    let mut sections = vec![
        StartSummary {
            title: "Janiform stylized-facts validation plan".to_string(),
            rows: vec![
                KeyValueRow::new("Mode", execution_mode_label(plan.execution_mode)),
                KeyValueRow::new(
                    "Model",
                    plan.model_path
                        .as_deref()
                        .map(path_label)
                        .unwrap_or_else(|| "not used".to_string()),
                ),
                KeyValueRow::new(
                    "Empirical data",
                    plan.empirical_data_path
                        .as_deref()
                        .map(path_label)
                        .unwrap_or_else(|| "not used".to_string()),
                ),
                KeyValueRow::new("Output", path_label(&plan.output_dir)),
            ],
        }
        .render(renderer),
        StartSummary {
            title: "Inferred".to_string(),
            rows: vec![
                KeyValueRow::new(
                    "Empirical format",
                    plan.empirical_data_format.as_deref().unwrap_or("not used"),
                ),
                KeyValueRow::new(
                    "Timestamp column",
                    plan.empirical_timestamp_column
                        .as_deref()
                        .unwrap_or("not used"),
                ),
                KeyValueRow::new(
                    "Price column",
                    plan.empirical_price_column.as_deref().unwrap_or("not used"),
                ),
                KeyValueRow::new(
                    "Empirical span",
                    match (plan.empirical_start_utc_nanos, plan.empirical_end_utc_nanos) {
                        (Some(start), Some(end)) => format!("{start}..{end}"),
                        _ => "not used".to_string(),
                    },
                ),
                KeyValueRow::new(
                    "Intervals",
                    format_count(plan.intervals.len(), "interval", "intervals"),
                ),
                KeyValueRow::new("Simulation paths", plan.simulation_path_count.to_string()),
                KeyValueRow::new("Base seed", plan.seed.to_string()),
                KeyValueRow::new("CPU count", plan.cpu_count.to_string()),
            ],
        }
        .render(renderer),
        interval_summary(plan, renderer),
        computation_checklist(plan).render(renderer),
        StartSummary {
            title: "Decision policy".to_string(),
            rows: vec![KeyValueRow::new(
                "Mode",
                decision_mode_label(plan.decision_mode),
            )],
        }
        .render(renderer),
        artifact_summary("Report artifacts", &plan.report_artifacts, renderer),
    ];

    if !plan.cache_artifacts.is_empty() {
        sections.push(artifact_summary(
            "Cache artifacts",
            &plan.cache_artifacts,
            renderer,
        ));
    }

    sections.join("\n\n")
}

fn interval_summary(plan: &StylizedFactsValidationPlan, renderer: &TerminalRenderer) -> String {
    let rows = plan
        .intervals
        .iter()
        .map(|interval| {
            KeyValueRow::new(
                interval.label.as_str(),
                match interval.interval_count {
                    Some(interval_count) => format!(
                        "{} ns, {}",
                        interval.duration_nanos,
                        format_count(interval_count, "bucket", "buckets")
                    ),
                    None => format!("{} ns", interval.duration_nanos),
                },
            )
        })
        .collect();

    StartSummary {
        title: "Return intervals".to_string(),
        rows,
    }
    .render(renderer)
}

fn computation_checklist(plan: &StylizedFactsValidationPlan) -> StageChecklist {
    let simulation_label = format!(
        "{} ({})",
        stylized_facts_validation_phase_label(
            StylizedFactsValidationProgressPhase::SimulateSeededPath
        ),
        format_count(plan.simulation_path_count, "path", "paths")
    );

    StageChecklist {
        title: "Computation".to_string(),
        stages: vec![
            StageChecklistItem {
                name: stylized_facts_validation_phase_label(
                    StylizedFactsValidationProgressPhase::LoadEmpiricalData,
                )
                .to_string(),
                status: StageChecklistStatus::Pending,
            },
            StageChecklistItem {
                name: stylized_facts_validation_phase_label(
                    StylizedFactsValidationProgressPhase::DeriveIntervalReturns,
                )
                .to_string(),
                status: StageChecklistStatus::Pending,
            },
            StageChecklistItem {
                name: simulation_label,
                status: StageChecklistStatus::Pending,
            },
            StageChecklistItem {
                name: stylized_facts_validation_phase_label(
                    StylizedFactsValidationProgressPhase::ComputeMetrics,
                )
                .to_string(),
                status: StageChecklistStatus::Pending,
            },
            StageChecklistItem {
                name: "Compare empirical and simulated metrics".to_string(),
                status: StageChecklistStatus::Pending,
            },
            StageChecklistItem {
                name: "Write validation report".to_string(),
                status: StageChecklistStatus::Pending,
            },
        ],
    }
}

fn artifact_summary(title: &str, artifacts: &[String], renderer: &TerminalRenderer) -> String {
    StartSummary {
        title: title.to_string(),
        rows: artifacts
            .iter()
            .map(|artifact| KeyValueRow::new("Path", path_label(artifact)))
            .collect(),
    }
    .render(renderer)
}

fn decision_mode_label(mode: StylizedFactsValidationDecisionMode) -> &'static str {
    match mode {
        StylizedFactsValidationDecisionMode::ThresholdPolicy => "threshold policy",
        StylizedFactsValidationDecisionMode::StatisticalBootstrap => "statistical bootstrap tests",
    }
}

fn execution_mode_label(mode: StylizedFactsValidationExecutionMode) -> &'static str {
    match mode {
        StylizedFactsValidationExecutionMode::RawData => "raw data",
        StylizedFactsValidationExecutionMode::PrecomputedComparison => "precomputed comparison",
    }
}

fn path_label(path: &str) -> String {
    format_path(Path::new(path))
}
