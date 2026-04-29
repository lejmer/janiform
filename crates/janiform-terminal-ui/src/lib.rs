//! Shared semantic terminal rendering and progress adapters for Janiform.
//!
//! This crate owns reusable terminal primitives only. Workflow-specific terminal
//! summaries and progress labels live in `janiform-workflows`.

#![forbid(unsafe_code)]

mod color;
mod formatting;
mod layout;
mod policy;
mod progress;
mod renderer;
mod style;

pub use color::ColorMode;
pub use formatting::format_artifact_path;
pub use formatting::format_count;
pub use formatting::format_duration_seconds;
pub use formatting::format_elapsed_label;
pub use formatting::format_file_size;
pub use formatting::format_optional_path_relative_to;
pub use formatting::format_path;
pub use formatting::format_path_relative_to;
pub use formatting::format_selected_candidate;
pub use formatting::format_status_label;
pub use formatting::format_token_as_kebab;
pub use formatting::format_token_as_words;
pub use formatting::format_warning_count;
pub use layout::AnalyticalResultTable;
pub use layout::ArtifactPathGroup;
pub use layout::CompactTable;
pub use layout::CompletionSummary;
pub use layout::CompletionSummaryStatus;
pub use layout::FailureBlock;
pub use layout::KeyValueRow;
pub use layout::KeyValueRowLayout;
pub use layout::StageChecklist;
pub use layout::StageChecklistItem;
pub use layout::StageChecklistStatus;
pub use layout::StartSummary;
pub use layout::WarningBlock;
pub use policy::TerminalSummaryPolicy;
pub use progress::ProgressAdapter;
pub use progress::ProgressContractError;
pub use progress::ProgressEvent;
pub use progress::ProgressMeasure;
pub use progress::ProgressSnapshot;
pub use progress::ProgressWorkItemColumn;
pub use progress::ProgressWorkItemMetric;
pub use renderer::TerminalRenderer;
pub use style::SemanticStyle;
