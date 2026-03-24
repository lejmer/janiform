from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Any, Optional, cast

from .contracts import (
    MarketDataSessionRecurringClosedWindow,
    MarketDataSessionRegularWindow,
    _session_resolution_error,
)
from ...contracts import serialize_deterministic_json


_TRADING_WEEKDAYS: tuple[str, ...] = (
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
)
_WEEKDAY_NAMES: tuple[str, ...] = (
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
)
_WEEKDAY_INDEX_BY_NAME: dict[str, int] = {
    weekday_name: weekday_index
    for weekday_index, weekday_name in enumerate(_WEEKDAY_NAMES)
}


@dataclass(frozen=True, slots=True)
class _SessionTemplate:
    policy_id: str
    exchange_name: str
    timezone_name: str
    timezone_source: str
    holiday_calendar_code: str
    early_close_rule_set: Optional[str]
    regular_session_windows: tuple[MarketDataSessionRegularWindow, ...]
    continuous_weekday_template: bool = False


def _build_calendar_registry_json_from_windows(
    *,
    exchange_code: str,
    exchange_name: str,
    timezone_name: str,
    holiday_calendar_code: str,
    early_close_rule_set: Optional[str],
    regular_session_windows: tuple[MarketDataSessionRegularWindow, ...],
    recurring_closed_windows: tuple[MarketDataSessionRecurringClosedWindow, ...],
) -> str:
    calendar_registry_payload: dict[str, Any] = {
        "schema_version": "calendar.registry.v1",
        "artifact_date": datetime.now(tz=UTC).date().isoformat(),
        "exchanges": [
            {
                "schema_version": "calendar.v1",
                "exchange_code": exchange_code,
                "exchange_name": exchange_name,
                "timezone_name": timezone_name,
                "rule_metadata": {
                    "holiday_calendar_code": holiday_calendar_code,
                    "early_close_rule_set": early_close_rule_set,
                },
                "regular_sessions": _materialize_calendar_regular_sessions(
                    regular_session_windows=regular_session_windows,
                    recurring_closed_windows=recurring_closed_windows,
                ),
                "holiday_dates": [],
                "early_close_sessions": [],
            }
        ],
    }

    return serialize_deterministic_json(calendar_registry_payload)


def _materialize_calendar_regular_sessions(
    *,
    regular_session_windows: tuple[MarketDataSessionRegularWindow, ...],
    recurring_closed_windows: tuple[MarketDataSessionRecurringClosedWindow, ...],
) -> list[dict[str, Any]]:
    retained_open_intervals = _subtract_closed_intervals_from_open_intervals(
        open_intervals=tuple(
            (
                _weekday_start_minute(window.weekday) + _clock_minutes(window.open_time),
                _weekday_start_minute(window.weekday) + _clock_minutes(window.close_time),
                window.label,
            )
            for window in regular_session_windows
        ),
        closed_intervals=_merge_weekly_intervals(
            tuple(
                interval
                for closed_window in recurring_closed_windows
                for interval in _weekly_intervals_for_recurring_closed_window(closed_window)
            )
        ),
    )

    windows_by_weekday: dict[str, list[dict[str, str]]] = {}
    for open_start, open_end, label in retained_open_intervals:
        current_minute = open_start
        while current_minute < open_end:
            day_start_minute = (current_minute // (24 * 60)) * (24 * 60)
            day_end_minute = day_start_minute + (24 * 60)
            segment_end_minute = min(open_end, day_end_minute)
            weekday_name = _weekday_name_for_minute(current_minute)

            windows_by_weekday.setdefault(weekday_name, []).append(
                {
                    "label": label,
                    "open_time": _clock_time_for_minute_of_day(current_minute - day_start_minute),
                    "close_time": _clock_time_for_minute_of_day(
                        segment_end_minute - day_start_minute
                    ),
                }
            )
            current_minute = segment_end_minute

    return [
        {
            "weekday": weekday,
            "windows": sorted(
                windows_by_weekday[weekday],
                key=lambda window: (
                    cast(str, window["open_time"]),
                    cast(str, window["close_time"]),
                    cast(str, window["label"]),
                ),
            ),
        }
        for weekday in _WEEKDAY_NAMES
        if weekday in windows_by_weekday
    ]


def _weekly_intervals_for_recurring_closed_window(
    closed_window: MarketDataSessionRecurringClosedWindow,
) -> tuple[tuple[int, int], ...]:
    week_minutes = 7 * 24 * 60
    start_minute = _weekday_start_minute(closed_window.start_weekday) + _clock_minutes(
        closed_window.start_time
    )
    end_minute = _weekday_start_minute(closed_window.end_weekday) + _clock_minutes(
        closed_window.end_time
    )

    if start_minute < end_minute:
        return ((start_minute, end_minute),)
    if start_minute > end_minute:
        return (start_minute, week_minutes), (0, end_minute)

    return ((0, week_minutes),)


def _merge_weekly_intervals(
    intervals: tuple[tuple[int, int], ...],
) -> tuple[tuple[int, int], ...]:
    if len(intervals) == 0:
        return ()

    sorted_intervals = sorted(intervals)
    merged_intervals: list[tuple[int, int]] = [sorted_intervals[0]]

    for start_minute, end_minute in sorted_intervals[1:]:
        previous_start, previous_end = merged_intervals[-1]
        if start_minute <= previous_end:
            merged_intervals[-1] = (previous_start, max(previous_end, end_minute))
            continue

        merged_intervals.append((start_minute, end_minute))

    return tuple(merged_intervals)


def _subtract_closed_intervals_from_open_intervals(
    *,
    open_intervals: tuple[tuple[int, int, str], ...],
    closed_intervals: tuple[tuple[int, int], ...],
) -> tuple[tuple[int, int, str], ...]:
    retained_intervals: list[tuple[int, int, str]] = []

    for open_start, open_end, label in open_intervals:
        current_segments: list[tuple[int, int]] = [(open_start, open_end)]
        for closed_start, closed_end in closed_intervals:
            next_segments: list[tuple[int, int]] = []
            for segment_start, segment_end in current_segments:
                if closed_end <= segment_start or closed_start >= segment_end:
                    next_segments.append((segment_start, segment_end))
                    continue

                if closed_start > segment_start:
                    next_segments.append((segment_start, closed_start))
                if closed_end < segment_end:
                    next_segments.append((closed_end, segment_end))

            current_segments = next_segments
            if len(current_segments) == 0:
                break

        retained_intervals.extend(
            (segment_start, segment_end, label)
            for segment_start, segment_end in current_segments
            if segment_start < segment_end
        )

    return tuple(retained_intervals)


def _regular_session_windows_from_exchange_artifact(
    exchange_artifact: dict[str, Any],
) -> tuple[MarketDataSessionRegularWindow, ...]:
    raw_regular_sessions = exchange_artifact.get("regular_sessions")
    if not isinstance(raw_regular_sessions, list):
        raise _session_resolution_error(
            "session_configuration",
            "resolved exchange artifact must contain `regular_sessions`",
        )

    regular_session_windows: list[MarketDataSessionRegularWindow] = []
    for regular_session in raw_regular_sessions:
        if not isinstance(regular_session, dict):
            raise _session_resolution_error(
                "session_configuration",
                "resolved exchange artifact regular_sessions entries must be objects",
            )

        weekday = regular_session.get("weekday")
        raw_windows = regular_session.get("windows")
        if not isinstance(weekday, str) or not isinstance(raw_windows, list):
            raise _session_resolution_error(
                "session_configuration",
                "resolved exchange artifact regular_sessions entries must contain weekday and windows",
            )

        for raw_window in raw_windows:
            if not isinstance(raw_window, dict):
                raise _session_resolution_error(
                    "session_configuration",
                    "resolved exchange artifact windows must be objects",
                )

            regular_session_windows.append(
                MarketDataSessionRegularWindow(
                    weekday=weekday,
                    label=str(raw_window.get("label", "")),
                    open_time=str(raw_window.get("open_time", "")),
                    close_time=str(raw_window.get("close_time", "")),
                )
            )

    return tuple(regular_session_windows)


def _build_recurring_closed_windows(
    session_template: _SessionTemplate,
) -> tuple[MarketDataSessionRecurringClosedWindow, ...]:
    if session_template.continuous_weekday_template:
        return (
            MarketDataSessionRecurringClosedWindow(
                start_weekday="Saturday",
                start_time="00:00",
                end_weekday="Monday",
                end_time="00:00",
                reason="weekend inactivity window inferred for weekday-continuous 24/5 trading",
            ),
        )

    open_intervals: list[tuple[int, int]] = sorted(
        (
            _weekday_start_minute(window.weekday) + _clock_minutes(window.open_time),
            _weekday_start_minute(window.weekday) + _clock_minutes(window.close_time),
        )
        for window in session_template.regular_session_windows
    )
    week_minutes = 7 * 24 * 60
    closed_intervals: list[tuple[int, int]] = []
    cursor = 0

    for open_start, open_end in open_intervals:
        if cursor < open_start:
            closed_intervals.append((cursor, open_start))
        cursor = max(cursor, open_end)

    if cursor < week_minutes:
        closed_intervals.append((cursor, week_minutes))

    return tuple(
        MarketDataSessionRecurringClosedWindow(
            start_weekday=_weekday_name_for_minute(start_minute),
            start_time=_clock_time_for_minute(start_minute),
            end_weekday=_weekday_name_for_minute(end_minute % week_minutes),
            end_time=_clock_time_for_minute(end_minute % week_minutes),
            reason="recurring closure derived from the resolved weekly session pattern",
        )
        for start_minute, end_minute in closed_intervals
    )


def _looks_like_continuous_weekday_template(
    *,
    exchange_code: str,
    regular_session_windows: tuple[MarketDataSessionRegularWindow, ...],
) -> bool:
    if not (exchange_code.startswith("FX_") or exchange_code.startswith("CFD_")):
        return False

    if len(regular_session_windows) != len(_TRADING_WEEKDAYS):
        return False

    return all(
        window.open_time == "00:00" and window.close_time == "24:00"
        for window in regular_session_windows
    )


def _weekday_start_minute(weekday_name: str) -> int:
    return _WEEKDAY_INDEX_BY_NAME[weekday_name] * 24 * 60


def _clock_minutes(clock_text: str) -> int:
    raw_hour_text, raw_minute_text = clock_text.split(":")
    return int(raw_hour_text) * 60 + int(raw_minute_text)


def _weekday_name_for_minute(week_minute: int) -> str:
    return _WEEKDAY_NAMES[(week_minute // (24 * 60)) % 7]


def _clock_time_for_minute(week_minute: int) -> str:
    minute_of_day = week_minute % (24 * 60)
    hour = minute_of_day // 60
    minute = minute_of_day % 60

    return f"{hour:02d}:{minute:02d}"


def _clock_time_for_minute_of_day(minute_of_day: int) -> str:
    if minute_of_day == 24 * 60:
        return "24:00"

    hour = minute_of_day // 60
    minute = minute_of_day % 60

    return f"{hour:02d}:{minute:02d}"


__all__: list[str] = []
