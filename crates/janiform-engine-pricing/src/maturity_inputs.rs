use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Duration, Months, NaiveDate, Utc};
use janiform_foundations::time::parse_utc_datetime;
use serde::{Deserialize, Serialize};

use crate::interval_schedule::{
    PricingIntervalSchedule, PricingIntervalScheduleError, PricingIntervalSchedulePolicy,
    derive_pricing_interval_schedule,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HighLevelPricingMaturityResolutionRequest {
    pub valuation_time_utc: String,
    pub expiry_timestamps_utc: Option<Vec<Option<String>>>,
    pub expiry_dates: Option<Vec<Option<String>>>,
    pub tenors: Option<Vec<Option<String>>>,
    #[serde(default)]
    pub pricing_interval_schedule_policy: PricingIntervalSchedulePolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HighLevelPricingMaturityResolution {
    pub maturity_intervals: Vec<f64>,
    pub resolved_expiry_timestamps_utc: Vec<String>,
    pub pricing_interval_schedule: PricingIntervalSchedule,
    pub maturity_terminal_interval_indices: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HighLevelPricingMaturityResolutionError {
    InvalidValuationTime {
        value: String,
    },
    MissingInputs,
    EmptyAxis {
        field: &'static str,
    },
    AxisLengthMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    InvalidNodeSelection {
        index: usize,
    },
    InvalidExpiryTimestamp {
        index: usize,
        value: String,
    },
    InvalidExpiryDate {
        index: usize,
        value: String,
    },
    InvalidTenor {
        index: usize,
        value: String,
    },
    NonPositiveExpiry {
        field: &'static str,
        index: usize,
    },
    TenorOverflow {
        index: usize,
        value: String,
    },
    InvalidPricingIntervalSchedule {
        source: PricingIntervalScheduleError,
    },
}

impl Display for HighLevelPricingMaturityResolutionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidValuationTime { value } => write!(
                formatter,
                "valuation_time_utc must be a valid ISO 8601 datetime with timezone information, found {value}"
            ),
            Self::MissingInputs => write!(
                formatter,
                "at least one of expiry_timestamps_utc, expiry_dates, or tenors must be provided"
            ),
            Self::EmptyAxis { field } => {
                write!(
                    formatter,
                    "{field} must contain at least one maturity input when provided"
                )
            }
            Self::AxisLengthMismatch {
                field,
                expected,
                found,
            } => write!(
                formatter,
                "{field} must contain {expected} entries to match the other maturity input axes, found {found}"
            ),
            Self::InvalidNodeSelection { index } => write!(
                formatter,
                "exactly one of expiry_timestamps_utc[{index}], expiry_dates[{index}], or tenors[{index}] must be provided"
            ),
            Self::InvalidExpiryTimestamp { index, value } => write!(
                formatter,
                "expiry_timestamps_utc[{index}] must be a valid ISO 8601 datetime with timezone information, found {value}"
            ),
            Self::InvalidExpiryDate { index, value } => write!(
                formatter,
                "expiry_dates[{index}] must be a valid ISO 8601 date in YYYY-MM-DD format, found {value}"
            ),
            Self::InvalidTenor { index, value } => write!(
                formatter,
                "tenors[{index}] must match <positive integer><unit> where unit is one of h, d, w, m, y, found {value}"
            ),
            Self::NonPositiveExpiry { field, index } => write!(
                formatter,
                "{field}[{index}] must resolve to a timestamp strictly after valuation_time_utc"
            ),
            Self::TenorOverflow { index, value } => write!(
                formatter,
                "tenors[{index}] with value {value} cannot be represented safely"
            ),
            Self::InvalidPricingIntervalSchedule { source } => {
                write!(formatter, "pricing interval schedule is invalid: {source}")
            }
        }
    }
}

impl Error for HighLevelPricingMaturityResolutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPricingIntervalSchedule { source } => Some(source),
            _ => None,
        }
    }
}

pub fn resolve_high_level_pricing_maturity_inputs(
    request: HighLevelPricingMaturityResolutionRequest,
) -> Result<HighLevelPricingMaturityResolution, HighLevelPricingMaturityResolutionError> {
    let valuation_time_utc = parse_utc_datetime(&request.valuation_time_utc).map_err(|_| {
        HighLevelPricingMaturityResolutionError::InvalidValuationTime {
            value: request.valuation_time_utc.clone(),
        }
    })?;

    let entry_count = maturity_input_entry_count(&request)?;

    let mut resolved_expiry_timestamps_utc_nanos = Vec::with_capacity(entry_count);
    let mut resolved_expiry_timestamps_utc = Vec::with_capacity(entry_count);

    for index in 0..entry_count {
        let expiry_timestamp = resolve_entry_expiry_timestamp(index, &request, valuation_time_utc)?;
        let expiry_timestamp_utc_nanos =
            expiry_timestamp.timestamp_nanos_opt().ok_or_else(|| {
                HighLevelPricingMaturityResolutionError::TenorOverflow {
                    index,
                    value: expiry_timestamp.to_rfc3339(),
                }
            })?;

        if expiry_timestamp_utc_nanos
            <= valuation_time_utc.timestamp_nanos_opt().ok_or_else(|| {
                HighLevelPricingMaturityResolutionError::InvalidValuationTime {
                    value: request.valuation_time_utc.clone(),
                }
            })?
        {
            return Err(match entry_kind(index, &request) {
                EntryKind::ExpiryTimestamp => {
                    HighLevelPricingMaturityResolutionError::NonPositiveExpiry {
                        field: "expiry_timestamps_utc",
                        index,
                    }
                }
                EntryKind::ExpiryDate => {
                    HighLevelPricingMaturityResolutionError::NonPositiveExpiry {
                        field: "expiry_dates",
                        index,
                    }
                }
                EntryKind::Tenor => HighLevelPricingMaturityResolutionError::NonPositiveExpiry {
                    field: "tenors",
                    index,
                },
            });
        }

        resolved_expiry_timestamps_utc_nanos.push(expiry_timestamp_utc_nanos);
        resolved_expiry_timestamps_utc.push(expiry_timestamp.to_rfc3339());
    }

    let derived_schedule = derive_pricing_interval_schedule(
        valuation_time_utc.timestamp_nanos_opt().ok_or_else(|| {
            HighLevelPricingMaturityResolutionError::InvalidValuationTime {
                value: request.valuation_time_utc.clone(),
            }
        })?,
        resolved_expiry_timestamps_utc_nanos.as_slice(),
        &request.pricing_interval_schedule_policy,
    )
    .map_err(|source| {
        HighLevelPricingMaturityResolutionError::InvalidPricingIntervalSchedule { source }
    })?;

    let maturity_intervals = derived_schedule
        .maturity_terminal_interval_indices
        .iter()
        .copied()
        .map(|terminal_interval_index| {
            derived_schedule
                .schedule
                .maturity_interval_hours(terminal_interval_index)
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| {
            HighLevelPricingMaturityResolutionError::InvalidPricingIntervalSchedule { source }
        })?;

    Ok(HighLevelPricingMaturityResolution {
        maturity_intervals,
        resolved_expiry_timestamps_utc,
        pricing_interval_schedule: derived_schedule.schedule,
        maturity_terminal_interval_indices: derived_schedule.maturity_terminal_interval_indices,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    ExpiryTimestamp,
    ExpiryDate,
    Tenor,
}

fn parse_iso_date(raw_value: &str) -> Result<NaiveDate, ()> {
    NaiveDate::parse_from_str(raw_value, "%Y-%m-%d").map_err(|_| ())
}

fn maturity_input_entry_count(
    request: &HighLevelPricingMaturityResolutionRequest,
) -> Result<usize, HighLevelPricingMaturityResolutionError> {
    let mut entry_count = None::<usize>;

    validate_axis_length(
        "expiry_timestamps_utc",
        request.expiry_timestamps_utc.as_ref(),
        &mut entry_count,
    )?;

    validate_axis_length(
        "expiry_dates",
        request.expiry_dates.as_ref(),
        &mut entry_count,
    )?;

    validate_axis_length("tenors", request.tenors.as_ref(), &mut entry_count)?;

    let Some(entry_count) = entry_count else {
        return Err(HighLevelPricingMaturityResolutionError::MissingInputs);
    };

    for index in 0..entry_count {
        let selected_count =
            usize::from(axis_value_is_present(
                request.expiry_timestamps_utc.as_ref(),
                index,
            )) + usize::from(axis_value_is_present(request.expiry_dates.as_ref(), index))
                + usize::from(axis_value_is_present(request.tenors.as_ref(), index));

        if selected_count != 1 {
            return Err(HighLevelPricingMaturityResolutionError::InvalidNodeSelection { index });
        }
    }

    Ok(entry_count)
}

fn validate_axis_length(
    field: &'static str,
    values: Option<&Vec<Option<String>>>,
    entry_count: &mut Option<usize>,
) -> Result<(), HighLevelPricingMaturityResolutionError> {
    let Some(values) = values else {
        return Ok(());
    };

    if values.is_empty() {
        return Err(HighLevelPricingMaturityResolutionError::EmptyAxis { field });
    }

    match entry_count {
        Some(expected) if *expected != values.len() => Err(
            HighLevelPricingMaturityResolutionError::AxisLengthMismatch {
                field,
                expected: *expected,
                found: values.len(),
            },
        ),
        None => {
            *entry_count = Some(values.len());
            Ok(())
        }
        Some(_) => Ok(()),
    }
}

fn axis_value_is_present(values: Option<&Vec<Option<String>>>, index: usize) -> bool {
    values
        .and_then(|entries| entries.get(index))
        .is_some_and(Option::is_some)
}

fn entry_kind(index: usize, request: &HighLevelPricingMaturityResolutionRequest) -> EntryKind {
    if axis_value_is_present(request.expiry_timestamps_utc.as_ref(), index) {
        EntryKind::ExpiryTimestamp
    } else if axis_value_is_present(request.expiry_dates.as_ref(), index) {
        EntryKind::ExpiryDate
    } else {
        EntryKind::Tenor
    }
}

fn resolve_entry_expiry_timestamp(
    index: usize,
    request: &HighLevelPricingMaturityResolutionRequest,
    valuation_time_utc: DateTime<Utc>,
) -> Result<DateTime<Utc>, HighLevelPricingMaturityResolutionError> {
    if let Some(raw_value) = axis_entry_value(request.expiry_timestamps_utc.as_ref(), index) {
        return parse_utc_datetime(raw_value).map_err(|_| {
            HighLevelPricingMaturityResolutionError::InvalidExpiryTimestamp {
                index,
                value: raw_value.to_string(),
            }
        });
    }

    if let Some(raw_value) = axis_entry_value(request.expiry_dates.as_ref(), index) {
        let expiry_date = parse_iso_date(raw_value).map_err(|_| {
            HighLevelPricingMaturityResolutionError::InvalidExpiryDate {
                index,
                value: raw_value.to_string(),
            }
        })?;

        let expiry_timestamp = expiry_date.and_time(valuation_time_utc.time());

        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(
            expiry_timestamp,
            Utc,
        ));
    }

    let raw_value = axis_entry_value(request.tenors.as_ref(), index)
        .ok_or(HighLevelPricingMaturityResolutionError::InvalidNodeSelection { index })?;

    resolve_tenor_expiry_timestamp(index, raw_value, valuation_time_utc)
}

fn axis_entry_value(values: Option<&Vec<Option<String>>>, index: usize) -> Option<&str> {
    values
        .and_then(|entries| entries.get(index))
        .and_then(Option::as_deref)
}

fn resolve_tenor_expiry_timestamp(
    index: usize,
    raw_value: &str,
    valuation_time_utc: DateTime<Utc>,
) -> Result<DateTime<Utc>, HighLevelPricingMaturityResolutionError> {
    let Some(unit) = raw_value.chars().last() else {
        return Err(HighLevelPricingMaturityResolutionError::InvalidTenor {
            index,
            value: raw_value.to_string(),
        });
    };

    let magnitude_text = &raw_value[..raw_value.len() - unit.len_utf8()];
    if magnitude_text.is_empty()
        || !magnitude_text
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return Err(HighLevelPricingMaturityResolutionError::InvalidTenor {
            index,
            value: raw_value.to_string(),
        });
    }

    let magnitude = magnitude_text.parse::<u32>().map_err(|_| {
        HighLevelPricingMaturityResolutionError::InvalidTenor {
            index,
            value: raw_value.to_string(),
        }
    })?;
    if magnitude == 0 {
        return Err(HighLevelPricingMaturityResolutionError::InvalidTenor {
            index,
            value: raw_value.to_string(),
        });
    }

    match unit.to_ascii_lowercase() {
        'h' => add_fixed_hour_tenor(index, raw_value, valuation_time_utc, magnitude, 1),
        'd' => add_fixed_hour_tenor(index, raw_value, valuation_time_utc, magnitude, 24),
        'w' => add_fixed_hour_tenor(index, raw_value, valuation_time_utc, magnitude, 24 * 7),
        'm' => add_calendar_month_tenor(index, raw_value, valuation_time_utc, magnitude),
        'y' => add_calendar_year_tenor(index, raw_value, valuation_time_utc, magnitude),
        _ => Err(HighLevelPricingMaturityResolutionError::InvalidTenor {
            index,
            value: raw_value.to_string(),
        }),
    }
}

fn add_fixed_hour_tenor(
    index: usize,
    raw_value: &str,
    valuation_time_utc: DateTime<Utc>,
    magnitude: u32,
    hour_multiplier: i64,
) -> Result<DateTime<Utc>, HighLevelPricingMaturityResolutionError> {
    let total_hours = i64::from(magnitude)
        .checked_mul(hour_multiplier)
        .ok_or_else(|| HighLevelPricingMaturityResolutionError::TenorOverflow {
            index,
            value: raw_value.to_string(),
        })?;

    let duration = Duration::hours(total_hours);

    valuation_time_utc
        .checked_add_signed(duration)
        .ok_or_else(|| HighLevelPricingMaturityResolutionError::TenorOverflow {
            index,
            value: raw_value.to_string(),
        })
}

fn add_calendar_month_tenor(
    index: usize,
    raw_value: &str,
    valuation_time_utc: DateTime<Utc>,
    month_count: u32,
) -> Result<DateTime<Utc>, HighLevelPricingMaturityResolutionError> {
    let target_date = valuation_time_utc
        .date_naive()
        .checked_add_months(Months::new(month_count))
        .ok_or_else(|| HighLevelPricingMaturityResolutionError::TenorOverflow {
            index,
            value: raw_value.to_string(),
        })?;

    Ok(DateTime::<Utc>::from_naive_utc_and_offset(
        target_date.and_time(valuation_time_utc.time()),
        Utc,
    ))
}

fn add_calendar_year_tenor(
    index: usize,
    raw_value: &str,
    valuation_time_utc: DateTime<Utc>,
    year_count: u32,
) -> Result<DateTime<Utc>, HighLevelPricingMaturityResolutionError> {
    let month_count = year_count.checked_mul(12).ok_or_else(|| {
        HighLevelPricingMaturityResolutionError::TenorOverflow {
            index,
            value: raw_value.to_string(),
        }
    })?;

    add_calendar_month_tenor(index, raw_value, valuation_time_utc, month_count)
}

#[cfg(test)]
mod tests {
    use super::{
        HighLevelPricingMaturityResolutionError, HighLevelPricingMaturityResolutionRequest,
        resolve_high_level_pricing_maturity_inputs,
    };
    use crate::PricingIntervalSchedulePolicy;

    #[test]
    fn resolves_expiry_timestamps_to_maturity_intervals() {
        let resolved =
            resolve_high_level_pricing_maturity_inputs(HighLevelPricingMaturityResolutionRequest {
                valuation_time_utc: "2026-02-02T15:00:00Z".to_string(),
                expiry_timestamps_utc: Some(vec![Some("2026-02-02T19:00:00Z".to_string())]),
                expiry_dates: None,
                tenors: None,
                pricing_interval_schedule_policy: PricingIntervalSchedulePolicy::AutoExactSparse,
            })
            .expect("timestamp maturity resolution should succeed");

        assert_eq!(resolved.maturity_intervals, vec![4.0]);
        assert_eq!(
            resolved.resolved_expiry_timestamps_utc,
            vec!["2026-02-02T19:00:00+00:00".to_string()]
        );
    }

    #[test]
    fn resolves_expiry_dates_by_preserving_the_valuation_clock_time() {
        let resolved =
            resolve_high_level_pricing_maturity_inputs(HighLevelPricingMaturityResolutionRequest {
                valuation_time_utc: "2026-02-02T15:00:00Z".to_string(),
                expiry_timestamps_utc: None,
                expiry_dates: Some(vec![Some("2026-02-04".to_string())]),
                tenors: None,
                pricing_interval_schedule_policy: PricingIntervalSchedulePolicy::AutoExactSparse,
            })
            .expect("date maturity resolution should succeed");

        assert_eq!(resolved.maturity_intervals, vec![48.0]);
        assert_eq!(
            resolved.resolved_expiry_timestamps_utc,
            vec!["2026-02-04T15:00:00+00:00".to_string()]
        );
    }

    #[test]
    fn resolves_interval_and_calendar_tenors() {
        let resolved =
            resolve_high_level_pricing_maturity_inputs(HighLevelPricingMaturityResolutionRequest {
                valuation_time_utc: "2026-01-31T15:00:00Z".to_string(),
                expiry_timestamps_utc: None,
                expiry_dates: None,
                tenors: Some(vec![Some("2d".to_string()), Some("1m".to_string())]),
                pricing_interval_schedule_policy: PricingIntervalSchedulePolicy::AutoExactSparse,
            })
            .expect("tenor maturity resolution should succeed");

        assert_eq!(resolved.maturity_intervals, vec![48.0, 672.0]);
        assert_eq!(
            resolved.resolved_expiry_timestamps_utc,
            vec![
                "2026-02-02T15:00:00+00:00".to_string(),
                "2026-02-28T15:00:00+00:00".to_string(),
            ]
        );
    }

    #[test]
    fn rejects_rows_with_multiple_maturity_inputs() {
        let error =
            resolve_high_level_pricing_maturity_inputs(HighLevelPricingMaturityResolutionRequest {
                valuation_time_utc: "2026-02-02T15:00:00Z".to_string(),
                expiry_timestamps_utc: Some(vec![Some("2026-02-02T19:00:00Z".to_string())]),
                expiry_dates: Some(vec![Some("2026-02-02".to_string())]),
                tenors: None,
                pricing_interval_schedule_policy: PricingIntervalSchedulePolicy::AutoExactSparse,
            })
            .expect_err("multiple maturity inputs must fail");

        assert_eq!(
            error,
            HighLevelPricingMaturityResolutionError::InvalidNodeSelection { index: 0 }
        );
    }

    #[test]
    fn accepts_non_hour_aligned_expiry_timestamps_via_sparse_schedule() {
        let resolved =
            resolve_high_level_pricing_maturity_inputs(HighLevelPricingMaturityResolutionRequest {
                valuation_time_utc: "2026-02-02T15:00:00Z".to_string(),
                expiry_timestamps_utc: Some(vec![Some("2026-02-02T16:30:00Z".to_string())]),
                expiry_dates: None,
                tenors: None,
                pricing_interval_schedule_policy: PricingIntervalSchedulePolicy::AutoExactSparse,
            })
            .expect("non-hour-aligned timestamps should resolve");

        assert_eq!(resolved.maturity_intervals, vec![1.5]);
        assert_eq!(resolved.maturity_terminal_interval_indices, vec![1]);
    }
}
