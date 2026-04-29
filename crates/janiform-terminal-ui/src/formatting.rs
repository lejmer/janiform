use std::path::Path;
use std::time::Duration;

const BYTES_PER_KIBIBYTE: u64 = 1024;

/// Render a path for scan-friendly terminal output.
pub fn format_path(path: &Path) -> String {
    path.display().to_string()
}

/// Render a path relative to a base directory when possible.
pub fn format_path_relative_to(base_directory: &Path, path: &Path) -> String {
    if let Ok(relative_path) = path.strip_prefix(base_directory) {
        return format_path(relative_path);
    }

    format_path(path)
}

/// Render an optional path-like value relative to a base directory when possible.
pub fn format_optional_path_relative_to(
    base_directory: &Path,
    path: Option<&str>,
) -> Option<String> {
    path.map(|path| format_path_relative_to(base_directory, Path::new(path)))
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
    format_token_as_words(&status.to_ascii_lowercase())
}

/// Render a machine token as separate words.
pub fn format_token_as_words(value: &str) -> String {
    value.trim().replace('_', " ")
}

/// Render a machine token with hyphen separators.
pub fn format_token_as_kebab(value: &str) -> String {
    value.trim().replace('_', "-")
}

/// Render a file size for compact terminal summaries.
pub fn format_file_size(byte_count: u64) -> String {
    if byte_count < BYTES_PER_KIBIBYTE {
        return format!("{byte_count} B");
    }

    let kibibytes = byte_count / BYTES_PER_KIBIBYTE;

    format!("{kibibytes} KiB")
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
