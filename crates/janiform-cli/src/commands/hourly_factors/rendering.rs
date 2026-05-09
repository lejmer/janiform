use std::path::Path;

use janiform::model::factors::hourly::{
    HourlyFactorContractOutput, HourlyNewsIntensityMaterializationOutput,
    HourlySeasonalityMaterializationOutput,
};
use janiform_terminal_ui::KeyValueRow;
use serde_json::Value;

use crate::commands::shared::terminal::{
    append_output_path_row, emit_json_or_success_summary, output_path_rows,
};
use crate::context::CommandContext;
use crate::error::CliError;

pub(super) fn emit_hourly_output(
    context: &CommandContext,
    payload: &Value,
    output_path: Option<&Path>,
    title: &'static str,
    terminal_rows: Vec<KeyValueRow>,
) -> Result<(), CliError> {
    emit_json_or_success_summary(
        context,
        payload,
        title,
        terminal_rows,
        output_path_rows(output_path),
    )
}

pub(super) fn seasonality_materialization_rows(
    output: &HourlySeasonalityMaterializationOutput,
    output_path: Option<&Path>,
) -> Vec<KeyValueRow> {
    let mut rows = vec![
        KeyValueRow::new("Exchange", output.exchange_code.clone()),
        KeyValueRow::new(
            "Grid",
            format!(
                "{} intervals, {} ns",
                output.diagnostics.interval_count, output.interval_duration_nanos
            ),
        ),
        KeyValueRow::new(
            "Source points",
            output.diagnostics.source_point_count.to_string(),
        ),
        KeyValueRow::new(
            "Sparse intervals",
            output.diagnostics.sparse_interval_count.to_string(),
        ),
        KeyValueRow::new(
            "Interpolated intervals",
            output.diagnostics.interpolated_interval_count.to_string(),
        ),
        KeyValueRow::new(
            "Fallback intervals",
            output.diagnostics.fallback_interval_count.to_string(),
        ),
        KeyValueRow::new(
            "Timezone transitions",
            format!(
                "skipped={}, repeated={}",
                output.diagnostics.skipped_local_hour_interval_count,
                output.diagnostics.repeated_local_hour_interval_count
            ),
        ),
        KeyValueRow::new(
            "Methods",
            format!(
                "{}, {}, {}",
                output.metadata.interpolation_method,
                output.metadata.aggregation_method,
                output.metadata.sparse_fallback_policy
            ),
        ),
    ];
    append_output_path_row(&mut rows, output_path);

    rows
}

pub(super) fn news_intensity_materialization_rows(
    output: &HourlyNewsIntensityMaterializationOutput,
    output_path: Option<&Path>,
) -> Vec<KeyValueRow> {
    let mut rows = vec![
        KeyValueRow::new("Exchange", output.exchange_code.clone()),
        KeyValueRow::new(
            "Grid",
            format!(
                "{} intervals, {} ns",
                output.diagnostics.interval_count, output.interval_duration_nanos
            ),
        ),
        KeyValueRow::new(
            "Known arrivals",
            output.diagnostics.known_scheduled_arrival_count.to_string(),
        ),
        KeyValueRow::new(
            "Non-zero intensity intervals",
            output
                .diagnostics
                .combined_component_non_zero_interval_count
                .to_string(),
        ),
        KeyValueRow::new(
            "Timezone transitions",
            format!(
                "skipped={}, repeated={}",
                output.diagnostics.skipped_local_hour_interval_count,
                output.diagnostics.repeated_local_hour_interval_count
            ),
        ),
        KeyValueRow::new("Method", output.metadata.materialization_method.to_string()),
    ];

    append_output_path_row(&mut rows, output_path);

    rows
}

pub(super) fn contract_rows(
    output: &HourlyFactorContractOutput,
    output_path: Option<&Path>,
) -> Vec<KeyValueRow> {
    let mut rows = vec![
        KeyValueRow::new("Exchange", output.exchange_code.clone()),
        KeyValueRow::new(
            "Grid",
            format!(
                "{} intervals, {} ns",
                output.diagnostics.interval_count, output.interval_duration_nanos
            ),
        ),
        KeyValueRow::new(
            "Timezone transitions",
            format!(
                "skipped={}, repeated={}",
                output.diagnostics.skipped_local_hour_interval_count,
                output.diagnostics.repeated_local_hour_interval_count
            ),
        ),
        KeyValueRow::new(
            "Seasonality method",
            output.metadata.seasonality_materialization_method.clone(),
        ),
        KeyValueRow::new(
            "News intensity method",
            output
                .metadata
                .news_intensity_materialization_method
                .clone(),
        ),
    ];

    append_output_path_row(&mut rows, output_path);

    rows
}
