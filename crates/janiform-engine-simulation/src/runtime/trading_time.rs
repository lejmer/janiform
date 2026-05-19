use chrono::{DateTime, Datelike, LocalResult, NaiveDate, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use janiform_foundations::randomness::SimulationRandomnessStream;
use janiform_foundations::time::{elapsed_utc_nanos_to_hours, utc_datetime_from_nanos};
use janiform_market_time::seasonality::HOURS_PER_WEEK;
use janiform_model_kernels::hawkes::HawkesExcitationState;

use crate::error::SeededTickSimulationRunError;
use janiform_market_time::calendar::{CalendarDate, SessionTime, TradingWeekday};

use super::market_state::SeededSimulationMarketStateResolver;
use super::results::validate_integrated_intensity;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TradingTimeReturnComponents {
    pub(crate) buy_side_return: f64,
    pub(crate) sell_side_return: f64,
    pub(crate) trading_time_return: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct OpenMarketElapsedTimeWeights {
    open_elapsed_hours: f64,
    open_decay_weight: f64,
}

pub(crate) type IntervalOpenMarketSegments = Vec<(i64, i64)>;

pub(crate) struct TransitionTradingTimeReturnInput<'a, R> {
    pub(crate) driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    pub(crate) state: HawkesExcitationState,
    pub(crate) lambda_bar: f64,
    pub(crate) beta: f64,
    pub(crate) seasonality_factor: f64,
    pub(crate) return_mark_scale_factor: f64,
    pub(crate) interval_open_market_segments: &'a [(i64, i64)],
    pub(crate) previous_timestamp_utc_nanos: i64,
    pub(crate) timestamp_utc_nanos: i64,
    pub(crate) interval_index: usize,
    pub(crate) randomness_stream: R,
}

pub(crate) fn sample_transition_trading_time_return<R>(
    input: TransitionTradingTimeReturnInput<'_, R>,
) -> Result<TradingTimeReturnComponents, SeededTickSimulationRunError>
where
    R: SimulationRandomnessStream,
{
    let TransitionTradingTimeReturnInput {
        driver_parameters,
        state,
        lambda_bar,
        beta,
        seasonality_factor,
        return_mark_scale_factor,
        interval_open_market_segments,
        previous_timestamp_utc_nanos,
        timestamp_utc_nanos,
        interval_index,
        mut randomness_stream,
    } = input;

    let open_market_elapsed_time_weights = integrate_open_market_elapsed_time_weights(
        interval_open_market_segments,
        previous_timestamp_utc_nanos,
        timestamp_utc_nanos,
        beta,
    );
    let integrated_buy_intensity = integrate_side_intensity(
        state,
        lambda_bar,
        seasonality_factor,
        open_market_elapsed_time_weights,
        true,
    );
    let integrated_sell_intensity = integrate_side_intensity(
        state,
        lambda_bar,
        seasonality_factor,
        open_market_elapsed_time_weights,
        false,
    );

    validate_integrated_intensity("a_plus", integrated_buy_intensity, interval_index)?;
    validate_integrated_intensity("a_minus", integrated_sell_intensity, interval_index)?;

    let trading_time_contributions = driver_parameters
        .sample_trading_time_return_increment(
            integrated_buy_intensity,
            integrated_sell_intensity,
            &mut randomness_stream,
        )
        .map_err(
            |source| SeededTickSimulationRunError::TradingTimeSamplingFailure {
                interval_index,
                source: Box::new(source),
            },
        )?;

    Ok(TradingTimeReturnComponents {
        buy_side_return: return_mark_scale_factor * trading_time_contributions.buy_side_increment,
        sell_side_return: -return_mark_scale_factor
            * trading_time_contributions.sell_side_increment,
        trading_time_return: return_mark_scale_factor
            * trading_time_contributions.combined_increment,
    })
}

fn integrate_open_market_elapsed_time_weights(
    interval_open_market_segments: &[(i64, i64)],
    previous_timestamp_utc_nanos: i64,
    timestamp_utc_nanos: i64,
    beta: f64,
) -> OpenMarketElapsedTimeWeights {
    if timestamp_utc_nanos <= previous_timestamp_utc_nanos {
        return OpenMarketElapsedTimeWeights {
            open_elapsed_hours: 0.0,
            open_decay_weight: 0.0,
        };
    }

    let mut open_elapsed_hours = 0.0;
    let mut open_decay_weight = 0.0;

    for (open_timestamp_utc_nanos, close_timestamp_utc_nanos) in interval_open_market_segments {
        let overlap_start_utc_nanos = previous_timestamp_utc_nanos.max(*open_timestamp_utc_nanos);
        let overlap_end_utc_nanos = timestamp_utc_nanos.min(*close_timestamp_utc_nanos);
        if overlap_end_utc_nanos <= overlap_start_utc_nanos {
            continue;
        }

        let overlap_start_hours =
            elapsed_utc_nanos_to_hours(previous_timestamp_utc_nanos, overlap_start_utc_nanos);
        let overlap_end_hours =
            elapsed_utc_nanos_to_hours(previous_timestamp_utc_nanos, overlap_end_utc_nanos);

        let overlap_open_hours = overlap_end_hours - overlap_start_hours;

        open_elapsed_hours += overlap_open_hours;
        open_decay_weight += if beta > 0.0 {
            ((-beta * overlap_start_hours).exp() - (-beta * overlap_end_hours).exp()) / beta
        } else {
            overlap_open_hours
        };
    }

    OpenMarketElapsedTimeWeights {
        open_elapsed_hours,
        open_decay_weight,
    }
}

pub(crate) fn integrate_open_market_elapsed_hours(
    interval_open_market_segments: &[(i64, i64)],
    previous_timestamp_utc_nanos: i64,
    timestamp_utc_nanos: i64,
) -> f64 {
    integrate_open_market_elapsed_time_weights(
        interval_open_market_segments,
        previous_timestamp_utc_nanos,
        timestamp_utc_nanos,
        0.0,
    )
    .open_elapsed_hours
}

pub(crate) fn return_mark_scale_factor_for_timestamp(
    return_mark_hour_of_week_factors: &[f64],
    exchange_timezone: Tz,
    timestamp_utc_nanos: i64,
) -> Result<f64, SeededTickSimulationRunError> {
    if return_mark_hour_of_week_factors.len() != HOURS_PER_WEEK {
        return Err(
            SeededTickSimulationRunError::ReturnMarkHourOfWeekFactorCountMismatch {
                expected_factor_count: HOURS_PER_WEEK,
                observed_factor_count: return_mark_hour_of_week_factors.len(),
            },
        );
    }

    let timestamp_utc = utc_datetime_from_nanos(timestamp_utc_nanos).ok_or(
        SeededTickSimulationRunError::InvalidKnownScheduledNewsTimestampUtcNanos {
            value: timestamp_utc_nanos,
        },
    )?;

    let timestamp_local = timestamp_utc.with_timezone(&exchange_timezone);

    let weekday_offset = timestamp_local.weekday().num_days_from_monday() as usize * 24;

    let hour_of_week = weekday_offset + timestamp_local.hour() as usize;

    let return_mark_scale_factor = return_mark_hour_of_week_factors[hour_of_week];
    if !return_mark_scale_factor.is_finite() || return_mark_scale_factor <= 0.0 {
        return Err(
            SeededTickSimulationRunError::InvalidReturnMarkHourOfWeekFactor {
                factor_index: hour_of_week,
                value: return_mark_scale_factor,
            },
        );
    }

    Ok(return_mark_scale_factor)
}

pub(crate) fn build_interval_open_market_segments(
    market_state_resolver: &SeededSimulationMarketStateResolver<'_>,
    interval_start_utc_nanos: &[i64],
    interval_end_utc_nanos: &[i64],
) -> Result<Vec<IntervalOpenMarketSegments>, SeededTickSimulationRunError> {
    interval_start_utc_nanos
        .iter()
        .copied()
        .zip(interval_end_utc_nanos.iter().copied())
        .map(|(interval_start_utc_nanos, interval_end_utc_nanos)| {
            interval_open_market_segments_for_range(
                market_state_resolver,
                interval_start_utc_nanos,
                interval_end_utc_nanos,
            )
        })
        .collect()
}

fn integrate_side_intensity(
    state: HawkesExcitationState,
    lambda_bar: f64,
    seasonality_factor: f64,
    open_market_elapsed_time_weights: OpenMarketElapsedTimeWeights,
    plus_side: bool,
) -> f64 {
    if open_market_elapsed_time_weights.open_elapsed_hours <= 0.0 {
        return 0.0;
    }

    let baseline = lambda_bar
        * seasonality_factor.max(0.0)
        * open_market_elapsed_time_weights.open_elapsed_hours;

    let endogenous_component = if plus_side {
        state.target_plus_sum() * open_market_elapsed_time_weights.open_decay_weight
    } else {
        state.target_minus_sum() * open_market_elapsed_time_weights.open_decay_weight
    };

    (baseline + endogenous_component).max(0.0)
}

fn interval_open_market_segments_for_range(
    market_state_resolver: &SeededSimulationMarketStateResolver<'_>,
    interval_start_utc_nanos: i64,
    interval_end_utc_nanos: i64,
) -> Result<IntervalOpenMarketSegments, SeededTickSimulationRunError> {
    if interval_end_utc_nanos <= interval_start_utc_nanos {
        return Ok(Vec::new());
    }

    let start_timestamp_utc = utc_datetime_from_nanos(interval_start_utc_nanos).ok_or(
        SeededTickSimulationRunError::InvalidKnownScheduledNewsTimestampUtcNanos {
            value: interval_start_utc_nanos,
        },
    )?;

    let end_timestamp_utc = utc_datetime_from_nanos(interval_end_utc_nanos - 1).ok_or(
        SeededTickSimulationRunError::InvalidKnownScheduledNewsTimestampUtcNanos {
            value: interval_end_utc_nanos - 1,
        },
    )?;

    let start_local_date = CalendarDate::from_local_timestamp(
        &start_timestamp_utc.with_timezone(&market_state_resolver.timezone),
    );

    let end_local_date = CalendarDate::from_local_timestamp(
        &end_timestamp_utc.with_timezone(&market_state_resolver.timezone),
    );

    let mut current_date = NaiveDate::from_ymd_opt(
        start_local_date.year,
        start_local_date.month as u32,
        start_local_date.day as u32,
    )
    .ok_or(
        SeededTickSimulationRunError::CalendarSessionBoundaryResolutionFailed {
            exchange_code: market_state_resolver.exchange_code.to_string(),
            timezone_name: market_state_resolver.timezone.name().to_string(),
            local_date: start_local_date.to_string(),
            local_time: "00:00".to_string(),
        },
    )?;

    let last_date = NaiveDate::from_ymd_opt(
        end_local_date.year,
        end_local_date.month as u32,
        end_local_date.day as u32,
    )
    .ok_or(
        SeededTickSimulationRunError::CalendarSessionBoundaryResolutionFailed {
            exchange_code: market_state_resolver.exchange_code.to_string(),
            timezone_name: market_state_resolver.timezone.name().to_string(),
            local_date: end_local_date.to_string(),
            local_time: "00:00".to_string(),
        },
    )?;

    let mut open_market_segments = Vec::new();

    while current_date <= last_date {
        let local_date = CalendarDate {
            year: current_date.year(),
            month: current_date.month() as u8,
            day: current_date.day() as u8,
        };
        let local_weekday = TradingWeekday::from_chrono_weekday(current_date.weekday());

        for session_window in market_state_resolver
            .definition
            .session_windows_for_local_date(local_date, local_weekday)
        {
            let open_timestamp_utc_nanos = session_boundary_timestamp_utc_nanos(
                market_state_resolver,
                local_date,
                session_window.open_time(),
                false,
            )?;

            let close_timestamp_utc_nanos = session_boundary_timestamp_utc_nanos(
                market_state_resolver,
                local_date,
                session_window.close_time(),
                true,
            )?;

            let overlap_start_utc_nanos = interval_start_utc_nanos.max(open_timestamp_utc_nanos);
            let overlap_end_utc_nanos = interval_end_utc_nanos.min(close_timestamp_utc_nanos);
            if overlap_end_utc_nanos > overlap_start_utc_nanos {
                open_market_segments.push((overlap_start_utc_nanos, overlap_end_utc_nanos));
            }
        }

        current_date = current_date.succ_opt().ok_or(
            SeededTickSimulationRunError::CalendarSessionBoundaryResolutionFailed {
                exchange_code: market_state_resolver.exchange_code.to_string(),
                timezone_name: market_state_resolver.timezone.name().to_string(),
                local_date: local_date.to_string(),
                local_time: "24:00".to_string(),
            },
        )?;
    }

    Ok(open_market_segments)
}

fn session_boundary_timestamp_utc_nanos(
    market_state_resolver: &SeededSimulationMarketStateResolver<'_>,
    local_date: CalendarDate,
    session_time: SessionTime,
    prefer_latest_ambiguous_timestamp: bool,
) -> Result<i64, SeededTickSimulationRunError> {
    let session_boundary_local = local_datetime_for_session_boundary(
        market_state_resolver,
        local_date,
        session_time,
        prefer_latest_ambiguous_timestamp,
    )?;

    session_boundary_local
        .with_timezone(&Utc)
        .timestamp_nanos_opt()
        .ok_or(
            SeededTickSimulationRunError::CalendarSessionBoundaryResolutionFailed {
                exchange_code: market_state_resolver.exchange_code.to_string(),
                timezone_name: market_state_resolver.timezone.name().to_string(),
                local_date: local_date.to_string(),
                local_time: session_time.to_string(),
            },
        )
}

fn local_datetime_for_session_boundary(
    market_state_resolver: &SeededSimulationMarketStateResolver<'_>,
    local_date: CalendarDate,
    session_time: SessionTime,
    prefer_latest_ambiguous_timestamp: bool,
) -> Result<DateTime<Tz>, SeededTickSimulationRunError> {
    let (resolved_year, resolved_month, resolved_day, resolved_hour, resolved_minute) =
        if session_time.hour() == 24 && session_time.minute() == 0 {
            let next_day = NaiveDate::from_ymd_opt(
                local_date.year,
                local_date.month as u32,
                local_date.day as u32,
            )
            .and_then(|date| date.succ_opt())
            .ok_or(
                SeededTickSimulationRunError::CalendarSessionBoundaryResolutionFailed {
                    exchange_code: market_state_resolver.exchange_code.to_string(),
                    timezone_name: market_state_resolver.timezone.name().to_string(),
                    local_date: local_date.to_string(),
                    local_time: session_time.to_string(),
                },
            )?;

            (
                next_day.year(),
                next_day.month(),
                next_day.day(),
                0_u32,
                0_u32,
            )
        } else {
            (
                local_date.year,
                local_date.month as u32,
                local_date.day as u32,
                session_time.hour() as u32,
                session_time.minute() as u32,
            )
        };

    match market_state_resolver.timezone.with_ymd_and_hms(
        resolved_year,
        resolved_month,
        resolved_day,
        resolved_hour,
        resolved_minute,
        0,
    ) {
        LocalResult::Single(timestamp_local) => Ok(timestamp_local),
        LocalResult::Ambiguous(earliest_timestamp_local, latest_timestamp_local) => {
            if prefer_latest_ambiguous_timestamp {
                Ok(latest_timestamp_local)
            } else {
                Ok(earliest_timestamp_local)
            }
        }
        LocalResult::None => Err(
            SeededTickSimulationRunError::CalendarSessionBoundaryResolutionFailed {
                exchange_code: market_state_resolver.exchange_code.to_string(),
                timezone_name: market_state_resolver.timezone.name().to_string(),
                local_date: local_date.to_string(),
                local_time: session_time.to_string(),
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use janiform_market_time::calendar::{
        ExchangeCalendarArtifact, ExchangeCalendarRegistry, ExchangeCalendarRegistryArtifact,
        RegularSessionArtifact, SessionWindowArtifact, TradingWeekday,
    };

    use super::{
        SeededSimulationMarketStateResolver, build_interval_open_market_segments,
        integrate_open_market_elapsed_time_weights,
    };

    fn build_full_weekday_registry() -> ExchangeCalendarRegistry {
        let regular_sessions = [
            TradingWeekday::Monday,
            TradingWeekday::Tuesday,
            TradingWeekday::Wednesday,
            TradingWeekday::Thursday,
            TradingWeekday::Friday,
        ]
        .into_iter()
        .map(|weekday| RegularSessionArtifact {
            weekday,
            windows: vec![SessionWindowArtifact {
                label: "full_day".to_string(),
                open_time: "00:00".to_string(),
                close_time: "24:00".to_string(),
            }],
        })
        .collect();

        ExchangeCalendarRegistry::from_artifact(ExchangeCalendarRegistryArtifact {
            schema_version: "calendar.registry.v1".to_string(),
            artifact_date: "2025-01-01".to_string(),
            exchanges: vec![ExchangeCalendarArtifact {
                schema_version: "exchange.calendar.v1".to_string(),
                exchange_code: "TEST".to_string(),
                exchange_name: "Test Exchange".to_string(),
                timezone_name: Some("UTC".to_string()),
                rule_metadata: janiform_market_time::calendar::CalendarRuleMetadataArtifact {
                    holiday_calendar_code: "NONE".to_string(),
                    early_close_rule_set: None,
                },
                regular_sessions,
                holiday_dates: Vec::new(),
                early_close_sessions: Vec::new(),
            }],
        })
        .expect("test calendar registry should validate")
    }

    #[test]
    fn open_market_elapsed_time_weights_zero_closed_weekend_interval() {
        let calendar_registry = build_full_weekday_registry();

        let market_state_resolver =
            SeededSimulationMarketStateResolver::new(&calendar_registry, "TEST")
                .expect("market-state resolver should load");

        let start_timestamp_utc_nanos = Utc
            .with_ymd_and_hms(2025, 3, 22, 12, 0, 0)
            .single()
            .expect("start timestamp should exist")
            .timestamp_nanos_opt()
            .expect("start timestamp should fit in nanos");

        let end_timestamp_utc_nanos = Utc
            .with_ymd_and_hms(2025, 3, 22, 13, 0, 0)
            .single()
            .expect("end timestamp should exist")
            .timestamp_nanos_opt()
            .expect("end timestamp should fit in nanos");

        let interval_open_market_segments = build_interval_open_market_segments(
            &market_state_resolver,
            &[start_timestamp_utc_nanos],
            &[end_timestamp_utc_nanos],
        )
        .expect("interval open-market segments should build");

        let weights = integrate_open_market_elapsed_time_weights(
            interval_open_market_segments[0].as_slice(),
            start_timestamp_utc_nanos,
            end_timestamp_utc_nanos,
            1.0,
        );

        assert_eq!(weights.open_elapsed_hours, 0.0);
        assert_eq!(weights.open_decay_weight, 0.0);
    }
}
