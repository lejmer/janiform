use std::collections::BTreeMap;
use std::str::FromStr;

use chrono::{Datelike, LocalResult, NaiveDate, TimeZone};
use chrono_tz::Tz;

use crate::market_data::preprocessing::MarketDataTranslationTickRecord;
use janiform_market_time::calendar::{
    CalendarDate, ExchangeCalendarDefinition, ExchangeCalendarRegistry,
    ExchangeCalendarRegistryLookupError, SessionTime, SessionWindowDefinition, TradingWeekday,
};

use super::contracts::{
    MarketDataLoadingError, MarketDataSessionRecurringClosedWindowPayload,
    MarketDataSessionRegularWindowPayload,
};
use super::session_gap_spectrum::ObservedGapSpectrum;
use super::shared::{
    CalendarRuleMetadataArtifact, DateTime, ExchangeCalendarArtifact,
    ExchangeCalendarRegistryArtifact, RegularSessionArtifact, SessionTemplate,
    SessionWindowArtifact, Utc,
};

pub(super) fn trading_weekdays() -> [&'static str; 5] {
    ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday"]
}

pub(super) fn weekday_names() -> [&'static str; 7] {
    [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ]
}

pub(super) fn regular_session_windows_from_definition(
    definition: &ExchangeCalendarDefinition,
) -> Vec<MarketDataSessionRegularWindowPayload> {
    let mut result = Vec::new();
    for regular_session in definition.regular_sessions() {
        for window in regular_session.windows() {
            result.push(MarketDataSessionRegularWindowPayload {
                weekday: regular_session.weekday().to_string(),
                label: window.label().to_string(),
                open_time: window.open_time().to_string(),
                close_time: window.close_time().to_string(),
            });
        }
    }

    result
}

pub(super) fn looks_like_continuous_weekday_template(
    exchange_code: &str,
    regular_session_windows: &[MarketDataSessionRegularWindowPayload],
) -> bool {
    if !(exchange_code.starts_with("FX_") || exchange_code.starts_with("CFD_")) {
        return false;
    }
    if regular_session_windows.len() != trading_weekdays().len() {
        return false;
    }

    regular_session_windows
        .iter()
        .all(|window| window.open_time == "00:00" && window.close_time == "24:00")
}

pub(super) fn build_recurring_closed_windows(
    session_template: &SessionTemplate,
) -> Vec<MarketDataSessionRecurringClosedWindowPayload> {
    if !session_template.recurring_closed_windows.is_empty() {
        return session_template.recurring_closed_windows.clone();
    }

    if session_template.continuous_weekday_template {
        return vec![MarketDataSessionRecurringClosedWindowPayload {
            start_weekday: "Saturday".to_string(),
            start_time: "00:00".to_string(),
            end_weekday: "Monday".to_string(),
            end_time: "00:00".to_string(),
            reason: "weekend inactivity window inferred for weekday-continuous 24/5 trading"
                .to_string(),
        }];
    }

    let mut open_intervals = session_template
        .regular_session_windows
        .iter()
        .map(|window| {
            (
                weekday_start_minute(window.weekday.as_str())
                    + clock_minutes(window.open_time.as_str()),
                weekday_start_minute(window.weekday.as_str())
                    + clock_minutes(window.close_time.as_str()),
            )
        })
        .collect::<Vec<_>>();
    open_intervals.sort_unstable();

    let week_minutes = 7 * 24 * 60;
    let mut closed_intervals = Vec::new();
    let mut cursor = 0;

    for (open_start, open_end) in open_intervals {
        if cursor < open_start {
            closed_intervals.push((cursor, open_start));
        }
        cursor = cursor.max(open_end);
    }

    if cursor < week_minutes {
        closed_intervals.push((cursor, week_minutes));
    }

    closed_intervals
        .into_iter()
        .map(
            |(start_minute, end_minute)| MarketDataSessionRecurringClosedWindowPayload {
                start_weekday: weekday_name_for_minute(start_minute).to_string(),
                start_time: clock_time_for_minute(start_minute),
                end_weekday: weekday_name_for_minute(end_minute % week_minutes).to_string(),
                end_time: clock_time_for_minute(end_minute % week_minutes),
                reason: "recurring closure derived from the resolved weekly session pattern"
                    .to_string(),
            },
        )
        .collect()
}

pub(super) fn compute_open_session_duration_hours_per_week(
    session_template: &SessionTemplate,
) -> f64 {
    let recurring_closed_windows = build_recurring_closed_windows(session_template);

    let open_minutes = retained_open_session_intervals(
        session_template.regular_session_windows.as_slice(),
        recurring_closed_windows.as_slice(),
    )
    .iter()
    .map(|(open_start, open_end, _label)| open_end - open_start)
    .sum::<i32>();

    f64::from(open_minutes) / 60.0
}

fn retained_open_session_intervals(
    regular_session_windows: &[MarketDataSessionRegularWindowPayload],
    recurring_closed_windows: &[MarketDataSessionRecurringClosedWindowPayload],
) -> Vec<(i32, i32, String)> {
    subtract_closed_intervals_from_open_intervals(
        regular_session_windows
            .iter()
            .map(|window| {
                (
                    weekday_start_minute(window.weekday.as_str())
                        + clock_minutes(window.open_time.as_str()),
                    weekday_start_minute(window.weekday.as_str())
                        + clock_minutes(window.close_time.as_str()),
                    window.label.clone(),
                )
            })
            .collect(),
        merge_weekly_intervals(
            recurring_closed_windows
                .iter()
                .flat_map(weekly_intervals_for_recurring_closed_window)
                .collect(),
        ),
    )
}

pub(super) fn build_calendar_registry_json(
    exchange_code: &str,
    session_template: &SessionTemplate,
) -> Result<String, MarketDataLoadingError> {
    let exchange_artifact = ExchangeCalendarArtifact {
        schema_version: "calendar.v1".to_string(),
        exchange_code: exchange_code.to_string(),
        exchange_name: session_template.exchange_name.clone(),
        timezone_name: Some(session_template.timezone_name.clone()),
        rule_metadata: CalendarRuleMetadataArtifact {
            holiday_calendar_code: session_template.holiday_calendar_code.clone(),
            early_close_rule_set: session_template.early_close_rule_set.clone(),
        },
        regular_sessions: build_regular_session_artifacts(
            session_template.regular_session_windows.as_slice(),
            build_recurring_closed_windows(session_template).as_slice(),
        ),
        holiday_dates: Vec::new(),
        early_close_sessions: Vec::new(),
    };

    let artifact = ExchangeCalendarRegistryArtifact {
        schema_version: "calendar.registry.v1".to_string(),
        artifact_date: Utc::now().date_naive().to_string(),
        exchanges: vec![exchange_artifact],
    };

    serde_json::to_string(&artifact).map_err(|error| MarketDataLoadingError::InvalidValue {
        field_path: "session_configuration".to_string(),
        message: format!("failed to serialize resolved calendar registry: {error}"),
    })
}

pub(super) fn build_regular_session_artifacts(
    regular_session_windows: &[MarketDataSessionRegularWindowPayload],
    recurring_closed_windows: &[MarketDataSessionRecurringClosedWindowPayload],
) -> Vec<RegularSessionArtifact> {
    let retained_open_intervals =
        retained_open_session_intervals(regular_session_windows, recurring_closed_windows);

    let mut windows_by_weekday: BTreeMap<String, Vec<SessionWindowArtifact>> = BTreeMap::new();

    for (open_start, open_end, label) in retained_open_intervals {
        let mut current_minute = open_start;

        while current_minute < open_end {
            let day_start_minute = (current_minute / (24 * 60)) * (24 * 60);
            let day_end_minute = day_start_minute + (24 * 60);
            let segment_end_minute = open_end.min(day_end_minute);

            let weekday_name = weekday_name_for_minute(current_minute).to_string();

            windows_by_weekday
                .entry(weekday_name)
                .or_default()
                .push(SessionWindowArtifact {
                    label: label.clone(),
                    open_time: clock_time_for_minute_of_day(current_minute - day_start_minute),
                    close_time: clock_time_for_minute_of_day(segment_end_minute - day_start_minute),
                });

            current_minute = segment_end_minute;
        }
    }

    weekday_names()
        .iter()
        .filter_map(|weekday| {
            windows_by_weekday.remove(*weekday).map(|mut windows| {
                windows.sort_by(|left, right| {
                    (
                        left.open_time.as_str(),
                        left.close_time.as_str(),
                        left.label.as_str(),
                    )
                        .cmp(&(
                            right.open_time.as_str(),
                            right.close_time.as_str(),
                            right.label.as_str(),
                        ))
                });
                RegularSessionArtifact {
                    weekday: trading_weekday_from_name(weekday),
                    windows,
                }
            })
        })
        .collect()
}

/// Build a calendar-registry JSON document from an explicit session
/// configuration payload.
pub fn build_market_data_session_definition_json(
    exchange_code: &str,
    exchange_name: &str,
    timezone_name: &str,
    regular_session_windows: &[MarketDataSessionRegularWindowPayload],
    recurring_closed_windows: &[MarketDataSessionRecurringClosedWindowPayload],
) -> Result<String, MarketDataLoadingError> {
    let session_template = SessionTemplate {
        policy_id: "high_level.explicit_session_configuration.v1".to_string(),
        provenance_kind: "explicit_configuration".to_string(),
        exchange_name: exchange_name.to_string(),
        timezone_name: timezone_name.to_string(),
        timezone_source: "session_configuration".to_string(),
        holiday_calendar_code: "USER_DEFINED".to_string(),
        early_close_rule_set: None,
        regular_session_windows: regular_session_windows.to_vec(),
        recurring_closed_windows: recurring_closed_windows.to_vec(),
        continuous_weekday_template: looks_like_continuous_weekday_template(
            exchange_code,
            regular_session_windows,
        ),
    };

    build_calendar_registry_json(exchange_code, &session_template)
}

pub(super) fn weekly_intervals_for_recurring_closed_window(
    closed_window: &MarketDataSessionRecurringClosedWindowPayload,
) -> Vec<(i32, i32)> {
    let week_minutes = 7 * 24 * 60;

    let start_minute = weekday_start_minute(closed_window.start_weekday.as_str())
        + clock_minutes(closed_window.start_time.as_str());
    let end_minute = weekday_start_minute(closed_window.end_weekday.as_str())
        + clock_minutes(closed_window.end_time.as_str());

    if start_minute < end_minute {
        vec![(start_minute, end_minute)]
    } else if start_minute > end_minute {
        vec![(start_minute, week_minutes), (0, end_minute)]
    } else {
        vec![(0, week_minutes)]
    }
}

pub(super) fn merge_weekly_intervals(mut intervals: Vec<(i32, i32)>) -> Vec<(i32, i32)> {
    if intervals.is_empty() {
        return intervals;
    }

    intervals.sort_unstable();

    let mut merged = vec![intervals[0]];

    for (start_minute, end_minute) in intervals.into_iter().skip(1) {
        if let Some(previous) = merged.last_mut() {
            if start_minute <= previous.1 {
                previous.1 = previous.1.max(end_minute);
            } else {
                merged.push((start_minute, end_minute));
            }
        } else {
            merged.push((start_minute, end_minute));
        }
    }

    merged
}

pub(super) fn subtract_closed_intervals_from_open_intervals(
    open_intervals: Vec<(i32, i32, String)>,
    closed_intervals: Vec<(i32, i32)>,
) -> Vec<(i32, i32, String)> {
    let mut retained = Vec::new();

    for (open_start, open_end, label) in open_intervals {
        let mut current_segments = vec![(open_start, open_end)];

        for (closed_start, closed_end) in &closed_intervals {
            let mut next_segments = Vec::new();

            for (segment_start, segment_end) in &current_segments {
                if *closed_end <= *segment_start || *closed_start >= *segment_end {
                    next_segments.push((*segment_start, *segment_end));
                    continue;
                }

                if *closed_start > *segment_start {
                    next_segments.push((*segment_start, *closed_start));
                }

                if *closed_end < *segment_end {
                    next_segments.push((*closed_end, *segment_end));
                }
            }

            current_segments = next_segments;

            if current_segments.is_empty() {
                break;
            }
        }

        retained.extend(
            current_segments
                .into_iter()
                .filter(|(segment_start, segment_end)| segment_start < segment_end)
                .map(|(segment_start, segment_end)| (segment_start, segment_end, label.clone())),
        );
    }

    retained
}

pub(super) fn evaluate_gap_alignment(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    tick_records: &[MarketDataTranslationTickRecord],
    observed_gap_spectrum: &ObservedGapSpectrum,
) -> Result<(usize, usize, usize), MarketDataLoadingError> {
    let mut inactivity_gap_count = 0;
    let mut aligned_gap_count = 0;
    let mut unexpected_gap_count = 0;

    for pair in tick_records.windows(2) {
        let gap_nanos = pair[1].timestamp_utc_nanos - pair[0].timestamp_utc_nanos;
        if !observed_gap_spectrum.is_inactivity_gap(gap_nanos) {
            continue;
        }

        inactivity_gap_count += 1;

        if gap_contains_closed_period(
            calendar_registry,
            exchange_code,
            pair[0].timestamp_utc_nanos,
            pair[1].timestamp_utc_nanos,
        )? {
            aligned_gap_count += 1;
        } else {
            unexpected_gap_count += 1;
        }
    }

    Ok((
        inactivity_gap_count,
        aligned_gap_count,
        unexpected_gap_count,
    ))
}

pub(super) fn gap_contains_closed_period(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    gap_start_utc_nanos: i64,
    gap_end_utc_nanos: i64,
) -> Result<bool, MarketDataLoadingError> {
    if gap_end_utc_nanos <= gap_start_utc_nanos {
        return Ok(false);
    }

    let (definition, timezone) =
        required_definition_and_timezone(calendar_registry, exchange_code)?;

    let overlapping_open_intervals = overlapping_open_intervals(
        definition,
        timezone,
        exchange_code,
        gap_start_utc_nanos,
        gap_end_utc_nanos,
    )?;

    if overlapping_open_intervals.is_empty() {
        return Ok(true);
    }

    let mut covered_until_utc_nanos = gap_start_utc_nanos;

    for (interval_start_utc_nanos, interval_end_utc_nanos) in overlapping_open_intervals {
        if interval_start_utc_nanos > covered_until_utc_nanos {
            return Ok(true);
        }

        covered_until_utc_nanos = covered_until_utc_nanos.max(interval_end_utc_nanos);
        if covered_until_utc_nanos >= gap_end_utc_nanos {
            return Ok(false);
        }
    }

    Ok(covered_until_utc_nanos < gap_end_utc_nanos)
}

pub(super) fn confidence_score_for_inference(
    template: &SessionTemplate,
    gap_alignment_diagnostics: (usize, usize, usize),
) -> f64 {
    // This score is an operator-facing diagnostic for automatic session inference,
    // not a calibration or simulation control. The resolved session definition is
    // the runtime artifact that downstream code consumes.
    //
    // Keep this deliberately lightweight. The loading path already does the exact
    // work that matters for correctness: it materializes one calendar definition
    // and checks whether derived inactivity gaps overlap inferred closed periods.
    // A more elaborate confidence model would add hidden policy, retries, or extra
    // clustering passes without improving the persisted session interpretation.
    let (inactivity_gap_count, aligned_gap_count, unexpected_gap_count) = gap_alignment_diagnostics;

    let mut confidence_score: f64 = if template.timezone_source == "exchange_code_builtin_default" {
        0.65
    } else {
        0.55
    };

    if aligned_gap_count > 0 {
        confidence_score += 0.25;
    }

    if template.continuous_weekday_template {
        confidence_score += 0.10;
    }

    if inactivity_gap_count > 0 && unexpected_gap_count == 0 {
        confidence_score += 0.10;
    }

    if unexpected_gap_count > 0 {
        confidence_score -= 0.20;
    }

    confidence_score.clamp(0.0, 1.0)
}

pub(super) fn confidence_label_for_gap_alignment(
    template: &SessionTemplate,
    gap_alignment_diagnostics: (usize, usize, usize),
) -> String {
    let confidence_score = confidence_score_for_inference(template, gap_alignment_diagnostics);

    if confidence_score >= 0.9 {
        "high".to_string()
    } else if confidence_score >= 0.75 {
        "medium".to_string()
    } else {
        "low".to_string()
    }
}

pub(super) fn automatic_resolution_reasons(
    exchange_code: &str,
    template: &SessionTemplate,
    gap_alignment_diagnostics: (usize, usize, usize),
) -> Vec<String> {
    let (inactivity_gap_count, aligned_gap_count, _) = gap_alignment_diagnostics;

    let mut reasons = Vec::new();

    if template.timezone_source == "exchange_code_builtin_default" {
        reasons.push(format!(
            "used the built-in exchange default for `{exchange_code}` to resolve the trading schedule"
        ));
    } else {
        reasons.push(
            "selected the generic weekday-continuous 24/5 broker-market template based on exchange code or workflow defaults".to_string(),
        );
    }

    if aligned_gap_count > 0 {
        reasons.push(format!(
            "validated the inferred session against {aligned_gap_count} observed inactivity gaps from the derived gap spectrum that overlapped inferred closed windows"
        ));
    } else if inactivity_gap_count == 0 {
        reasons.push(
            "the observed gap spectrum did not isolate recurring inactivity gaps, so exchange metadata carried the inference"
                .to_string(),
        );
    }

    reasons
}

pub(super) fn automatic_resolution_warnings(
    template: &SessionTemplate,
    gap_alignment_diagnostics: (usize, usize, usize),
) -> Vec<String> {
    let (inactivity_gap_count, _, unexpected_gap_count) = gap_alignment_diagnostics;

    let mut warnings = Vec::new();

    if inactivity_gap_count == 0 && template.timezone_source == "exchange_code_builtin_default" {
        warnings.push(
            "the observed gap spectrum did not isolate recurring inactivity gaps, so the session was inferred from exchange defaults".to_string(),
        );
    }

    if unexpected_gap_count > 0 {
        warnings.push(format!(
            "{unexpected_gap_count} derived inactivity gaps did not clearly overlap inferred closed windows. If downstream diagnostics look inconsistent, provide an explicit session-definition override."
        ));
    }

    warnings
}

fn required_definition_and_timezone<'a>(
    calendar_registry: &'a ExchangeCalendarRegistry,
    exchange_code: &str,
) -> Result<(&'a ExchangeCalendarDefinition, Tz), MarketDataLoadingError> {
    let definition =
        calendar_registry
            .get_required(exchange_code)
            .map_err(|error| match error {
                ExchangeCalendarRegistryLookupError::MissingExchangeId { exchange_code } => {
                    MarketDataLoadingError::InvalidValue {
                        field_path: "session_configuration.exchange_code".to_string(),
                        message: format!(
                            "resolved calendar registry does not contain `{exchange_code}`"
                        ),
                    }
                }
            })?;

    let timezone_name = definition.timezone_name().to_string();
    let timezone =
        Tz::from_str(timezone_name.as_str()).map_err(|_| MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration".to_string(),
            message: format!(
                "resolved session definition contains an invalid timezone: {timezone_name}"
            ),
        })?;

    Ok((definition, timezone))
}

fn overlapping_open_intervals(
    definition: &ExchangeCalendarDefinition,
    timezone: Tz,
    exchange_code: &str,
    gap_start_utc_nanos: i64,
    gap_end_utc_nanos: i64,
) -> Result<Vec<(i64, i64)>, MarketDataLoadingError> {
    let gap_start_utc = DateTime::<Utc>::from_timestamp_nanos(gap_start_utc_nanos);
    let gap_end_utc = DateTime::<Utc>::from_timestamp_nanos(gap_end_utc_nanos);

    let mut current_local_date =
        CalendarDate::from_local_timestamp(&gap_start_utc.with_timezone(&timezone));
    let final_local_date =
        CalendarDate::from_local_timestamp(&gap_end_utc.with_timezone(&timezone));

    let mut open_intervals = Vec::new();

    loop {
        let local_weekday = TradingWeekday::from_chrono_weekday(
            calendar_date_as_naive_date(current_local_date, exchange_code)?.weekday(),
        );

        for session_window in
            definition.session_windows_for_local_date(current_local_date, local_weekday)
        {
            let (open_utc_nanos, close_utc_nanos) = session_window_utc_boundaries(
                timezone,
                exchange_code,
                current_local_date,
                session_window,
            )?;

            let interval_start_utc_nanos = open_utc_nanos.max(gap_start_utc_nanos);
            let interval_end_utc_nanos = close_utc_nanos.min(gap_end_utc_nanos);

            if interval_start_utc_nanos < interval_end_utc_nanos {
                open_intervals.push((interval_start_utc_nanos, interval_end_utc_nanos));
            }
        }

        if current_local_date == final_local_date {
            break;
        }

        current_local_date =
            next_calendar_date(current_local_date, exchange_code, gap_end_utc_nanos)?;
    }

    Ok(open_intervals)
}

fn session_window_utc_boundaries(
    timezone: Tz,
    exchange_code: &str,
    local_date: CalendarDate,
    session_window: &SessionWindowDefinition,
) -> Result<(i64, i64), MarketDataLoadingError> {
    let open_utc_nanos = local_datetime_for_session_time(
        timezone,
        exchange_code,
        local_date,
        session_window.open_time(),
    )?
    .with_timezone(&Utc)
    .timestamp_nanos_opt()
    .ok_or_else(|| MarketDataLoadingError::InvalidValue {
        field_path: "session_configuration".to_string(),
        message: format!(
            "resolved session boundary fell outside the supported nanosecond range for `{exchange_code}`"
        ),
    })?;

    let close_utc_nanos = local_datetime_for_session_time(
        timezone,
        exchange_code,
        local_date,
        session_window.close_time(),
    )?
    .with_timezone(&Utc)
    .timestamp_nanos_opt()
    .ok_or_else(|| MarketDataLoadingError::InvalidValue {
        field_path: "session_configuration".to_string(),
        message: format!(
            "resolved session boundary fell outside the supported nanosecond range for `{exchange_code}`"
        ),
    })?;

    Ok((open_utc_nanos, close_utc_nanos))
}

fn local_datetime_for_session_time(
    timezone: Tz,
    exchange_code: &str,
    local_date: CalendarDate,
    session_time: SessionTime,
) -> Result<DateTime<Tz>, MarketDataLoadingError> {
    let (resolved_year, resolved_month, resolved_day, resolved_hour, resolved_minute) =
        if session_time.hour() == 24 && session_time.minute() == 0 {
            let next_date = calendar_date_as_naive_date(local_date, exchange_code)?
                .succ_opt()
                .ok_or_else(|| MarketDataLoadingError::InvalidValue {
                    field_path: "session_configuration".to_string(),
                    message: format!(
                        "resolved session boundary overflowed the calendar for `{exchange_code}`"
                    ),
                })?;

            (
                next_date.year(),
                next_date.month(),
                next_date.day(),
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

    match timezone.with_ymd_and_hms(
        resolved_year,
        resolved_month,
        resolved_day,
        resolved_hour,
        resolved_minute,
        0,
    ) {
        LocalResult::Single(timestamp_local) => Ok(timestamp_local),
        LocalResult::Ambiguous(_, _) | LocalResult::None => {
            Err(MarketDataLoadingError::InvalidValue {
                field_path: "session_configuration".to_string(),
                message: format!(
                    "resolved session boundary is ambiguous in timezone `{}` for `{exchange_code}`",
                    timezone.name()
                ),
            })
        }
    }
}

fn calendar_date_as_naive_date(
    local_date: CalendarDate,
    exchange_code: &str,
) -> Result<NaiveDate, MarketDataLoadingError> {
    NaiveDate::from_ymd_opt(
        local_date.year,
        local_date.month as u32,
        local_date.day as u32,
    )
    .ok_or_else(|| MarketDataLoadingError::InvalidValue {
        field_path: "session_configuration".to_string(),
        message: format!(
            "resolved session definition contains an invalid calendar date for `{exchange_code}`"
        ),
    })
}

fn next_calendar_date(
    local_date: CalendarDate,
    exchange_code: &str,
    gap_end_utc_nanos: i64,
) -> Result<CalendarDate, MarketDataLoadingError> {
    let next_date = calendar_date_as_naive_date(local_date, exchange_code)?
        .succ_opt()
        .ok_or_else(|| MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration".to_string(),
            message: format!(
                "could not advance the resolved session calendar while checking gap alignment at {gap_end_utc_nanos}"
            ),
        })?;

    Ok(CalendarDate {
        year: next_date.year(),
        month: next_date.month() as u8,
        day: next_date.day() as u8,
    })
}

pub(super) fn weekday_start_minute(weekday_name: &str) -> i32 {
    match weekday_name {
        "Monday" => 0,
        "Tuesday" => 24 * 60,
        "Wednesday" => 2 * 24 * 60,
        "Thursday" => 3 * 24 * 60,
        "Friday" => 4 * 24 * 60,
        "Saturday" => 5 * 24 * 60,
        "Sunday" => 6 * 24 * 60,
        _ => 0,
    }
}

pub(super) fn clock_minutes(clock_text: &str) -> i32 {
    let mut parts = clock_text.split(':');

    let hour = parts.next().unwrap_or("0").parse::<i32>().unwrap_or(0);
    let minute = parts.next().unwrap_or("0").parse::<i32>().unwrap_or(0);

    (hour * 60) + minute
}

pub(super) fn weekday_name_for_minute(week_minute: i32) -> &'static str {
    weekday_names()[(week_minute / (24 * 60)).rem_euclid(7) as usize]
}

pub(super) fn clock_time_for_minute(week_minute: i32) -> String {
    let minute_of_day = week_minute.rem_euclid(24 * 60);
    let hour = minute_of_day / 60;
    let minute = minute_of_day % 60;

    format!("{hour:02}:{minute:02}")
}

pub(super) fn clock_time_for_minute_of_day(minute_of_day: i32) -> String {
    if minute_of_day == 24 * 60 {
        return "24:00".to_string();
    }

    let hour = minute_of_day / 60;
    let minute = minute_of_day % 60;

    format!("{hour:02}:{minute:02}")
}

pub(super) fn trading_weekday_from_name(name: &str) -> TradingWeekday {
    match name {
        "Monday" => TradingWeekday::Monday,
        "Tuesday" => TradingWeekday::Tuesday,
        "Wednesday" => TradingWeekday::Wednesday,
        "Thursday" => TradingWeekday::Thursday,
        "Friday" => TradingWeekday::Friday,
        "Saturday" => TradingWeekday::Saturday,
        "Sunday" => TradingWeekday::Sunday,
        _ => TradingWeekday::Monday,
    }
}
