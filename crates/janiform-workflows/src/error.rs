use std::io;
use std::path::PathBuf;

use janiform_foundations::cancellation::CancellationError;
use thiserror::Error;

/// Error returned by shared workflow orchestration.
#[derive(Debug, Error)]
pub enum WorkflowError {
    /// The workflow observed cooperative cancellation.
    #[error("workflow cancelled")]
    Cancelled {
        /// Source cancellation error.
        #[source]
        source: CancellationError,
    },

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

    /// Fit workflow input validation failed.
    #[error("{field}: {message}")]
    FitValidationFailed {
        /// Field that failed validation.
        field: &'static str,
        /// User-readable validation message.
        message: String,
    },

    /// Persisted fitted-model validation failed.
    #[error("{field}: {message}")]
    ModelValidationFailed {
        /// Field that failed validation.
        field: &'static str,
        /// User-readable validation message.
        message: String,
    },

    /// Pricing workflow input validation failed.
    #[error("{field}: {message}")]
    PricingValidationFailed {
        /// Field that failed validation.
        field: &'static str,
        /// User-readable validation message.
        message: String,
    },

    /// Validation workflow input validation failed.
    #[error("{field}: {message}")]
    ValidationInputFailed {
        /// Field that failed validation.
        field: &'static str,
        /// User-readable validation message.
        message: String,
    },

    /// Validation workflow required input was missing.
    #[error("missing required validation input `{field}`")]
    ValidationMissingRequired {
        /// Field that was missing.
        field: &'static str,
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

    /// Simulation workflow input validation failed.
    #[error("{field}: {message}")]
    SimulationValidationFailed {
        /// Field that failed validation.
        field: &'static str,
        /// User-readable validation message.
        message: String,
    },

    /// Forecast workflow input validation failed.
    #[error("{field}: {message}")]
    ForecastValidationFailed {
        /// Field that failed validation.
        field: &'static str,
        /// User-readable validation message.
        message: String,
    },

    /// Simulation artifact writing failed.
    #[error("failed to write simulation artifact `{}`", path.display())]
    SimulationArtifactWrite {
        /// Artifact path.
        path: PathBuf,
        /// Source IO error.
        #[source]
        source: io::Error,
    },

    /// Forecast artifact writing failed.
    #[error("failed to write forecast artifact `{}`", path.display())]
    ForecastArtifactWrite {
        /// Artifact path.
        path: PathBuf,
        /// Source IO error.
        #[source]
        source: io::Error,
    },

    /// Forecast artifact reading failed.
    #[error("failed to read forecast artifact `{}`", path.display())]
    ForecastArtifactRead {
        /// Artifact path.
        path: PathBuf,
        /// Source IO error.
        #[source]
        source: io::Error,
    },

    /// Forecast artifact parsing failed.
    #[error("failed to parse forecast artifact `{}`", path.display())]
    ForecastArtifactParse {
        /// Artifact path.
        path: PathBuf,
        /// Source JSON error.
        #[source]
        source: serde_json::Error,
    },

    /// Forecast artifact payload was malformed.
    #[error("forecast artifact `{}` was malformed: {message}", path.display())]
    ForecastArtifactMalformed {
        /// Artifact path.
        path: PathBuf,
        /// User-readable validation message.
        message: String,
    },

    /// Pricing artifact writing failed.
    #[error("failed to write pricing artifact `{}`", path.display())]
    PricingArtifactWrite {
        /// Artifact path.
        path: PathBuf,
        /// Source IO error.
        #[source]
        source: io::Error,
    },

    /// Validation artifact reading failed.
    #[error("failed to read validation artifact `{}`", path.display())]
    ValidationArtifactRead {
        /// Artifact path.
        path: PathBuf,
        /// Source IO error.
        #[source]
        source: io::Error,
    },

    /// Validation artifact parsing failed.
    #[error("failed to parse validation artifact `{}`", path.display())]
    ValidationArtifactParse {
        /// Artifact path.
        path: PathBuf,
        /// Source JSON error.
        #[source]
        source: serde_json::Error,
    },

    /// Validation artifact payload was malformed.
    #[error("validation artifact `{}` was malformed: {message}", path.display())]
    ValidationArtifactMalformed {
        /// Artifact path.
        path: PathBuf,
        /// User-readable validation message.
        message: String,
    },

    /// Validation artifact writing failed.
    #[error("failed to write validation artifact `{}`", path.display())]
    ValidationArtifactWrite {
        /// Artifact path.
        path: PathBuf,
        /// Source IO error.
        #[source]
        source: io::Error,
    },

    /// Pricing execution failed before producing an artifact.
    #[error("pricing execution failed: {message}")]
    PricingExecutionFailed {
        /// User-readable failure reason.
        message: String,
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

    /// Serialized workflow output did not match the public payload shape.
    #[error("workflow output payload was invalid: {message}")]
    OutputPayloadInvalid {
        /// User-readable validation message.
        message: String,
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

impl From<CancellationError> for WorkflowError {
    fn from(source: CancellationError) -> Self {
        Self::Cancelled { source }
    }
}
