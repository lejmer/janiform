pub const INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION: &str = "seasonality.intraday.estimate.v1";
pub const DAY_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION: &str =
    "seasonality.day_of_week.estimate.v1";
pub const HOUR_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION: &str =
    "seasonality.hour_of_week.estimate.v1";
pub const DAY_OF_YEAR_SEASONALITY_ESTIMATE_SCHEMA_VERSION: &str =
    "seasonality.day_of_year.estimate.v1";
pub const DETERMINISTIC_SEASONALITY_COMPOSITION_SCHEMA_VERSION: &str =
    "seasonality.deterministic.composition.v1";
pub const SEASONALITY_DIAGNOSTICS_REPORT_SCHEMA_VERSION: &str = "seasonality.diagnostics.report.v1";
pub const SEASONALITY_DIAGNOSTICS_BATCH_REPORT_SCHEMA_VERSION: &str =
    "seasonality.diagnostics.batch.v1";

// Exact calendar bin count for a 24-hour intraday seasonality grid.
pub(crate) const MINUTES_PER_DAY: usize = 1_440;
// Exact minute count for one wall-clock hour.
pub(crate) const MINUTES_PER_HOUR: usize = 60;
// Exact hour count for one wall-clock day.
pub(crate) const HOURS_PER_DAY: usize = 24;
// Exact hourly bucket count for a full local calendar week.
pub(crate) const HOURS_PER_WEEK: usize = 168;
// Exact weekday count for day-of-week seasonality factors.
pub(crate) const DAYS_PER_WEEK: usize = 7;
// Calendar-year support includes leap-day indexing, so the factor table always has 366 slots.
pub(crate) const DAYS_PER_YEAR: usize = 366;
pub(crate) const IDENTIFIABILITY_NORMALIZATION_POLICY: &str = "mean_factor_anchor.v1";
// Identifiability normalization anchors the mean deterministic factor at exactly one by contract.
pub(crate) const IDENTIFIABILITY_TARGET_MEAN_FACTOR: f64 = 1.0;
pub(crate) const SPECIAL_OVERLAY_PRECEDENCE_POLICY: &str = "highest_precedence_wins.v1";
pub(crate) const SPECIAL_OVERLAY_MERGE_POLICY: &str = "equal_precedence_additive.v1";
pub(crate) const SPECIAL_OVERLAY_WINDOW_BOUNDARY_POLICY: &str = "start_inclusive_end_exclusive.v1";
pub(crate) const INPUT_SNAPSHOT_FINGERPRINT_ALGORITHM: &str = "fnv1a64.v1";
