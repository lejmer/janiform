use std::path::Path;

use janiform_terminal_ui::CompletionSummary;
use janiform_terminal_ui::CompletionSummaryStatus;
use janiform_terminal_ui::KeyValueRow;
use janiform_terminal_ui::ProgressAdapter;
use janiform_terminal_ui::StartSummary;
use janiform_terminal_ui::TerminalRenderer;
use janiform_terminal_ui::TerminalSummaryPolicy;
use janiform_terminal_ui::format_artifact_path;
use serde::Serialize;

use crate::commands::shared::output::write_json_payload;
use crate::context::{CommandContext, OutputMode};
use crate::error::CliError;

pub(crate) fn terminal_output_for_context(
    context: &CommandContext,
) -> Option<TerminalSummaryPolicy> {
    match context.output_mode() {
        OutputMode::Terminal => Some(TerminalSummaryPolicy::start_and_completion(
            context.stdout_renderer(),
        )),
        OutputMode::Quiet => Some(TerminalSummaryPolicy::completion_only(
            context.stdout_renderer(),
        )),
        OutputMode::Json | OutputMode::Silent => None,
    }
}

pub(crate) fn terminal_progress_adapter_for_context(
    context: &CommandContext,
) -> Option<ProgressAdapter> {
    match context.output_mode() {
        OutputMode::Terminal | OutputMode::Quiet => Some(ProgressAdapter::stderr()),
        OutputMode::Json | OutputMode::Silent => None,
    }
}

pub(crate) fn write_terminal_output(
    context: &CommandContext,
    output: &str,
) -> Result<(), CliError> {
    context
        .stdout_renderer()
        .write_stdout(output)
        .map_err(|source| CliError::OutputWrite { source })
}

pub(crate) fn write_terminal_summary(
    context: &CommandContext,
    summary: CompletionSummary,
) -> Result<(), CliError> {
    write_terminal_output(context, &summary.render(&context.stdout_renderer()))
}

pub(crate) fn render_success_completion_summary(
    title: impl Into<String>,
    rows: Vec<KeyValueRow>,
    renderer: &TerminalRenderer,
) -> String {
    CompletionSummary {
        title: title.into(),
        status: CompletionSummaryStatus::Succeeded,
        rows,
    }
    .render(renderer)
}

pub(crate) fn emit_json_or_success_summary<T>(
    context: &CommandContext,
    payload: &T,
    title: impl Into<String>,
    terminal_rows: Vec<KeyValueRow>,
    quiet_rows: Vec<KeyValueRow>,
) -> Result<(), CliError>
where
    T: Serialize + ?Sized,
{
    let title = title.into();

    match context.output_mode() {
        OutputMode::Json => write_json_payload(payload),
        OutputMode::Terminal => write_terminal_summary(
            context,
            CompletionSummary {
                title: title.clone(),
                status: CompletionSummaryStatus::Succeeded,
                rows: terminal_rows,
            },
        ),
        OutputMode::Quiet => write_terminal_summary(
            context,
            CompletionSummary {
                title,
                status: CompletionSummaryStatus::Succeeded,
                rows: quiet_rows,
            },
        ),
        OutputMode::Silent => Ok(()),
    }
}

pub(crate) fn output_path_rows(output_path: Option<&Path>) -> Vec<KeyValueRow> {
    let mut rows = Vec::new();

    append_output_path_row(&mut rows, output_path);

    rows
}

pub(crate) fn append_output_path_row(rows: &mut Vec<KeyValueRow>, output_path: Option<&Path>) {
    if let Some(output_path) = output_path {
        rows.push(KeyValueRow::new(
            "Output",
            format_artifact_path(output_path),
        ));
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArtifactListItem {
    label: String,
    value: String,
}

impl ArtifactListItem {
    pub(crate) fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }

    fn render(&self) -> String {
        format!("{}: {}", self.label, self.value)
    }
}

pub(crate) fn render_artifact_list(
    title: impl Into<String>,
    items: impl IntoIterator<Item = ArtifactListItem>,
    renderer: &TerminalRenderer,
) -> Option<String> {
    let items = unique_artifact_items(items);

    if items.is_empty() {
        return None;
    }

    let rendered_items = items
        .iter()
        .map(ArtifactListItem::render)
        .collect::<Vec<_>>()
        .join("\n");

    Some(
        StartSummary {
            title: title.into(),
            rows: vec![KeyValueRow::block("Paths", rendered_items)],
        }
        .render(renderer),
    )
}

fn unique_artifact_items(
    items: impl IntoIterator<Item = ArtifactListItem>,
) -> Vec<ArtifactListItem> {
    let mut unique_items = Vec::new();

    for item in items {
        if unique_items
            .iter()
            .any(|existing: &ArtifactListItem| existing == &item)
        {
            continue;
        }

        unique_items.push(item);
    }

    unique_items
}
