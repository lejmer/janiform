from dataclasses import dataclass
from typing import Any

from ..._contracts import ContractModel, require_non_empty_string
from .._errors import _calibration_error


CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_SCHEMA_VERSION = "calibration.market_data.session_resolution.v1"
CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_ERROR_ID = "calibration.validation.session_resolution"
CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_ENTRYPOINT = "market_data_session_resolution"


def _session_resolution_error(
    field_path: str,
    message: str,
    *,
    context: dict[str, str | int | float | bool | None] | None = None,
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


__all__ = [
    "CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_ENTRYPOINT",
    "CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_ERROR_ID",
    "CALIBRATION_MARKET_DATA_SESSION_RESOLUTION_SCHEMA_VERSION",
    "MarketDataSessionRecurringClosedWindow",
    "MarketDataSessionRegularWindow",
    "MarketDataSessionResolution",
]
