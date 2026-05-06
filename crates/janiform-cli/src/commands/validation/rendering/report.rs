use std::cmp::Ordering;
use std::path::Path;

use clap::ValueEnum;
use janiform::calibration::artifacts::CalibrationExecutionMode;
use janiform::validation::stylized_facts::{
    StylizedFactComparisonDecisionState, StylizedFactDecisionReasonCode,
    StylizedFactHorizonComparisonReportBundle, StylizedFactHorizonComparisonTableSummaryRow,
    StylizedFactMetricClass,
};
use janiform_terminal_ui::{
    AnalyticalResultTable, ArtifactPathGroup, CompactTable, KeyValueRow, StartSummary,
    TerminalRenderer, format_artifact_path, format_count,
};

use crate::commands::shared::output::write_json_payload;
use crate::commands::shared::terminal::write_terminal_output;
use crate::context::CommandContext;
use crate::context::OutputMode;
use crate::error::CliError;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum StylizedFactCheckDisplay {
    #[value(help = "Show summary counts and a compact set of notable checks.")]
    Summary,
    #[value(help = "Show failed checks.")]
    Failed,
    #[value(help = "Show checks closest to a threshold.")]
    Closest,
    #[value(help = "Show all checks up to --max-checks.")]
    All,
}

pub(crate) fn emit_stylized_fact_validation_report_output(
    context: &CommandContext,
    report_bundle: &StylizedFactHorizonComparisonReportBundle,
    execution_mode: CalibrationExecutionMode,
    check_display: StylizedFactCheckDisplay,
    max_checks: usize,
) -> Result<(), CliError> {
    match context.output_mode() {
        OutputMode::Json => write_json_payload(report_bundle),
        OutputMode::Human => write_terminal_output(
            context,
            &render_human_report(
                report_bundle,
                execution_mode,
                check_display,
                max_checks,
                &context.stdout_renderer(),
            ),
        ),
        OutputMode::Quiet => write_terminal_output(
            context,
            &render_quiet_report(report_bundle, execution_mode, &context.stdout_renderer()),
        ),
        OutputMode::Silent => Ok(()),
    }
}

fn render_human_report(
    report_bundle: &StylizedFactHorizonComparisonReportBundle,
    execution_mode: CalibrationExecutionMode,
    check_display: StylizedFactCheckDisplay,
    max_checks: usize,
    renderer: &TerminalRenderer,
) -> String {
    let comparison_report = &report_bundle.comparison_report;
    let summary = comparison_report.summary;

    let mut sections = vec![
        StartSummary {
            title: "Stylized-fact interval comparison".to_string(),
            rows: vec![
                KeyValueRow::new("Status", decision_state_label(comparison_report.status)),
                KeyValueRow::new("Execution mode", execution_mode_label(execution_mode)),
                KeyValueRow::new("Metric catalog", &comparison_report.metric_catalog_id),
                KeyValueRow::new("Runs", summary.total_run_count.to_string()),
                KeyValueRow::new("Metric checks", summary.total_metric_count.to_string()),
                KeyValueRow::new(
                    "Passed",
                    summary.metric_decision_counts.pass_count.to_string(),
                ),
                KeyValueRow::new(
                    "Failed",
                    summary.metric_decision_counts.fail_count.to_string(),
                ),
                KeyValueRow::new(
                    "Not enough data",
                    summary
                        .metric_decision_counts
                        .insufficient_data_count
                        .to_string(),
                ),
            ],
        }
        .render(renderer),
    ];

    if summary.metric_decision_counts.fail_count > 0 {
        sections.push(render_failure_counts_by_interval(report_bundle, renderer));
        sections.push(render_failure_counts_by_metric_class(
            report_bundle,
            renderer,
        ));
    }

    if summary.metric_decision_counts.fail_count == 0
        && summary.metric_decision_counts.insufficient_data_count > 0
    {
        sections.push(
            StartSummary {
                title: "Coverage notes".to_string(),
                rows: vec![KeyValueRow::new(
                    "Not enough data",
                    format_count(
                        summary.metric_decision_counts.insufficient_data_count,
                        "metric check",
                        "metric checks",
                    ),
                )],
            }
            .render(renderer),
        );
    }

    for check_section in render_check_sections(report_bundle, check_display, max_checks, renderer) {
        sections.push(check_section);
    }

    if let Some(artifact_export) = &report_bundle.artifact_export {
        sections.push(
            ArtifactPathGroup {
                title: "Artifact output".to_string(),
                paths: artifact_export
                    .exported_files
                    .iter()
                    .map(|file| format_artifact_path(Path::new(file.absolute_path.as_str())))
                    .collect(),
            }
            .render(renderer),
        );
    }

    format!("{}\n", sections.join("\n\n"))
}

fn render_quiet_report(
    report_bundle: &StylizedFactHorizonComparisonReportBundle,
    execution_mode: CalibrationExecutionMode,
    renderer: &TerminalRenderer,
) -> String {
    let comparison_report = &report_bundle.comparison_report;
    let summary = comparison_report.summary;

    let mut rows = vec![
        KeyValueRow::new("Status", decision_state_label(comparison_report.status)),
        KeyValueRow::new("Execution mode", execution_mode_label(execution_mode)),
        KeyValueRow::new("Metric checks", summary.total_metric_count.to_string()),
        KeyValueRow::new(
            "Failed",
            summary.metric_decision_counts.fail_count.to_string(),
        ),
    ];

    if let Some(artifact_export) = &report_bundle.artifact_export {
        rows.push(KeyValueRow::new(
            "Artifact files",
            artifact_export.exported_files.len().to_string(),
        ));
    }

    format!(
        "{}\n",
        StartSummary {
            title: "Stylized-fact interval comparison".to_string(),
            rows,
        }
        .render(renderer)
    )
}

fn render_failure_counts_by_interval(
    report_bundle: &StylizedFactHorizonComparisonReportBundle,
    renderer: &TerminalRenderer,
) -> String {
    let mut rows: Vec<Vec<String>> = Vec::new();

    for run_report in &report_bundle.comparison_report.run_reports {
        for interval in &run_report.horizons {
            if interval.decision_counts.fail_count == 0 {
                continue;
            }

            rows.push(vec![
                run_report.calibration_run_id.clone(),
                interval.horizon_label.clone(),
                interval.decision_counts.fail_count.to_string(),
            ]);
        }
    }

    AnalyticalResultTable {
        title: "Failures by interval".to_string(),
        table: CompactTable {
            headers: vec![
                "Run".to_string(),
                "Interval".to_string(),
                "Failed".to_string(),
            ],
            rows,
        },
    }
    .render(renderer)
}

fn render_failure_counts_by_metric_class(
    report_bundle: &StylizedFactHorizonComparisonReportBundle,
    renderer: &TerminalRenderer,
) -> String {
    let mut primary_robust_count = 0usize;
    let mut secondary_moment_count = 0usize;

    for row in &report_bundle.table_summary.rows {
        if row.status != StylizedFactComparisonDecisionState::Fail {
            continue;
        }

        match row.metric_class {
            StylizedFactMetricClass::PrimaryRobust => primary_robust_count += 1,
            StylizedFactMetricClass::SecondaryMoment => secondary_moment_count += 1,
        }
    }

    AnalyticalResultTable {
        title: "Failures by metric class".to_string(),
        table: CompactTable {
            headers: vec!["Metric class".to_string(), "Failed".to_string()],
            rows: vec![
                vec![
                    metric_class_label(StylizedFactMetricClass::PrimaryRobust).to_string(),
                    primary_robust_count.to_string(),
                ],
                vec![
                    metric_class_label(StylizedFactMetricClass::SecondaryMoment).to_string(),
                    secondary_moment_count.to_string(),
                ],
            ],
        },
    }
    .render(renderer)
}

fn render_check_sections(
    report_bundle: &StylizedFactHorizonComparisonReportBundle,
    check_display: StylizedFactCheckDisplay,
    max_checks: usize,
    renderer: &TerminalRenderer,
) -> Vec<String> {
    match check_display {
        StylizedFactCheckDisplay::Summary => {
            let mut sections = Vec::new();

            if report_bundle
                .comparison_report
                .summary
                .metric_decision_counts
                .fail_count
                > 0
                && let Some(section) =
                    render_failed_check_section(report_bundle, max_checks, renderer)
            {
                sections.push(section);
            }

            if let Some(section) = render_passed_check_section(report_bundle, max_checks, renderer)
            {
                sections.push(section);
            }

            sections
        }
        StylizedFactCheckDisplay::Failed => {
            render_failed_check_section(report_bundle, max_checks, renderer)
                .into_iter()
                .collect()
        }
        StylizedFactCheckDisplay::Closest => {
            render_passed_check_section(report_bundle, max_checks, renderer)
                .into_iter()
                .collect()
        }
        StylizedFactCheckDisplay::All => {
            let mut sections = Vec::new();
            if let Some(section) = render_failed_check_section(report_bundle, max_checks, renderer)
            {
                sections.push(section);
            }
            if let Some(section) = render_passed_check_section(report_bundle, max_checks, renderer)
            {
                sections.push(section);
            }

            sections
        }
    }
}

fn render_failed_check_section(
    report_bundle: &StylizedFactHorizonComparisonReportBundle,
    max_checks: usize,
    renderer: &TerminalRenderer,
) -> Option<String> {
    let rows = report_bundle
        .table_summary
        .rows
        .iter()
        .filter(|row| row.status == StylizedFactComparisonDecisionState::Fail)
        .take(max_checks)
        .map(check_table_row)
        .collect::<Vec<_>>();

    if rows.is_empty() {
        return None;
    }

    Some(render_check_table("Failed checks", rows, renderer))
}

fn render_passed_check_section(
    report_bundle: &StylizedFactHorizonComparisonReportBundle,
    max_checks: usize,
    renderer: &TerminalRenderer,
) -> Option<String> {
    let mut rows = report_bundle
        .table_summary
        .rows
        .iter()
        .filter(|row| row.status == StylizedFactComparisonDecisionState::Pass)
        .collect::<Vec<_>>();

    rows.sort_by(compare_closeness);

    let table_rows = rows
        .into_iter()
        .take(max_checks)
        .map(check_table_row)
        .collect::<Vec<_>>();

    if table_rows.is_empty() {
        return None;
    }

    Some(render_check_table("Passed checks", table_rows, renderer))
}

fn render_check_table(title: &str, rows: Vec<Vec<String>>, renderer: &TerminalRenderer) -> String {
    AnalyticalResultTable {
        title: title.to_string(),
        table: CompactTable {
            headers: vec![
                "Run".to_string(),
                "Interval".to_string(),
                "Metric".to_string(),
                "Class".to_string(),
                "Absolute delta".to_string(),
                "Standardized delta".to_string(),
                "Reasons".to_string(),
            ],
            rows,
        },
    }
    .render(renderer)
}

fn check_table_row(row: &StylizedFactHorizonComparisonTableSummaryRow) -> Vec<String> {
    vec![
        row.calibration_run_id.clone(),
        row.horizon_label.clone(),
        metric_label(row),
        metric_class_label(row.metric_class).to_string(),
        optional_number(row.absolute_delta),
        optional_number(row.standardized_delta),
        decision_reasons_label(&row.decision_reason_codes),
    ]
}

fn compare_closeness(
    left: &&StylizedFactHorizonComparisonTableSummaryRow,
    right: &&StylizedFactHorizonComparisonTableSummaryRow,
) -> Ordering {
    compare_optional_f64_desc(left.standardized_delta, right.standardized_delta)
        .then_with(|| compare_optional_f64_desc(left.absolute_delta, right.absolute_delta))
        .then_with(|| left.calibration_run_id.cmp(&right.calibration_run_id))
        .then_with(|| left.horizon_label.cmp(&right.horizon_label))
        .then_with(|| left.metric_id.cmp(&right.metric_id))
}

fn compare_optional_f64_desc(left: Option<f64>, right: Option<f64>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => right.total_cmp(&left),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn metric_label(row: &StylizedFactHorizonComparisonTableSummaryRow) -> String {
    match row.metric_variant.as_deref() {
        Some(variant) => format!("{} ({variant})", row.metric_id),
        None => row.metric_id.clone(),
    }
}

fn optional_number(value: Option<f64>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "not available".to_string())
}

fn decision_reasons_label(reasons: &[StylizedFactDecisionReasonCode]) -> String {
    if reasons.is_empty() {
        return "none".to_string();
    }

    reasons
        .iter()
        .map(|reason| decision_reason_label(*reason))
        .collect::<Vec<_>>()
        .join(", ")
}

fn decision_reason_label(reason: StylizedFactDecisionReasonCode) -> &'static str {
    match reason {
        StylizedFactDecisionReasonCode::MissingEmpiricalMetric => "missing empirical metric",
        StylizedFactDecisionReasonCode::MissingSimulatedMetric => "missing simulated metric",
        StylizedFactDecisionReasonCode::HorizonDurationMismatch => "interval duration mismatch",
        StylizedFactDecisionReasonCode::InsufficientEmpiricalData => "insufficient empirical data",
        StylizedFactDecisionReasonCode::InsufficientSimulatedData => "insufficient simulated data",
        StylizedFactDecisionReasonCode::AbsoluteDeltaThresholdExceeded => {
            "absolute delta threshold exceeded"
        }
        StylizedFactDecisionReasonCode::StandardizedDeltaThresholdExceeded => {
            "standardized delta threshold exceeded"
        }
    }
}

fn metric_class_label(metric_class: StylizedFactMetricClass) -> &'static str {
    match metric_class {
        StylizedFactMetricClass::PrimaryRobust => "primary robust",
        StylizedFactMetricClass::SecondaryMoment => "secondary moment",
    }
}

fn decision_state_label(state: StylizedFactComparisonDecisionState) -> &'static str {
    match state {
        StylizedFactComparisonDecisionState::Pass => "pass",
        StylizedFactComparisonDecisionState::Fail => "fail",
        StylizedFactComparisonDecisionState::InsufficientData => "not enough data",
    }
}

fn execution_mode_label(mode: CalibrationExecutionMode) -> &'static str {
    match mode {
        CalibrationExecutionMode::Memory => "memory",
        CalibrationExecutionMode::Disk => "disk",
    }
}
