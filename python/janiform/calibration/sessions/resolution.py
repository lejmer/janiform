from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from typing import Any, Optional, cast

from .contracts import (
    CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_SCHEMA_VERSION,
    MarketDataSessionRecurringClosedWindow,
    MarketDataSessionRegularWindow,
    MarketDataSessionResolution,
    _session_resolution_error,
)
from ..market_data.contracts import CalibrationInputFileReference
from ..market_data.ingestion import LoadedTickDataSource
from ..market_data.request import MarketDataCalibrationRequest
from ..._contracts import serialize_deterministic_json
from ...calendar import CalendarRegistry, CalendarRegistryLoadError, MarketState


_LONG_GAP_THRESHOLD = timedelta(hours=4)
_GAP_SCAN_STEP = timedelta(minutes=30)
_MINIMUM_GENERIC_24_5_WEEKDAY_COUNT = 4
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
class ResolvedMarketDataSession:
    calendar_registry: CalendarRegistry
    session_resolution: MarketDataSessionResolution
    session_definition_path: Optional[str]


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


def resolve_market_data_session_configuration(
    request: MarketDataCalibrationRequest,
    tick_data: LoadedTickDataSource,
) -> ResolvedMarketDataSession:
    session_configuration = request.session_configuration
    session_source = session_configuration.session_definition_source
    if session_source is not None:
        return _resolve_explicit_session_definition(
            exchange_code=session_configuration.exchange_code,
            session_source=session_source,
            configured_session_timezone=session_configuration.session_timezone,
        )

    inferred_template = _infer_session_template(
        exchange_code=session_configuration.exchange_code,
        configured_session_timezone=session_configuration.session_timezone,
        tick_data=tick_data,
    )
    registry_json = _build_calendar_registry_json(
        exchange_code=session_configuration.exchange_code,
        session_template=inferred_template,
    )
    calendar_registry = _load_calendar_registry_from_json(
        registry_json,
        field_path="session_configuration",
        path_context=None,
    )
    gap_alignment_diagnostics = _evaluate_gap_alignment(
        calendar_registry=calendar_registry,
        exchange_code=session_configuration.exchange_code,
        tick_data=tick_data,
    )

    return ResolvedMarketDataSession(
        calendar_registry=calendar_registry,
        session_definition_path=None,
        session_resolution=MarketDataSessionResolution(
            schema_version=CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_SCHEMA_VERSION,
            resolution_kind="automatic_inference",
            selected_policy_id=inferred_template.policy_id,
            manual_override_applied=False,
            exchange_code=session_configuration.exchange_code,
            exchange_name=inferred_template.exchange_name,
            resolved_timezone_name=inferred_template.timezone_name,
            timezone_source=inferred_template.timezone_source,
            open_session_duration_hours_per_week=_compute_open_session_duration_hours_per_week(
                inferred_template
            ),
            confidence_score=_confidence_score_for_inference(
                template=inferred_template,
                gap_alignment_diagnostics=gap_alignment_diagnostics,
            ),
            confidence_label=_confidence_label_for_gap_alignment(
                inferred_template,
                gap_alignment_diagnostics,
            ),
            regular_session_windows=inferred_template.regular_session_windows,
            recurring_closed_windows=_build_recurring_closed_windows(inferred_template),
            reasons=_automatic_resolution_reasons(
                exchange_code=session_configuration.exchange_code,
                template=inferred_template,
                gap_alignment_diagnostics=gap_alignment_diagnostics,
            ),
            ambiguity_warnings=_automatic_resolution_warnings(
                template=inferred_template,
                gap_alignment_diagnostics=gap_alignment_diagnostics,
            ),
            metadata_discrepancies=(),
            resolved_session_definition_json=registry_json,
        ),
    )


def _resolve_explicit_session_definition(
    *,
    exchange_code: str,
    session_source: CalibrationInputFileReference,
    configured_session_timezone: Optional[str],
) -> ResolvedMarketDataSession:
    if session_source.file_format.value != "json":
        raise _session_resolution_error(
            "session_configuration.session_definition_source.file_format",
            "must be json when a session-definition source is provided",
            context={"resolution_step": "explicit_session_definition_loading"},
        )

    try:
        with open(session_source.path, encoding="utf-8") as input_file:
            calendar_registry_json = input_file.read()
    except FileNotFoundError as e:
        raise _session_resolution_error(
            "session_configuration.session_definition_source.path",
            "does not exist",
            context={
                "path": session_source.path,
                "resolution_step": "explicit_session_definition_loading",
            },
        ) from e
    except OSError as e:
        raise _session_resolution_error(
            "session_configuration.session_definition_source.path",
            f"could not be read: {e}",
            context={
                "path": session_source.path,
                "resolution_step": "explicit_session_definition_loading",
            },
        ) from e

    calendar_registry = _load_calendar_registry_from_json(
        calendar_registry_json,
        field_path="session_configuration.session_definition_source.path",
        path_context=session_source.path,
    )
    exchange_artifact = _require_exchange_artifact(calendar_registry, exchange_code)

    resolved_timezone_name = cast(str, exchange_artifact["timezone_name"])
    if configured_session_timezone is not None and configured_session_timezone != resolved_timezone_name:
        metadata_discrepancies: tuple[str, ...] = (
            (
                "configured session_timezone does not match explicit session-definition timezone "
                f"for `{exchange_code}` and the session-definition timezone was used instead"
            ),
        )
    else:
        metadata_discrepancies = ()

    regular_session_windows = _regular_session_windows_from_exchange_artifact(exchange_artifact)
    session_template = _SessionTemplate(
        policy_id="session_definition_source.calendar_registry_json.v1",
        exchange_name=cast(str, exchange_artifact["exchange_name"]),
        timezone_name=resolved_timezone_name,
        timezone_source="session_definition_source",
        holiday_calendar_code=cast(
            str,
            cast(dict[str, Any], exchange_artifact["rule_metadata"])["holiday_calendar_code"],
        ),
        early_close_rule_set=cast(
            Optional[str],
            cast(dict[str, Any], exchange_artifact["rule_metadata"]).get("early_close_rule_set"),
        ),
        regular_session_windows=regular_session_windows,
        continuous_weekday_template=_looks_like_continuous_weekday_template(
            exchange_code=exchange_code,
            regular_session_windows=regular_session_windows,
        ),
    )

    return ResolvedMarketDataSession(
        calendar_registry=calendar_registry,
        session_definition_path=session_source.path,
        session_resolution=MarketDataSessionResolution(
            schema_version=CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_SCHEMA_VERSION,
            resolution_kind="explicit_override",
            selected_policy_id="session_definition_source.calendar_registry_json.v1",
            manual_override_applied=True,
            exchange_code=exchange_code,
            exchange_name=cast(str, exchange_artifact["exchange_name"]),
            resolved_timezone_name=resolved_timezone_name,
            timezone_source="session_definition_source",
            open_session_duration_hours_per_week=_compute_open_session_duration_hours_per_week(
                session_template
            ),
            confidence_score=1.0,
            confidence_label="explicit_override",
            regular_session_windows=regular_session_windows,
            recurring_closed_windows=_build_recurring_closed_windows(session_template),
            reasons=(
                (
                    "loaded session behavior from the explicit session-definition override and "
                    "reused it for calibration and simulation preparation"
                ),
            ),
            ambiguity_warnings=(),
            metadata_discrepancies=metadata_discrepancies,
            resolved_session_definition_json=calendar_registry_json,
        ),
    )


def _infer_session_template(
    *,
    exchange_code: str,
    configured_session_timezone: Optional[str],
    tick_data: LoadedTickDataSource,
) -> _SessionTemplate:
    if exchange_code in {"XNYS", "XNAS"}:
        default_timezone_name = "America/New_York"
        exchange_name = (
            "New York Stock Exchange"
            if exchange_code == "XNYS"
            else "Nasdaq Stock Market"
        )
        if configured_session_timezone is not None and configured_session_timezone != "America/New_York":
            raise _session_resolution_error(
                "session_configuration.session_timezone",
                (
                    f"conflicts with the built-in exchange default for {exchange_code}. "
                    "Provide an explicit session_definition_source to override the default."
                ),
                context={"exchange_code": exchange_code},
            )

        return _SessionTemplate(
            policy_id=f"exchange_code_builtin_default.{exchange_code.lower()}_regular_session.v1",
            exchange_name=exchange_name,
            timezone_name=default_timezone_name,
            timezone_source="exchange_code_builtin_default",
            holiday_calendar_code="US_EQUITIES",
            early_close_rule_set="US_EQUITIES_EARLY_CLOSE",
            regular_session_windows=tuple(
                MarketDataSessionRegularWindow(
                    weekday=weekday,
                    label="regular",
                    open_time="09:30",
                    close_time="16:00",
                )
                for weekday in _TRADING_WEEKDAYS
            ),
        )

    if exchange_code == "FXSPOT":
        if configured_session_timezone is not None and configured_session_timezone != "UTC":
            raise _session_resolution_error(
                "session_configuration.session_timezone",
                (
                    "conflicts with the built-in exchange default for FXSPOT. "
                    "Provide an explicit session_definition_source to override the default."
                ),
                context={"exchange_code": exchange_code},
            )

        return _SessionTemplate(
            policy_id="exchange_code_builtin_default.fxspot_weekend_window.v1",
            exchange_name="OTC FX Spot 24/5",
            timezone_name="UTC",
            timezone_source="exchange_code_builtin_default",
            holiday_calendar_code="FX_24_5",
            early_close_rule_set=None,
            regular_session_windows=(
                *tuple(
                    MarketDataSessionRegularWindow(
                        weekday=weekday,
                        label="spot",
                        open_time="00:00",
                        close_time="24:00",
                    )
                    for weekday in ("Monday", "Tuesday", "Wednesday", "Thursday")
                ),
                MarketDataSessionRegularWindow(
                    weekday="Friday",
                    label="spot",
                    open_time="00:00",
                    close_time="19:00",
                ),
                MarketDataSessionRegularWindow(
                    weekday="Sunday",
                    label="spot",
                    open_time="21:00",
                    close_time="24:00",
                ),
            ),
        )

    if exchange_code == "CRYPTO":
        if configured_session_timezone is not None and configured_session_timezone != "UTC":
            raise _session_resolution_error(
                "session_configuration.session_timezone",
                (
                    "conflicts with the built-in exchange default for CRYPTO. "
                    "Provide an explicit session_definition_source to override the default."
                ),
                context={"exchange_code": exchange_code},
            )

        return _SessionTemplate(
            policy_id="exchange_code_builtin_default.crypto_continuous_session.v1",
            exchange_name="Crypto Spot 24/7",
            timezone_name="UTC",
            timezone_source="exchange_code_builtin_default",
            holiday_calendar_code="CRYPTO_24_7",
            early_close_rule_set=None,
            regular_session_windows=tuple(
                MarketDataSessionRegularWindow(
                    weekday=weekday,
                    label="spot",
                    open_time="00:00",
                    close_time="24:00",
                )
                for weekday in _WEEKDAY_NAMES
            ),
        )

    if exchange_code.startswith("FX_") or exchange_code.startswith("CFD_"):
        return _build_generic_24_5_template(
            exchange_code=exchange_code,
            configured_session_timezone=configured_session_timezone,
        )

    if configured_session_timezone is None:
        raise _session_resolution_error(
            "session_configuration",
            (
                "could not infer session behavior because no explicit session_definition_source "
                "or session_timezone was provided for an exchange code without built-in defaults. "
                "Provide a session-definition override."
            ),
            context={"exchange_code": exchange_code},
        )

    generic_template = _build_generic_24_5_template(
        exchange_code=exchange_code,
        configured_session_timezone=configured_session_timezone,
    )

    _validate_generic_24_5_inference(exchange_code=exchange_code, tick_data=tick_data)

    return generic_template


def _build_generic_24_5_template(
    *,
    exchange_code: str,
    configured_session_timezone: Optional[str],
) -> _SessionTemplate:
    timezone_name = configured_session_timezone if configured_session_timezone is not None else "UTC"
    exchange_label = exchange_code.replace("_", " ").strip() or exchange_code

    if exchange_code.startswith("CFD_"):
        holiday_calendar_code = "CFD_24_5"
        exchange_name = f"{exchange_label} CFD 24/5"
    else:
        holiday_calendar_code = "FX_24_5"
        exchange_name = f"{exchange_label} 24/5"

    return _SessionTemplate(
        policy_id="recurring_weekly_inactivity.generic_24_5_broker_market.v1",
        exchange_name=exchange_name,
        timezone_name=timezone_name,
        timezone_source=(
            "session_configuration.session_timezone"
            if configured_session_timezone is not None
            else "default_utc_for_generic_24_5_inference"
        ),
        holiday_calendar_code=holiday_calendar_code,
        early_close_rule_set=None,
        regular_session_windows=tuple(
            MarketDataSessionRegularWindow(
                weekday=weekday,
                label="regular",
                open_time="00:00",
                close_time="24:00",
            )
            for weekday in _TRADING_WEEKDAYS
        ),
        continuous_weekday_template=True,
    )


def _validate_generic_24_5_inference(
    *,
    exchange_code: str,
    tick_data: LoadedTickDataSource,
) -> None:
    distinct_weekday_count = len(
        {
            record.timestamp.astimezone(UTC).weekday()
            for record in tick_data.records
        }
    )
    long_gap_count = 0
    weekend_gap_count = 0
    unexpected_weekday_gap_count = 0

    for previous_record, next_record in zip(tick_data.records, tick_data.records[1:]):
        gap_duration = next_record.timestamp - previous_record.timestamp
        if gap_duration < _LONG_GAP_THRESHOLD:
            continue

        long_gap_count += 1

        midpoint = previous_record.timestamp + (gap_duration / 2)
        midpoint_weekday = midpoint.astimezone(UTC).weekday()
        if midpoint_weekday >= 5:
            weekend_gap_count += 1
        else:
            unexpected_weekday_gap_count += 1

    if distinct_weekday_count < _MINIMUM_GENERIC_24_5_WEEKDAY_COUNT:
        raise _session_resolution_error(
            "session_configuration",
            (
                "automatic 24/5 session inference is ambiguous because the observed tick window "
                "does not span enough weekdays. Provide an explicit session-definition override."
            ),
            context={
                "exchange_code": exchange_code,
                "observed_weekday_count": distinct_weekday_count,
            },
        )

    if long_gap_count == 0 or weekend_gap_count == 0 or unexpected_weekday_gap_count > 0:
        raise _session_resolution_error(
            "session_configuration",
            (
                "automatic 24/5 session inference is ambiguous because the observed gap structure "
                "does not clearly support a weekday-continuous broker market. "
                "Provide an explicit session-definition override."
            ),
            context={
                "exchange_code": exchange_code,
                "long_gap_count": long_gap_count,
                "weekend_gap_count": weekend_gap_count,
                "unexpected_weekday_gap_count": unexpected_weekday_gap_count,
            },
        )


def _confidence_score_for_inference(
    *,
    template: _SessionTemplate,
    gap_alignment_diagnostics: tuple[int, int, int],
) -> float:
    long_gap_count, aligned_gap_count, unexpected_gap_count = gap_alignment_diagnostics

    confidence_score = 0.65 if template.timezone_source == "exchange_code_builtin_default" else 0.55
    if aligned_gap_count > 0:
        confidence_score += 0.25
    if template.continuous_weekday_template:
        confidence_score += 0.10
    if long_gap_count > 0 and unexpected_gap_count == 0:
        confidence_score += 0.10
    if unexpected_gap_count > 0:
        confidence_score -= 0.20

    return max(0.0, min(1.0, confidence_score))


def _confidence_label_for_gap_alignment(
    template: _SessionTemplate,
    gap_alignment_diagnostics: tuple[int, int, int],
) -> str:
    confidence_score = _confidence_score_for_inference(
        template=template,
        gap_alignment_diagnostics=gap_alignment_diagnostics,
    )
    if confidence_score >= 0.9:
        return "high"
    if confidence_score >= 0.75:
        return "medium"

    return "low"


def _automatic_resolution_reasons(
    *,
    exchange_code: str,
    template: _SessionTemplate,
    gap_alignment_diagnostics: tuple[int, int, int],
) -> tuple[str, ...]:
    long_gap_count, aligned_gap_count, _ = gap_alignment_diagnostics
    reasons: list[str] = []

    if template.timezone_source == "exchange_code_builtin_default":
        reasons.append(
            f"used the built-in exchange default for `{exchange_code}` to resolve the trading schedule"
        )
    else:
        reasons.append(
            (
                "selected the generic weekday-continuous 24/5 broker-market template based on "
                "exchange code or workflow defaults"
            )
        )

    if aligned_gap_count > 0:
        reasons.append(
            (
                f"validated the inferred session against {aligned_gap_count} observed long gaps "
                "that overlapped inferred closed windows"
            )
        )
    elif long_gap_count == 0:
        reasons.append(
            "no long gaps were present in the observed window, so exchange metadata carried the inference"
        )

    return tuple(reasons)


def _automatic_resolution_warnings(
    *,
    template: _SessionTemplate,
    gap_alignment_diagnostics: tuple[int, int, int],
) -> tuple[str, ...]:
    long_gap_count, _, unexpected_gap_count = gap_alignment_diagnostics
    warnings: list[str] = []

    if long_gap_count == 0 and template.timezone_source == "exchange_code_builtin_default":
        warnings.append(
            "observed data did not include long inactivity gaps, so the session was inferred from exchange defaults"
        )

    if unexpected_gap_count > 0:
        warnings.append(
            (
                f"{unexpected_gap_count} observed long gaps did not clearly overlap inferred closed windows. "
                "If downstream diagnostics look inconsistent, provide an explicit session-definition override."
            )
        )

    return tuple(warnings)


def _build_calendar_registry_json(
    *,
    exchange_code: str,
    session_template: _SessionTemplate,
) -> str:
    return _build_calendar_registry_json_from_windows(
        exchange_code=exchange_code,
        exchange_name=session_template.exchange_name,
        timezone_name=session_template.timezone_name,
        holiday_calendar_code=session_template.holiday_calendar_code,
        early_close_rule_set=session_template.early_close_rule_set,
        regular_session_windows=session_template.regular_session_windows,
        recurring_closed_windows=(),
    )


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


def _load_calendar_registry_from_json(
    registry_json: str,
    *,
    field_path: str,
    path_context: Optional[str],
) -> CalendarRegistry:
    try:
        return CalendarRegistry(registry_json)
    except CalendarRegistryLoadError as e:
        error_context: dict[str, str] = {
            "resolution_step": "calendar_registry_loading",
        }
        if path_context is not None:
            error_context["path"] = path_context

        raise _session_resolution_error(
            field_path,
            f"does not contain a valid calendar registry: {e}",
            context=error_context,
        ) from e


def _require_exchange_artifact(
    calendar_registry: CalendarRegistry,
    exchange_code: str,
) -> dict[str, Any]:
    registry_payload = calendar_registry._registry_payload
    raw_exchanges = registry_payload.get("exchanges")
    if not isinstance(raw_exchanges, list):
        raise _session_resolution_error(
            "session_configuration",
            "calendar registry payload must contain an `exchanges` array",
        )

    for exchange_value in raw_exchanges:
        if (
            isinstance(exchange_value, dict)
            and exchange_value.get("exchange_code") == exchange_code
        ):
            return cast(dict[str, Any], exchange_value)

    raise _session_resolution_error(
        "session_configuration.exchange_code",
        "must exist in the resolved calendar registry",
        context={"exchange_code": exchange_code},
    )


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


def _compute_open_session_duration_hours_per_week(session_template: _SessionTemplate) -> float:
    if session_template.continuous_weekday_template:
        return 120.0

    open_minutes = sum(
        _clock_minutes(window.close_time) - _clock_minutes(window.open_time)
        for window in session_template.regular_session_windows
    )
    return open_minutes / 60.0


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


def _evaluate_gap_alignment(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    tick_data: LoadedTickDataSource,
) -> tuple[int, int, int]:
    long_gap_count = 0
    aligned_gap_count = 0
    unexpected_gap_count = 0

    for previous_record, next_record in zip(tick_data.records, tick_data.records[1:]):
        gap_duration = next_record.timestamp - previous_record.timestamp
        if gap_duration < _LONG_GAP_THRESHOLD:
            continue

        long_gap_count += 1
        if _gap_contains_closed_period(
            calendar_registry=calendar_registry,
            exchange_code=exchange_code,
            gap_start_utc=previous_record.timestamp,
            gap_end_utc=next_record.timestamp,
        ):
            aligned_gap_count += 1
        else:
            unexpected_gap_count += 1

    return long_gap_count, aligned_gap_count, unexpected_gap_count


def _gap_contains_closed_period(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    gap_start_utc: datetime,
    gap_end_utc: datetime,
) -> bool:
    probe_timestamp = gap_start_utc + _GAP_SCAN_STEP
    while probe_timestamp < gap_end_utc:
        if calendar_registry.resolve_market_state(exchange_code, probe_timestamp) == MarketState.CLOSED:
            return True
        probe_timestamp += _GAP_SCAN_STEP

    return False


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


__all__ = [
    "ResolvedMarketDataSession",
    "resolve_market_data_session_configuration",
]
