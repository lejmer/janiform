use janiform_governance::{RegressionCaseFailure, RegressionCaseKind, RegressionHarnessReport};
use janiform_verification::{
    OpenClosedGapLeakageRegressionFailure, OpenClosedGapLeakageRegressionHarnessReport,
};

use crate::commands::shared::output::write_json_payload;
use crate::commands::shared::terminal::write_terminal_output;
use crate::context::{CommandContext, OutputMode};
use crate::error::CliError;

pub(super) fn emit_deterministic_regression_output(
    context: &CommandContext,
    report: &RegressionHarnessReport,
) -> Result<(), CliError> {
    match context.output_mode() {
        OutputMode::Json => write_json_payload(report),
        OutputMode::Terminal | OutputMode::Quiet => {
            let output = render_deterministic_regression_human(report);

            if report.summary.failed_case_count == 0 {
                write_terminal_output(context, &output)
            } else {
                write_failure_output(context, &output)
            }
        }
        OutputMode::Silent => Ok(()),
    }
}

pub(super) fn emit_open_closed_gap_leakage_output(
    context: &CommandContext,
    report: &OpenClosedGapLeakageRegressionHarnessReport,
) -> Result<(), CliError> {
    match context.output_mode() {
        OutputMode::Json => write_json_payload(report),
        OutputMode::Terminal | OutputMode::Quiet => {
            let output = render_open_closed_gap_leakage_human(report);

            if report.summary.failed_case_count == 0 {
                write_terminal_output(context, &output)
            } else {
                write_failure_output(context, &output)
            }
        }
        OutputMode::Silent => Ok(()),
    }
}

fn render_deterministic_regression_human(report: &RegressionHarnessReport) -> String {
    let summary = &report.summary;

    if summary.failed_case_count == 0 {
        return format!(
            "deterministic regression passed: {}/{} cases, calendar_cases={}, seasonality_cases={}\n",
            summary.passed_case_count,
            summary.total_case_count,
            calendar_case_count(report),
            seasonality_case_count(report),
        );
    }

    let mut output = format!(
        "deterministic regression failed: {}/{} cases\n",
        summary.failed_case_count, summary.total_case_count,
    );

    for case_report in &report.case_reports {
        if case_report.failure.is_none() {
            continue;
        }

        output.push_str(&format!(
            "fixture_id={} case_kind={:?} status={:?} summary=\"{}\"\n",
            case_report.fixture_id, case_report.case_kind, case_report.status, case_report.summary,
        ));

        if let Some(failure) = &case_report.failure {
            render_deterministic_failure(&mut output, failure);
        }
    }

    output
}

fn render_deterministic_failure(output: &mut String, failure: &RegressionCaseFailure) {
    match failure {
        RegressionCaseFailure::CalendarResolutionError {
            exchange_code,
            timestamp_utc,
            error_message,
        } => output.push_str(&format!(
            "  failure_kind=calendar_resolution_error exchange_code={exchange_code} timestamp_utc={timestamp_utc} error_message=\"{error_message}\"\n",
        )),
        RegressionCaseFailure::MarketStateMismatch {
            exchange_code,
            timestamp_utc,
            expected_state,
            observed_state,
        } => output.push_str(&format!(
            "  failure_kind=market_state_mismatch exchange_code={exchange_code} timestamp_utc={timestamp_utc} expected_value={expected_state} observed_value={observed_state}\n",
        )),
        RegressionCaseFailure::IntervalFactorGridResolutionError { error_message } => {
            output.push_str(&format!(
                "  failure_kind=interval_factor_grid_resolution_error error_message=\"{error_message}\"\n",
            ));
        }
        RegressionCaseFailure::FactorLengthMismatch {
            field_name,
            expected_length,
            observed_length,
        } => output.push_str(&format!(
            "  failure_kind=factor_length_mismatch field_name={field_name} expected_value={expected_length} observed_value={observed_length}\n",
        )),
        RegressionCaseFailure::FactorToleranceExceeded {
            field_name,
            value_index,
            expected_value,
            observed_value,
            absolute_difference,
            absolute_tolerance,
        } => output.push_str(&format!(
            "  failure_kind=factor_tolerance_exceeded field_name={field_name} value_index={value_index} expected_value={expected_value} observed_value={observed_value} difference={absolute_difference} tolerance={absolute_tolerance}\n",
        )),
    }
}

fn render_open_closed_gap_leakage_human(
    report: &OpenClosedGapLeakageRegressionHarnessReport,
) -> String {
    let summary = &report.summary;
    let behavior_diff_summary = &report.behavior_diff_report.summary;

    if summary.failed_case_count == 0 {
        return format!(
            "open closed gap leakage regression passed: {}/{} scenarios, semantic_mismatches={}, diff_regression_cases={}\n",
            summary.passed_case_count,
            summary.total_case_count,
            summary.total_semantic_mismatch_failure_count,
            behavior_diff_summary.regression_case_count,
        );
    }

    let mut output = format!(
        "open closed gap leakage regression failed: {}/{} scenarios, semantic_mismatches={}, diff_regression_cases={}\n",
        summary.failed_case_count,
        summary.total_case_count,
        summary.total_semantic_mismatch_failure_count,
        behavior_diff_summary.regression_case_count,
    );

    for case_report in &report.case_reports {
        if case_report.failures.is_empty() && case_report.behavior_diff.regression_metric_count == 0
        {
            continue;
        }

        output.push_str(&format!(
            "fixture_id={} exchange_code={} scenario_class={:?} semantic_mismatch_failure_count={} max_absolute_mismatch={} summary=\"{}\"\n",
            case_report.fixture_id,
            case_report.exchange_code,
            case_report.scenario_class,
            case_report.semantic_mismatch_failure_count,
            case_report.max_absolute_mismatch,
            case_report.summary,
        ));

        let contributions = &case_report.contribution_summary;
        output.push_str(&format!(
            "  contributions expected_activity={} observed_activity={} expected_trading={} observed_trading={} expected_open_jump={} observed_open_jump={} expected_closed_gap={} observed_closed_gap={}\n",
            contributions.expected_activity_coupling_total,
            contributions.observed_activity_coupling_total,
            contributions.expected_trading_return_total,
            contributions.observed_trading_return_total,
            contributions.expected_open_jump_total,
            contributions.observed_open_jump_total,
            contributions.expected_closed_gap_total,
            contributions.observed_closed_gap_total,
        ));

        for metric in &case_report.behavior_diff.metrics {
            if !metric.regression_detected {
                continue;
            }

            output.push_str(&format!(
                "  diff_regression metric_id={} semantic_bucket={:?} expected_value={} observed_value={} difference={} tolerance={}\n",
                metric.metric_id,
                metric.semantic_bucket,
                metric.expected_value,
                metric.observed_value,
                metric.absolute_difference,
                metric.absolute_tolerance,
            ));
        }

        for failure in &case_report.failures {
            render_open_closed_gap_failure(&mut output, failure);
        }
    }

    output
}

fn render_open_closed_gap_failure(
    output: &mut String,
    failure: &OpenClosedGapLeakageRegressionFailure,
) {
    match failure {
        OpenClosedGapLeakageRegressionFailure::DispatchError { error_message } => {
            output.push_str(&format!(
                "  failure_kind=dispatch_error error_message=\"{error_message}\"\n",
            ));
        }
        OpenClosedGapLeakageRegressionFailure::SemanticMismatch {
            semantic_bucket,
            mismatch_kind,
            event_id,
            event_timestamp_utc,
            expected_value,
            observed_value,
            absolute_difference,
        } => output.push_str(&format!(
            "  failure_kind=semantic_mismatch semantic_bucket={semantic_bucket:?} mismatch_kind={mismatch_kind} event_id={event_id} event_timestamp_utc={event_timestamp_utc} expected_value={expected_value} observed_value={observed_value} difference={absolute_difference}\n",
        )),
    }
}

fn write_failure_output(context: &CommandContext, output: &str) -> Result<(), CliError> {
    context
        .stderr_renderer()
        .write_stderr(output)
        .map_err(|source| CliError::OutputWrite { source })
}

fn calendar_case_count(report: &RegressionHarnessReport) -> usize {
    report
        .case_reports
        .iter()
        .filter(|case_report| {
            matches!(
                case_report.case_kind,
                RegressionCaseKind::CalendarMarketState
            )
        })
        .count()
}

fn seasonality_case_count(report: &RegressionHarnessReport) -> usize {
    report.summary.total_case_count - calendar_case_count(report)
}
