from __future__ import annotations

from dataclasses import dataclass
from collections.abc import Mapping
from typing import Any, Optional

from ...contracts import ContractModel, require_non_empty_string, require_sequence
from .._errors import _calibration_error


CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_SCHEMA_VERSION = "calibration.market_data.session_resolution.v1"
CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_ERROR_ID = "calibration.validation.session_resolution"
CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_ENTRYPOINT = "market_data_session_resolution"


def _session_resolution_error(
    field_path: str,
    message: str,
    *,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> Exception:
    return _calibration_error(
        field_path,
        message,
        entrypoint=CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_ENTRYPOINT,
        error_id=CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_ERROR_ID,
        context=context,
    )


@dataclass(frozen=True, slots=True)
class MarketDataSessionRegularWindow(ContractModel):
    weekday: str
    label: str
    open_time: str
    close_time: str

    def __post_init__(self) -> None:
        if not self.weekday.strip():
            raise _session_resolution_error("weekday", "must be a non-empty string")
        if not self.label.strip():
            raise _session_resolution_error("label", "must be a non-empty string")
        if not self.open_time.strip():
            raise _session_resolution_error("open_time", "must be a non-empty string")
        if not self.close_time.strip():
            raise _session_resolution_error("close_time", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> MarketDataSessionRegularWindow:
        if not isinstance(raw_payload, dict):
            raise _session_resolution_error(field_path, "must be an object")

        return MarketDataSessionRegularWindow(
            weekday=require_non_empty_string(
                raw_payload,
                "weekday",
                f"{field_path}.weekday",
                _session_resolution_error,
            ),
            label=require_non_empty_string(
                raw_payload,
                "label",
                f"{field_path}.label",
                _session_resolution_error,
            ),
            open_time=require_non_empty_string(
                raw_payload,
                "open_time",
                f"{field_path}.open_time",
                _session_resolution_error,
            ),
            close_time=require_non_empty_string(
                raw_payload,
                "close_time",
                f"{field_path}.close_time",
                _session_resolution_error,
            ),
        )


@dataclass(frozen=True, slots=True)
class MarketDataSessionRecurringClosedWindow(ContractModel):
    start_weekday: str
    start_time: str
    end_weekday: str
    end_time: str
    reason: str

    def __post_init__(self) -> None:
        for field_name, field_value in (
            ("start_weekday", self.start_weekday),
            ("start_time", self.start_time),
            ("end_weekday", self.end_weekday),
            ("end_time", self.end_time),
            ("reason", self.reason),
        ):
            if not field_value.strip():
                raise _session_resolution_error(field_name, "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> MarketDataSessionRecurringClosedWindow:
        if not isinstance(raw_payload, dict):
            raise _session_resolution_error(field_path, "must be an object")

        return MarketDataSessionRecurringClosedWindow(
            start_weekday=require_non_empty_string(
                raw_payload,
                "start_weekday",
                f"{field_path}.start_weekday",
                _session_resolution_error,
            ),
            start_time=require_non_empty_string(
                raw_payload,
                "start_time",
                f"{field_path}.start_time",
                _session_resolution_error,
            ),
            end_weekday=require_non_empty_string(
                raw_payload,
                "end_weekday",
                f"{field_path}.end_weekday",
                _session_resolution_error,
            ),
            end_time=require_non_empty_string(
                raw_payload,
                "end_time",
                f"{field_path}.end_time",
                _session_resolution_error,
            ),
            reason=require_non_empty_string(
                raw_payload,
                "reason",
                f"{field_path}.reason",
                _session_resolution_error,
            ),
        )


@dataclass(frozen=True, slots=True)
class MarketDataSessionResolution(ContractModel):
    schema_version: str
    resolution_kind: str
    provenance_kind: str
    selected_policy_id: str
    manual_override_applied: bool
    exchange_code: str
    exchange_name: str
    resolved_timezone_name: str
    timezone_source: str
    open_session_duration_hours_per_week: float
    confidence_score: float
    confidence_label: str
    regular_session_windows: tuple[MarketDataSessionRegularWindow, ...]
    recurring_closed_windows: tuple[MarketDataSessionRecurringClosedWindow, ...]
    reasons: tuple[str, ...]
    ambiguity_warnings: tuple[str, ...]
    metadata_discrepancies: tuple[str, ...]
    resolved_session_definition_json: str

    def __post_init__(self) -> None:
        if self.schema_version != CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_SCHEMA_VERSION:
            raise _session_resolution_error(
                "schema_version",
                f"must be `{CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_SCHEMA_VERSION}`",
            )

        for field_name, field_value in (
            ("resolution_kind", self.resolution_kind),
            ("provenance_kind", self.provenance_kind),
            ("selected_policy_id", self.selected_policy_id),
            ("exchange_code", self.exchange_code),
            ("exchange_name", self.exchange_name),
            ("resolved_timezone_name", self.resolved_timezone_name),
            ("timezone_source", self.timezone_source),
            ("confidence_label", self.confidence_label),
            ("resolved_session_definition_json", self.resolved_session_definition_json),
        ):
            if not field_value.strip():
                raise _session_resolution_error(field_name, "must be a non-empty string")

        if self.open_session_duration_hours_per_week <= 0.0:
            raise _session_resolution_error(
                "open_session_duration_hours_per_week",
                "must be strictly positive",
            )

        if not 0.0 <= self.confidence_score <= 1.0:
            raise _session_resolution_error(
                "confidence_score",
                "must be between 0.0 and 1.0",
            )

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> MarketDataSessionResolution:
        if not isinstance(raw_payload, Mapping):
            raise _session_resolution_error(field_path, "must be an object")

        return MarketDataSessionResolution(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                _session_resolution_error,
            ),
            resolution_kind=require_non_empty_string(
                raw_payload,
                "resolution_kind",
                f"{field_path}.resolution_kind",
                _session_resolution_error,
            ),
            provenance_kind=require_non_empty_string(
                raw_payload,
                "provenance_kind",
                f"{field_path}.provenance_kind",
                _session_resolution_error,
            ),
            selected_policy_id=require_non_empty_string(
                raw_payload,
                "selected_policy_id",
                f"{field_path}.selected_policy_id",
                _session_resolution_error,
            ),
            manual_override_applied=_require_session_resolution_boolean(
                raw_payload,
                "manual_override_applied",
                field_path=f"{field_path}.manual_override_applied",
            ),
            exchange_code=require_non_empty_string(
                raw_payload,
                "exchange_code",
                f"{field_path}.exchange_code",
                _session_resolution_error,
            ),
            exchange_name=require_non_empty_string(
                raw_payload,
                "exchange_name",
                f"{field_path}.exchange_name",
                _session_resolution_error,
            ),
            resolved_timezone_name=require_non_empty_string(
                raw_payload,
                "resolved_timezone_name",
                f"{field_path}.resolved_timezone_name",
                _session_resolution_error,
            ),
            timezone_source=require_non_empty_string(
                raw_payload,
                "timezone_source",
                f"{field_path}.timezone_source",
                _session_resolution_error,
            ),
            open_session_duration_hours_per_week=_require_session_resolution_number(
                raw_payload,
                "open_session_duration_hours_per_week",
                field_path=f"{field_path}.open_session_duration_hours_per_week",
            ),
            confidence_score=_require_session_resolution_number(
                raw_payload,
                "confidence_score",
                field_path=f"{field_path}.confidence_score",
            ),
            confidence_label=require_non_empty_string(
                raw_payload,
                "confidence_label",
                f"{field_path}.confidence_label",
                _session_resolution_error,
            ),
            regular_session_windows=tuple(
                MarketDataSessionRegularWindow.from_mapping(
                    raw_window,
                    field_path=f"{field_path}.regular_session_windows[{window_index}]",
                )
                for window_index, raw_window in enumerate(
                    require_sequence(
                        raw_payload,
                        "regular_session_windows",
                        f"{field_path}.regular_session_windows",
                        _session_resolution_error,
                    )
                )
            ),
            recurring_closed_windows=tuple(
                MarketDataSessionRecurringClosedWindow.from_mapping(
                    raw_window,
                    field_path=f"{field_path}.recurring_closed_windows[{window_index}]",
                )
                for window_index, raw_window in enumerate(
                    require_sequence(
                        raw_payload,
                        "recurring_closed_windows",
                        f"{field_path}.recurring_closed_windows",
                        _session_resolution_error,
                    )
                )
            ),
            reasons=tuple(
                _require_session_resolution_string_sequence(
                    raw_payload,
                    "reasons",
                    field_path=f"{field_path}.reasons",
                )
            ),
            ambiguity_warnings=tuple(
                _require_session_resolution_string_sequence(
                    raw_payload,
                    "ambiguity_warnings",
                    field_path=f"{field_path}.ambiguity_warnings",
                )
            ),
            metadata_discrepancies=tuple(
                _require_session_resolution_string_sequence(
                    raw_payload,
                    "metadata_discrepancies",
                    field_path=f"{field_path}.metadata_discrepancies",
                )
            ),
            resolved_session_definition_json=require_non_empty_string(
                raw_payload,
                "resolved_session_definition_json",
                f"{field_path}.resolved_session_definition_json",
                _session_resolution_error,
            ),
        )


def _require_session_resolution_string_sequence(
    raw_payload: Mapping[str, Any],
    field_name: str,
    *,
    field_path: str,
) -> list[str]:
    raw_values = require_sequence(
        raw_payload,
        field_name,
        field_path,
        _session_resolution_error,
    )
    values: list[str] = []
    for value_index, raw_value in enumerate(raw_values):
        if not isinstance(raw_value, str):
            raise _session_resolution_error(
                f"{field_path}[{value_index}]",
                "must be a string",
            )
        values.append(raw_value)

    return values


def _require_session_resolution_boolean(
    raw_payload: Mapping[str, Any],
    field_name: str,
    *,
    field_path: str,
) -> bool:
    raw_value = raw_payload.get(field_name)
    if not isinstance(raw_value, bool):
        raise _session_resolution_error(field_path, "must be a boolean")

    return raw_value


def _require_session_resolution_number(
    raw_payload: Mapping[str, Any],
    field_name: str,
    *,
    field_path: str,
) -> float:
    raw_value = raw_payload.get(field_name)
    if not isinstance(raw_value, int | float) or isinstance(raw_value, bool):
        raise _session_resolution_error(field_path, "must be a number")

    return float(raw_value)


__all__ = [
    "CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_ENTRYPOINT",
    "CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_ERROR_ID",
    "CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_SCHEMA_VERSION",
    "MarketDataSessionRecurringClosedWindow",
    "MarketDataSessionRegularWindow",
    "MarketDataSessionResolution",
]
