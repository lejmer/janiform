use std::path::Path;
use std::time::Duration;

use janiform_terminal_ui::{
    CompletionSummary, CompletionSummaryStatus, KeyValueRow, StartSummary, format_artifact_path,
    format_elapsed_label, format_path,
};
use janiform_workflows::pricing::FittedModelSingleOptionPricingOutput;

pub(crate) fn pricing_start_summary(
    run_id: &str,
    model_path: &Path,
    output_path: &Path,
    valuation_time_utc: &str,
) -> StartSummary {
    StartSummary {
        title: "Janiform price".to_string(),
        rows: vec![
            KeyValueRow::new("Run", run_id),
            KeyValueRow::new("Scope", "single vanilla option from fitted model"),
            KeyValueRow::block("Inputs", format!("model: {}", format_path(model_path))),
            KeyValueRow::new("Valuation", valuation_time_utc),
            KeyValueRow::new("Output", format_path(output_path)),
        ],
    }
}

pub(crate) fn pricing_completion_summary(
    output: &FittedModelSingleOptionPricingOutput,
    elapsed: Duration,
) -> CompletionSummary {
    let failed_node_count = if output.run_result.response.price.is_some() {
        0
    } else {
        1
    };

    let mut rows = vec![
        KeyValueRow::new("Priced nodes", "1"),
        KeyValueRow::new("Failed nodes", failed_node_count.to_string()),
    ];

    if let Some(paths) = &output.artifact_paths {
        rows.push(KeyValueRow::new(
            "Path",
            format_artifact_path(&paths.primary_output_path),
        ));
        rows.push(KeyValueRow::new(
            "Report",
            format_artifact_path(&paths.report_path),
        ));
    }

    CompletionSummary {
        status: CompletionSummaryStatus::Succeeded,
        title: format!("Pricing completed in {}", format_elapsed_label(elapsed)),
        rows,
    }
}
