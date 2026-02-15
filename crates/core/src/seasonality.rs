//! Nonparametric seasonality estimators used by deterministic model preprocessing.

use std::error::Error;
use std::fmt::{Display, Formatter};

pub const INTRADAY_SEASONALITY_ESTIMATE_SCHEMA_VERSION: &str = "seasonality.intraday.estimate.v1";

const MINUTES_PER_DAY: usize = 1_440;

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

#[cfg(test)]
mod tests {
    use super::{
        IntradaySeasonalityEstimationError, IntradaySeasonalityEstimatorSettings,
        IntradaySmoothingKernel, estimate_intraday_seasonality,
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
}
