use std::time::Duration;

use crate::formatting::format_elapsed_label;

use super::ProgressMeasure;
use super::ProgressStageStatus;

const PROGRESS_BAR_WIDTH: usize = 14;
const WORK_ITEM_COMPLETED_STATUS: &str = "completed";
const WORK_ITEM_RUNNING_STATUS: &str = "running";
const WORK_ITEM_WAITING_STATUS: &str = "waiting";

pub(super) fn stage_status_marker(status: ProgressStageStatus) -> &'static str {
    match status {
        ProgressStageStatus::Pending => " ",
        ProgressStageStatus::Running => "→",
        ProgressStageStatus::Completed => "✓",
        ProgressStageStatus::Failed => "✗",
    }
}

pub(super) fn stage_status_text(status: ProgressStageStatus) -> &'static str {
    match status {
        ProgressStageStatus::Pending => "waiting",
        ProgressStageStatus::Running => "running",
        ProgressStageStatus::Completed => "completed",
        ProgressStageStatus::Failed => "failed",
    }
}

pub(super) fn work_item_status_text(progress_percent: Option<usize>) -> &'static str {
    match progress_percent {
        Some(progress_percent) if progress_percent >= 100 => WORK_ITEM_COMPLETED_STATUS,
        _ => WORK_ITEM_RUNNING_STATUS,
    }
}

pub(super) fn work_item_waiting_status_text() -> &'static str {
    WORK_ITEM_WAITING_STATUS
}

pub(super) fn work_item_status_is_completed(status: &str) -> bool {
    status == WORK_ITEM_COMPLETED_STATUS
}

pub(super) fn work_item_status_sort_rank(status: &str) -> usize {
    match status {
        WORK_ITEM_RUNNING_STATUS => 0,
        WORK_ITEM_WAITING_STATUS => 1,
        WORK_ITEM_COMPLETED_STATUS => 2,
        _ => 3,
    }
}

pub(super) fn progress_measure_label(measure: ProgressMeasure) -> String {
    match measure {
        ProgressMeasure::Bounded {
            completed_units,
            total_units,
        } => format!(
            "{:>3}% {}",
            progress_percent(completed_units, total_units),
            progress_bar_label(completed_units, total_units)
        ),
        ProgressMeasure::Percent { percent } => progress_percent_label(percent),
        ProgressMeasure::Unbounded => "-".to_string(),
    }
}

pub(super) fn progress_percent_label(percent: usize) -> String {
    let percent = percent.min(100);

    format!("{percent:>3}% {}", progress_bar_label_from_percent(percent))
}

pub(super) fn progress_percent(completed_units: usize, total_units: usize) -> usize {
    completed_units
        .saturating_mul(100)
        .checked_div(total_units.max(1))
        .unwrap_or(0)
        .min(100)
}

pub(super) fn progress_bar_label(completed_units: usize, total_units: usize) -> String {
    progress_bar_label_from_percent(progress_percent(completed_units, total_units))
}

fn progress_bar_label_from_percent(percent: usize) -> String {
    let filled_width = percent
        .min(100)
        .saturating_mul(PROGRESS_BAR_WIDTH)
        .checked_div(100)
        .unwrap_or(0);

    let empty_width = PROGRESS_BAR_WIDTH.saturating_sub(filled_width);

    "█".repeat(filled_width) + &"·".repeat(empty_width)
}

pub(super) fn truncate_for_table(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }

    value
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "."
}

pub(super) fn format_elapsed_seconds(runtime_seconds: f64) -> String {
    if !runtime_seconds.is_finite() || runtime_seconds <= 0.0 {
        return "00:00:00".to_string();
    }

    format_elapsed_label(Duration::from_secs_f64(runtime_seconds))
}

pub(super) fn format_elapsed_duration(duration: Duration) -> String {
    format_elapsed_label(duration)
}
