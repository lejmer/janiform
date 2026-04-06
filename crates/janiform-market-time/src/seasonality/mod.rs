//! Nonparametric seasonality estimators used by deterministic model preprocessing.

mod calendar_buckets;
mod composition;
mod constants;
mod diagnostics;
mod discrete;
mod intraday;
mod normalization;
mod numerics;

pub use calendar_buckets::{
    DayOfWeekSeasonalityDiagnostics, DayOfWeekSeasonalityEstimateOutput,
    DayOfWeekSeasonalityEstimationError, DayOfWeekSeasonalityEstimatorSettings,
    DayOfYearSeasonalityDiagnostics, DayOfYearSeasonalityEstimateOutput,
    DayOfYearSeasonalityEstimationError, DayOfYearSeasonalityEstimatorSettings,
    estimate_day_of_week_seasonality, estimate_day_of_year_seasonality,
};
pub use composition::{
    DeterministicSeasonalityCompositionDiagnostics, DeterministicSeasonalityCompositionError,
    DeterministicSeasonalityCompositionOutput, ScheduledEventOverlayDefinition,
    ScheduledEventOverlayProvenanceMetadata, SeasonalityNormalizationMetadata,
    compose_deterministic_seasonality_signal,
};
pub use constants::{
    DAY_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
    DAY_OF_YEAR_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
    DETERMINISTIC_SEASONALITY_COMPOSITION_SCHEMA_VERSION,
    INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
    SEASONALITY_DIAGNOSTICS_BATCH_REPORT_SCHEMA_VERSION,
    SEASONALITY_DIAGNOSTICS_REPORT_SCHEMA_VERSION,
};
pub(crate) use constants::{
    DAYS_PER_WEEK, DAYS_PER_YEAR, IDENTIFIABILITY_MEAN_ANCHOR_TOLERANCE,
    IDENTIFIABILITY_NORMALIZATION_POLICY, IDENTIFIABILITY_TARGET_MEAN_FACTOR,
    INPUT_SNAPSHOT_FINGERPRINT_ALGORITHM, MINUTES_PER_DAY, SPECIAL_OVERLAY_MERGE_POLICY,
    SPECIAL_OVERLAY_PRECEDENCE_POLICY, SPECIAL_OVERLAY_WINDOW_BOUNDARY_POLICY,
};
pub use diagnostics::{
    SeasonalityComponentDiagnosticsSummary, SeasonalityDiagnosticsBatchReportOutput,
    SeasonalityDiagnosticsReportError, SeasonalityDiagnosticsReportInput,
    SeasonalityDiagnosticsReportOutput, SeasonalityEstimatorConfigurationMetadata,
    SeasonalityFitQualityMetrics, SeasonalityInputSnapshotMetadata,
    SeasonalityOverlayComponentDiagnosticsSummary, SeasonalityReproducibilityMetadata,
    generate_seasonality_diagnostics_batch_report, generate_seasonality_diagnostics_report,
};
pub use intraday::{
    IntradaySeasonalityDiagnostics, IntradaySeasonalityEstimateOutput,
    IntradaySeasonalityEstimationError, IntradaySeasonalityEstimatorSettings,
    IntradaySmoothingKernel, estimate_intraday_seasonality,
};

#[cfg(test)]
use normalization::{
    SeasonalityNormalizationError, normalize_seasonality_factors_for_identifiability,
};
#[cfg(test)]
mod tests;
