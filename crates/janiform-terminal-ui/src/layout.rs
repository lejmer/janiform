use comfy_table::Cell;
use comfy_table::ContentArrangement;
use comfy_table::Table;
use comfy_table::presets::NOTHING;
use console::Alignment;
use console::measure_text_width;
use console::pad_str;
use textwrap::Options;

use crate::renderer::TerminalRenderer;
use crate::style::SemanticStyle;

const KEY_VALUE_SEPARATOR: &str = ": ";
const STAGE_SEPARATOR: &str = " ";
const COMPLETED_MARKER: &str = "✓";
const FAILURE_MARKER: &str = "✗";
const WARNING_MARKER: &str = "!";
const ACTIVE_MARKER: &str = "→";
const PENDING_MARKER: &str = " ";

/// One semantic label/value row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyValueRow {
    /// User-facing label.
    pub label: String,
    /// User-facing value.
    pub value: String,
    /// Semantic style applied to the value.
    pub value_style: SemanticStyle,
    /// Layout mode used for the row.
    pub layout: KeyValueRowLayout,
}

/// Layout mode for a semantic label/value row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyValueRowLayout {
    /// Render label and first value line on the same row.
    Inline,
    /// Render label as a group heading and value lines below it.
    Block,
}

impl KeyValueRow {
    /// Create a label/value row with default value styling.
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            value_style: SemanticStyle::Value,
            layout: KeyValueRowLayout::Inline,
        }
    }

    /// Create a label/value row with an explicit semantic value style.
    pub fn styled(
        label: impl Into<String>,
        value: impl Into<String>,
        value_style: SemanticStyle,
    ) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            value_style,
            layout: KeyValueRowLayout::Inline,
        }
    }

    /// Create a label/value row that renders the value on indented child rows.
    pub fn block(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            value_style: SemanticStyle::Value,
            layout: KeyValueRowLayout::Block,
        }
    }
}

/// Start-of-run terminal summary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartSummary {
    /// Section title.
    pub title: String,
    /// Summary rows.
    pub rows: Vec<KeyValueRow>,
}

impl StartSummary {
    /// Render the start summary to a terminal string.
    pub fn render(&self, renderer: &TerminalRenderer) -> String {
        render_section(renderer, &self.title, &self.rows)
    }
}

/// Final completion state for a terminal summary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionSummaryStatus {
    /// Run completed successfully.
    Succeeded,
    /// Run completed and emitted warnings.
    SucceededWithWarnings,
    /// Run failed.
    Failed,
}

/// End-of-run terminal summary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionSummary {
    /// Section title.
    pub title: String,
    /// Completion status.
    pub status: CompletionSummaryStatus,
    /// Summary rows.
    pub rows: Vec<KeyValueRow>,
}

impl CompletionSummary {
    /// Render the completion summary to a terminal string.
    pub fn render(&self, renderer: &TerminalRenderer) -> String {
        let status_style = match self.status {
            CompletionSummaryStatus::Succeeded => SemanticStyle::CompletedMarker,
            CompletionSummaryStatus::SucceededWithWarnings => SemanticStyle::WarningMarker,
            CompletionSummaryStatus::Failed => SemanticStyle::FailureMarker,
        };

        let mut rows = vec![KeyValueRow::styled(
            "Status",
            completion_status_text(self.status),
            status_style,
        )];

        rows.extend(self.rows.clone());

        let marker = match self.status {
            CompletionSummaryStatus::Succeeded => COMPLETED_MARKER,
            CompletionSummaryStatus::SucceededWithWarnings => WARNING_MARKER,
            CompletionSummaryStatus::Failed => FAILURE_MARKER,
        };
        let heading = renderer.style_text(status_style, &format!("{marker} {}", self.title));
        let details = render_rows(renderer, &rows);

        format!("{heading}\n\n{details}")
    }
}

/// Warning block with one or more user-readable warnings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WarningBlock {
    /// Block title.
    pub title: String,
    /// Warning messages.
    pub warnings: Vec<String>,
}

impl WarningBlock {
    /// Render the warning block to a terminal string.
    pub fn render(&self, renderer: &TerminalRenderer) -> String {
        let mut lines = vec![renderer.style_text(SemanticStyle::WarningMarker, &self.title)];

        for warning in &self.warnings {
            let marker = renderer.style_text(SemanticStyle::WarningMarker, WARNING_MARKER);
            lines.push(format!("{marker} {warning}"));
        }

        lines.join("\n")
    }
}

/// Failure block with an exact reason and optional supporting details.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailureBlock {
    /// Block title.
    pub title: String,
    /// Exact failure reason.
    pub reason: String,
    /// Supporting diagnostic details.
    pub details: Vec<KeyValueRow>,
}

impl FailureBlock {
    /// Render the failure block to a terminal string.
    pub fn render(&self, renderer: &TerminalRenderer) -> String {
        let mut rows = vec![KeyValueRow::styled(
            "Reason",
            self.reason.clone(),
            SemanticStyle::Value,
        )];

        rows.extend(self.details.clone());

        let heading = renderer.style_text(
            SemanticStyle::FailureMarker,
            &format!("{FAILURE_MARKER} {}", self.title),
        );
        let details = render_rows(renderer, &rows);

        format!("{heading}\n\n{details}")
    }
}

/// Compact key/value table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompactTable {
    /// Header labels.
    pub headers: Vec<String>,
    /// Table rows.
    pub rows: Vec<Vec<String>>,
}

impl CompactTable {
    /// Render the table with library-managed column alignment.
    pub fn render(&self, renderer: &TerminalRenderer) -> String {
        if self.headers.is_empty() {
            return String::new();
        }

        let mut table = Table::new();
        table.load_preset(NOTHING);

        match renderer
            .terminal_width()
            .and_then(|width| u16::try_from(width).ok())
        {
            Some(width) => {
                table.set_content_arrangement(ContentArrangement::Dynamic);
                table.set_width(width);
            }
            None => {
                table.set_content_arrangement(ContentArrangement::Disabled);
            }
        }

        table.set_header(
            self.headers
                .iter()
                .map(|header| Cell::new(renderer.style_text(SemanticStyle::TableHeader, header)))
                .collect::<Vec<Cell>>(),
        );

        for row in &self.rows {
            table.add_row(
                row.iter()
                    .map(|cell| Cell::new(cell.as_str()))
                    .collect::<Vec<Cell>>(),
            );
        }

        for column in table.column_iter_mut() {
            column.set_padding((0, 2));
        }

        table.trim_fmt()
    }
}

/// Curated analytical result table with a section title.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalyticalResultTable {
    /// Section title.
    pub title: String,
    /// Analytical table payload.
    pub table: CompactTable,
}

impl AnalyticalResultTable {
    /// Render an analytical result table.
    pub fn render(&self, renderer: &TerminalRenderer) -> String {
        let heading = renderer.style_text(SemanticStyle::Label, &self.title);
        let table = self.table.render(renderer);

        format!("{heading}\n{table}")
    }
}

/// Stage checklist state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageChecklistStatus {
    /// Stage has not started.
    Pending,
    /// Stage is active.
    Active,
    /// Stage completed successfully.
    Completed,
    /// Stage failed.
    Failed,
}

/// One stage checklist item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StageChecklistItem {
    /// Stage name.
    pub name: String,
    /// Stage status.
    pub status: StageChecklistStatus,
}

/// Stable stage checklist for long runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StageChecklist {
    /// Section title.
    pub title: String,
    /// Stage items.
    pub stages: Vec<StageChecklistItem>,
}

impl StageChecklist {
    /// Render a stage checklist.
    pub fn render(&self, renderer: &TerminalRenderer) -> String {
        let mut lines = vec![renderer.style_text(SemanticStyle::Label, &self.title)];

        for stage in &self.stages {
            let marker_style = stage_status_style(stage.status);
            let marker = renderer.style_text(marker_style, stage_status_text(stage.status));

            lines.push(format!("{marker}{STAGE_SEPARATOR}{}", stage.name));
        }

        lines.join("\n")
    }
}

/// Named group of artifact paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactPathGroup {
    /// Section title.
    pub title: String,
    /// Artifact paths or path-like references.
    pub paths: Vec<String>,
}

impl ArtifactPathGroup {
    /// Render an artifact path group.
    pub fn render(&self, renderer: &TerminalRenderer) -> String {
        let mut lines = vec![renderer.style_text(SemanticStyle::Label, &self.title)];

        for path in &self.paths {
            lines.push(renderer.style_text(SemanticStyle::ImportantPath, path));
        }

        lines.join("\n")
    }
}

fn render_section(renderer: &TerminalRenderer, title: &str, rows: &[KeyValueRow]) -> String {
    let heading = renderer.style_text(SemanticStyle::Heading, title);
    let rendered_rows = render_rows(renderer, rows);
    format!("{heading}\n\n{rendered_rows}")
}

fn render_rows(renderer: &TerminalRenderer, rows: &[KeyValueRow]) -> String {
    let label_width = rows
        .iter()
        .map(|row| measure_text_width(row.label.as_str()))
        .max()
        .unwrap_or_default();

    let value_width = renderer.terminal_width().and_then(|terminal_width| {
        terminal_width.checked_sub(label_width + measure_text_width(KEY_VALUE_SEPARATOR))
    });

    let continuation_indent = " ".repeat(label_width + measure_text_width(KEY_VALUE_SEPARATOR));

    let mut lines = Vec::new();
    for row in rows {
        if row.layout == KeyValueRowLayout::Block {
            let label = renderer.style_text(SemanticStyle::Label, &format!("{}:", row.label));
            lines.push(label);

            for value_line in split_lines(&row.value) {
                let rendered_value = renderer.style_text(row.value_style, &value_line);
                lines.push(format!("  {rendered_value}"));
            }

            continue;
        }

        let label = pad_str(&row.label, label_width, Alignment::Left, None).into_owned();
        let rendered_label = renderer.style_text(SemanticStyle::Label, &label);
        let value_lines = wrap_text(&row.value, value_width);

        let first_value = value_lines.first().map(String::as_str).unwrap_or_default();
        let rendered_value = renderer.style_text(row.value_style, first_value);

        lines.push(format!(
            "{rendered_label}{KEY_VALUE_SEPARATOR}{rendered_value}"
        ));

        for continuation in value_lines.iter().skip(1) {
            let rendered_continuation = renderer.style_text(row.value_style, continuation);
            lines.push(format!("{continuation_indent}{rendered_continuation}"));
        }
    }

    lines.join("\n")
}

fn wrap_text(text: &str, max_width: Option<usize>) -> Vec<String> {
    let Some(max_width) = max_width else {
        return split_lines(text);
    };
    if max_width == 0 {
        return split_lines(text);
    }

    let lines = text
        .lines()
        .flat_map(|line| {
            textwrap::wrap(line, Options::new(max_width).break_words(true))
                .into_iter()
                .map(|line| line.into_owned())
                .collect::<Vec<String>>()
        })
        .collect::<Vec<String>>();

    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn split_lines(text: &str) -> Vec<String> {
    let lines = text.lines().map(str::to_string).collect::<Vec<String>>();

    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn completion_status_text(status: CompletionSummaryStatus) -> &'static str {
    match status {
        CompletionSummaryStatus::Succeeded => "succeeded",
        CompletionSummaryStatus::SucceededWithWarnings => "succeeded with warnings",
        CompletionSummaryStatus::Failed => "failed",
    }
}

fn stage_status_text(status: StageChecklistStatus) -> &'static str {
    match status {
        StageChecklistStatus::Pending => PENDING_MARKER,
        StageChecklistStatus::Active => ACTIVE_MARKER,
        StageChecklistStatus::Completed => COMPLETED_MARKER,
        StageChecklistStatus::Failed => FAILURE_MARKER,
    }
}

fn stage_status_style(status: StageChecklistStatus) -> SemanticStyle {
    match status {
        StageChecklistStatus::Pending => SemanticStyle::Metadata,
        StageChecklistStatus::Active => SemanticStyle::ActiveStage,
        StageChecklistStatus::Completed => SemanticStyle::CompletedMarker,
        StageChecklistStatus::Failed => SemanticStyle::FailureMarker,
    }
}
