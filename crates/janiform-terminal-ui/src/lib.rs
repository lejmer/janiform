//! Shared semantic terminal rendering and progress adapters for Janiform.

#![forbid(unsafe_code)]

mod color;
mod formatting;
mod layout;
mod progress;
mod renderer;
mod style;

pub use color::ColorMode;
pub use formatting::format_artifact_path;
pub use formatting::format_count;
pub use formatting::format_duration_seconds;
pub use formatting::format_path;
pub use formatting::format_selected_candidate;
pub use formatting::format_status_label;
pub use formatting::format_warning_count;
pub use layout::AnalyticalResultTable;
pub use layout::ArtifactPathGroup;
pub use layout::CompactTable;
pub use layout::CompletionSummary;
pub use layout::CompletionSummaryStatus;
pub use layout::FailureBlock;
pub use layout::KeyValueRow;
pub use layout::StageChecklist;
pub use layout::StageChecklistItem;
pub use layout::StageChecklistStatus;
pub use layout::StartSummary;
pub use layout::WarningBlock;
pub use progress::ProgressAdapter;
pub use progress::ProgressContractError;
pub use progress::ProgressEvent;
pub use progress::ProgressMeasure;
pub use progress::ProgressSnapshot;
pub use renderer::TerminalRenderer;
pub use style::SemanticStyle;
