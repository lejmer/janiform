//! Nonparametric seasonality estimators used by deterministic model preprocessing.

use std::error::Error;
use std::fmt::{Display, Formatter};

pub const INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION: &str = "seasonality.intraday.estimate.v1";
pub const DAY_OF_WEEK_SEASONALITY_ESTIMATE_SCHEMA_VERSION: &str =
    "seasonality.day_of_week.estimate.v1";
pub const DAY_OF_YEAR_SEASONALITY_ESTIMATE_SCHEMA_VERSION: &str =
    "seasonality.day_of_year.estimate.v1";
pub const DETERMINISTIC_SEASONALITY_COMPOSITION_SCHEMA_VERSION: &str =
    "seasonality.deterministic.composition.v1";

const MINUTES_PER_DAY: usize = 1_440;
const DAYS_PER_WEEK: usize = 7;
const DAYS_PER_YEAR: usize = 366;
const IDENTIFIABILITY_NORMALIZATION_POLICY: &str = "mean_factor_anchor.v1";
const IDENTIFIABILITY_TARGET_MEAN_FACTOR: f64 = 1.0;
const IDENTIFIABILITY_MEAN_ANCHOR_TOLERANCE: f64 = 1e-12;

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

/// Diagnostics emitted by deterministic seasonality composition.
#[derive(Debug, Clone, PartialEq)]
pub struct DeterministicSeasonalityCompositionDiagnostics {
    pub input_event_count: usize,
    pub normalization_metadata: SeasonalityNormalizationMetadata,
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
    pub combined_log_component_by_event: Vec<f64>,
    pub combined_factor_by_event: Vec<f64>,
    pub diagnostics: DeterministicSeasonalityCompositionDiagnostics,
}

/// Compose intraday, day-of-week, and day-of-year log components into a unified signal.
pub fn compose_deterministic_seasonality_signal(
    exchange_local_minute_of_day: &[u16],
    exchange_local_day_of_week: &[u8],
    exchange_local_day_of_year: &[u16],
    intraday_minutes_per_bin: u16,
    intraday_log_component_by_bin: &[f64],
    day_of_week_log_component: &[f64],
    day_of_year_log_component: &[f64],
) -> Result<DeterministicSeasonalityCompositionOutput, DeterministicSeasonalityCompositionError> {
    if exchange_local_minute_of_day.is_empty() {
        return Err(DeterministicSeasonalityCompositionError::EmptyInputEvents);
    }
    let input_event_count = exchange_local_minute_of_day.len();
    if exchange_local_day_of_week.len() != input_event_count
        || exchange_local_day_of_year.len() != input_event_count
    {
        return Err(
            DeterministicSeasonalityCompositionError::MismatchedInputLengths {
                minute_of_day_count: input_event_count,
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

    let mut intraday_bin_index_by_event: Vec<u16> = Vec::with_capacity(input_event_count);
    let mut intraday_log_component_by_event: Vec<f64> = Vec::with_capacity(input_event_count);
    let mut day_of_week_log_component_by_event: Vec<f64> = Vec::with_capacity(input_event_count);
    let mut day_of_year_log_component_by_event: Vec<f64> = Vec::with_capacity(input_event_count);
    let mut combined_log_component_by_event: Vec<f64> = Vec::with_capacity(input_event_count);
    let mut combined_factor_by_event: Vec<f64> = Vec::with_capacity(input_event_count);

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
        let combined_log_component = intraday_log_component
            + day_of_week_log_component_value
            + day_of_year_log_component_value;
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
        combined_log_component_by_event.push(combined_log_component);
        combined_factor_by_event.push(combined_factor);
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
        combined_log_component_by_event: normalization_result.normalized_log_component_by_event,
        combined_factor_by_event: normalization_result.normalized_factor_by_event,
        diagnostics: DeterministicSeasonalityCompositionDiagnostics {
            input_event_count,
            normalization_metadata: normalization_result.normalization_metadata,
        },
    })
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
                minute_of_day_count,
                day_of_week_count,
                day_of_year_count,
            } => write!(
                formatter,
                "composition inputs must have matching lengths, received minute_of_day={minute_of_day_count}, day_of_week={day_of_week_count}, day_of_year={day_of_year_count}"
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
        SeasonalityNormalizationError, compose_deterministic_seasonality_signal,
        estimate_day_of_week_seasonality, estimate_day_of_year_seasonality,
        estimate_intraday_seasonality, normalize_seasonality_factors_for_identifiability,
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
            &[570, 930],
            &[1, 5],
            &[32, 201],
            60,
            &intraday_log_component_by_bin,
            &day_of_week_log_component,
            &day_of_year_log_component,
        )
        .expect("composition should succeed");
        let second_output = compose_deterministic_seasonality_signal(
            &[570, 930],
            &[1, 5],
            &[32, 201],
            60,
            &intraday_log_component_by_bin,
            &day_of_week_log_component,
            &day_of_year_log_component,
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
    fn deterministic_seasonality_composition_rejects_mismatched_input_lengths() {
        let error = compose_deterministic_seasonality_signal(
            &[570, 930],
            &[1],
            &[32, 201],
            60,
            &vec![0.0; 24],
            &vec![0.0; 7],
            &vec![0.0; 366],
        )
        .expect_err("composition should fail for mismatched input lengths");

        assert_eq!(
            error,
            DeterministicSeasonalityCompositionError::MismatchedInputLengths {
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
            &[570],
            &[1],
            &[32],
            60,
            &intraday_log_component_by_bin,
            &vec![0.0; 7],
            &vec![0.0; 366],
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
}
