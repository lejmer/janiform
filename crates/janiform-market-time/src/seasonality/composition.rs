use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use super::normalization::{
    SeasonalityNormalizationError, normalize_seasonality_factors_for_identifiability,
};
use super::{
    DAYS_PER_WEEK, DAYS_PER_YEAR, DETERMINISTIC_SEASONALITY_COMPOSITION_SCHEMA_VERSION,
    MINUTES_PER_DAY, SPECIAL_OVERLAY_MERGE_POLICY, SPECIAL_OVERLAY_PRECEDENCE_POLICY,
    SPECIAL_OVERLAY_WINDOW_BOUNDARY_POLICY,
};

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

/// Input contract for deterministic seasonality composition.
#[derive(Debug, Clone, Copy)]
pub struct DeterministicSeasonalityCompositionInput<'a> {
    pub timestamp_utc_nanos: &'a [i64],
    pub exchange_local_minute_of_day: &'a [u16],
    pub exchange_local_day_of_week: &'a [u8],
    pub exchange_local_day_of_year: &'a [u16],
    pub intraday_minutes_per_bin: u16,
    pub intraday_log_component_by_bin: &'a [f64],
    pub day_of_week_log_component: &'a [f64],
    pub day_of_year_log_component: &'a [f64],
    pub scheduled_event_overlays: &'a [ScheduledEventOverlayDefinition],
}

/// Compose intraday, day-of-week, and day-of-year log components into a unified signal.
pub fn compose_deterministic_seasonality_signal(
    input: DeterministicSeasonalityCompositionInput<'_>,
) -> Result<DeterministicSeasonalityCompositionOutput, DeterministicSeasonalityCompositionError> {
    let DeterministicSeasonalityCompositionInput {
        timestamp_utc_nanos,
        exchange_local_minute_of_day,
        exchange_local_day_of_week,
        exchange_local_day_of_year,
        intraday_minutes_per_bin,
        intraday_log_component_by_bin,
        day_of_week_log_component,
        day_of_year_log_component,
        scheduled_event_overlays,
    } = input;

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
