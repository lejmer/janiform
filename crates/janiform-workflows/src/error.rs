use std::io;
use std::path::PathBuf;

use thiserror::Error;

/// Error returned by shared workflow orchestration.
#[derive(Debug, Error)]
pub enum WorkflowError {
    /// Staged execution input could not produce an execution plan.
    #[error("failed to build staged execution plan")]
    StagedExecutionPlanFailed {
        /// Source error.
        #[source]
        source: janiform_calibration_runtime::CalibrationStagedExecutionBridgeError,
    },

    /// The resolved calendar registry could not be loaded.
    #[error("failed to load resolved calendar registry: {message}")]
    CalendarRegistryLoadFailed {
        /// User-readable source message.
        message: String,
    },

    /// Fit execution completed with a failed report.
    #[error("fit execution failed: {message}")]
    FitExecutionFailed {
        /// User-readable failure reason.
        message: String,
    },

    /// Fit artifact writing failed.
    #[error("failed to write fit artifact `{}`", path.display())]
    FitArtifactWrite {
        /// Artifact path.
        path: PathBuf,
        /// Source IO error.
        #[source]
        source: io::Error,
    },

    /// Fit event-log writing failed.
    #[error("failed to write fit event log")]
    FitEventLog {
        /// Source event-log error.
        #[source]
        source: janiform_event_log::EventLogError,
    },

    /// JSON serialization failed.
    #[error("failed to serialize workflow output")]
    OutputSerialize {
        /// Source serialization error.
        #[source]
        source: serde_json::Error,
    },

    /// Output writing failed.
    #[error("failed to write workflow output")]
    OutputWrite {
        /// Source IO error.
        #[source]
        source: io::Error,
    },

    /// Terminal progress rendering failed.
    #[error("failed to render workflow progress")]
    OutputProgress {
        /// Source progress contract error.
        #[source]
        source: janiform_terminal_ui::ProgressContractError,
    },
}
