use std::path::Path;
use std::path::PathBuf;

use janiform_event_log::RunStateSnapshot;
use janiform_event_log::default_run_event_paths;
use janiform_event_log::load_run_state_snapshot;
use janiform_terminal_ui::FailureBlock;
use janiform_terminal_ui::KeyValueRow;
use janiform_terminal_ui::StartSummary;
use janiform_terminal_ui::TerminalRenderer;
use janiform_terminal_ui::format_elapsed_label;
use janiform_terminal_ui::format_token_as_words;
use serde_json::Value;

use crate::commands::reporting::ReportFitCommand;
use crate::commands::shared::artifacts::load_json_artifact;
use crate::commands::shared::artifacts::require_json_object;
use crate::commands::shared::output::write_json_payload;
use crate::context::CommandContext;
use crate::context::OutputMode;
use crate::error::CliError;
use janiform_foundations::json::display::compact_value_string;
use janiform_foundations::json::query::nested_string;
use janiform_foundations::json::query::nested_value;
use janiform_foundations::json::query::string_field;
use janiform_workflows::fit::stage_label_from_token;
use janiform_workflows::fit::workflow_scope_label_from_token;

mod export;
mod parameter_fits;
mod sections;
mod timing;

use export::export_fit_report;
use parameter_fits::report_with_parameter_fits;
use sections::render_artifacts;
use sections::render_calibration_outcomes;
use sections::render_diagnostics;
use sections::render_resume;
use sections::render_selected_candidates;
use sections::render_warnings;
use timing::run_elapsed;

const FIT_REPORT_FILE_NAME: &str = "report.json";
pub(super) const FIT_REPORT_ARTIFACT_DIRECTORY: &str = "artifacts";
pub(super) const FIT_REPORT_HTML_EXTENSION: &str = "html";

pub(crate) fn run_report_fit(
    context: &CommandContext,
    command: ReportFitCommand,
) -> Result<(), CliError> {
    let input_path = context
        .paths()
        .resolve_process_path(&command.run_or_report_path);

    let report_context = resolve_fit_report_context(&input_path)?;

    let report = load_json_artifact(&report_context.report_path, "fit report")?;

    require_json_object(&report, "fit report", &report_context.report_path)?;

    let run_state = report_context
        .run_directory
        .as_ref()
        .and_then(|run_directory| {
            load_run_state_snapshot(
                default_run_event_paths(run_directory).latest_state_snapshot_path,
            )
            .ok()
        });

    let model = load_optional_model_artifact(&report_context);

    if command.html || command.output_path.output_path.is_some() {
        return export_fit_report(context, command, &report_context, &report, model.as_ref());
    }

    match context.output_mode() {
        OutputMode::Json => {
            write_json_payload(&report_with_parameter_fits(&report, model.as_ref()))
        }
        OutputMode::Silent => Ok(()),
        OutputMode::Terminal | OutputMode::Quiet => {
            let rendered = render_fit_report(
                &report,
                model.as_ref(),
                run_state.as_ref(),
                &report_context,
                &context.stdout_renderer(),
            );
            context
                .stdout_renderer()
                .write_stdout(&rendered)
                .map_err(|source| CliError::OutputWrite { source })
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct FitReportContext {
    pub(super) report_path: PathBuf,
    pub(super) run_directory: Option<PathBuf>,
    model_path: Option<PathBuf>,
}

fn resolve_fit_report_context(input_path: &Path) -> Result<FitReportContext, CliError> {
    if input_path.is_dir() {
        return Ok(FitReportContext {
            report_path: input_path
                .join(FIT_REPORT_ARTIFACT_DIRECTORY)
                .join(FIT_REPORT_FILE_NAME),
            model_path: Some(
                input_path
                    .join(FIT_REPORT_ARTIFACT_DIRECTORY)
                    .join("model.jf"),
            ),
            run_directory: Some(input_path.to_path_buf()),
        });
    }

    let run_directory = input_path.parent().and_then(|parent| {
        if parent
            .file_name()
            .is_some_and(|name| name == FIT_REPORT_ARTIFACT_DIRECTORY)
        {
            return parent.parent().map(Path::to_path_buf);
        }

        None
    });

    Ok(FitReportContext {
        report_path: input_path.to_path_buf(),
        model_path: run_directory.as_ref().map(|run_directory| {
            run_directory
                .join(FIT_REPORT_ARTIFACT_DIRECTORY)
                .join("model.jf")
        }),
        run_directory,
    })
}

fn render_fit_report(
    report: &Value,
    model: Option<&Value>,
    run_state: Option<&RunStateSnapshot>,
    report_context: &FitReportContext,
    renderer: &TerminalRenderer,
) -> String {
    let mut sections = Vec::new();

    if report_success(report) {
        sections.push(render_success_summary(report, run_state, renderer));
    } else {
        sections.push(render_failure_summary(report, run_state, renderer));
    }

    sections.push(render_fit_summary(report, renderer));
    sections.push(render_calibration_outcomes(
        report, model, run_state, renderer,
    ));

    if let Some(rendered_candidates) = render_selected_candidates(model, renderer) {
        sections.push(rendered_candidates);
    }

    if let Some(rendered_diagnostics) = render_diagnostics(report, report_context, renderer) {
        sections.push(rendered_diagnostics);
    }

    if let Some(rendered_artifacts) = render_artifacts(report_context, renderer) {
        sections.push(rendered_artifacts);
    }

    if let Some(rendered_resume) = render_resume(report, run_state, report_context, renderer) {
        sections.push(rendered_resume);
    }

    if let Some(warnings) = render_warnings(run_state, report) {
        sections.push(warnings.render(renderer));
    }

    sections.join("\n\n")
}

fn render_success_summary(
    report: &Value,
    run_state: Option<&RunStateSnapshot>,
    renderer: &TerminalRenderer,
) -> String {
    let mut rows = vec![
        KeyValueRow::new("Run", report_run_id(report)),
        KeyValueRow::new("Status", "completed"),
    ];

    if let Some(elapsed) = run_state.and_then(run_elapsed) {
        rows.push(KeyValueRow::new("Elapsed", format_elapsed_label(elapsed)));
    }

    StartSummary {
        title: "Fit report".to_string(),
        rows,
    }
    .render(renderer)
}

fn render_fit_summary(report: &Value, renderer: &TerminalRenderer) -> String {
    let mut rows = Vec::new();

    if let Some(workflow_scope) = string_field(report, "workflow_scope")
        .or_else(|| nested_string(report, &["summary", "workflow_scope"]))
    {
        rows.push(KeyValueRow::new(
            "Workflow scope",
            workflow_scope_label_from_token(workflow_scope),
        ));
    }

    if let Some(selected_stages) = selected_stage_summary(report) {
        rows.push(KeyValueRow::block("Selected stages", selected_stages));
    }

    if let Some(prepared_data_reference) = string_field(report, "prepared_data_reference") {
        rows.push(KeyValueRow::new(
            "Prepared data",
            prepared_data_reference.to_string(),
        ));
    }

    if let Some(diagnostic_count) =
        nested_value(report, &["summary", "diagnostic_count"]).and_then(Value::as_u64)
    {
        rows.push(KeyValueRow::new(
            "Diagnostics",
            diagnostic_count.to_string(),
        ));
    }

    if rows.is_empty() {
        rows.push(KeyValueRow::new("Summary", "not recorded"));
    }

    StartSummary {
        title: "Fit summary".to_string(),
        rows,
    }
    .render(renderer)
}

fn render_failure_summary(
    report: &Value,
    run_state: Option<&RunStateSnapshot>,
    renderer: &TerminalRenderer,
) -> String {
    let mut details = Vec::new();

    if let Some(elapsed) = run_state.and_then(run_elapsed) {
        details.push(KeyValueRow::new("Elapsed", format_elapsed_label(elapsed)));
    }

    if let Some(failed_stage) = run_state
        .and_then(|state| state.failure.as_ref())
        .and_then(|failure| failure.failed_stage.as_ref())
    {
        details.push(KeyValueRow::new(
            "Failed stage",
            failed_stage
                .label
                .clone()
                .unwrap_or_else(|| stage_label_from_token(&failed_stage.stage_id)),
        ));
    }

    if let Some(failed_phase) = run_state
        .and_then(|state| state.failure.as_ref())
        .and_then(|failure| failure.failed_phase.as_ref())
    {
        details.push(KeyValueRow::new(
            "Failed phase",
            failed_phase
                .label
                .clone()
                .unwrap_or_else(|| format_token_as_words(&failed_phase.phase_id)),
        ));
    }

    FailureBlock {
        title: format!("Fit report: {}", report_run_id(report)),
        reason: failure_reason(report, run_state),
        details,
    }
    .render(renderer)
}

pub(super) fn report_success(report: &Value) -> bool {
    report
        .get("success")
        .and_then(Value::as_bool)
        .or_else(|| {
            nested_string(report, &["staged_response", "status"]).map(|status| status == "success")
        })
        .unwrap_or(false)
}

pub(super) fn report_run_id(report: &Value) -> String {
    string_field(report, "run_id")
        .or_else(|| nested_string(report, &["summary", "run_id"]))
        .or_else(|| nested_string(report, &["staged_response", "run_id"]))
        .unwrap_or("not recorded")
        .to_string()
}

fn failure_reason(report: &Value, run_state: Option<&RunStateSnapshot>) -> String {
    if let Some(reason) = run_state
        .and_then(|state| state.failure.as_ref())
        .map(|failure| failure.reason.clone())
    {
        return reason;
    }

    nested_value(report, &["staged_response", "failure"])
        .map(compact_value_string)
        .filter(|reason| !reason.is_empty() && reason != "not recorded")
        .unwrap_or_else(|| "fit did not complete successfully".to_string())
}

fn selected_stage_summary(report: &Value) -> Option<String> {
    let selected_stages = nested_value(report, &["summary", "selected_stages"])
        .or_else(|| nested_value(report, &["staged_request", "selected_stages"]))
        .and_then(Value::as_array)?;

    let stages = selected_stages
        .iter()
        .filter_map(Value::as_str)
        .map(stage_label_from_token)
        .collect::<Vec<_>>();

    if stages.is_empty() {
        return None;
    }

    Some(stages.join("\n"))
}

fn load_optional_model_artifact(report_context: &FitReportContext) -> Option<Value> {
    let model_path = report_context.model_path.as_ref()?;

    if !model_path.is_file() {
        return None;
    }

    load_json_artifact(model_path, "model").ok()
}
