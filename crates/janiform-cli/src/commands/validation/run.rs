use std::path::PathBuf;

use janiform_workflows::validation::{
    StylizedFactsValidationCacheLevel, StylizedFactsValidationProgressEvent,
    StylizedFactsValidationWorkflowRequest, build_stylized_facts_validation_plan,
    run_stylized_facts_validation_workflow,
};

use super::args::{
    StylizedFactsCacheLevel, ValidateStylizedFactsCommand, ValidateStylizedFactsOptions,
    ValidateStylizedFactsSubcommand,
};
use super::rendering::{
    emit_stylized_fact_validation_report_output, emit_stylized_facts_validation_plan,
    render_plan_human,
};
use crate::commands::shared::calibration::{
    duplicate_record_policy, market_data_file_format, tick_price_input_policy,
};
use crate::commands::shared::inputs::required_input_file;
use crate::commands::shared::output_paths::default_run_output_directory;
use crate::commands::shared::progress::{StageProgressUpdate, TerminalCommandProgress};
use crate::commands::shared::terminal::write_terminal_output;
use crate::context::{CommandContext, OutputMode};
use crate::error::{CliError, CliExitCode};

pub(crate) fn run_validate_stylized_facts(
    context: &CommandContext,
    command: ValidateStylizedFactsCommand,
) -> Result<CliExitCode, CliError> {
    match command.command {
        Some(ValidateStylizedFactsSubcommand::Plan(plan_command)) => {
            let request = workflow_request(context, &plan_command.options)?;
            let plan = build_stylized_facts_validation_plan(&request)?;

            emit_stylized_facts_validation_plan(context, &plan)?;

            Ok(CliExitCode::Success)
        }
        None => run_validation(context, command.options),
    }
}

fn run_validation(
    context: &CommandContext,
    options: ValidateStylizedFactsOptions,
) -> Result<CliExitCode, CliError> {
    let request = workflow_request(context, &options)?;
    let plan = build_stylized_facts_validation_plan(&request)?;

    if matches!(
        context.output_mode(),
        OutputMode::Terminal | OutputMode::Quiet
    ) {
        write_terminal_output(
            context,
            &format!("{}\n", render_plan_human(&plan, &context.stdout_renderer())),
        )?;
    }

    let output_dir = PathBuf::from(&plan.output_dir);

    let mut progress = TerminalCommandProgress::start(
        context,
        "validate stylized facts",
        2,
        Some(output_dir.as_path()),
        &[
            "load empirical data",
            "derive interval returns",
            "simulate seeded paths",
            "compute return-based metrics",
            "compare metrics",
            "write artifacts",
        ],
    )?;

    let mut progress_error = None;

    let workflow_output = {
        let mut observer = |event: StylizedFactsValidationProgressEvent| {
            if progress_error.is_none()
                && let Err(source) = progress.observe_stage_progress(StageProgressUpdate {
                    phase: &event.label,
                    stage_number: event.step,
                    completed_units: event.completed_units,
                    total_units: event.total_units,
                    work_item_label: event.work_item_label,
                    work_item_progress_percent: event.work_item_progress_percent,
                    planned_work_item_labels: event.planned_work_item_labels,
                    work_item_columns: event.work_item_columns,
                    work_item_metrics: event.work_item_metrics,
                    latest_signal: event.latest_signal,
                })
            {
                progress_error = Some(source);
            }
        };

        run_stylized_facts_validation_workflow(request, &mut observer)?
    };

    if let Some(error) = progress_error {
        return Err(error);
    }

    progress.complete()?;

    emit_stylized_fact_validation_report_output(
        context,
        &workflow_output.report_bundle,
        janiform::calibration::artifacts::CalibrationExecutionMode::Disk,
        options.show_checks,
        options.max_checks,
    )?;

    if options.fail_on_failed_checks
        && workflow_output
            .report_bundle
            .comparison_report
            .summary
            .metric_decision_counts
            .fail_count
            > 0
    {
        return Ok(CliExitCode::ReportFailedChecks);
    }

    Ok(CliExitCode::Success)
}

fn workflow_request(
    context: &CommandContext,
    options: &ValidateStylizedFactsOptions,
) -> Result<StylizedFactsValidationWorkflowRequest, CliError> {
    let model_path = match options.model_path.as_ref() {
        Some(_) => Some(required_input_file(
            context,
            "--model-path",
            &options.model_path,
        )?),
        None => None,
    };

    let empirical_data_path = match options.empirical_data_path.as_ref() {
        Some(_) => Some(required_input_file(
            context,
            "--empirical-data-path",
            &options.empirical_data_path,
        )?),
        None => None,
    };

    let comparison_request = match options.comparison_request.as_ref() {
        Some(_) => Some(required_input_file(
            context,
            "--comparison-request",
            &options.comparison_request,
        )?),
        None => None,
    };

    let metric_catalog = match options.metric_catalog.as_ref() {
        Some(_) => Some(required_input_file(
            context,
            "--metric-catalog",
            &options.metric_catalog,
        )?),
        None => None,
    };

    let threshold_policy = match options.threshold_policy.as_ref() {
        Some(_) => Some(required_input_file(
            context,
            "--threshold-policy",
            &options.threshold_policy,
        )?),
        None => None,
    };

    Ok(StylizedFactsValidationWorkflowRequest {
        model_path,
        empirical_data_path,
        empirical_data_format: options.empirical_data_format.map(market_data_file_format),
        intervals: options.intervals.clone(),
        simulation_path_count: options.simulation_path_count,
        seed: options.seed,
        cpu_count: options.cpu_count,
        output_dir: resolve_output_dir(context, options),
        threshold_policy,
        significance_level: options.significance_level,
        cache: options.cache.map(validation_cache_level),
        comparison_request,
        metric_catalog,
        empirical_timestamp_column: options.empirical_timestamp_column.clone(),
        empirical_price_column: options.empirical_price_column.clone(),
        empirical_trade_sign_column: options.empirical_trade_sign_column.clone(),
        empirical_bid_column: options.empirical_bid_column.clone(),
        empirical_ask_column: options.empirical_ask_column.clone(),
        empirical_price_input_policy: options
            .empirical_price_input_policy
            .map(tick_price_input_policy),
        empirical_timestamp_timezone: options.empirical_timestamp_timezone.clone(),
        empirical_duplicate_record_policy: options
            .empirical_duplicate_record_policy
            .map(duplicate_record_policy),
        keep_empirical_source_order: options.keep_empirical_source_order,
    })
}

fn resolve_output_dir(context: &CommandContext, options: &ValidateStylizedFactsOptions) -> PathBuf {
    if let Some(output_dir) = &options.output_dir {
        return context.paths().resolve_process_path(output_dir);
    }

    if let Some(model_path) = &options.model_path {
        let resolved_model_path = context.paths().resolve_process_path(model_path);

        if let Some(parent) = resolved_model_path.parent() {
            return parent.join("validation").join("stylized-facts");
        }
    }

    default_run_output_directory(context, "validation").join("stylized-facts")
}

fn validation_cache_level(
    cache_level: StylizedFactsCacheLevel,
) -> StylizedFactsValidationCacheLevel {
    match cache_level {
        StylizedFactsCacheLevel::Derived => StylizedFactsValidationCacheLevel::Derived,
        StylizedFactsCacheLevel::Full => StylizedFactsValidationCacheLevel::Full,
    }
}
