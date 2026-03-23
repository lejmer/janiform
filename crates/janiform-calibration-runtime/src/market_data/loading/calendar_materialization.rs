use std::collections::BTreeMap;

use crate::calendar::ExchangeCalendarDefinition;
use crate::calendar::ExchangeCalendarRegistry;
use crate::market_data::preprocessing::MarketDataTranslationTickRecord;

use super::contracts::{
    MarketDataLoadingError, MarketDataSessionRecurringClosedWindowPayload,
    MarketDataSessionRegularWindowPayload,
};
use super::shared::{
    CalendarRuleMetadataArtifact, DateTime, Duration, ExchangeCalendarArtifact,
    ExchangeCalendarRegistryArtifact, GAP_SCAN_STEP_MINUTES, LONG_GAP_THRESHOLD_HOURS, MarketState,
    RegularSessionArtifact, SessionTemplate, SessionWindowArtifact, TradingWeekday, Utc,
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
    if session_template.continuous_weekday_template {
        return 120.0;
    }

    let open_minutes = session_template
        .regular_session_windows
        .iter()
        .map(|window| {
            clock_minutes(window.close_time.as_str()) - clock_minutes(window.open_time.as_str())
        })
        .sum::<i32>();

    f64::from(open_minutes) / 60.0
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
    let retained_open_intervals = subtract_closed_intervals_from_open_intervals(
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
    );

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
        let previous = merged.last_mut().expect("merged is initialized");
        if start_minute <= previous.1 {
            previous.1 = previous.1.max(end_minute);
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
) -> Result<(usize, usize, usize), MarketDataLoadingError> {
    let mut long_gap_count = 0;
    let mut aligned_gap_count = 0;
    let mut unexpected_gap_count = 0;

    for pair in tick_records.windows(2) {
        let gap_nanos = pair[1].timestamp_utc_nanos - pair[0].timestamp_utc_nanos;
        if gap_nanos
            < Duration::hours(LONG_GAP_THRESHOLD_HOURS)
                .num_nanoseconds()
                .unwrap_or(0)
        {
            continue;
        }

        long_gap_count += 1;

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

    Ok((long_gap_count, aligned_gap_count, unexpected_gap_count))
}

pub(super) fn gap_contains_closed_period(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    gap_start_utc_nanos: i64,
    gap_end_utc_nanos: i64,
) -> Result<bool, MarketDataLoadingError> {
    let mut probe_timestamp = DateTime::<Utc>::from_timestamp_nanos(gap_start_utc_nanos)
        + Duration::minutes(GAP_SCAN_STEP_MINUTES);

    let gap_end = DateTime::<Utc>::from_timestamp_nanos(gap_end_utc_nanos);
    while probe_timestamp < gap_end {
        let market_state = calendar_registry
            .resolve_market_state_at_utc(exchange_code, probe_timestamp)
            .map_err(|error| MarketDataLoadingError::InvalidValue {
                field_path: "session_configuration".to_string(),
                message: format!("failed to resolve inferred market state: {error}"),
            })?;
        if market_state == MarketState::Closed {
            return Ok(true);
        }

        probe_timestamp += Duration::minutes(GAP_SCAN_STEP_MINUTES);
    }

    Ok(false)
}

pub(super) fn confidence_score_for_inference(
    template: &SessionTemplate,
    gap_alignment_diagnostics: (usize, usize, usize),
) -> f64 {
    let (long_gap_count, aligned_gap_count, unexpected_gap_count) = gap_alignment_diagnostics;

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
    if long_gap_count > 0 && unexpected_gap_count == 0 {
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
    let (long_gap_count, aligned_gap_count, _) = gap_alignment_diagnostics;
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
            "validated the inferred session against {aligned_gap_count} observed long gaps that overlapped inferred closed windows"
        ));
    } else if long_gap_count == 0 {
        reasons.push(
            "no long gaps were present in the observed window, so exchange metadata carried the inference"
                .to_string(),
        );
    }

    reasons
}

pub(super) fn automatic_resolution_warnings(
    template: &SessionTemplate,
    gap_alignment_diagnostics: (usize, usize, usize),
) -> Vec<String> {
    let (long_gap_count, _, unexpected_gap_count) = gap_alignment_diagnostics;
    let mut warnings = Vec::new();

    if long_gap_count == 0 && template.timezone_source == "exchange_code_builtin_default" {
        warnings.push(
            "observed data did not include long inactivity gaps, so the session was inferred from exchange defaults".to_string(),
        );
    }
    if unexpected_gap_count > 0 {
        warnings.push(format!(
            "{unexpected_gap_count} observed long gaps did not clearly overlap inferred closed windows. If downstream diagnostics look inconsistent, provide an explicit session-definition override."
        ));
    }

    warnings
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
