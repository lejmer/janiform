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

pub(crate) const MINUTES_PER_DAY: usize = 1_440;
pub(crate) const DAYS_PER_WEEK: usize = 7;
pub(crate) const DAYS_PER_YEAR: usize = 366;
pub(crate) const IDENTIFIABILITY_NORMALIZATION_POLICY: &str = "mean_factor_anchor.v1";
pub(crate) const IDENTIFIABILITY_TARGET_MEAN_FACTOR: f64 = 1.0;
pub(crate) const IDENTIFIABILITY_MEAN_ANCHOR_TOLERANCE: f64 = 1e-12;
pub(crate) const SPECIAL_OVERLAY_PRECEDENCE_POLICY: &str = "highest_precedence_wins.v1";
pub(crate) const SPECIAL_OVERLAY_MERGE_POLICY: &str = "equal_precedence_additive.v1";
pub(crate) const SPECIAL_OVERLAY_WINDOW_BOUNDARY_POLICY: &str = "start_inclusive_end_exclusive.v1";
pub(crate) const INPUT_SNAPSHOT_FINGERPRINT_ALGORITHM: &str = "fnv1a64.v1";
