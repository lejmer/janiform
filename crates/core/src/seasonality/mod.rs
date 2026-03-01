//! Nonparametric seasonality estimators used by deterministic model preprocessing.

mod calendar_buckets;
mod composition;
mod diagnostics;
mod discrete;
mod intraday;
mod normalization;

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

pub const INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION: &str = "seasonality.intraday.estimate.v1";
pub const DAY_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION: &str =
    "seasonality.day_of_week.estimate.v1";
pub const DAY_OF_YEAR_SEASONALITY_ESTIMATE_SCHEMA_VERSION: &str =
    "seasonality.day_of_year.estimate.v1";
pub const DETERMINISTIC_SEASONALITY_COMPOSITION_SCHEMA_VERSION: &str =
    "seasonality.deterministic.composition.v1";
pub const SEASONALITY_DIAGNOSTICS_REPORT_SCHEMA_VERSION: &str = "seasonality.diagnostics.report.v1";
pub const SEASONALITY_DIAGNOSTICS_BATCH_REPORT_SCHEMA_VERSION: &str =
    "seasonality.diagnostics.batch.v1";

const MINUTES_PER_DAY: usize = 1_440;
const DAYS_PER_WEEK: usize = 7;
const DAYS_PER_YEAR: usize = 366;
const IDENTIFIABILITY_NORMALIZATION_POLICY: &str = "mean_factor_anchor.v1";
const IDENTIFIABILITY_TARGET_MEAN_FACTOR: f64 = 1.0;
const IDENTIFIABILITY_MEAN_ANCHOR_TOLERANCE: f64 = 1e-12;
const SPECIAL_OVERLAY_PRECEDENCE_POLICY: &str = "highest_precedence_wins.v1";
const SPECIAL_OVERLAY_MERGE_POLICY: &str = "equal_precedence_additive.v1";
const SPECIAL_OVERLAY_WINDOW_BOUNDARY_POLICY: &str = "start_inclusive_end_exclusive.v1";
const INPUT_SNAPSHOT_FINGERPRINT_ALGORITHM: &str = "fnv1a64.v1";

#[cfg(test)]
use normalization::{
    SeasonalityNormalizationError, normalize_seasonality_factors_for_identifiability,
};
#[cfg(test)]
mod tests;
