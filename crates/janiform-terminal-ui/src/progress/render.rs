use std::collections::BTreeMap;
use std::time::Instant;

use console::measure_text_width;

use crate::style::SemanticStyle;
use crate::style::style_contract;

use super::display::{
    format_elapsed_duration, progress_bar_label, progress_percent, stage_status_marker,
    stage_status_text, work_item_status_is_completed, work_item_status_sort_rank,
};
use super::table::{
    WorkItemTableColumn, context_work_item_column_width, default_work_item_table_columns,
    format_work_item_table_row,
};
use super::{
    ProgressMeasure, ProgressSnapshot, ProgressStageStatus, ProgressWorkItemColumn,
    ProgressWorkItemSnapshot,
};

#[derive(Clone, Debug, Default)]
pub(super) struct ProgressRenderState {
    pub(super) snapshot: ProgressSnapshot,
    pub(super) run_started_at: Option<Instant>,
    pub(super) active_stage_started_at: Option<Instant>,
    pub(super) active_work_started_at_by_label: BTreeMap<String, Instant>,
    pub(super) color_enabled: bool,
}

impl ProgressRenderState {
    pub(super) fn progress_message(&self) -> String {
        let mut lines = Vec::new();

        self.push_header_lines(&mut lines);
        self.push_stage_lines(&mut lines);
        self.push_phase_and_progress_lines(&mut lines);
        self.push_work_item_lines(&mut lines);

        lines.join("\n")
    }

    fn push_header_lines(&self, lines: &mut Vec<String>) {
        let scope = self.snapshot.scope.as_deref().unwrap_or("Working");
        let total_elapsed = self.total_elapsed_label();

        if let Some(reason) = &self.snapshot.failure_reason {
            lines.push(self.workflow_status_line(scope, "failed", total_elapsed.as_deref()));
            lines.push(format!("  {}", self.labeled_value("Reason:", reason)));

            return;
        }

        if let Some(status) = &self.snapshot.terminal_status {
            lines.push(self.workflow_status_line(scope, status, total_elapsed.as_deref()));

            return;
        }

        if let Some(stage) = &self.snapshot.active_stage {
            lines.push(self.active_stage_header_line(
                scope,
                stage,
                total_elapsed.unwrap_or_else(|| "00:00:00".to_string()),
            ));

            return;
        }

        lines.push(self.workflow_status_line(scope, "running", total_elapsed.as_deref()));
        self.push_run_metadata_lines(lines);
    }

    fn push_stage_lines(&self, lines: &mut Vec<String>) {
        if !self.snapshot.stages.is_empty() {
            lines.push(String::new());
            lines.extend(self.stage_lines());
        }
    }

    fn push_phase_and_progress_lines(&self, lines: &mut Vec<String>) {
        if let Some(phase) = &self.snapshot.phase {
            lines.push(String::new());
            lines.push(format!("  {}", self.labeled_value("Phase:", phase)));
        }

        if let Some(progress_line) = self.progress_line() {
            lines.push(progress_line);
        }
    }

    fn push_work_item_lines(&self, lines: &mut Vec<String>) {
        if !self.snapshot.work_items.is_empty() {
            lines.push(String::new());
            lines.extend(self.work_item_lines());
        }
    }

    fn active_stage_header_line(&self, scope: &str, stage: &str, total_elapsed: String) -> String {
        if let (Some(stage_number), Some(stage_count)) =
            (self.snapshot.stage_number, self.snapshot.stage_count)
        {
            return format!(
                "{} {stage_number}/{stage_count}: {}   {} {}",
                self.styled_active(scope),
                self.styled_value(stage),
                self.styled_label("Total elapsed:"),
                self.styled_value(&total_elapsed)
            );
        }

        format!(
            "{}: {}   {} {}",
            self.styled_active(scope),
            self.styled_value(stage),
            self.styled_label("Total elapsed:"),
            self.styled_value(&total_elapsed)
        )
    }

    fn push_run_metadata_lines(&self, lines: &mut Vec<String>) {
        if let Some(run_id) = &self.snapshot.run_id {
            lines.push(format!("  {}", self.labeled_value("Run:", run_id)));
        }

        if let Some(input_count) = self.snapshot.input_count {
            lines.push(format!(
                "  {}",
                self.labeled_value("Inputs:", &input_count.to_string())
            ));
        }

        if let Some(output_path) = &self.snapshot.output_path {
            lines.push(format!("  {}", self.labeled_value("Output:", output_path)));
        }
    }

    fn progress_line(&self) -> Option<String> {
        match self.snapshot.measure {
            Some(ProgressMeasure::Bounded {
                completed_units,
                total_units,
            }) => Some(self.bounded_progress_line(
                completed_units,
                total_units,
                self.snapshot.latest_signal.as_deref(),
            )),
            Some(ProgressMeasure::Percent { percent }) => {
                Some(self.percent_progress_line(percent, self.snapshot.latest_signal.as_deref()))
            }
            Some(ProgressMeasure::Unbounded) => self
                .snapshot
                .latest_signal
                .as_deref()
                .map(|signal| self.unbounded_progress_line(signal)),
            None => None,
        }
    }

    fn bounded_progress_line(
        &self,
        completed_units: usize,
        total_units: usize,
        signal: Option<&str>,
    ) -> String {
        let progress = self.styled_progress_bar(completed_units, total_units);

        match signal {
            Some(signal) => format!(
                "  {} {progress}, {}",
                self.styled_label("Progress:"),
                self.styled_value(signal)
            ),
            None => format!("  {} {progress}", self.styled_label("Progress:")),
        }
    }

    fn percent_progress_line(&self, percent: usize, signal: Option<&str>) -> String {
        let progress = self.styled_percent_progress_bar(percent);

        match signal {
            Some(signal) => format!(
                "  {} {progress}, {}",
                self.styled_label("Progress:"),
                self.styled_value(signal)
            ),
            None => format!("  {} {progress}", self.styled_label("Progress:")),
        }
    }

    fn unbounded_progress_line(&self, signal: &str) -> String {
        format!(
            "  {} {}",
            self.styled_label("Progress:"),
            self.styled_value(signal)
        )
    }

    fn stage_lines(&self) -> Vec<String> {
        let stage_count = self
            .snapshot
            .stage_count
            .unwrap_or(self.snapshot.stages.len());

        let stage_name_width = self
            .snapshot
            .stages
            .iter()
            .map(|stage| measure_text_width(stage.name.as_str()))
            .max()
            .unwrap_or_default();

        let status_width = "completed".len();

        self.snapshot
            .stages
            .iter()
            .map(|stage| {
                let elapsed = match stage.status {
                    ProgressStageStatus::Running => self.active_stage_elapsed_label(),
                    _ => stage.elapsed.clone(),
                };

                let status = stage_status_text(stage.status);

                let elapsed = elapsed
                    .map(|elapsed| format!("({elapsed})"))
                    .unwrap_or_default();

                let line = format!(
                    "{} {}/{}: {:<stage_name_width$}   {:<status_width$} {}",
                    stage_status_marker(stage.status),
                    stage.stage_number,
                    stage_count,
                    stage.name,
                    status,
                    elapsed
                );

                self.style_stage_line(stage.status, &line)
            })
            .collect()
    }

    fn active_stage_elapsed_label(&self) -> Option<String> {
        self.active_stage_started_at
            .map(|started_at| format_elapsed_duration(started_at.elapsed()))
    }

    fn total_elapsed_label(&self) -> Option<String> {
        self.run_started_at
            .map(|started_at| format_elapsed_duration(started_at.elapsed()))
    }

    fn work_item_lines(&self) -> Vec<String> {
        let columns = self.work_item_table_columns();

        let header = format_work_item_table_row(
            columns.as_slice(),
            columns
                .iter()
                .map(|column| column.label.as_str())
                .collect::<Vec<_>>()
                .as_slice(),
        );

        let mut lines = vec![self.styled(SemanticStyle::TableHeader, &header)];

        let mut work_items = self.snapshot.work_items.iter().collect::<Vec<_>>();
        work_items.sort_by_key(|work_item| work_item_status_sort_rank(&work_item.status));

        lines.extend(
            work_items
                .into_iter()
                .map(|work_item| self.work_item_line(work_item, columns.as_slice())),
        );

        lines
    }

    fn work_item_line(
        &self,
        work_item: &ProgressWorkItemSnapshot,
        columns: &[WorkItemTableColumn],
    ) -> String {
        let elapsed = if work_item_status_is_completed(&work_item.status) {
            work_item.elapsed.clone()
        } else {
            self.active_work_started_at_by_label
                .get(work_item.label.as_str())
                .map(|started_at| format_elapsed_duration(started_at.elapsed()))
                .unwrap_or_else(|| work_item.elapsed.clone())
        };

        let values = columns
            .iter()
            .map(|column| match column.key.as_str() {
                "label" => work_item.label.as_str(),
                "progress" => work_item.progress.as_str(),
                "elapsed" => elapsed.as_str(),
                "status" => work_item.status.as_str(),
                key => work_item
                    .metrics
                    .get(key)
                    .map(String::as_str)
                    .unwrap_or("-"),
            })
            .collect::<Vec<_>>();

        self.styled(
            SemanticStyle::Value,
            &format_work_item_table_row(columns, values.as_slice()),
        )
    }

    fn work_item_table_columns(&self) -> Vec<WorkItemTableColumn> {
        let mut columns = default_work_item_table_columns();

        columns.extend(self.snapshot.work_item_columns.iter().map(|column| {
            WorkItemTableColumn::new(
                column.key.as_str(),
                column.label.as_str(),
                self.context_work_item_column_width(column),
            )
        }));

        columns
    }

    fn context_work_item_column_width(&self, column: &ProgressWorkItemColumn) -> usize {
        let label_width = measure_text_width(column.label.as_str());

        let value_width = self
            .snapshot
            .work_items
            .iter()
            .filter_map(|work_item| work_item.metrics.get(column.key.as_str()))
            .map(|value| measure_text_width(value.as_str()))
            .max()
            .unwrap_or_default();

        context_work_item_column_width(label_width, value_width)
    }

    fn workflow_status_line(
        &self,
        scope: &str,
        status: &str,
        total_elapsed: Option<&str>,
    ) -> String {
        let elapsed = total_elapsed.unwrap_or("00:00:00");

        format!(
            "{}: {}   {} {}",
            self.styled_active(scope),
            self.styled_value(status),
            self.styled_label("Total elapsed:"),
            self.styled_value(elapsed)
        )
    }

    fn styled_progress_bar(&self, completed_units: usize, total_units: usize) -> String {
        let percent = progress_percent(completed_units, total_units);
        let bar = progress_bar_label(completed_units, total_units);

        format!(
            "{percent:>3}% {} {completed_units}/{total_units}",
            self.styled_bar(&bar)
        )
    }

    fn styled_percent_progress_bar(&self, percent: usize) -> String {
        let percent = percent.min(100);
        let bar = progress_bar_label(percent, 100);

        format!("{percent:>3}% {}", self.styled_bar(&bar))
    }

    fn styled_bar(&self, text: &str) -> String {
        self.styled(SemanticStyle::ActiveStage, text)
    }

    fn styled_active(&self, text: &str) -> String {
        self.styled(SemanticStyle::ActiveStage, text)
    }

    fn style_stage_line(&self, status: ProgressStageStatus, text: &str) -> String {
        match status {
            ProgressStageStatus::Pending => self.styled(SemanticStyle::Metadata, text),
            ProgressStageStatus::Running => self.styled(SemanticStyle::ActiveStage, text),
            ProgressStageStatus::Completed => self.styled(SemanticStyle::CompletedMarker, text),
            ProgressStageStatus::CompletedWithWarnings => {
                self.styled(SemanticStyle::WarningMarker, text)
            }
            ProgressStageStatus::Failed => self.styled(SemanticStyle::FailureMarker, text),
        }
    }

    fn labeled_value(&self, label: &str, value: &str) -> String {
        format!("{} {}", self.styled_label(label), self.styled_value(value))
    }

    fn styled_label(&self, text: &str) -> String {
        self.styled(SemanticStyle::Label, text)
    }

    fn styled_value(&self, text: &str) -> String {
        self.styled(SemanticStyle::Value, text)
    }

    fn styled(&self, style: SemanticStyle, text: &str) -> String {
        style_contract(style)
            .force_styling(self.color_enabled)
            .apply_to(text)
            .to_string()
    }
}
