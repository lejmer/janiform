//! Nonparametric seasonality estimators used by deterministic model preprocessing.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Serialize;

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

/// Supported smoothing kernels for nonparametric intraday seasonality estimation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntradaySmoothingKernel {
    Gaussian,
    Uniform,
}

impl IntradaySmoothingKernel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gaussian => "gaussian",
            Self::Uniform => "uniform",
        }
    }
}

/// Configuration for intraday seasonality estimation from timestamp event density.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntradaySeasonalityEstimatorSettings {
    pub minutes_per_bin: u16,
    pub smoothing_kernel: IntradaySmoothingKernel,
    pub smoothing_bandwidth_bins: f64,
    pub minimum_bin_events: usize,
    pub pseudocount: f64,
}

impl Default for IntradaySeasonalityEstimatorSettings {
    fn default() -> Self {
        Self {
            minutes_per_bin: 1,
            smoothing_kernel: IntradaySmoothingKernel::Gaussian,
            smoothing_bandwidth_bins: 3.0,
            minimum_bin_events: 1,
            pseudocount: 1e-6,
        }
    }
}

/// Diagnostics emitted by intraday seasonality estimation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntradaySeasonalityDiagnostics {
    pub input_event_count: usize,
    pub bin_count: usize,
    pub zero_event_bin_count: usize,
    pub sparse_bin_count: usize,
}

/// Deterministic nonparametric intraday seasonality estimate.
#[derive(Debug, Clone, PartialEq)]
pub struct IntradaySeasonalityEstimateOutput {
    pub schema_version: &'static str,
    pub minutes_per_bin: u16,
    pub smoothing_kernel: IntradaySmoothingKernel,
    pub smoothing_bandwidth_bins: f64,
    pub minimum_bin_events: usize,
    pub pseudocount: f64,
    pub bin_start_minute_of_day: Vec<u16>,
    pub event_count_by_bin: Vec<u64>,
    pub sparse_bin_flags: Vec<u8>,
    pub smoothed_density_by_bin: Vec<f64>,
    pub intraday_log_component_by_bin: Vec<f64>,
    pub intraday_factor_by_bin: Vec<f64>,
    pub diagnostics: IntradaySeasonalityDiagnostics,
}

/// Estimate a nonparametric intraday seasonality component from local minute-of-day events.
pub fn estimate_intraday_seasonality(
    exchange_local_minute_of_day: &[u16],
    settings: IntradaySeasonalityEstimatorSettings,
) -> Result<IntradaySeasonalityEstimateOutput, IntradaySeasonalityEstimationError> {
    if exchange_local_minute_of_day.is_empty() {
        return Err(IntradaySeasonalityEstimationError::EmptyInputEvents);
    }

    if settings.minutes_per_bin == 0
        || usize::from(settings.minutes_per_bin) > MINUTES_PER_DAY
        || !MINUTES_PER_DAY.is_multiple_of(usize::from(settings.minutes_per_bin))
    {
        return Err(IntradaySeasonalityEstimationError::InvalidMinutesPerBin {
            minutes_per_bin: settings.minutes_per_bin,
        });
    }

    if !settings.smoothing_bandwidth_bins.is_finite() || settings.smoothing_bandwidth_bins <= 0.0 {
        return Err(
            IntradaySeasonalityEstimationError::InvalidSmoothingBandwidthBins {
                smoothing_bandwidth_bins: settings.smoothing_bandwidth_bins,
            },
        );
    }

    if settings.minimum_bin_events < 1 {
        return Err(
            IntradaySeasonalityEstimationError::InvalidMinimumBinEvents {
                minimum_bin_events: settings.minimum_bin_events,
            },
        );
    }

    if !settings.pseudocount.is_finite() || settings.pseudocount <= 0.0 {
        return Err(IntradaySeasonalityEstimationError::InvalidPseudocount {
            pseudocount: settings.pseudocount,
        });
    }

    let bin_count: usize = MINUTES_PER_DAY / usize::from(settings.minutes_per_bin);
    let mut event_count_by_bin: Vec<u64> = vec![0; bin_count];

    for (record_position, minute_of_day) in exchange_local_minute_of_day.iter().copied().enumerate()
    {
        if usize::from(minute_of_day) >= MINUTES_PER_DAY {
            return Err(IntradaySeasonalityEstimationError::InvalidMinuteOfDay {
                record_position,
                minute_of_day,
            });
        }
        let bin_index: usize = usize::from(minute_of_day / settings.minutes_per_bin);
        event_count_by_bin[bin_index] += 1;
    }

    let zero_event_bin_count: usize = event_count_by_bin
        .iter()
        .copied()
        .filter(|event_count| *event_count == 0)
        .count();
    let sparse_bin_flags: Vec<u8> = event_count_by_bin
        .iter()
        .copied()
        .map(|event_count| {
            if event_count < settings.minimum_bin_events as u64 {
                1
            } else {
                0
            }
        })
        .collect();
    let sparse_bin_count: usize = sparse_bin_flags
        .iter()
        .copied()
        .filter(|sparse_flag| *sparse_flag == 1)
        .count();

    let baseline_count_by_bin: Vec<f64> = event_count_by_bin
        .iter()
        .map(|event_count| *event_count as f64 + settings.pseudocount)
        .collect();
    let (kernel_offsets, kernel_weights) = build_intraday_smoothing_kernel(
        bin_count,
        settings.smoothing_kernel,
        settings.smoothing_bandwidth_bins,
    );
    let smoothed_count_by_bin =
        circular_smooth_values(&baseline_count_by_bin, &kernel_offsets, &kernel_weights);
    let total_smoothed_count: f64 = smoothed_count_by_bin.iter().sum();

    if !total_smoothed_count.is_finite() || total_smoothed_count <= 0.0 {
        return Err(IntradaySeasonalityEstimationError::UnstableNormalization);
    }

    let bin_count_float = bin_count as f64;
    let smoothed_density_by_bin: Vec<f64> = smoothed_count_by_bin
        .iter()
        .map(|smoothed_count| *smoothed_count / total_smoothed_count)
        .collect();
    let intraday_factor_by_bin: Vec<f64> = smoothed_density_by_bin
        .iter()
        .map(|smoothed_density| *smoothed_density * bin_count_float)
        .collect();
    let intraday_log_component_by_bin: Vec<f64> = intraday_factor_by_bin
        .iter()
        .map(|intraday_factor| intraday_factor.ln())
        .collect();
    let bin_start_minute_of_day: Vec<u16> = (0..bin_count)
        .map(|bin_index| (bin_index * usize::from(settings.minutes_per_bin)) as u16)
        .collect();

    Ok(IntradaySeasonalityEstimateOutput {
        schema_version: INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
        minutes_per_bin: settings.minutes_per_bin,
        smoothing_kernel: settings.smoothing_kernel,
        smoothing_bandwidth_bins: settings.smoothing_bandwidth_bins,
        minimum_bin_events: settings.minimum_bin_events,
        pseudocount: settings.pseudocount,
        bin_start_minute_of_day,
        event_count_by_bin,
        sparse_bin_flags,
        smoothed_density_by_bin,
        intraday_log_component_by_bin,
        intraday_factor_by_bin,
        diagnostics: IntradaySeasonalityDiagnostics {
            input_event_count: exchange_local_minute_of_day.len(),
            bin_count,
            zero_event_bin_count,
            sparse_bin_count,
        },
    })
}

fn build_intraday_smoothing_kernel(
    bin_count: usize,
    smoothing_kernel: IntradaySmoothingKernel,
    smoothing_bandwidth_bins: f64,
) -> (Vec<isize>, Vec<f64>) {
    let max_radius = (bin_count / 2) as isize;
    let raw_radius = match smoothing_kernel {
        IntradaySmoothingKernel::Gaussian => (smoothing_bandwidth_bins * 3.0).ceil() as isize,
        IntradaySmoothingKernel::Uniform => smoothing_bandwidth_bins.floor() as isize,
    };
    let radius = raw_radius.max(0).min(max_radius);

    if radius == 0 {
        return (vec![0], vec![1.0]);
    }

    let mut offsets: Vec<isize> = Vec::with_capacity((radius * 2 + 1) as usize);
    let mut weights: Vec<f64> = Vec::with_capacity((radius * 2 + 1) as usize);

    for offset in -radius..=radius {
        offsets.push(offset);
        let weight = match smoothing_kernel {
            IntradaySmoothingKernel::Gaussian => {
                let standardized_offset = offset as f64 / smoothing_bandwidth_bins;
                (-0.5 * standardized_offset.powi(2)).exp()
            }
            IntradaySmoothingKernel::Uniform => 1.0,
        };
        weights.push(weight);
    }

    let total_weight: f64 = weights.iter().sum();
    if total_weight > 0.0 && total_weight.is_finite() {
        for weight in &mut weights {
            *weight /= total_weight;
        }
    }

    (offsets, weights)
}

fn circular_smooth_values(
    values: &[f64],
    kernel_offsets: &[isize],
    kernel_weights: &[f64],
) -> Vec<f64> {
    let mut smoothed_values: Vec<f64> = vec![0.0; values.len()];
    let value_count = values.len() as isize;

    for target_position in 0..values.len() {
        let mut smoothed_value = 0.0;

        for (kernel_position, kernel_offset) in kernel_offsets.iter().enumerate() {
            let source_position =
                (target_position as isize + *kernel_offset).rem_euclid(value_count) as usize;
            smoothed_value += values[source_position] * kernel_weights[kernel_position];
        }

        smoothed_values[target_position] = smoothed_value;
    }

    smoothed_values
}

#[derive(Debug, Clone, PartialEq)]
pub enum IntradaySeasonalityEstimationError {
    EmptyInputEvents,
    InvalidMinutesPerBin {
        minutes_per_bin: u16,
    },
    InvalidSmoothingBandwidthBins {
        smoothing_bandwidth_bins: f64,
    },
    InvalidMinimumBinEvents {
        minimum_bin_events: usize,
    },
    InvalidPseudocount {
        pseudocount: f64,
    },
    InvalidMinuteOfDay {
        record_position: usize,
        minute_of_day: u16,
    },
    UnstableNormalization,
}

impl Display for IntradaySeasonalityEstimationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInputEvents => {
                write!(
                    formatter,
                    "intraday seasonality estimation requires at least one accepted event"
                )
            }
            Self::InvalidMinutesPerBin { minutes_per_bin } => write!(
                formatter,
                "minutes_per_bin must be a positive divisor of 1440, received {minutes_per_bin}"
            ),
            Self::InvalidSmoothingBandwidthBins {
                smoothing_bandwidth_bins,
            } => write!(
                formatter,
                "smoothing_bandwidth_bins must be a finite positive value, received {smoothing_bandwidth_bins}"
            ),
            Self::InvalidMinimumBinEvents { minimum_bin_events } => write!(
                formatter,
                "minimum_bin_events must be at least 1, received {minimum_bin_events}"
            ),
            Self::InvalidPseudocount { pseudocount } => write!(
                formatter,
                "pseudocount must be a finite positive value, received {pseudocount}"
            ),
            Self::InvalidMinuteOfDay {
                record_position,
                minute_of_day,
            } => write!(
                formatter,
                "event record at position {record_position} has invalid local minute-of-day value {minute_of_day}"
            ),
            Self::UnstableNormalization => write!(
                formatter,
                "intraday seasonality estimation produced a non-positive normalization constant"
            ),
        }
    }
}

impl Error for IntradaySeasonalityEstimationError {}

/// Configuration for day-of-week seasonality estimation from timestamp event density.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayOfWeekSeasonalityEstimatorSettings {
    pub minimum_bucket_events: usize,
    pub pseudocount: f64,
}

impl Default for DayOfWeekSeasonalityEstimatorSettings {
    fn default() -> Self {
        Self {
            minimum_bucket_events: 1,
            pseudocount: 1e-6,
        }
    }
}

/// Diagnostics emitted by day-of-week seasonality estimation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayOfWeekSeasonalityDiagnostics {
    pub input_event_count: usize,
    pub bucket_count: usize,
    pub zero_event_bucket_count: usize,
    pub sparse_bucket_count: usize,
    pub fallback_bucket_count: usize,
}

/// Deterministic nonparametric day-of-week seasonality estimate.
#[derive(Debug, Clone, PartialEq)]
pub struct DayOfWeekSeasonalityEstimateOutput {
    pub schema_version: &'static str,
    pub minimum_bucket_events: usize,
    pub pseudocount: f64,
    pub day_of_week_number: Vec<u8>,
    pub event_count_by_day_of_week: Vec<u64>,
    pub sparse_bucket_flags: Vec<u8>,
    pub fallback_bucket_flags: Vec<u8>,
    pub day_of_week_log_component: Vec<f64>,
    pub day_of_week_factor: Vec<f64>,
    pub diagnostics: DayOfWeekSeasonalityDiagnostics,
}

/// Estimate a nonparametric day-of-week seasonality component.
pub fn estimate_day_of_week_seasonality(
    exchange_local_day_of_week: &[u8],
    settings: DayOfWeekSeasonalityEstimatorSettings,
) -> Result<DayOfWeekSeasonalityEstimateOutput, DayOfWeekSeasonalityEstimationError> {
    if exchange_local_day_of_week.is_empty() {
        return Err(DayOfWeekSeasonalityEstimationError::EmptyInputEvents);
    }

    if settings.minimum_bucket_events < 1 {
        return Err(
            DayOfWeekSeasonalityEstimationError::InvalidMinimumBucketEvents {
                minimum_bucket_events: settings.minimum_bucket_events,
            },
        );
    }

    if !settings.pseudocount.is_finite() || settings.pseudocount <= 0.0 {
        return Err(DayOfWeekSeasonalityEstimationError::InvalidPseudocount {
            pseudocount: settings.pseudocount,
        });
    }

    let mut bucket_index_by_event: Vec<usize> =
        Vec::with_capacity(exchange_local_day_of_week.len());
    for (record_position, day_of_week) in exchange_local_day_of_week.iter().copied().enumerate() {
        if !(1..=7).contains(&day_of_week) {
            return Err(DayOfWeekSeasonalityEstimationError::InvalidDayOfWeek {
                record_position,
                day_of_week,
            });
        }

        bucket_index_by_event.push((day_of_week - 1) as usize);
    }

    let estimate_by_bucket = estimate_discrete_seasonality_by_bucket(
        &bucket_index_by_event,
        DAYS_PER_WEEK,
        settings.minimum_bucket_events,
        settings.pseudocount,
    )
    .map_err(|_| DayOfWeekSeasonalityEstimationError::UnstableNormalization)?;

    Ok(DayOfWeekSeasonalityEstimateOutput {
        schema_version: DAY_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
        minimum_bucket_events: settings.minimum_bucket_events,
        pseudocount: settings.pseudocount,
        day_of_week_number: (1..=DAYS_PER_WEEK)
            .map(|day_of_week| day_of_week as u8)
            .collect(),
        event_count_by_day_of_week: estimate_by_bucket.event_count_by_bucket,
        sparse_bucket_flags: estimate_by_bucket.sparse_bucket_flags,
        fallback_bucket_flags: estimate_by_bucket.fallback_bucket_flags,
        day_of_week_log_component: estimate_by_bucket.log_component_by_bucket,
        day_of_week_factor: estimate_by_bucket.factor_by_bucket,
        diagnostics: DayOfWeekSeasonalityDiagnostics {
            input_event_count: exchange_local_day_of_week.len(),
            bucket_count: DAYS_PER_WEEK,
            zero_event_bucket_count: estimate_by_bucket.zero_event_bucket_count,
            sparse_bucket_count: estimate_by_bucket.sparse_bucket_count,
            fallback_bucket_count: estimate_by_bucket.fallback_bucket_count,
        },
    })
}

/// Configuration for day-of-year seasonality estimation from timestamp event density.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayOfYearSeasonalityEstimatorSettings {
    pub minimum_bucket_events: usize,
    pub pseudocount: f64,
}

impl Default for DayOfYearSeasonalityEstimatorSettings {
    fn default() -> Self {
        Self {
            minimum_bucket_events: 1,
            pseudocount: 1e-6,
        }
    }
}

/// Diagnostics emitted by day-of-year seasonality estimation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayOfYearSeasonalityDiagnostics {
    pub input_event_count: usize,
    pub bucket_count: usize,
    pub zero_event_bucket_count: usize,
    pub sparse_bucket_count: usize,
    pub fallback_bucket_count: usize,
}

/// Deterministic nonparametric day-of-year seasonality estimate.
#[derive(Debug, Clone, PartialEq)]
pub struct DayOfYearSeasonalityEstimateOutput {
    pub schema_version: &'static str,
    pub minimum_bucket_events: usize,
    pub pseudocount: f64,
    pub day_of_year_number: Vec<u16>,
    pub event_count_by_day_of_year: Vec<u64>,
    pub sparse_bucket_flags: Vec<u8>,
    pub fallback_bucket_flags: Vec<u8>,
    pub day_of_year_log_component: Vec<f64>,
    pub day_of_year_factor: Vec<f64>,
    pub diagnostics: DayOfYearSeasonalityDiagnostics,
}

/// Estimate a nonparametric day-of-year seasonality component.
pub fn estimate_day_of_year_seasonality(
    exchange_local_day_of_year: &[u16],
    settings: DayOfYearSeasonalityEstimatorSettings,
) -> Result<DayOfYearSeasonalityEstimateOutput, DayOfYearSeasonalityEstimationError> {
    if exchange_local_day_of_year.is_empty() {
        return Err(DayOfYearSeasonalityEstimationError::EmptyInputEvents);
    }

    if settings.minimum_bucket_events < 1 {
        return Err(
            DayOfYearSeasonalityEstimationError::InvalidMinimumBucketEvents {
                minimum_bucket_events: settings.minimum_bucket_events,
            },
        );
    }

    if !settings.pseudocount.is_finite() || settings.pseudocount <= 0.0 {
        return Err(DayOfYearSeasonalityEstimationError::InvalidPseudocount {
            pseudocount: settings.pseudocount,
        });
    }

    let mut bucket_index_by_event: Vec<usize> =
        Vec::with_capacity(exchange_local_day_of_year.len());
    for (record_position, day_of_year) in exchange_local_day_of_year.iter().copied().enumerate() {
        if !(1..=366).contains(&day_of_year) {
            return Err(DayOfYearSeasonalityEstimationError::InvalidDayOfYear {
                record_position,
                day_of_year,
            });
        }

        bucket_index_by_event.push((day_of_year - 1) as usize);
    }

    let estimate_by_bucket = estimate_discrete_seasonality_by_bucket(
        &bucket_index_by_event,
        DAYS_PER_YEAR,
        settings.minimum_bucket_events,
        settings.pseudocount,
    )
    .map_err(|_| DayOfYearSeasonalityEstimationError::UnstableNormalization)?;

    Ok(DayOfYearSeasonalityEstimateOutput {
        schema_version: DAY_OF_YEAR_SEASONALITY_ESTIMATE_SCHEMA_VERSION,
        minimum_bucket_events: settings.minimum_bucket_events,
        pseudocount: settings.pseudocount,
        day_of_year_number: (1..=DAYS_PER_YEAR)
            .map(|day_of_year| day_of_year as u16)
            .collect(),
        event_count_by_day_of_year: estimate_by_bucket.event_count_by_bucket,
        sparse_bucket_flags: estimate_by_bucket.sparse_bucket_flags,
        fallback_bucket_flags: estimate_by_bucket.fallback_bucket_flags,
        day_of_year_log_component: estimate_by_bucket.log_component_by_bucket,
        day_of_year_factor: estimate_by_bucket.factor_by_bucket,
        diagnostics: DayOfYearSeasonalityDiagnostics {
            input_event_count: exchange_local_day_of_year.len(),
            bucket_count: DAYS_PER_YEAR,
            zero_event_bucket_count: estimate_by_bucket.zero_event_bucket_count,
            sparse_bucket_count: estimate_by_bucket.sparse_bucket_count,
            fallback_bucket_count: estimate_by_bucket.fallback_bucket_count,
        },
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeasonalityNormalizationMetadata {
    pub normalization_policy: &'static str,
    pub fit_window_event_count: usize,
    pub target_mean_factor: f64,
    pub pre_normalization_mean_factor: f64,
    pub normalization_constant: f64,
    pub normalization_log_offset: f64,
    pub post_normalization_mean_factor: f64,
    pub mean_anchor_tolerance: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledEventOverlayDefinition {
    pub overlay_id: String,
    pub start_timestamp_utc_nanos: i64,
    pub end_timestamp_utc_nanos: i64,
    pub log_adjustment: f64,
    pub precedence: u16,
    pub enabled: bool,
    pub source_label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledEventOverlayProvenanceMetadata {
    pub overlays_enabled: bool,
    pub configured_overlay_count: usize,
    pub enabled_overlay_count: usize,
    pub active_overlay_event_count: usize,
    pub precedence_policy: &'static str,
    pub merge_policy: &'static str,
    pub window_boundary_policy: &'static str,
    pub configured_overlay_ids_in_order: Vec<String>,
    pub applied_overlay_ids_in_order: Vec<String>,
    pub overlay_source_labels_in_order: Vec<String>,
}

/// Diagnostics emitted by deterministic seasonality composition.
#[derive(Debug, Clone, PartialEq)]
pub struct DeterministicSeasonalityCompositionDiagnostics {
    pub input_event_count: usize,
    pub normalization_metadata: SeasonalityNormalizationMetadata,
    pub overlay_provenance_metadata: ScheduledEventOverlayProvenanceMetadata,
}

/// Deterministic composition output for intraday, day-of-week, and day-of-year components.
#[derive(Debug, Clone, PartialEq)]
pub struct DeterministicSeasonalityCompositionOutput {
    pub schema_version: &'static str,
    pub intraday_minutes_per_bin: u16,
    pub intraday_bin_index_by_event: Vec<u16>,
    pub intraday_log_component_by_event: Vec<f64>,
    pub day_of_week_log_component_by_event: Vec<f64>,
    pub day_of_year_log_component_by_event: Vec<f64>,
    pub special_log_component_by_event: Vec<f64>,
    pub special_overlay_active_flags_by_event: Vec<u8>,
    pub active_overlay_ids_by_event: Vec<String>,
    pub combined_log_component_by_event: Vec<f64>,
    pub combined_factor_by_event: Vec<f64>,
    pub diagnostics: DeterministicSeasonalityCompositionDiagnostics,
}

/// Compose intraday, day-of-week, and day-of-year log components into a unified signal.
pub fn compose_deterministic_seasonality_signal(
    timestamp_utc_nanos: &[i64],
    exchange_local_minute_of_day: &[u16],
    exchange_local_day_of_week: &[u8],
    exchange_local_day_of_year: &[u16],
    intraday_minutes_per_bin: u16,
    intraday_log_component_by_bin: &[f64],
    day_of_week_log_component: &[f64],
    day_of_year_log_component: &[f64],
    scheduled_event_overlays: &[ScheduledEventOverlayDefinition],
) -> Result<DeterministicSeasonalityCompositionOutput, DeterministicSeasonalityCompositionError> {
    if timestamp_utc_nanos.is_empty() {
        return Err(DeterministicSeasonalityCompositionError::EmptyInputEvents);
    }

    let input_event_count = timestamp_utc_nanos.len();

    if exchange_local_day_of_week.len() != input_event_count
        || exchange_local_day_of_year.len() != input_event_count
        || exchange_local_minute_of_day.len() != input_event_count
    {
        return Err(
            DeterministicSeasonalityCompositionError::MismatchedInputLengths {
                timestamp_utc_nanos_count: input_event_count,
                minute_of_day_count: exchange_local_minute_of_day.len(),
                day_of_week_count: exchange_local_day_of_week.len(),
                day_of_year_count: exchange_local_day_of_year.len(),
            },
        );
    }

    if intraday_minutes_per_bin == 0
        || usize::from(intraday_minutes_per_bin) > MINUTES_PER_DAY
        || !MINUTES_PER_DAY.is_multiple_of(usize::from(intraday_minutes_per_bin))
    {
        return Err(
            DeterministicSeasonalityCompositionError::InvalidIntradayMinutesPerBin {
                intraday_minutes_per_bin,
            },
        );
    }

    let intraday_bin_count = MINUTES_PER_DAY / usize::from(intraday_minutes_per_bin);

    if intraday_log_component_by_bin.len() != intraday_bin_count {
        return Err(
            DeterministicSeasonalityCompositionError::InvalidIntradayComponentLength {
                expected_intraday_bin_count: intraday_bin_count,
                actual_intraday_bin_count: intraday_log_component_by_bin.len(),
            },
        );
    }

    if day_of_week_log_component.len() != DAYS_PER_WEEK {
        return Err(
            DeterministicSeasonalityCompositionError::InvalidDayOfWeekComponentLength {
                expected_day_of_week_bucket_count: DAYS_PER_WEEK,
                actual_day_of_week_bucket_count: day_of_week_log_component.len(),
            },
        );
    }

    if day_of_year_log_component.len() != DAYS_PER_YEAR {
        return Err(
            DeterministicSeasonalityCompositionError::InvalidDayOfYearComponentLength {
                expected_day_of_year_bucket_count: DAYS_PER_YEAR,
                actual_day_of_year_bucket_count: day_of_year_log_component.len(),
            },
        );
    }

    let mut configured_overlay_ids_in_order: Vec<String> =
        Vec::with_capacity(scheduled_event_overlays.len());
    let mut overlay_source_labels_in_order: Vec<String> =
        Vec::with_capacity(scheduled_event_overlays.len());
    let mut observed_overlay_ids: BTreeSet<String> = BTreeSet::new();
    let mut enabled_overlay_count = 0_usize;

    for (overlay_position, overlay) in scheduled_event_overlays.iter().enumerate() {
        if overlay.overlay_id.trim().is_empty() {
            return Err(DeterministicSeasonalityCompositionError::EmptyOverlayId {
                overlay_position,
            });
        }

        if !observed_overlay_ids.insert(overlay.overlay_id.clone()) {
            return Err(
                DeterministicSeasonalityCompositionError::DuplicateOverlayId {
                    overlay_id: overlay.overlay_id.clone(),
                },
            );
        }

        if !overlay.log_adjustment.is_finite() {
            return Err(
                DeterministicSeasonalityCompositionError::InvalidOverlayLogAdjustment {
                    overlay_position,
                },
            );
        }

        if overlay.start_timestamp_utc_nanos >= overlay.end_timestamp_utc_nanos {
            return Err(
                DeterministicSeasonalityCompositionError::InvalidOverlayWindow {
                    overlay_position,
                    start_timestamp_utc_nanos: overlay.start_timestamp_utc_nanos,
                    end_timestamp_utc_nanos: overlay.end_timestamp_utc_nanos,
                },
            );
        }

        if overlay.enabled {
            enabled_overlay_count += 1;
        }

        configured_overlay_ids_in_order.push(overlay.overlay_id.clone());
        overlay_source_labels_in_order.push(overlay.source_label.clone());
    }

    let mut intraday_bin_index_by_event: Vec<u16> = Vec::with_capacity(input_event_count);
    let mut intraday_log_component_by_event: Vec<f64> = Vec::with_capacity(input_event_count);
    let mut day_of_week_log_component_by_event: Vec<f64> = Vec::with_capacity(input_event_count);
    let mut day_of_year_log_component_by_event: Vec<f64> = Vec::with_capacity(input_event_count);
    let mut special_log_component_by_event: Vec<f64> = Vec::with_capacity(input_event_count);
    let mut special_overlay_active_flags_by_event: Vec<u8> = Vec::with_capacity(input_event_count);
    let mut active_overlay_ids_by_event: Vec<String> = Vec::with_capacity(input_event_count);
    let mut combined_log_component_by_event: Vec<f64> = Vec::with_capacity(input_event_count);
    let mut combined_factor_by_event: Vec<f64> = Vec::with_capacity(input_event_count);
    let mut overlay_application_count_by_index: Vec<u64> = vec![0; scheduled_event_overlays.len()];
    let mut active_overlay_event_count = 0_usize;

    for record_position in 0..input_event_count {
        let minute_of_day = exchange_local_minute_of_day[record_position];
        if usize::from(minute_of_day) >= MINUTES_PER_DAY {
            return Err(
                DeterministicSeasonalityCompositionError::InvalidMinuteOfDay {
                    record_position,
                    minute_of_day,
                },
            );
        }

        let day_of_week = exchange_local_day_of_week[record_position];
        if !(1..=7).contains(&day_of_week) {
            return Err(DeterministicSeasonalityCompositionError::InvalidDayOfWeek {
                record_position,
                day_of_week,
            });
        }

        let day_of_year = exchange_local_day_of_year[record_position];
        if !(1..=366).contains(&day_of_year) {
            return Err(DeterministicSeasonalityCompositionError::InvalidDayOfYear {
                record_position,
                day_of_year,
            });
        }

        let intraday_bin_index = minute_of_day / intraday_minutes_per_bin;
        let intraday_log_component = intraday_log_component_by_bin[usize::from(intraday_bin_index)];
        let day_of_week_log_component_value = day_of_week_log_component[(day_of_week - 1) as usize];
        let day_of_year_log_component_value = day_of_year_log_component[(day_of_year - 1) as usize];
        let event_timestamp_utc_nanos = timestamp_utc_nanos[record_position];
        let mut selected_overlay_indices: Vec<usize> = Vec::new();
        let mut selected_overlay_precedence: Option<u16> = None;
        let mut special_log_component = 0.0;

        for (overlay_index, overlay) in scheduled_event_overlays.iter().enumerate() {
            if !overlay.enabled {
                continue;
            }

            if event_timestamp_utc_nanos < overlay.start_timestamp_utc_nanos
                || event_timestamp_utc_nanos >= overlay.end_timestamp_utc_nanos
            {
                continue;
            }

            match selected_overlay_precedence {
                None => {
                    selected_overlay_precedence = Some(overlay.precedence);
                    selected_overlay_indices.push(overlay_index);
                    special_log_component = overlay.log_adjustment;
                }
                Some(active_precedence) if overlay.precedence > active_precedence => {
                    selected_overlay_precedence = Some(overlay.precedence);
                    selected_overlay_indices.clear();
                    selected_overlay_indices.push(overlay_index);
                    special_log_component = overlay.log_adjustment;
                }
                Some(active_precedence) if overlay.precedence == active_precedence => {
                    selected_overlay_indices.push(overlay_index);
                    special_log_component += overlay.log_adjustment;
                }
                Some(_) => {}
            }
        }

        if !special_log_component.is_finite() {
            return Err(
                DeterministicSeasonalityCompositionError::InvalidSpecialOverlayLogComponent {
                    record_position,
                },
            );
        }

        let mut active_overlay_ids_for_event: Vec<String> =
            Vec::with_capacity(selected_overlay_indices.len());

        if selected_overlay_indices.is_empty() {
            special_overlay_active_flags_by_event.push(0);
            active_overlay_ids_by_event.push(String::new());
        } else {
            special_overlay_active_flags_by_event.push(1);
            active_overlay_event_count += 1;

            for selected_overlay_index in selected_overlay_indices {
                let selected_overlay = &scheduled_event_overlays[selected_overlay_index];
                active_overlay_ids_for_event.push(selected_overlay.overlay_id.clone());
                overlay_application_count_by_index[selected_overlay_index] += 1;
            }

            active_overlay_ids_for_event.sort_unstable();
            active_overlay_ids_by_event.push(active_overlay_ids_for_event.join(","));
        }

        let combined_log_component = intraday_log_component
            + day_of_week_log_component_value
            + day_of_year_log_component_value
            + special_log_component;

        if !combined_log_component.is_finite() {
            return Err(
                DeterministicSeasonalityCompositionError::InvalidCombinedLogComponent {
                    record_position,
                },
            );
        }

        let combined_factor = combined_log_component.exp();
        if !combined_factor.is_finite() || combined_factor <= 0.0 {
            return Err(
                DeterministicSeasonalityCompositionError::UnstableCombinedFactor {
                    record_position,
                },
            );
        }

        intraday_bin_index_by_event.push(intraday_bin_index);
        intraday_log_component_by_event.push(intraday_log_component);
        day_of_week_log_component_by_event.push(day_of_week_log_component_value);
        day_of_year_log_component_by_event.push(day_of_year_log_component_value);
        special_log_component_by_event.push(special_log_component);
        combined_log_component_by_event.push(combined_log_component);
        combined_factor_by_event.push(combined_factor);
    }

    let mut applied_overlay_ids_in_order: Vec<String> = Vec::new();
    for (overlay_index, overlay_application_count) in overlay_application_count_by_index
        .iter()
        .copied()
        .enumerate()
    {
        if overlay_application_count > 0 {
            applied_overlay_ids_in_order
                .push(scheduled_event_overlays[overlay_index].overlay_id.clone());
        }
    }

    let normalization_result = normalize_seasonality_factors_for_identifiability(
        &combined_factor_by_event,
        &combined_log_component_by_event,
    )
    .map_err(|error| match error {
        SeasonalityNormalizationError::EmptyFitWindow => {
            DeterministicSeasonalityCompositionError::EmptyInputEvents
        }
        SeasonalityNormalizationError::InvalidInputFactor { record_position } => {
            DeterministicSeasonalityCompositionError::InvalidNormalizationInputFactor {
                record_position,
            }
        }
        SeasonalityNormalizationError::InvalidInputLogComponent { record_position } => {
            DeterministicSeasonalityCompositionError::InvalidNormalizationInputLogComponent {
                record_position,
            }
        }
        SeasonalityNormalizationError::UnstableNormalizationConstant => {
            DeterministicSeasonalityCompositionError::UnstableNormalizationConstant
        }
        SeasonalityNormalizationError::IdentifiabilityGuardViolation {
            anchored_mean_factor,
            target_mean_factor,
            mean_anchor_tolerance,
        } => DeterministicSeasonalityCompositionError::IdentifiabilityGuardViolation {
            anchored_mean_factor,
            target_mean_factor,
            mean_anchor_tolerance,
        },
    })?;

    Ok(DeterministicSeasonalityCompositionOutput {
        schema_version: DETERMINISTIC_SEASONALITY_COMPOSITION_SCHEMA_VERSION,
        intraday_minutes_per_bin,
        intraday_bin_index_by_event,
        intraday_log_component_by_event,
        day_of_week_log_component_by_event,
        day_of_year_log_component_by_event,
        special_log_component_by_event,
        special_overlay_active_flags_by_event,
        active_overlay_ids_by_event,
        combined_log_component_by_event: normalization_result.normalized_log_component_by_event,
        combined_factor_by_event: normalization_result.normalized_factor_by_event,
        diagnostics: DeterministicSeasonalityCompositionDiagnostics {
            input_event_count,
            normalization_metadata: normalization_result.normalization_metadata,
            overlay_provenance_metadata: ScheduledEventOverlayProvenanceMetadata {
                overlays_enabled: enabled_overlay_count > 0,
                configured_overlay_count: scheduled_event_overlays.len(),
                enabled_overlay_count,
                active_overlay_event_count,
                precedence_policy: SPECIAL_OVERLAY_PRECEDENCE_POLICY,
                merge_policy: SPECIAL_OVERLAY_MERGE_POLICY,
                window_boundary_policy: SPECIAL_OVERLAY_WINDOW_BOUNDARY_POLICY,
                configured_overlay_ids_in_order,
                applied_overlay_ids_in_order,
                overlay_source_labels_in_order,
            },
        },
    })
}

#[derive(Debug, Clone, Copy)]
pub struct SeasonalityDiagnosticsReportInput<'a> {
    pub dataset_id: &'a str,
    pub input_schema_version: &'a str,
    pub preprocessed_schema_version: &'a str,
    pub exchange_code: &'a str,
    pub exchange_timezone_name: &'a str,
    pub source_record_positions: &'a [u64],
    pub timestamp_utc_nanos: &'a [i64],
    pub exchange_local_minute_of_day: &'a [u16],
    pub exchange_local_day_of_week: &'a [u8],
    pub exchange_local_day_of_year: &'a [u16],
    pub market_state_codes: &'a [u8],
    pub intraday_estimate: &'a IntradaySeasonalityEstimateOutput,
    pub day_of_week_estimate: &'a DayOfWeekSeasonalityEstimateOutput,
    pub day_of_year_estimate: &'a DayOfYearSeasonalityEstimateOutput,
    pub composition: &'a DeterministicSeasonalityCompositionOutput,
    pub estimator_version: &'a str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityComponentDiagnosticsSummary {
    pub component_name: &'static str,
    pub component_schema_version: String,
    pub input_event_count: usize,
    pub bucket_count: usize,
    pub zero_bucket_count: usize,
    pub sparse_bucket_count: usize,
    pub fallback_bucket_count: usize,
    pub zero_bucket_ratio: f64,
    pub sparse_bucket_ratio: f64,
    pub fallback_bucket_ratio: f64,
    pub mean_factor: f64,
    pub min_factor: f64,
    pub max_factor: f64,
    pub mean_absolute_factor_error: f64,
    pub root_mean_squared_factor_error: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityOverlayComponentDiagnosticsSummary {
    pub configured_overlay_count: usize,
    pub enabled_overlay_count: usize,
    pub active_overlay_event_count: usize,
    pub active_overlay_event_ratio: f64,
    pub active_event_mean_log_adjustment: f64,
    pub active_event_min_log_adjustment: f64,
    pub active_event_max_log_adjustment: f64,
    pub precedence_policy: String,
    pub merge_policy: String,
    pub window_boundary_policy: String,
    pub configured_overlay_ids_in_order: Vec<String>,
    pub applied_overlay_ids_in_order: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityFitQualityMetrics {
    pub weighted_mean_absolute_factor_error: f64,
    pub weighted_root_mean_squared_factor_error: f64,
    pub normalization_anchor_error: f64,
    pub combined_factor_mean: f64,
    pub combined_factor_standard_deviation: f64,
    pub combined_factor_min: f64,
    pub combined_factor_max: f64,
    pub overlay_active_event_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityInputSnapshotMetadata {
    pub input_schema_version: String,
    pub preprocessed_schema_version: String,
    pub exchange_code: String,
    pub exchange_timezone_name: String,
    pub input_event_count: usize,
    pub first_event_timestamp_utc_nanos: i64,
    pub last_event_timestamp_utc_nanos: i64,
    pub snapshot_fingerprint_algorithm: &'static str,
    pub snapshot_fingerprint_hex: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityEstimatorConfigurationMetadata {
    pub intraday_schema_version: String,
    pub day_of_week_schema_version: String,
    pub day_of_year_schema_version: String,
    pub composition_schema_version: String,
    pub intraday_minutes_per_bin: u16,
    pub intraday_smoothing_kernel: String,
    pub intraday_smoothing_bandwidth_bins: f64,
    pub intraday_minimum_bin_events: usize,
    pub intraday_pseudocount: f64,
    pub day_of_week_minimum_bucket_events: usize,
    pub day_of_week_pseudocount: f64,
    pub day_of_year_minimum_bucket_events: usize,
    pub day_of_year_pseudocount: f64,
    pub normalization_policy: String,
    pub overlay_precedence_policy: String,
    pub overlay_merge_policy: String,
    pub overlay_window_boundary_policy: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityReproducibilityMetadata {
    pub estimator_version: String,
    pub input_snapshot: SeasonalityInputSnapshotMetadata,
    pub estimator_configuration: SeasonalityEstimatorConfigurationMetadata,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityDiagnosticsReportOutput {
    pub schema_version: &'static str,
    pub dataset_id: String,
    pub intraday_component_summary: SeasonalityComponentDiagnosticsSummary,
    pub day_of_week_component_summary: SeasonalityComponentDiagnosticsSummary,
    pub day_of_year_component_summary: SeasonalityComponentDiagnosticsSummary,
    pub overlay_component_summary: Option<SeasonalityOverlayComponentDiagnosticsSummary>,
    pub fit_quality_metrics: SeasonalityFitQualityMetrics,
    pub reproducibility_metadata: SeasonalityReproducibilityMetadata,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityDiagnosticsBatchReportOutput {
    pub schema_version: &'static str,
    pub report_count: usize,
    pub dataset_ids_in_order: Vec<String>,
    pub reports: Vec<SeasonalityDiagnosticsReportOutput>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SeasonalityDiagnosticsReportError {
    EmptyDatasetId,
    EmptyEstimatorVersion,
    EmptyInputSchemaVersion,
    EmptyPreprocessedSchemaVersion,
    EmptyExchangeCode,
    EmptyExchangeTimezoneName,
    EmptyInputEvents,
    MismatchedInputLengths {
        timestamp_utc_nanos_count: usize,
        source_record_position_count: usize,
        minute_of_day_count: usize,
        day_of_week_count: usize,
        day_of_year_count: usize,
        market_state_code_count: usize,
        combined_log_component_count: usize,
        combined_factor_count: usize,
        special_log_component_count: usize,
        special_overlay_flag_count: usize,
    },
    InputEventCountMismatch {
        component_name: &'static str,
        expected_input_event_count: usize,
        actual_input_event_count: usize,
    },
    ComponentLengthMismatch {
        component_name: &'static str,
        expected_bucket_count: usize,
        actual_bucket_count: usize,
    },
    InvalidComponentPseudocount {
        component_name: &'static str,
        pseudocount: f64,
    },
    InvalidComponentFactor {
        component_name: &'static str,
        bucket_position: usize,
    },
    InvalidCombinedFactor {
        record_position: usize,
    },
    InvalidCombinedLogComponent {
        record_position: usize,
    },
    InvalidSpecialOverlayFlag {
        record_position: usize,
        overlay_active_flag: u8,
    },
    EmptyBatchInput,
    DuplicateDatasetId {
        dataset_id: String,
    },
}

impl Display for SeasonalityDiagnosticsReportError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyDatasetId => write!(
                formatter,
                "seasonality diagnostics report requires a non-empty dataset_id"
            ),
            Self::EmptyEstimatorVersion => write!(
                formatter,
                "seasonality diagnostics report requires a non-empty estimator_version"
            ),
            Self::EmptyInputSchemaVersion => write!(
                formatter,
                "seasonality diagnostics report requires a non-empty input_schema_version"
            ),
            Self::EmptyPreprocessedSchemaVersion => write!(
                formatter,
                "seasonality diagnostics report requires a non-empty preprocessed_schema_version"
            ),
            Self::EmptyExchangeCode => write!(
                formatter,
                "seasonality diagnostics report requires a non-empty exchange_code"
            ),
            Self::EmptyExchangeTimezoneName => write!(
                formatter,
                "seasonality diagnostics report requires a non-empty exchange_timezone_name"
            ),
            Self::EmptyInputEvents => write!(
                formatter,
                "seasonality diagnostics report requires at least one accepted input event"
            ),
            Self::MismatchedInputLengths {
                timestamp_utc_nanos_count,
                source_record_position_count,
                minute_of_day_count,
                day_of_week_count,
                day_of_year_count,
                market_state_code_count,
                combined_log_component_count,
                combined_factor_count,
                special_log_component_count,
                special_overlay_flag_count,
            } => write!(
                formatter,
                "seasonality diagnostics report inputs must have matching lengths, received timestamp_utc_nanos={timestamp_utc_nanos_count}, source_record_positions={source_record_position_count}, minute_of_day={minute_of_day_count}, day_of_week={day_of_week_count}, day_of_year={day_of_year_count}, market_state_codes={market_state_code_count}, combined_log_component={combined_log_component_count}, combined_factor={combined_factor_count}, special_log_component={special_log_component_count}, special_overlay_flags={special_overlay_flag_count}"
            ),
            Self::InputEventCountMismatch {
                component_name,
                expected_input_event_count,
                actual_input_event_count,
            } => write!(
                formatter,
                "seasonality diagnostics report component '{component_name}' input_event_count mismatch, expected {expected_input_event_count} and received {actual_input_event_count}"
            ),
            Self::ComponentLengthMismatch {
                component_name,
                expected_bucket_count,
                actual_bucket_count,
            } => write!(
                formatter,
                "seasonality diagnostics report component '{component_name}' length mismatch, expected {expected_bucket_count} buckets and received {actual_bucket_count}"
            ),
            Self::InvalidComponentPseudocount {
                component_name,
                pseudocount,
            } => write!(
                formatter,
                "seasonality diagnostics report component '{component_name}' has invalid pseudocount value {pseudocount}"
            ),
            Self::InvalidComponentFactor {
                component_name,
                bucket_position,
            } => write!(
                formatter,
                "seasonality diagnostics report component '{component_name}' has non-finite or non-positive factor at bucket position {bucket_position}"
            ),
            Self::InvalidCombinedFactor { record_position } => write!(
                formatter,
                "seasonality diagnostics report received non-finite or non-positive combined factor at record position {record_position}"
            ),
            Self::InvalidCombinedLogComponent { record_position } => write!(
                formatter,
                "seasonality diagnostics report received non-finite combined log component at record position {record_position}"
            ),
            Self::InvalidSpecialOverlayFlag {
                record_position,
                overlay_active_flag,
            } => write!(
                formatter,
                "seasonality diagnostics report received invalid special overlay active flag {overlay_active_flag} at record position {record_position}"
            ),
            Self::EmptyBatchInput => write!(
                formatter,
                "seasonality diagnostics batch report requires at least one dataset input"
            ),
            Self::DuplicateDatasetId { dataset_id } => write!(
                formatter,
                "seasonality diagnostics batch report received duplicate dataset id '{dataset_id}'"
            ),
        }
    }
}

impl Error for SeasonalityDiagnosticsReportError {}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ComponentFitErrorMetrics {
    mean_absolute_factor_error: f64,
    root_mean_squared_factor_error: f64,
}

pub fn generate_seasonality_diagnostics_report(
    report_input: SeasonalityDiagnosticsReportInput<'_>,
) -> Result<SeasonalityDiagnosticsReportOutput, SeasonalityDiagnosticsReportError> {
    if report_input.dataset_id.trim().is_empty() {
        return Err(SeasonalityDiagnosticsReportError::EmptyDatasetId);
    }
    if report_input.estimator_version.trim().is_empty() {
        return Err(SeasonalityDiagnosticsReportError::EmptyEstimatorVersion);
    }
    if report_input.input_schema_version.trim().is_empty() {
        return Err(SeasonalityDiagnosticsReportError::EmptyInputSchemaVersion);
    }
    if report_input.preprocessed_schema_version.trim().is_empty() {
        return Err(SeasonalityDiagnosticsReportError::EmptyPreprocessedSchemaVersion);
    }
    if report_input.exchange_code.trim().is_empty() {
        return Err(SeasonalityDiagnosticsReportError::EmptyExchangeCode);
    }
    if report_input.exchange_timezone_name.trim().is_empty() {
        return Err(SeasonalityDiagnosticsReportError::EmptyExchangeTimezoneName);
    }
    if report_input.timestamp_utc_nanos.is_empty() {
        return Err(SeasonalityDiagnosticsReportError::EmptyInputEvents);
    }

    let input_event_count = report_input.timestamp_utc_nanos.len();
    let composed_input_event_count = report_input.composition.diagnostics.input_event_count;

    if report_input.source_record_positions.len() != input_event_count
        || report_input.exchange_local_minute_of_day.len() != input_event_count
        || report_input.exchange_local_day_of_week.len() != input_event_count
        || report_input.exchange_local_day_of_year.len() != input_event_count
        || report_input.market_state_codes.len() != input_event_count
        || report_input
            .composition
            .combined_log_component_by_event
            .len()
            != input_event_count
        || report_input.composition.combined_factor_by_event.len() != input_event_count
        || report_input
            .composition
            .special_log_component_by_event
            .len()
            != input_event_count
        || report_input
            .composition
            .special_overlay_active_flags_by_event
            .len()
            != input_event_count
        || composed_input_event_count != input_event_count
    {
        return Err(SeasonalityDiagnosticsReportError::MismatchedInputLengths {
            timestamp_utc_nanos_count: input_event_count,
            source_record_position_count: report_input.source_record_positions.len(),
            minute_of_day_count: report_input.exchange_local_minute_of_day.len(),
            day_of_week_count: report_input.exchange_local_day_of_week.len(),
            day_of_year_count: report_input.exchange_local_day_of_year.len(),
            market_state_code_count: report_input.market_state_codes.len(),
            combined_log_component_count: report_input
                .composition
                .combined_log_component_by_event
                .len(),
            combined_factor_count: report_input.composition.combined_factor_by_event.len(),
            special_log_component_count: report_input
                .composition
                .special_log_component_by_event
                .len(),
            special_overlay_flag_count: report_input
                .composition
                .special_overlay_active_flags_by_event
                .len(),
        });
    }

    let intraday_summary = build_component_summary(
        "intraday",
        report_input.intraday_estimate.schema_version,
        report_input.intraday_estimate.diagnostics.input_event_count,
        input_event_count,
        &report_input.intraday_estimate.event_count_by_bin,
        &report_input.intraday_estimate.intraday_factor_by_bin,
        report_input.intraday_estimate.diagnostics.sparse_bin_count,
        0,
        report_input.intraday_estimate.pseudocount,
    )?;

    let day_of_week_summary = build_component_summary(
        "day_of_week",
        report_input.day_of_week_estimate.schema_version,
        report_input
            .day_of_week_estimate
            .diagnostics
            .input_event_count,
        input_event_count,
        &report_input.day_of_week_estimate.event_count_by_day_of_week,
        &report_input.day_of_week_estimate.day_of_week_factor,
        report_input
            .day_of_week_estimate
            .diagnostics
            .sparse_bucket_count,
        report_input
            .day_of_week_estimate
            .diagnostics
            .fallback_bucket_count,
        report_input.day_of_week_estimate.pseudocount,
    )?;

    let day_of_year_summary = build_component_summary(
        "day_of_year",
        report_input.day_of_year_estimate.schema_version,
        report_input
            .day_of_year_estimate
            .diagnostics
            .input_event_count,
        input_event_count,
        &report_input.day_of_year_estimate.event_count_by_day_of_year,
        &report_input.day_of_year_estimate.day_of_year_factor,
        report_input
            .day_of_year_estimate
            .diagnostics
            .sparse_bucket_count,
        report_input
            .day_of_year_estimate
            .diagnostics
            .fallback_bucket_count,
        report_input.day_of_year_estimate.pseudocount,
    )?;

    let overlay_summary =
        build_overlay_component_summary(input_event_count, report_input.composition)?;

    let fit_quality_metrics = build_fit_quality_metrics(
        report_input.composition,
        &intraday_summary,
        &day_of_week_summary,
        &day_of_year_summary,
        overlay_summary.as_ref(),
    )?;

    let reproducibility_metadata = build_reproducibility_metadata(&report_input);

    Ok(SeasonalityDiagnosticsReportOutput {
        schema_version: SEASONALITY_DIAGNOSTICS_REPORT_SCHEMA_VERSION,
        dataset_id: report_input.dataset_id.to_string(),
        intraday_component_summary: intraday_summary,
        day_of_week_component_summary: day_of_week_summary,
        day_of_year_component_summary: day_of_year_summary,
        overlay_component_summary: overlay_summary,
        fit_quality_metrics,
        reproducibility_metadata,
    })
}

pub fn generate_seasonality_diagnostics_batch_report(
    report_inputs: &[SeasonalityDiagnosticsReportInput<'_>],
) -> Result<SeasonalityDiagnosticsBatchReportOutput, SeasonalityDiagnosticsReportError> {
    if report_inputs.is_empty() {
        return Err(SeasonalityDiagnosticsReportError::EmptyBatchInput);
    }

    let mut seen_dataset_ids: BTreeSet<String> = BTreeSet::new();
    let mut dataset_ids_in_order: Vec<String> = Vec::with_capacity(report_inputs.len());
    let mut reports: Vec<SeasonalityDiagnosticsReportOutput> =
        Vec::with_capacity(report_inputs.len());

    for report_input in report_inputs {
        if !seen_dataset_ids.insert(report_input.dataset_id.to_string()) {
            return Err(SeasonalityDiagnosticsReportError::DuplicateDatasetId {
                dataset_id: report_input.dataset_id.to_string(),
            });
        }

        let report = generate_seasonality_diagnostics_report(*report_input)?;

        dataset_ids_in_order.push(report.dataset_id.clone());
        reports.push(report);
    }

    Ok(SeasonalityDiagnosticsBatchReportOutput {
        schema_version: SEASONALITY_DIAGNOSTICS_BATCH_REPORT_SCHEMA_VERSION,
        report_count: reports.len(),
        dataset_ids_in_order,
        reports,
    })
}

fn build_component_summary(
    component_name: &'static str,
    component_schema_version: &str,
    component_input_event_count: usize,
    expected_input_event_count: usize,
    event_count_by_bucket: &[u64],
    factor_by_bucket: &[f64],
    sparse_bucket_count: usize,
    fallback_bucket_count: usize,
    pseudocount: f64,
) -> Result<SeasonalityComponentDiagnosticsSummary, SeasonalityDiagnosticsReportError> {
    if component_input_event_count != expected_input_event_count {
        return Err(SeasonalityDiagnosticsReportError::InputEventCountMismatch {
            component_name,
            expected_input_event_count,
            actual_input_event_count: component_input_event_count,
        });
    }

    if event_count_by_bucket.len() != factor_by_bucket.len() {
        return Err(SeasonalityDiagnosticsReportError::ComponentLengthMismatch {
            component_name,
            expected_bucket_count: event_count_by_bucket.len(),
            actual_bucket_count: factor_by_bucket.len(),
        });
    }

    if !pseudocount.is_finite() || pseudocount <= 0.0 {
        return Err(
            SeasonalityDiagnosticsReportError::InvalidComponentPseudocount {
                component_name,
                pseudocount,
            },
        );
    }

    let bucket_count = event_count_by_bucket.len();
    if bucket_count == 0 {
        return Err(SeasonalityDiagnosticsReportError::ComponentLengthMismatch {
            component_name,
            expected_bucket_count: 1,
            actual_bucket_count: 0,
        });
    }

    let observed_factor_by_bucket =
        build_observed_factor_by_bucket(event_count_by_bucket, pseudocount, bucket_count);
    let fit_error_metrics = calculate_component_fit_error_metrics(
        component_name,
        factor_by_bucket,
        &observed_factor_by_bucket,
    )?;

    let zero_bucket_count = event_count_by_bucket
        .iter()
        .copied()
        .filter(|event_count| *event_count == 0)
        .count();
    let bucket_count_float = bucket_count as f64;
    let mean_factor = factor_by_bucket.iter().sum::<f64>() / bucket_count_float;
    let min_factor = factor_by_bucket
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let max_factor = factor_by_bucket
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);

    Ok(SeasonalityComponentDiagnosticsSummary {
        component_name,
        component_schema_version: component_schema_version.to_string(),
        input_event_count: component_input_event_count,
        bucket_count,
        zero_bucket_count,
        sparse_bucket_count,
        fallback_bucket_count,
        zero_bucket_ratio: zero_bucket_count as f64 / bucket_count_float,
        sparse_bucket_ratio: sparse_bucket_count as f64 / bucket_count_float,
        fallback_bucket_ratio: fallback_bucket_count as f64 / bucket_count_float,
        mean_factor,
        min_factor,
        max_factor,
        mean_absolute_factor_error: fit_error_metrics.mean_absolute_factor_error,
        root_mean_squared_factor_error: fit_error_metrics.root_mean_squared_factor_error,
    })
}

fn build_observed_factor_by_bucket(
    event_count_by_bucket: &[u64],
    pseudocount: f64,
    bucket_count: usize,
) -> Vec<f64> {
    let adjusted_event_count_by_bucket: Vec<f64> = event_count_by_bucket
        .iter()
        .copied()
        .map(|event_count| event_count as f64 + pseudocount)
        .collect();
    let adjusted_event_count_total: f64 = adjusted_event_count_by_bucket.iter().sum();
    let bucket_count_float = bucket_count as f64;

    adjusted_event_count_by_bucket
        .iter()
        .map(|adjusted_event_count| *adjusted_event_count / adjusted_event_count_total)
        .map(|density| density * bucket_count_float)
        .collect()
}

fn calculate_component_fit_error_metrics(
    component_name: &'static str,
    factor_by_bucket: &[f64],
    observed_factor_by_bucket: &[f64],
) -> Result<ComponentFitErrorMetrics, SeasonalityDiagnosticsReportError> {
    let mut absolute_error_sum = 0.0;
    let mut squared_error_sum = 0.0;

    let bucket_count = factor_by_bucket.len();
    for bucket_position in 0..bucket_count {
        let factor = factor_by_bucket[bucket_position];
        let observed_factor = observed_factor_by_bucket[bucket_position];

        if !factor.is_finite()
            || factor <= 0.0
            || !observed_factor.is_finite()
            || observed_factor <= 0.0
        {
            return Err(SeasonalityDiagnosticsReportError::InvalidComponentFactor {
                component_name,
                bucket_position,
            });
        }

        let error = factor - observed_factor;

        absolute_error_sum += error.abs();
        squared_error_sum += error.powi(2);
    }

    let bucket_count_float = bucket_count as f64;

    Ok(ComponentFitErrorMetrics {
        mean_absolute_factor_error: absolute_error_sum / bucket_count_float,
        root_mean_squared_factor_error: (squared_error_sum / bucket_count_float).sqrt(),
    })
}

fn build_overlay_component_summary(
    input_event_count: usize,
    composition_output: &DeterministicSeasonalityCompositionOutput,
) -> Result<Option<SeasonalityOverlayComponentDiagnosticsSummary>, SeasonalityDiagnosticsReportError>
{
    let overlay_metadata = &composition_output.diagnostics.overlay_provenance_metadata;
    if overlay_metadata.configured_overlay_count == 0 {
        return Ok(None);
    }

    let mut active_overlay_log_adjustments: Vec<f64> = Vec::new();
    for (record_position, overlay_active_flag) in composition_output
        .special_overlay_active_flags_by_event
        .iter()
        .copied()
        .enumerate()
    {
        match overlay_active_flag {
            0 => {}
            1 => {
                active_overlay_log_adjustments
                    .push(composition_output.special_log_component_by_event[record_position]);
            }
            _ => {
                return Err(
                    SeasonalityDiagnosticsReportError::InvalidSpecialOverlayFlag {
                        record_position,
                        overlay_active_flag,
                    },
                );
            }
        }
    }

    let (
        active_event_mean_log_adjustment,
        active_event_min_log_adjustment,
        active_event_max_log_adjustment,
    ) = if active_overlay_log_adjustments.is_empty() {
        (0.0, 0.0, 0.0)
    } else {
        let event_count_float = active_overlay_log_adjustments.len() as f64;
        let mean_value = active_overlay_log_adjustments.iter().sum::<f64>() / event_count_float;

        let min_value = active_overlay_log_adjustments
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        let max_value = active_overlay_log_adjustments
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);

        (mean_value, min_value, max_value)
    };

    Ok(Some(SeasonalityOverlayComponentDiagnosticsSummary {
        configured_overlay_count: overlay_metadata.configured_overlay_count,
        enabled_overlay_count: overlay_metadata.enabled_overlay_count,
        active_overlay_event_count: overlay_metadata.active_overlay_event_count,
        active_overlay_event_ratio: overlay_metadata.active_overlay_event_count as f64
            / input_event_count as f64,
        active_event_mean_log_adjustment,
        active_event_min_log_adjustment,
        active_event_max_log_adjustment,
        precedence_policy: overlay_metadata.precedence_policy.to_string(),
        merge_policy: overlay_metadata.merge_policy.to_string(),
        window_boundary_policy: overlay_metadata.window_boundary_policy.to_string(),
        configured_overlay_ids_in_order: overlay_metadata.configured_overlay_ids_in_order.clone(),
        applied_overlay_ids_in_order: overlay_metadata.applied_overlay_ids_in_order.clone(),
    }))
}

fn build_fit_quality_metrics(
    composition_output: &DeterministicSeasonalityCompositionOutput,
    intraday_summary: &SeasonalityComponentDiagnosticsSummary,
    day_of_week_summary: &SeasonalityComponentDiagnosticsSummary,
    day_of_year_summary: &SeasonalityComponentDiagnosticsSummary,
    overlay_summary: Option<&SeasonalityOverlayComponentDiagnosticsSummary>,
) -> Result<SeasonalityFitQualityMetrics, SeasonalityDiagnosticsReportError> {
    let component_summaries = [intraday_summary, day_of_week_summary, day_of_year_summary];
    let total_bucket_count: usize = component_summaries
        .iter()
        .map(|summary| summary.bucket_count)
        .sum();
    let total_bucket_count_float = total_bucket_count as f64;
    let weighted_mean_absolute_factor_error = component_summaries
        .iter()
        .map(|summary| summary.mean_absolute_factor_error * summary.bucket_count as f64)
        .sum::<f64>()
        / total_bucket_count_float;
    let weighted_root_mean_squared_factor_error = component_summaries
        .iter()
        .map(|summary| summary.root_mean_squared_factor_error * summary.bucket_count as f64)
        .sum::<f64>()
        / total_bucket_count_float;

    let mut combined_factor_sum = 0.0;
    let mut combined_factor_min = f64::INFINITY;
    let mut combined_factor_max = f64::NEG_INFINITY;
    let mut combined_log_component_sum = 0.0;

    for record_position in 0..composition_output.combined_factor_by_event.len() {
        let combined_factor = composition_output.combined_factor_by_event[record_position];
        let combined_log_component =
            composition_output.combined_log_component_by_event[record_position];

        if !combined_factor.is_finite() || combined_factor <= 0.0 {
            return Err(SeasonalityDiagnosticsReportError::InvalidCombinedFactor {
                record_position,
            });
        }

        if !combined_log_component.is_finite() {
            return Err(
                SeasonalityDiagnosticsReportError::InvalidCombinedLogComponent { record_position },
            );
        }

        combined_factor_sum += combined_factor;
        combined_log_component_sum += combined_log_component;
        combined_factor_min = combined_factor_min.min(combined_factor);
        combined_factor_max = combined_factor_max.max(combined_factor);
    }

    let input_event_count = composition_output.combined_factor_by_event.len();
    let input_event_count_float = input_event_count as f64;
    let combined_factor_mean = combined_factor_sum / input_event_count_float;
    let _combined_log_component_mean = combined_log_component_sum / input_event_count_float;
    let combined_factor_variance = composition_output
        .combined_factor_by_event
        .iter()
        .copied()
        .map(|combined_factor| (combined_factor - combined_factor_mean).powi(2))
        .sum::<f64>()
        / input_event_count_float;

    let overlay_active_event_ratio = match overlay_summary {
        Some(summary) => summary.active_overlay_event_ratio,
        None => 0.0,
    };
    let normalization_metadata = &composition_output.diagnostics.normalization_metadata;
    let normalization_anchor_error = (normalization_metadata.post_normalization_mean_factor
        - normalization_metadata.target_mean_factor)
        .abs();

    Ok(SeasonalityFitQualityMetrics {
        weighted_mean_absolute_factor_error,
        weighted_root_mean_squared_factor_error,
        normalization_anchor_error,
        combined_factor_mean,
        combined_factor_standard_deviation: combined_factor_variance.sqrt(),
        combined_factor_min,
        combined_factor_max,
        overlay_active_event_ratio,
    })
}

fn build_reproducibility_metadata(
    report_input: &SeasonalityDiagnosticsReportInput<'_>,
) -> SeasonalityReproducibilityMetadata {
    let input_snapshot_fingerprint_hex = build_input_snapshot_fingerprint(report_input);
    let first_event_timestamp_utc_nanos = report_input.timestamp_utc_nanos[0];
    let last_event_timestamp_utc_nanos =
        report_input.timestamp_utc_nanos[report_input.timestamp_utc_nanos.len() - 1];
    let overlay_metadata = &report_input
        .composition
        .diagnostics
        .overlay_provenance_metadata;
    let normalization_metadata = &report_input.composition.diagnostics.normalization_metadata;

    SeasonalityReproducibilityMetadata {
        estimator_version: report_input.estimator_version.to_string(),
        input_snapshot: SeasonalityInputSnapshotMetadata {
            input_schema_version: report_input.input_schema_version.to_string(),
            preprocessed_schema_version: report_input.preprocessed_schema_version.to_string(),
            exchange_code: report_input.exchange_code.to_string(),
            exchange_timezone_name: report_input.exchange_timezone_name.to_string(),
            input_event_count: report_input.timestamp_utc_nanos.len(),
            first_event_timestamp_utc_nanos,
            last_event_timestamp_utc_nanos,
            snapshot_fingerprint_algorithm: INPUT_SNAPSHOT_FINGERPRINT_ALGORITHM,
            snapshot_fingerprint_hex: input_snapshot_fingerprint_hex,
        },
        estimator_configuration: SeasonalityEstimatorConfigurationMetadata {
            intraday_schema_version: report_input.intraday_estimate.schema_version.to_string(),
            day_of_week_schema_version: report_input
                .day_of_week_estimate
                .schema_version
                .to_string(),
            day_of_year_schema_version: report_input
                .day_of_year_estimate
                .schema_version
                .to_string(),
            composition_schema_version: report_input.composition.schema_version.to_string(),
            intraday_minutes_per_bin: report_input.intraday_estimate.minutes_per_bin,
            intraday_smoothing_kernel: report_input
                .intraday_estimate
                .smoothing_kernel
                .as_str()
                .to_string(),
            intraday_smoothing_bandwidth_bins: report_input
                .intraday_estimate
                .smoothing_bandwidth_bins,
            intraday_minimum_bin_events: report_input.intraday_estimate.minimum_bin_events,
            intraday_pseudocount: report_input.intraday_estimate.pseudocount,
            day_of_week_minimum_bucket_events: report_input
                .day_of_week_estimate
                .minimum_bucket_events,
            day_of_week_pseudocount: report_input.day_of_week_estimate.pseudocount,
            day_of_year_minimum_bucket_events: report_input
                .day_of_year_estimate
                .minimum_bucket_events,
            day_of_year_pseudocount: report_input.day_of_year_estimate.pseudocount,
            normalization_policy: normalization_metadata.normalization_policy.to_string(),
            overlay_precedence_policy: overlay_metadata.precedence_policy.to_string(),
            overlay_merge_policy: overlay_metadata.merge_policy.to_string(),
            overlay_window_boundary_policy: overlay_metadata.window_boundary_policy.to_string(),
        },
    }
}

fn build_input_snapshot_fingerprint(
    report_input: &SeasonalityDiagnosticsReportInput<'_>,
) -> String {
    let mut hash = 14_695_981_039_346_656_037_u64;

    hash_with_text(&mut hash, report_input.dataset_id);
    hash_with_text(&mut hash, report_input.input_schema_version);
    hash_with_text(&mut hash, report_input.preprocessed_schema_version);
    hash_with_text(&mut hash, report_input.exchange_code);
    hash_with_text(&mut hash, report_input.exchange_timezone_name);

    for source_record_position in report_input.source_record_positions {
        hash_with_u64(&mut hash, *source_record_position);
    }
    for timestamp_utc_nanos in report_input.timestamp_utc_nanos {
        hash_with_i64(&mut hash, *timestamp_utc_nanos);
    }
    for minute_of_day in report_input.exchange_local_minute_of_day {
        hash_with_u16(&mut hash, *minute_of_day);
    }
    for day_of_week in report_input.exchange_local_day_of_week {
        hash_with_u8(&mut hash, *day_of_week);
    }
    for day_of_year in report_input.exchange_local_day_of_year {
        hash_with_u16(&mut hash, *day_of_year);
    }
    for market_state_code in report_input.market_state_codes {
        hash_with_u8(&mut hash, *market_state_code);
    }

    format!("{hash:016x}")
}

fn hash_with_text(hash: &mut u64, text_value: &str) {
    hash_with_u64(hash, text_value.len() as u64);
    hash_with_bytes(hash, text_value.as_bytes());
}

fn hash_with_u8(hash: &mut u64, value: u8) {
    hash_with_bytes(hash, &[value]);
}

fn hash_with_u16(hash: &mut u64, value: u16) {
    hash_with_bytes(hash, &value.to_le_bytes());
}

fn hash_with_u64(hash: &mut u64, value: u64) {
    hash_with_bytes(hash, &value.to_le_bytes());
}

fn hash_with_i64(hash: &mut u64, value: i64) {
    hash_with_bytes(hash, &value.to_le_bytes());
}

fn hash_with_bytes(hash: &mut u64, raw_bytes: &[u8]) {
    const FNV_PRIME: u64 = 1_099_511_628_211;
    for raw_byte in raw_bytes {
        *hash ^= u64::from(*raw_byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DayOfWeekSeasonalityEstimationError {
    EmptyInputEvents,
    InvalidMinimumBucketEvents {
        minimum_bucket_events: usize,
    },
    InvalidPseudocount {
        pseudocount: f64,
    },
    InvalidDayOfWeek {
        record_position: usize,
        day_of_week: u8,
    },
    UnstableNormalization,
}

impl Display for DayOfWeekSeasonalityEstimationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInputEvents => {
                write!(
                    formatter,
                    "day-of-week seasonality estimation requires at least one accepted event"
                )
            }
            Self::InvalidMinimumBucketEvents {
                minimum_bucket_events,
            } => write!(
                formatter,
                "minimum_bucket_events must be at least 1, received {minimum_bucket_events}"
            ),
            Self::InvalidPseudocount { pseudocount } => write!(
                formatter,
                "pseudocount must be a finite positive value, received {pseudocount}"
            ),
            Self::InvalidDayOfWeek {
                record_position,
                day_of_week,
            } => write!(
                formatter,
                "event record at position {record_position} has invalid local day-of-week value {day_of_week}"
            ),
            Self::UnstableNormalization => write!(
                formatter,
                "day-of-week seasonality estimation produced a non-positive normalization constant"
            ),
        }
    }
}

impl Error for DayOfWeekSeasonalityEstimationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum DayOfYearSeasonalityEstimationError {
    EmptyInputEvents,
    InvalidMinimumBucketEvents {
        minimum_bucket_events: usize,
    },
    InvalidPseudocount {
        pseudocount: f64,
    },
    InvalidDayOfYear {
        record_position: usize,
        day_of_year: u16,
    },
    UnstableNormalization,
}

impl Display for DayOfYearSeasonalityEstimationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInputEvents => {
                write!(
                    formatter,
                    "day-of-year seasonality estimation requires at least one accepted event"
                )
            }
            Self::InvalidMinimumBucketEvents {
                minimum_bucket_events,
            } => write!(
                formatter,
                "minimum_bucket_events must be at least 1, received {minimum_bucket_events}"
            ),
            Self::InvalidPseudocount { pseudocount } => write!(
                formatter,
                "pseudocount must be a finite positive value, received {pseudocount}"
            ),
            Self::InvalidDayOfYear {
                record_position,
                day_of_year,
            } => write!(
                formatter,
                "event record at position {record_position} has invalid local day-of-year value {day_of_year}"
            ),
            Self::UnstableNormalization => write!(
                formatter,
                "day-of-year seasonality estimation produced a non-positive normalization constant"
            ),
        }
    }
}

impl Error for DayOfYearSeasonalityEstimationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum DeterministicSeasonalityCompositionError {
    EmptyInputEvents,
    MismatchedInputLengths {
        timestamp_utc_nanos_count: usize,
        minute_of_day_count: usize,
        day_of_week_count: usize,
        day_of_year_count: usize,
    },
    InvalidIntradayMinutesPerBin {
        intraday_minutes_per_bin: u16,
    },
    InvalidIntradayComponentLength {
        expected_intraday_bin_count: usize,
        actual_intraday_bin_count: usize,
    },
    InvalidDayOfWeekComponentLength {
        expected_day_of_week_bucket_count: usize,
        actual_day_of_week_bucket_count: usize,
    },
    InvalidDayOfYearComponentLength {
        expected_day_of_year_bucket_count: usize,
        actual_day_of_year_bucket_count: usize,
    },
    InvalidMinuteOfDay {
        record_position: usize,
        minute_of_day: u16,
    },
    InvalidDayOfWeek {
        record_position: usize,
        day_of_week: u8,
    },
    InvalidDayOfYear {
        record_position: usize,
        day_of_year: u16,
    },
    EmptyOverlayId {
        overlay_position: usize,
    },
    DuplicateOverlayId {
        overlay_id: String,
    },
    InvalidOverlayWindow {
        overlay_position: usize,
        start_timestamp_utc_nanos: i64,
        end_timestamp_utc_nanos: i64,
    },
    InvalidOverlayLogAdjustment {
        overlay_position: usize,
    },
    InvalidSpecialOverlayLogComponent {
        record_position: usize,
    },
    InvalidCombinedLogComponent {
        record_position: usize,
    },
    UnstableCombinedFactor {
        record_position: usize,
    },
    InvalidNormalizationInputFactor {
        record_position: usize,
    },
    InvalidNormalizationInputLogComponent {
        record_position: usize,
    },
    UnstableNormalizationConstant,
    IdentifiabilityGuardViolation {
        anchored_mean_factor: f64,
        target_mean_factor: f64,
        mean_anchor_tolerance: f64,
    },
}

impl Display for DeterministicSeasonalityCompositionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInputEvents => {
                write!(
                    formatter,
                    "deterministic seasonality composition requires at least one accepted event"
                )
            }
            Self::MismatchedInputLengths {
                timestamp_utc_nanos_count,
                minute_of_day_count,
                day_of_week_count,
                day_of_year_count,
            } => write!(
                formatter,
                "composition inputs must have matching lengths, received timestamp_utc_nanos={timestamp_utc_nanos_count}, minute_of_day={minute_of_day_count}, day_of_week={day_of_week_count}, day_of_year={day_of_year_count}"
            ),
            Self::InvalidIntradayMinutesPerBin {
                intraday_minutes_per_bin,
            } => write!(
                formatter,
                "intraday_minutes_per_bin must be a positive divisor of 1440, received {intraday_minutes_per_bin}"
            ),
            Self::InvalidIntradayComponentLength {
                expected_intraday_bin_count,
                actual_intraday_bin_count,
            } => write!(
                formatter,
                "intraday component length mismatch, expected {expected_intraday_bin_count} bins and received {actual_intraday_bin_count}"
            ),
            Self::InvalidDayOfWeekComponentLength {
                expected_day_of_week_bucket_count,
                actual_day_of_week_bucket_count,
            } => write!(
                formatter,
                "day-of-week component length mismatch, expected {expected_day_of_week_bucket_count} buckets and received {actual_day_of_week_bucket_count}"
            ),
            Self::InvalidDayOfYearComponentLength {
                expected_day_of_year_bucket_count,
                actual_day_of_year_bucket_count,
            } => write!(
                formatter,
                "day-of-year component length mismatch, expected {expected_day_of_year_bucket_count} buckets and received {actual_day_of_year_bucket_count}"
            ),
            Self::InvalidMinuteOfDay {
                record_position,
                minute_of_day,
            } => write!(
                formatter,
                "event record at position {record_position} has invalid local minute-of-day value {minute_of_day}"
            ),
            Self::InvalidDayOfWeek {
                record_position,
                day_of_week,
            } => write!(
                formatter,
                "event record at position {record_position} has invalid local day-of-week value {day_of_week}"
            ),
            Self::InvalidDayOfYear {
                record_position,
                day_of_year,
            } => write!(
                formatter,
                "event record at position {record_position} has invalid local day-of-year value {day_of_year}"
            ),
            Self::EmptyOverlayId { overlay_position } => write!(
                formatter,
                "scheduled event overlay at position {overlay_position} is missing a non-empty overlay_id"
            ),
            Self::DuplicateOverlayId { overlay_id } => write!(
                formatter,
                "scheduled event overlay id '{overlay_id}' is duplicated and must be unique"
            ),
            Self::InvalidOverlayWindow {
                overlay_position,
                start_timestamp_utc_nanos,
                end_timestamp_utc_nanos,
            } => write!(
                formatter,
                "scheduled event overlay at position {overlay_position} has an invalid window where start={start_timestamp_utc_nanos} and end={end_timestamp_utc_nanos}"
            ),
            Self::InvalidOverlayLogAdjustment { overlay_position } => write!(
                formatter,
                "scheduled event overlay at position {overlay_position} has a non-finite log_adjustment"
            ),
            Self::InvalidSpecialOverlayLogComponent { record_position } => write!(
                formatter,
                "deterministic seasonality composition produced a non-finite special overlay log component at record position {record_position}"
            ),
            Self::InvalidCombinedLogComponent { record_position } => write!(
                formatter,
                "deterministic seasonality composition produced a non-finite combined log component at record position {record_position}"
            ),
            Self::UnstableCombinedFactor { record_position } => write!(
                formatter,
                "deterministic seasonality composition produced a non-finite combined factor at record position {record_position}"
            ),
            Self::InvalidNormalizationInputFactor { record_position } => write!(
                formatter,
                "deterministic seasonality normalization received a non-finite or non-positive factor at record position {record_position}"
            ),
            Self::InvalidNormalizationInputLogComponent { record_position } => write!(
                formatter,
                "deterministic seasonality normalization received a non-finite log component at record position {record_position}"
            ),
            Self::UnstableNormalizationConstant => write!(
                formatter,
                "deterministic seasonality normalization produced a non-positive normalization constant"
            ),
            Self::IdentifiabilityGuardViolation {
                anchored_mean_factor,
                target_mean_factor,
                mean_anchor_tolerance,
            } => write!(
                formatter,
                "deterministic seasonality normalization failed identifiability guard, anchored mean={anchored_mean_factor}, target mean={target_mean_factor}, tolerance={mean_anchor_tolerance}"
            ),
        }
    }
}

impl Error for DeterministicSeasonalityCompositionError {}

#[derive(Debug, Clone, PartialEq)]
enum SeasonalityNormalizationError {
    EmptyFitWindow,
    InvalidInputFactor {
        record_position: usize,
    },
    InvalidInputLogComponent {
        record_position: usize,
    },
    UnstableNormalizationConstant,
    IdentifiabilityGuardViolation {
        anchored_mean_factor: f64,
        target_mean_factor: f64,
        mean_anchor_tolerance: f64,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct SeasonalityNormalizationResult {
    normalized_factor_by_event: Vec<f64>,
    normalized_log_component_by_event: Vec<f64>,
    normalization_metadata: SeasonalityNormalizationMetadata,
}

fn normalize_seasonality_factors_for_identifiability(
    factor_by_event: &[f64],
    log_component_by_event: &[f64],
) -> Result<SeasonalityNormalizationResult, SeasonalityNormalizationError> {
    if factor_by_event.is_empty() {
        return Err(SeasonalityNormalizationError::EmptyFitWindow);
    }

    if log_component_by_event.len() != factor_by_event.len() {
        return Err(SeasonalityNormalizationError::UnstableNormalizationConstant);
    }

    for (record_position, factor_value) in factor_by_event.iter().copied().enumerate() {
        if !factor_value.is_finite() || factor_value <= 0.0 {
            return Err(SeasonalityNormalizationError::InvalidInputFactor { record_position });
        }
    }

    for (record_position, log_component_value) in log_component_by_event.iter().copied().enumerate()
    {
        if !log_component_value.is_finite() {
            return Err(SeasonalityNormalizationError::InvalidInputLogComponent {
                record_position,
            });
        }
    }

    let pre_normalization_mean_factor =
        factor_by_event.iter().sum::<f64>() / factor_by_event.len() as f64;
    if !pre_normalization_mean_factor.is_finite() || pre_normalization_mean_factor <= 0.0 {
        return Err(SeasonalityNormalizationError::UnstableNormalizationConstant);
    }

    let normalization_constant = pre_normalization_mean_factor / IDENTIFIABILITY_TARGET_MEAN_FACTOR;
    if !normalization_constant.is_finite() || normalization_constant <= 0.0 {
        return Err(SeasonalityNormalizationError::UnstableNormalizationConstant);
    }

    let normalization_log_offset = normalization_constant.ln();
    if !normalization_log_offset.is_finite() {
        return Err(SeasonalityNormalizationError::UnstableNormalizationConstant);
    }

    let normalized_factor_by_event: Vec<f64> = factor_by_event
        .iter()
        .map(|factor_value| *factor_value / normalization_constant)
        .collect();
    let normalized_log_component_by_event: Vec<f64> = log_component_by_event
        .iter()
        .map(|log_component_value| *log_component_value - normalization_log_offset)
        .collect();

    for (record_position, normalized_factor) in
        normalized_factor_by_event.iter().copied().enumerate()
    {
        if !normalized_factor.is_finite() || normalized_factor <= 0.0 {
            return Err(SeasonalityNormalizationError::InvalidInputFactor { record_position });
        }
    }

    for (record_position, normalized_log_component) in normalized_log_component_by_event
        .iter()
        .copied()
        .enumerate()
    {
        if !normalized_log_component.is_finite() {
            return Err(SeasonalityNormalizationError::InvalidInputLogComponent {
                record_position,
            });
        }
    }

    let post_normalization_mean_factor =
        normalized_factor_by_event.iter().sum::<f64>() / normalized_factor_by_event.len() as f64;
    if !post_normalization_mean_factor.is_finite()
        || (post_normalization_mean_factor - IDENTIFIABILITY_TARGET_MEAN_FACTOR).abs()
            > IDENTIFIABILITY_MEAN_ANCHOR_TOLERANCE
    {
        return Err(
            SeasonalityNormalizationError::IdentifiabilityGuardViolation {
                anchored_mean_factor: post_normalization_mean_factor,
                target_mean_factor: IDENTIFIABILITY_TARGET_MEAN_FACTOR,
                mean_anchor_tolerance: IDENTIFIABILITY_MEAN_ANCHOR_TOLERANCE,
            },
        );
    }

    Ok(SeasonalityNormalizationResult {
        normalized_factor_by_event,
        normalized_log_component_by_event,
        normalization_metadata: SeasonalityNormalizationMetadata {
            normalization_policy: IDENTIFIABILITY_NORMALIZATION_POLICY,
            fit_window_event_count: factor_by_event.len(),
            target_mean_factor: IDENTIFIABILITY_TARGET_MEAN_FACTOR,
            pre_normalization_mean_factor,
            normalization_constant,
            normalization_log_offset,
            post_normalization_mean_factor,
            mean_anchor_tolerance: IDENTIFIABILITY_MEAN_ANCHOR_TOLERANCE,
        },
    })
}

#[derive(Debug, Clone, PartialEq)]
enum DiscreteSeasonalityByBucketEstimationError {
    UnstableNormalization,
}

#[derive(Debug, Clone, PartialEq)]
struct DiscreteSeasonalityByBucketEstimate {
    event_count_by_bucket: Vec<u64>,
    sparse_bucket_flags: Vec<u8>,
    fallback_bucket_flags: Vec<u8>,
    log_component_by_bucket: Vec<f64>,
    factor_by_bucket: Vec<f64>,
    zero_event_bucket_count: usize,
    sparse_bucket_count: usize,
    fallback_bucket_count: usize,
}

fn estimate_discrete_seasonality_by_bucket(
    bucket_index_by_event: &[usize],
    bucket_count: usize,
    minimum_bucket_events: usize,
    pseudocount: f64,
) -> Result<DiscreteSeasonalityByBucketEstimate, DiscreteSeasonalityByBucketEstimationError> {
    let mut event_count_by_bucket: Vec<u64> = vec![0; bucket_count];
    for bucket_index in bucket_index_by_event {
        event_count_by_bucket[*bucket_index] += 1;
    }

    let zero_event_bucket_count: usize = event_count_by_bucket
        .iter()
        .copied()
        .filter(|event_count| *event_count == 0)
        .count();
    let sparse_bucket_flags: Vec<u8> = event_count_by_bucket
        .iter()
        .copied()
        .map(|event_count| {
            if event_count < minimum_bucket_events as u64 {
                1
            } else {
                0
            }
        })
        .collect();
    let sparse_bucket_count: usize = sparse_bucket_flags
        .iter()
        .copied()
        .filter(|sparse_flag| *sparse_flag == 1)
        .count();
    let fallback_bucket_flags = sparse_bucket_flags.clone();
    let fallback_bucket_count = sparse_bucket_count;

    let fallback_event_count =
        bucket_index_by_event.len() as f64 / bucket_count as f64 + pseudocount;
    let adjusted_event_count_by_bucket: Vec<f64> = event_count_by_bucket
        .iter()
        .enumerate()
        .map(|(bucket_index, event_count)| {
            if sparse_bucket_flags[bucket_index] == 1 {
                fallback_event_count
            } else {
                *event_count as f64 + pseudocount
            }
        })
        .collect();

    let adjusted_event_count_total: f64 = adjusted_event_count_by_bucket.iter().sum();
    if !adjusted_event_count_total.is_finite() || adjusted_event_count_total <= 0.0 {
        return Err(DiscreteSeasonalityByBucketEstimationError::UnstableNormalization);
    }

    let bucket_count_float = bucket_count as f64;
    let density_by_bucket: Vec<f64> = adjusted_event_count_by_bucket
        .iter()
        .map(|adjusted_event_count| *adjusted_event_count / adjusted_event_count_total)
        .collect();
    let factor_by_bucket: Vec<f64> = density_by_bucket
        .iter()
        .map(|density_value| *density_value * bucket_count_float)
        .collect();
    let log_component_by_bucket: Vec<f64> = factor_by_bucket
        .iter()
        .map(|factor_value| factor_value.ln())
        .collect();

    Ok(DiscreteSeasonalityByBucketEstimate {
        event_count_by_bucket,
        sparse_bucket_flags,
        fallback_bucket_flags,
        log_component_by_bucket,
        factor_by_bucket,
        zero_event_bucket_count,
        sparse_bucket_count,
        fallback_bucket_count,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        DayOfWeekSeasonalityEstimatorSettings, DayOfYearSeasonalityEstimatorSettings,
        DeterministicSeasonalityCompositionError, IntradaySeasonalityEstimationError,
        IntradaySeasonalityEstimatorSettings, IntradaySmoothingKernel,
        ScheduledEventOverlayDefinition, SeasonalityDiagnosticsReportError,
        SeasonalityDiagnosticsReportInput, SeasonalityNormalizationError,
        compose_deterministic_seasonality_signal, estimate_day_of_week_seasonality,
        estimate_day_of_year_seasonality, estimate_intraday_seasonality,
        generate_seasonality_diagnostics_batch_report, generate_seasonality_diagnostics_report,
        normalize_seasonality_factors_for_identifiability,
    };

    #[test]
    fn intraday_estimator_recovers_non_parametric_bimodal_pattern() {
        let mut minute_of_day_events: Vec<u16> = Vec::new();

        for _ in 0..80 {
            minute_of_day_events.push(570);
        }
        for _ in 0..20 {
            minute_of_day_events.push(720);
        }
        for _ in 0..70 {
            minute_of_day_events.push(930);
        }

        let estimate = estimate_intraday_seasonality(
            &minute_of_day_events,
            IntradaySeasonalityEstimatorSettings {
                minutes_per_bin: 30,
                smoothing_kernel: IntradaySmoothingKernel::Gaussian,
                smoothing_bandwidth_bins: 1.2,
                minimum_bin_events: 1,
                pseudocount: 1e-6,
            },
        )
        .expect("intraday estimation should succeed");

        let open_bin = 570_usize / 30;
        let midday_bin = 720_usize / 30;
        let close_bin = 930_usize / 30;

        assert_eq!(estimate.minutes_per_bin, 30);
        assert!(
            estimate.intraday_factor_by_bin[open_bin] > estimate.intraday_factor_by_bin[midday_bin]
        );
        assert!(
            estimate.intraday_factor_by_bin[close_bin]
                > estimate.intraday_factor_by_bin[midday_bin]
        );
        assert_eq!(
            estimate.diagnostics.input_event_count,
            minute_of_day_events.len()
        );
    }

    #[test]
    fn intraday_estimator_handles_sparse_bins_and_day_boundary_wrap() {
        let minute_of_day_events: Vec<u16> = vec![0, 0, 1, 1439, 1438, 1439];
        let estimate = estimate_intraday_seasonality(
            &minute_of_day_events,
            IntradaySeasonalityEstimatorSettings {
                minutes_per_bin: 1,
                smoothing_kernel: IntradaySmoothingKernel::Gaussian,
                smoothing_bandwidth_bins: 2.0,
                minimum_bin_events: 2,
                pseudocount: 1e-6,
            },
        )
        .expect("intraday estimation should succeed");

        assert_eq!(estimate.diagnostics.bin_count, 1_440);
        assert!(estimate.diagnostics.sparse_bin_count > 1_000);
        assert!(
            estimate
                .intraday_factor_by_bin
                .iter()
                .all(|value| *value > 0.0)
        );
        assert!(
            (estimate.intraday_factor_by_bin[0] - estimate.intraday_factor_by_bin[1_439]).abs()
                < 0.3
        );
        assert!(estimate.intraday_factor_by_bin[0] > estimate.intraday_factor_by_bin[720]);
    }

    #[test]
    fn intraday_estimator_is_reproducible_for_identical_inputs() {
        let minute_of_day_events: Vec<u16> = vec![15, 16, 17, 540, 541, 542, 1000, 1001];
        let settings = IntradaySeasonalityEstimatorSettings {
            minutes_per_bin: 5,
            smoothing_kernel: IntradaySmoothingKernel::Uniform,
            smoothing_bandwidth_bins: 2.0,
            minimum_bin_events: 1,
            pseudocount: 1e-6,
        };

        let first_estimate = estimate_intraday_seasonality(&minute_of_day_events, settings)
            .expect("intraday estimation should succeed");
        let second_estimate = estimate_intraday_seasonality(&minute_of_day_events, settings)
            .expect("intraday estimation should succeed");

        assert_eq!(first_estimate, second_estimate);
    }

    #[test]
    fn intraday_estimator_rejects_empty_input() {
        let result = estimate_intraday_seasonality(
            &[],
            IntradaySeasonalityEstimatorSettings {
                minutes_per_bin: 1,
                smoothing_kernel: IntradaySmoothingKernel::Gaussian,
                smoothing_bandwidth_bins: 1.0,
                minimum_bin_events: 1,
                pseudocount: 1e-6,
            },
        );

        assert_eq!(
            result,
            Err(IntradaySeasonalityEstimationError::EmptyInputEvents)
        );
    }

    #[test]
    fn day_of_week_estimator_recovers_synthetic_weekday_pattern() {
        let mut day_of_week_events: Vec<u8> = Vec::new();

        for _ in 0..60 {
            day_of_week_events.push(1);
        }
        for _ in 0..30 {
            day_of_week_events.push(2);
        }
        for _ in 0..10 {
            day_of_week_events.push(3);
        }
        for _ in 0..20 {
            day_of_week_events.push(4);
        }
        for _ in 0..50 {
            day_of_week_events.push(5);
        }

        let estimate = estimate_day_of_week_seasonality(
            &day_of_week_events,
            DayOfWeekSeasonalityEstimatorSettings {
                minimum_bucket_events: 2,
                pseudocount: 1e-6,
            },
        )
        .expect("day-of-week estimation should succeed");

        assert!(estimate.day_of_week_factor[0] > estimate.day_of_week_factor[2]);
        assert!(estimate.day_of_week_factor[4] > estimate.day_of_week_factor[2]);
        assert_eq!(estimate.sparse_bucket_flags[5], 1);
        assert_eq!(estimate.sparse_bucket_flags[6], 1);

        let mean_factor = estimate.day_of_week_factor.iter().sum::<f64>()
            / estimate.day_of_week_factor.len() as f64;

        assert!((mean_factor - 1.0).abs() < 1e-12);
        assert_eq!(
            estimate.diagnostics.input_event_count,
            day_of_week_events.len()
        );
    }

    #[test]
    fn day_of_year_estimator_handles_sparse_day_buckets_with_defined_fallback() {
        let mut day_of_year_events: Vec<u16> = Vec::new();

        for _ in 0..60 {
            day_of_year_events.push(60);
        }
        for _ in 0..20 {
            day_of_year_events.push(200);
        }

        day_of_year_events.push(330);

        let estimate = estimate_day_of_year_seasonality(
            &day_of_year_events,
            DayOfYearSeasonalityEstimatorSettings {
                minimum_bucket_events: 5,
                pseudocount: 1e-6,
            },
        )
        .expect("day-of-year estimation should succeed");

        assert!(estimate.day_of_year_factor[59] > estimate.day_of_year_factor[199]);
        assert_eq!(estimate.sparse_bucket_flags[329], 1);
        assert_eq!(estimate.sparse_bucket_flags[149], 1);
        assert!(
            (estimate.day_of_year_factor[329] - estimate.day_of_year_factor[149]).abs() < 1e-12
        );

        let mean_factor = estimate.day_of_year_factor.iter().sum::<f64>()
            / estimate.day_of_year_factor.len() as f64;

        assert!((mean_factor - 1.0).abs() < 1e-12);
        assert!(estimate.diagnostics.fallback_bucket_count > 300);
    }

    #[test]
    fn deterministic_seasonality_composition_combines_components_deterministically() {
        let mut intraday_log_component_by_bin: Vec<f64> = vec![0.0; 24];
        intraday_log_component_by_bin[9] = 2.0_f64.ln();
        intraday_log_component_by_bin[15] = 1.5_f64.ln();

        let mut day_of_week_log_component: Vec<f64> = vec![0.0; 7];
        day_of_week_log_component[0] = 1.2_f64.ln();
        day_of_week_log_component[4] = 0.8_f64.ln();

        let mut day_of_year_log_component: Vec<f64> = vec![0.0; 366];
        day_of_year_log_component[31] = 1.1_f64.ln();
        day_of_year_log_component[200] = 0.9_f64.ln();

        let first_output = compose_deterministic_seasonality_signal(
            &[1_000, 2_000],
            &[570, 930],
            &[1, 5],
            &[32, 201],
            60,
            &intraday_log_component_by_bin,
            &day_of_week_log_component,
            &day_of_year_log_component,
            &[],
        )
        .expect("composition should succeed");

        let second_output = compose_deterministic_seasonality_signal(
            &[1_000, 2_000],
            &[570, 930],
            &[1, 5],
            &[32, 201],
            60,
            &intraday_log_component_by_bin,
            &day_of_week_log_component,
            &day_of_year_log_component,
            &[],
        )
        .expect("composition should succeed");

        assert_eq!(first_output, second_output);
        assert_eq!(first_output.intraday_bin_index_by_event, vec![9, 15]);
        assert!((first_output.combined_factor_by_event[0] - (2.64 / 1.86)).abs() < 1e-12);
        assert!((first_output.combined_factor_by_event[1] - (1.08 / 1.86)).abs() < 1e-12);
        assert_eq!(
            first_output
                .diagnostics
                .normalization_metadata
                .normalization_policy,
            "mean_factor_anchor.v1"
        );
        assert_eq!(
            first_output
                .diagnostics
                .normalization_metadata
                .fit_window_event_count,
            2
        );
        assert!(
            (first_output
                .diagnostics
                .normalization_metadata
                .pre_normalization_mean_factor
                - 1.86)
                .abs()
                < 1e-12
        );
        assert!(
            (first_output
                .diagnostics
                .normalization_metadata
                .post_normalization_mean_factor
                - 1.0)
                .abs()
                < 1e-12
        );
    }

    #[test]
    fn deterministic_seasonality_composition_applies_single_and_multiple_overlay_windows() {
        let output = compose_deterministic_seasonality_signal(
            &[1_000, 1_500, 5_000],
            &[570, 600, 630],
            &[1, 1, 1],
            &[32, 32, 32],
            60,
            &vec![0.0; 24],
            &vec![0.0; 7],
            &vec![0.0; 366],
            &[
                ScheduledEventOverlayDefinition {
                    overlay_id: "cpi_release".to_string(),
                    start_timestamp_utc_nanos: 900,
                    end_timestamp_utc_nanos: 1_600,
                    log_adjustment: 0.2,
                    precedence: 1,
                    enabled: true,
                    source_label: "macro".to_string(),
                },
                ScheduledEventOverlayDefinition {
                    overlay_id: "earnings_call".to_string(),
                    start_timestamp_utc_nanos: 4_800,
                    end_timestamp_utc_nanos: 5_100,
                    log_adjustment: -0.1,
                    precedence: 1,
                    enabled: true,
                    source_label: "corporate".to_string(),
                },
            ],
        )
        .expect("composition with overlays should succeed");

        assert_eq!(output.special_log_component_by_event, vec![0.2, 0.2, -0.1]);
        assert_eq!(output.special_overlay_active_flags_by_event, vec![1, 1, 1]);
        assert_eq!(
            output.active_overlay_ids_by_event,
            vec![
                "cpi_release".to_string(),
                "cpi_release".to_string(),
                "earnings_call".to_string(),
            ]
        );
        assert_eq!(
            output
                .diagnostics
                .overlay_provenance_metadata
                .configured_overlay_count,
            2
        );
        assert_eq!(
            output
                .diagnostics
                .overlay_provenance_metadata
                .active_overlay_event_count,
            3
        );
        assert_eq!(
            output
                .diagnostics
                .overlay_provenance_metadata
                .applied_overlay_ids_in_order,
            vec!["cpi_release".to_string(), "earnings_call".to_string()]
        );
    }

    #[test]
    fn deterministic_seasonality_composition_applies_overlap_precedence_deterministically() {
        let output = compose_deterministic_seasonality_signal(
            &[1_200],
            &[570],
            &[1],
            &[32],
            60,
            &vec![0.0; 24],
            &vec![0.0; 7],
            &vec![0.0; 366],
            &[
                ScheduledEventOverlayDefinition {
                    overlay_id: "low_priority".to_string(),
                    start_timestamp_utc_nanos: 900,
                    end_timestamp_utc_nanos: 1_500,
                    log_adjustment: 0.15,
                    precedence: 1,
                    enabled: true,
                    source_label: "macro".to_string(),
                },
                ScheduledEventOverlayDefinition {
                    overlay_id: "high_priority".to_string(),
                    start_timestamp_utc_nanos: 1_100,
                    end_timestamp_utc_nanos: 1_300,
                    log_adjustment: 0.3,
                    precedence: 2,
                    enabled: true,
                    source_label: "macro".to_string(),
                },
                ScheduledEventOverlayDefinition {
                    overlay_id: "high_priority_peer".to_string(),
                    start_timestamp_utc_nanos: 1_100,
                    end_timestamp_utc_nanos: 1_300,
                    log_adjustment: 0.2,
                    precedence: 2,
                    enabled: true,
                    source_label: "macro".to_string(),
                },
            ],
        )
        .expect("composition with overlap should succeed");

        assert_eq!(output.special_overlay_active_flags_by_event, vec![1]);
        assert_eq!(output.special_log_component_by_event, vec![0.5]);
        assert_eq!(
            output.active_overlay_ids_by_event,
            vec!["high_priority,high_priority_peer".to_string()]
        );
        assert_eq!(
            output
                .diagnostics
                .overlay_provenance_metadata
                .applied_overlay_ids_in_order,
            vec![
                "high_priority".to_string(),
                "high_priority_peer".to_string()
            ]
        );
    }

    #[test]
    fn deterministic_seasonality_composition_is_noop_when_overlays_are_disabled() {
        let without_overlays = compose_deterministic_seasonality_signal(
            &[1_000, 2_000],
            &[570, 630],
            &[1, 1],
            &[32, 32],
            60,
            &vec![0.0; 24],
            &vec![0.0; 7],
            &vec![0.0; 366],
            &[],
        )
        .expect("base composition should succeed");
        let with_disabled_overlay = compose_deterministic_seasonality_signal(
            &[1_000, 2_000],
            &[570, 630],
            &[1, 1],
            &[32, 32],
            60,
            &vec![0.0; 24],
            &vec![0.0; 7],
            &vec![0.0; 366],
            &[ScheduledEventOverlayDefinition {
                overlay_id: "disabled_overlay".to_string(),
                start_timestamp_utc_nanos: 900,
                end_timestamp_utc_nanos: 2_100,
                log_adjustment: 2.0,
                precedence: 99,
                enabled: false,
                source_label: "test".to_string(),
            }],
        )
        .expect("composition with disabled overlay should succeed");

        assert_eq!(
            without_overlays.special_log_component_by_event,
            with_disabled_overlay.special_log_component_by_event
        );
        assert_eq!(
            without_overlays.special_overlay_active_flags_by_event,
            with_disabled_overlay.special_overlay_active_flags_by_event
        );
        assert_eq!(
            without_overlays.combined_log_component_by_event,
            with_disabled_overlay.combined_log_component_by_event
        );
        assert_eq!(
            without_overlays.combined_factor_by_event,
            with_disabled_overlay.combined_factor_by_event
        );
        assert_eq!(
            with_disabled_overlay
                .diagnostics
                .overlay_provenance_metadata
                .active_overlay_event_count,
            0
        );
    }

    #[test]
    fn deterministic_seasonality_composition_rejects_mismatched_input_lengths() {
        let error = compose_deterministic_seasonality_signal(
            &[1_000, 2_000],
            &[570, 930],
            &[1],
            &[32, 201],
            60,
            &vec![0.0; 24],
            &vec![0.0; 7],
            &vec![0.0; 366],
            &[],
        )
        .expect_err("composition should fail for mismatched input lengths");

        assert_eq!(
            error,
            DeterministicSeasonalityCompositionError::MismatchedInputLengths {
                timestamp_utc_nanos_count: 2,
                minute_of_day_count: 2,
                day_of_week_count: 1,
                day_of_year_count: 2,
            }
        );
    }

    #[test]
    fn deterministic_seasonality_composition_rejects_non_finite_log_components() {
        let mut intraday_log_component_by_bin: Vec<f64> = vec![0.0; 24];
        intraday_log_component_by_bin[9] = f64::NAN;

        let error = compose_deterministic_seasonality_signal(
            &[1_000],
            &[570],
            &[1],
            &[32],
            60,
            &intraday_log_component_by_bin,
            &vec![0.0; 7],
            &vec![0.0; 366],
            &[],
        )
        .expect_err("composition should fail for non-finite log components");

        assert_eq!(
            error,
            DeterministicSeasonalityCompositionError::InvalidCombinedLogComponent {
                record_position: 0,
            }
        );
    }

    #[test]
    fn seasonality_normalization_anchors_mean_and_emits_metadata() {
        let factors = vec![2.64, 1.08];
        let logs = vec![2.64_f64.ln(), 1.08_f64.ln()];

        let normalized = normalize_seasonality_factors_for_identifiability(&factors, &logs)
            .expect("normalization should succeed");

        let anchored_mean = normalized.normalized_factor_by_event.iter().sum::<f64>()
            / normalized.normalized_factor_by_event.len() as f64;

        assert!((anchored_mean - 1.0).abs() < 1e-12);
        assert_eq!(
            normalized.normalization_metadata.normalization_policy,
            "mean_factor_anchor.v1"
        );
        assert_eq!(normalized.normalization_metadata.fit_window_event_count, 2);
    }

    #[test]
    fn seasonality_normalization_rejects_invalid_inputs_and_degenerate_windows() {
        let empty_result = normalize_seasonality_factors_for_identifiability(&[], &[]);
        assert_eq!(
            empty_result,
            Err(SeasonalityNormalizationError::EmptyFitWindow)
        );

        let invalid_factor_result =
            normalize_seasonality_factors_for_identifiability(&[1.0, f64::NAN], &[0.0, 0.0]);
        assert_eq!(
            invalid_factor_result,
            Err(SeasonalityNormalizationError::InvalidInputFactor { record_position: 1 })
        );

        let invalid_log_component_result =
            normalize_seasonality_factors_for_identifiability(&[1.0, 2.0], &[0.0, f64::INFINITY]);
        assert_eq!(
            invalid_log_component_result,
            Err(SeasonalityNormalizationError::InvalidInputLogComponent { record_position: 1 })
        );
    }

    #[test]
    fn diagnostics_report_generates_component_metrics_and_reproducibility_metadata() {
        let fixture = build_diagnostics_fixture();
        let report = generate_seasonality_diagnostics_report(SeasonalityDiagnosticsReportInput {
            dataset_id: "dataset_alpha",
            input_schema_version: "seasonality.timestamp_event.v1",
            preprocessed_schema_version: "seasonality.timestamp_event.preprocessed.v1",
            exchange_code: "XNYS",
            exchange_timezone_name: "America/New_York",
            source_record_positions: &fixture.source_record_positions,
            timestamp_utc_nanos: &fixture.timestamp_utc_nanos,
            exchange_local_minute_of_day: &fixture.exchange_local_minute_of_day,
            exchange_local_day_of_week: &fixture.exchange_local_day_of_week,
            exchange_local_day_of_year: &fixture.exchange_local_day_of_year,
            market_state_codes: &fixture.market_state_codes,
            intraday_estimate: &fixture.intraday_estimate,
            day_of_week_estimate: &fixture.day_of_week_estimate,
            day_of_year_estimate: &fixture.day_of_year_estimate,
            composition: &fixture.composition,
            estimator_version: "seasonality_estimator.v1",
        })
        .expect("diagnostics report generation should succeed");

        assert_eq!(report.schema_version, "seasonality.diagnostics.report.v1");
        assert_eq!(report.dataset_id, "dataset_alpha".to_string());
        assert_eq!(report.intraday_component_summary.component_name, "intraday");
        assert_eq!(
            report.day_of_week_component_summary.component_name,
            "day_of_week"
        );
        assert_eq!(
            report.day_of_year_component_summary.component_name,
            "day_of_year"
        );
        assert!(report.fit_quality_metrics.combined_factor_mean > 0.0);
        assert!(report.fit_quality_metrics.normalization_anchor_error <= 1e-12);

        let overlay_summary = report
            .overlay_component_summary
            .expect("overlay summary should exist when overlays are configured");

        assert_eq!(overlay_summary.configured_overlay_count, 1);
        assert_eq!(overlay_summary.active_overlay_event_count, 3);

        assert_eq!(
            report
                .reproducibility_metadata
                .input_snapshot
                .snapshot_fingerprint_algorithm,
            "fnv1a64.v1"
        );
        assert_eq!(
            report.reproducibility_metadata.estimator_version,
            "seasonality_estimator.v1".to_string()
        );
        assert_eq!(
            report
                .reproducibility_metadata
                .input_snapshot
                .input_event_count,
            fixture.timestamp_utc_nanos.len()
        );
        assert_eq!(
            report
                .reproducibility_metadata
                .input_snapshot
                .snapshot_fingerprint_hex
                .len(),
            16
        );
    }

    #[test]
    fn diagnostics_report_rejects_missing_required_metadata_fields() {
        let fixture = build_diagnostics_fixture();

        let error = generate_seasonality_diagnostics_report(SeasonalityDiagnosticsReportInput {
            dataset_id: "",
            input_schema_version: "seasonality.timestamp_event.v1",
            preprocessed_schema_version: "seasonality.timestamp_event.preprocessed.v1",
            exchange_code: "XNYS",
            exchange_timezone_name: "America/New_York",
            source_record_positions: &fixture.source_record_positions,
            timestamp_utc_nanos: &fixture.timestamp_utc_nanos,
            exchange_local_minute_of_day: &fixture.exchange_local_minute_of_day,
            exchange_local_day_of_week: &fixture.exchange_local_day_of_week,
            exchange_local_day_of_year: &fixture.exchange_local_day_of_year,
            market_state_codes: &fixture.market_state_codes,
            intraday_estimate: &fixture.intraday_estimate,
            day_of_week_estimate: &fixture.day_of_week_estimate,
            day_of_year_estimate: &fixture.day_of_year_estimate,
            composition: &fixture.composition,
            estimator_version: "seasonality_estimator.v1",
        })
        .expect_err("diagnostics report should fail when dataset id is empty");

        assert_eq!(error, SeasonalityDiagnosticsReportError::EmptyDatasetId);
    }

    #[test]
    fn diagnostics_report_is_deterministic_for_fixed_inputs() {
        let fixture = build_diagnostics_fixture();

        let first_report =
            generate_seasonality_diagnostics_report(SeasonalityDiagnosticsReportInput {
                dataset_id: "dataset_alpha",
                input_schema_version: "seasonality.timestamp_event.v1",
                preprocessed_schema_version: "seasonality.timestamp_event.preprocessed.v1",
                exchange_code: "XNYS",
                exchange_timezone_name: "America/New_York",
                source_record_positions: &fixture.source_record_positions,
                timestamp_utc_nanos: &fixture.timestamp_utc_nanos,
                exchange_local_minute_of_day: &fixture.exchange_local_minute_of_day,
                exchange_local_day_of_week: &fixture.exchange_local_day_of_week,
                exchange_local_day_of_year: &fixture.exchange_local_day_of_year,
                market_state_codes: &fixture.market_state_codes,
                intraday_estimate: &fixture.intraday_estimate,
                day_of_week_estimate: &fixture.day_of_week_estimate,
                day_of_year_estimate: &fixture.day_of_year_estimate,
                composition: &fixture.composition,
                estimator_version: "seasonality_estimator.v1",
            })
            .expect("first diagnostics report generation should succeed");

        let second_report =
            generate_seasonality_diagnostics_report(SeasonalityDiagnosticsReportInput {
                dataset_id: "dataset_alpha",
                input_schema_version: "seasonality.timestamp_event.v1",
                preprocessed_schema_version: "seasonality.timestamp_event.preprocessed.v1",
                exchange_code: "XNYS",
                exchange_timezone_name: "America/New_York",
                source_record_positions: &fixture.source_record_positions,
                timestamp_utc_nanos: &fixture.timestamp_utc_nanos,
                exchange_local_minute_of_day: &fixture.exchange_local_minute_of_day,
                exchange_local_day_of_week: &fixture.exchange_local_day_of_week,
                exchange_local_day_of_year: &fixture.exchange_local_day_of_year,
                market_state_codes: &fixture.market_state_codes,
                intraday_estimate: &fixture.intraday_estimate,
                day_of_week_estimate: &fixture.day_of_week_estimate,
                day_of_year_estimate: &fixture.day_of_year_estimate,
                composition: &fixture.composition,
                estimator_version: "seasonality_estimator.v1",
            })
            .expect("second diagnostics report generation should succeed");

        assert_eq!(first_report, second_report);
    }

    #[test]
    fn diagnostics_batch_report_supports_multiple_datasets_and_duplicate_guards() {
        let fixture = build_diagnostics_fixture();

        let first_input = SeasonalityDiagnosticsReportInput {
            dataset_id: "dataset_alpha",
            input_schema_version: "seasonality.timestamp_event.v1",
            preprocessed_schema_version: "seasonality.timestamp_event.preprocessed.v1",
            exchange_code: "XNYS",
            exchange_timezone_name: "America/New_York",
            source_record_positions: &fixture.source_record_positions,
            timestamp_utc_nanos: &fixture.timestamp_utc_nanos,
            exchange_local_minute_of_day: &fixture.exchange_local_minute_of_day,
            exchange_local_day_of_week: &fixture.exchange_local_day_of_week,
            exchange_local_day_of_year: &fixture.exchange_local_day_of_year,
            market_state_codes: &fixture.market_state_codes,
            intraday_estimate: &fixture.intraday_estimate,
            day_of_week_estimate: &fixture.day_of_week_estimate,
            day_of_year_estimate: &fixture.day_of_year_estimate,
            composition: &fixture.composition,
            estimator_version: "seasonality_estimator.v1",
        };

        let second_input = SeasonalityDiagnosticsReportInput {
            dataset_id: "dataset_beta",
            ..first_input
        };

        let batch_report =
            generate_seasonality_diagnostics_batch_report(&[first_input, second_input])
                .expect("batch diagnostics report generation should succeed");

        assert_eq!(
            batch_report.schema_version,
            "seasonality.diagnostics.batch.v1"
        );
        assert_eq!(batch_report.report_count, 2);
        assert_eq!(
            batch_report.dataset_ids_in_order,
            vec!["dataset_alpha".to_string(), "dataset_beta".to_string()]
        );

        let duplicate_dataset_error =
            generate_seasonality_diagnostics_batch_report(&[first_input, first_input])
                .expect_err("batch diagnostics should reject duplicate dataset ids");

        assert_eq!(
            duplicate_dataset_error,
            SeasonalityDiagnosticsReportError::DuplicateDatasetId {
                dataset_id: "dataset_alpha".to_string()
            }
        );
    }

    struct DiagnosticsFixture {
        source_record_positions: Vec<u64>,
        timestamp_utc_nanos: Vec<i64>,
        exchange_local_minute_of_day: Vec<u16>,
        exchange_local_day_of_week: Vec<u8>,
        exchange_local_day_of_year: Vec<u16>,
        market_state_codes: Vec<u8>,
        intraday_estimate: super::IntradaySeasonalityEstimateOutput,
        day_of_week_estimate: super::DayOfWeekSeasonalityEstimateOutput,
        day_of_year_estimate: super::DayOfYearSeasonalityEstimateOutput,
        composition: super::DeterministicSeasonalityCompositionOutput,
    }

    fn build_diagnostics_fixture() -> DiagnosticsFixture {
        let source_record_positions = vec![0, 1, 2, 3, 4, 5];
        let timestamp_utc_nanos = vec![1_000, 2_000, 3_000, 4_000, 5_000, 6_000];
        let exchange_local_minute_of_day = vec![570, 571, 572, 930, 931, 932];
        let exchange_local_day_of_week = vec![1, 1, 2, 2, 5, 5];
        let exchange_local_day_of_year = vec![32, 32, 33, 33, 36, 36];
        let market_state_codes = vec![1, 1, 1, 1, 1, 1];

        let intraday_estimate = estimate_intraday_seasonality(
            &exchange_local_minute_of_day,
            IntradaySeasonalityEstimatorSettings {
                minutes_per_bin: 60,
                smoothing_kernel: IntradaySmoothingKernel::Gaussian,
                smoothing_bandwidth_bins: 1.0,
                minimum_bin_events: 1,
                pseudocount: 1e-6,
            },
        )
        .expect("intraday estimation should succeed for diagnostics fixture");

        let day_of_week_estimate = estimate_day_of_week_seasonality(
            &exchange_local_day_of_week,
            DayOfWeekSeasonalityEstimatorSettings {
                minimum_bucket_events: 1,
                pseudocount: 1e-6,
            },
        )
        .expect("day-of-week estimation should succeed for diagnostics fixture");

        let day_of_year_estimate = estimate_day_of_year_seasonality(
            &exchange_local_day_of_year,
            DayOfYearSeasonalityEstimatorSettings {
                minimum_bucket_events: 1,
                pseudocount: 1e-6,
            },
        )
        .expect("day-of-year estimation should succeed for diagnostics fixture");

        let composition = compose_deterministic_seasonality_signal(
            &timestamp_utc_nanos,
            &exchange_local_minute_of_day,
            &exchange_local_day_of_week,
            &exchange_local_day_of_year,
            intraday_estimate.minutes_per_bin,
            &intraday_estimate.intraday_log_component_by_bin,
            &day_of_week_estimate.day_of_week_log_component,
            &day_of_year_estimate.day_of_year_log_component,
            &[ScheduledEventOverlayDefinition {
                overlay_id: "macro_release".to_string(),
                start_timestamp_utc_nanos: 2_500,
                end_timestamp_utc_nanos: 5_500,
                log_adjustment: 0.2,
                precedence: 1,
                enabled: true,
                source_label: "macro".to_string(),
            }],
        )
        .expect("composition should succeed for diagnostics fixture");

        DiagnosticsFixture {
            source_record_positions,
            timestamp_utc_nanos,
            exchange_local_minute_of_day,
            exchange_local_day_of_week,
            exchange_local_day_of_year,
            market_state_codes,
            intraday_estimate,
            day_of_week_estimate,
            day_of_year_estimate,
            composition,
        }
    }
}
