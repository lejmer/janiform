use std::path::Path;
use std::time::Duration;

/// Render a path for scan-friendly terminal output.
pub fn format_path(path: &Path) -> String {
    path.display().to_string()
}

/// Render an artifact path using the same stable path contract as ordinary paths.
pub fn format_artifact_path(path: &Path) -> String {
    format_path(path)
}

/// Render a count with an explicit unit label.
pub fn format_count(count: usize, singular_unit: &str, plural_unit: &str) -> String {
    if count == 1 {
        format!("{count} {singular_unit}")
    } else {
        format!("{count} {plural_unit}")
    }
}

/// Render warning counts for summaries and warning blocks.
pub fn format_warning_count(count: usize) -> String {
    format_count(count, "warning", "warnings")
}

/// Render a status label without relying on color for meaning.
pub fn format_status_label(status: &str) -> String {
    status.trim().to_ascii_lowercase().replace('_', " ")
}

/// Render a selected candidate identifier for terminal summaries.
pub fn format_selected_candidate(candidate_id: &str) -> String {
    format!("selected {candidate_id}")
}

/// Render a wall-clock duration from a typed duration contract.
pub fn format_duration_seconds(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let subsecond_millis = duration.subsec_millis();

    if subsecond_millis == 0 {
        format!("{seconds}s")
    } else {
        format!("{seconds}.{subsecond_millis:03}s")
    }
}

/// Render a clock-style elapsed label for live progress displays.
pub fn format_elapsed_label(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    format!("{hours:02}:{minutes:02}:{seconds:02}")
}
