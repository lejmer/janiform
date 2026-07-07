from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from enum import StrEnum
from typing import Any, Optional

from ..errors import calibration_error
from ..sessions.contracts import (
    MarketDataSessionRecurringClosedWindow,
    MarketDataSessionRegularWindow,
)
from ...contracts.base import (
    ContractModel,
    parse_json_object,
    require_datetime_utc,
    require_field,
    require_non_empty_string,
    require_non_empty_string_value,
    require_positive_finite_number,
    require_positive_finite_scalar,
    require_sequence,
    require_string,
    serialize_deterministic_json,
    validate_non_empty_string_values,
    validate_unique_string_values,
)


class PresetRegistryScope(StrEnum):
    BUILT_IN = 'built_in'
    USER_REGISTERED = 'user_registered'


class PresetReferenceUsageMode(StrEnum):
    AUTOMATIC = 'automatic'
    NAMED = 'named'
    EXPLICIT = 'explicit'


class MarketProfileKnownNewsPolicy(StrEnum):
    SUGGEST_ONLY = 'suggest_only'
    APPLY_BY_DEFAULT = 'apply_by_default'


@dataclass(frozen=True, slots=True)
class SessionExplicitConfiguration(ContractModel):
    exchange_code: str
    timezone_name: str
    regular_session_windows: tuple[MarketDataSessionRegularWindow, ...]
    recurring_closed_windows: tuple[MarketDataSessionRecurringClosedWindow, ...] = ()
    exchange_name: Optional[str] = None

    def __post_init__(self) -> None:
        if not self.exchange_code.strip():
            raise calibration_error("exchange_code", "must be a non-empty string")

        if not self.timezone_name.strip():
            raise calibration_error("timezone_name", "must be a non-empty string")

        if len(self.regular_session_windows) == 0:
            raise calibration_error(
                "regular_session_windows",
                "must contain at least one regular session window",
            )

        if self.exchange_name is not None and not self.exchange_name.strip():
            raise calibration_error("exchange_name", "must be non-empty when provided")

        _validate_unique_string_keys(
            values=tuple(
                f"{window.weekday}|{window.label}|{window.open_time}|{window.close_time}"
                for window in self.regular_session_windows
            ),
            field_path="regular_session_windows",
            duplicate_message="must not contain duplicate regular session windows",
        )

        _validate_unique_string_keys(
            values=tuple(
                (
                    f"{window.start_weekday}|{window.start_time}|"
                    f"{window.end_weekday}|{window.end_time}|{window.reason}"
                )
                for window in self.recurring_closed_windows
            ),
            field_path="recurring_closed_windows",
            duplicate_message="must not contain duplicate recurring closed windows",
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> SessionExplicitConfiguration:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        raw_regular_session_windows = require_sequence(
            raw_payload,
            "regular_session_windows",
            f"{field_path}.regular_session_windows",
            calibration_error,
        )

        raw_recurring_closed_windows = raw_payload.get("recurring_closed_windows", ())

        if isinstance(raw_recurring_closed_windows, str) or not isinstance(
            raw_recurring_closed_windows,
            list | tuple,
        ):
            raise calibration_error(
                f"{field_path}.recurring_closed_windows",
                "must be a sequence when provided",
            )

        raw_exchange_name = raw_payload.get("exchange_name")
        if raw_exchange_name is not None and not isinstance(raw_exchange_name, str):
            raise calibration_error(
                f"{field_path}.exchange_name",
                "must be a string when provided",
            )

        return SessionExplicitConfiguration(
            exchange_code=require_non_empty_string(
                raw_payload,
                "exchange_code",
                f"{field_path}.exchange_code",
                calibration_error,
            ),
            timezone_name=require_non_empty_string(
                raw_payload,
                "timezone_name",
                f"{field_path}.timezone_name",
                calibration_error,
            ),
            regular_session_windows=tuple(
                MarketDataSessionRegularWindow.from_mapping(
                    raw_window,
                    field_path=f"{field_path}.regular_session_windows[{window_index}]",
                )
                for window_index, raw_window in enumerate(raw_regular_session_windows)
            ),
            recurring_closed_windows=tuple(
                MarketDataSessionRecurringClosedWindow.from_mapping(
                    raw_window,
                    field_path=f"{field_path}.recurring_closed_windows[{window_index}]",
                )
                for window_index, raw_window in enumerate(raw_recurring_closed_windows)
            ),
            exchange_name=raw_exchange_name,
        )


@dataclass(frozen=True, slots=True)
class SessionConfigurationOverridePatch(ContractModel):
    timezone_name: Optional[str] = None
    regular_session_windows: Optional[tuple[MarketDataSessionRegularWindow, ...]] = None
    recurring_closed_windows: Optional[tuple[MarketDataSessionRecurringClosedWindow, ...]] = None

    def __post_init__(self) -> None:
        if (
            self.timezone_name is None
            and self.regular_session_windows is None
            and self.recurring_closed_windows is None
        ):
            raise calibration_error(
                "override_patch",
                "must change at least one session field",
            )

        if self.timezone_name is not None and not self.timezone_name.strip():
            raise calibration_error("timezone_name", "must be non-empty when provided")

        if self.regular_session_windows is not None and len(self.regular_session_windows) == 0:
            raise calibration_error(
                "regular_session_windows",
                "must contain at least one regular session window when provided",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> SessionConfigurationOverridePatch:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        raw_timezone_name = raw_payload.get("timezone_name")
        if raw_timezone_name is not None and not isinstance(raw_timezone_name, str):
            raise calibration_error(
                f"{field_path}.timezone_name",
                "must be a string when provided",
            )

        raw_regular_session_windows = raw_payload.get("regular_session_windows")
        if raw_regular_session_windows is not None and (
            isinstance(raw_regular_session_windows, str)
            or not isinstance(raw_regular_session_windows, list | tuple)
        ):
            raise calibration_error(
                f"{field_path}.regular_session_windows",
                "must be a sequence when provided",
            )

        raw_recurring_closed_windows = raw_payload.get("recurring_closed_windows")
        if raw_recurring_closed_windows is not None and (
            isinstance(raw_recurring_closed_windows, str)
            or not isinstance(raw_recurring_closed_windows, list | tuple)
        ):
            raise calibration_error(
                f"{field_path}.recurring_closed_windows",
                "must be a sequence when provided",
            )

        return SessionConfigurationOverridePatch(
            timezone_name=raw_timezone_name,
            regular_session_windows=(
                tuple(
                    MarketDataSessionRegularWindow.from_mapping(
                        raw_window,
                        field_path=f"{field_path}.regular_session_windows[{window_index}]",
                    )
                    for window_index, raw_window in enumerate(raw_regular_session_windows)
                )
                if raw_regular_session_windows is not None
                else None
            ),
            recurring_closed_windows=(
                tuple(
                    MarketDataSessionRecurringClosedWindow.from_mapping(
                        raw_window,
                        field_path=f"{field_path}.recurring_closed_windows[{window_index}]",
                    )
                    for window_index, raw_window in enumerate(raw_recurring_closed_windows)
                )
                if raw_recurring_closed_windows is not None
                else None
            ),
        )


@dataclass(frozen=True, slots=True)
class KnownNewsEvent(ContractModel):
    event_id: str
    scheduled_at: datetime
    event_name: Optional[str] = None
    tags: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        if not self.event_id.strip():
            raise calibration_error("event_id", "must be a non-empty string")

        if self.event_name is not None and not self.event_name.strip():
            raise calibration_error("event_name", "must be non-empty when provided")

        _validate_non_empty_strings(
            values=self.tags,
            field_path="tags",
        )

        _validate_unique_string_keys(
            values=self.tags,
            field_path="tags",
            duplicate_message="must not contain duplicate tags",
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> KnownNewsEvent:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        raw_event_name = raw_payload.get("event_name")
        if raw_event_name is not None and not isinstance(raw_event_name, str):
            raise calibration_error(
                f"{field_path}.event_name",
                "must be a string when provided",
            )

        raw_tags = raw_payload.get("tags", ())
        if isinstance(raw_tags, str) or not isinstance(raw_tags, list | tuple):
            raise calibration_error(f"{field_path}.tags", "must be a sequence when provided")

        parsed_tags = tuple(
            _require_non_empty_string_value(
                tag,
                f"{field_path}.tags[{tag_index}]",
            )
            for tag_index, tag in enumerate(raw_tags)
        )

        return KnownNewsEvent(
            event_id=require_non_empty_string(
                raw_payload,
                "event_id",
                f"{field_path}.event_id",
                calibration_error,
            ),
            scheduled_at=require_datetime_utc(
                require_field(
                    raw_payload,
                    "scheduled_at",
                    f"{field_path}.scheduled_at",
                    calibration_error,
                ),
                f"{field_path}.scheduled_at",
                calibration_error,
            ),
            event_name=raw_event_name,
            tags=parsed_tags,
        )


@dataclass(frozen=True, slots=True)
class KnownNewsExplicitConfiguration(ContractModel):
    events: tuple[KnownNewsEvent, ...]
    calendar_timezone_name: Optional[str] = None

    def __post_init__(self) -> None:
        if self.calendar_timezone_name is not None and not self.calendar_timezone_name.strip():
            raise calibration_error("calendar_timezone_name", "must be non-empty when provided")

        _validate_unique_string_keys(
            values=tuple(event.event_id for event in self.events),
            field_path="events",
            duplicate_message="must not contain duplicate event_id values",
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> KnownNewsExplicitConfiguration:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        raw_events = require_sequence(
            raw_payload,
            "events",
            f"{field_path}.events",
            calibration_error,
        )

        raw_calendar_timezone_name = raw_payload.get("calendar_timezone_name")
        if raw_calendar_timezone_name is not None and not isinstance(raw_calendar_timezone_name, str):
            raise calibration_error(
                f"{field_path}.calendar_timezone_name",
                "must be a string when provided",
            )

        return KnownNewsExplicitConfiguration(
            events=tuple(
                KnownNewsEvent.from_mapping(
                    raw_event,
                    field_path=f"{field_path}.events[{event_index}]",
                )
                for event_index, raw_event in enumerate(raw_events)
            ),
            calendar_timezone_name=raw_calendar_timezone_name,
        )


@dataclass(frozen=True, slots=True)
class KnownNewsConfigurationOverridePatch(ContractModel):
    calendar_timezone_name: Optional[str] = None
    additional_events: tuple[KnownNewsEvent, ...] = ()
    removed_event_ids: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        if (
            self.calendar_timezone_name is None
            and len(self.additional_events) == 0
            and len(self.removed_event_ids) == 0
        ):
            raise calibration_error(
                "override_patch",
                "must change at least one known-news field",
            )

        if self.calendar_timezone_name is not None and not self.calendar_timezone_name.strip():
            raise calibration_error("calendar_timezone_name", "must be non-empty when provided")

        added_event_ids = tuple(event.event_id for event in self.additional_events)

        _validate_unique_string_keys(
            values=added_event_ids,
            field_path="additional_events",
            duplicate_message="must not contain duplicate event_id values",
        )

        _validate_non_empty_strings(
            values=self.removed_event_ids,
            field_path="removed_event_ids",
        )

        _validate_unique_string_keys(
            values=self.removed_event_ids,
            field_path="removed_event_ids",
            duplicate_message="must not contain duplicate event identifiers",
        )

        removed_event_id_set = set(self.removed_event_ids)
        for event_id in added_event_ids:
            if event_id in removed_event_id_set:
                raise calibration_error(
                    "override_patch",
                    f"cannot add and remove the same event_id `{event_id}` in one override patch",
                )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> KnownNewsConfigurationOverridePatch:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        raw_calendar_timezone_name = raw_payload.get("calendar_timezone_name")
        if raw_calendar_timezone_name is not None and not isinstance(raw_calendar_timezone_name, str):
            raise calibration_error(
                f"{field_path}.calendar_timezone_name",
                "must be a string when provided",
            )

        raw_additional_events = raw_payload.get("additional_events", ())
        if isinstance(raw_additional_events, str) or not isinstance(raw_additional_events, list | tuple):
            raise calibration_error(
                f"{field_path}.additional_events",
                "must be a sequence when provided",
            )

        raw_removed_event_ids = raw_payload.get("removed_event_ids", ())
        if isinstance(raw_removed_event_ids, str) or not isinstance(raw_removed_event_ids, list | tuple):
            raise calibration_error(
                f"{field_path}.removed_event_ids",
                "must be a sequence when provided",
            )

        return KnownNewsConfigurationOverridePatch(
            calendar_timezone_name=raw_calendar_timezone_name,
            additional_events=tuple(
                KnownNewsEvent.from_mapping(
                    raw_event,
                    field_path=f"{field_path}.additional_events[{event_index}]",
                )
                for event_index, raw_event in enumerate(raw_additional_events)
            ),
            removed_event_ids=tuple(
                _require_non_empty_string_value(
                    event_id,
                    f"{field_path}.removed_event_ids[{event_index}]",
                )
                for event_index, event_id in enumerate(raw_removed_event_ids)
            ),
        )


@dataclass(frozen=True, slots=True)
class MarketProfileColumnAliasHint(ContractModel):
    canonical_name: str
    aliases: tuple[str, ...]

    def __post_init__(self) -> None:
        if not self.canonical_name.strip():
            raise calibration_error("canonical_name", "must be a non-empty string")

        if len(self.aliases) == 0:
            raise calibration_error("aliases", "must contain at least one alias")

        _validate_non_empty_strings(values=self.aliases, field_path="aliases")
        _validate_unique_string_keys(
            values=self.aliases,
            field_path="aliases",
            duplicate_message="must not contain duplicate aliases",
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> MarketProfileColumnAliasHint:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        raw_aliases = require_sequence(
            raw_payload,
            "aliases",
            f"{field_path}.aliases",
            calibration_error,
        )

        return MarketProfileColumnAliasHint(
            canonical_name=require_non_empty_string(
                raw_payload,
                "canonical_name",
                f"{field_path}.canonical_name",
                calibration_error,
            ),
            aliases=tuple(
                _require_non_empty_string_value(
                    alias,
                    f"{field_path}.aliases[{alias_index}]",
                )
                for alias_index, alias in enumerate(raw_aliases)
            ),
        )


@dataclass(frozen=True, slots=True)
class SessionPresetReference(ContractModel):
    usage_mode: PresetReferenceUsageMode
    registry_scope: Optional[PresetRegistryScope] = None
    preset_name: Optional[str] = None
    configuration: Optional[SessionExplicitConfiguration] = None
    override_patch: Optional[SessionConfigurationOverridePatch] = None
    exchange_code_hint: Optional[str] = None
    timezone_name_hint: Optional[str] = None

    def __post_init__(self) -> None:
        _validate_optional_non_empty_string(
            self.preset_name,
            field_path="preset_name",
        )

        _validate_optional_non_empty_string(
            self.exchange_code_hint,
            field_path="exchange_code_hint",
        )

        _validate_optional_non_empty_string(
            self.timezone_name_hint,
            field_path="timezone_name_hint",
        )

        match self.usage_mode:
            case PresetReferenceUsageMode.AUTOMATIC:
                if self.registry_scope is not None:
                    raise calibration_error(
                        "registry_scope",
                        "is not allowed when usage_mode=automatic. Use usage_mode=named for preset lookups.",
                    )

                if self.preset_name is not None:
                    raise calibration_error(
                        "preset_name",
                        "is not allowed when usage_mode=automatic. Remove the preset name or switch to usage_mode=named.",
                    )

                if self.configuration is not None:
                    raise calibration_error(
                        "configuration",
                        "is not allowed when usage_mode=automatic. Use usage_mode=explicit for full session configuration.",
                    )
            case PresetReferenceUsageMode.NAMED:
                if self.registry_scope is None:
                    raise calibration_error(
                        "registry_scope",
                        "is required when usage_mode=named. Choose built_in or user_registered.",
                    )

                if self.preset_name is None:
                    raise calibration_error(
                        "preset_name",
                        "is required when usage_mode=named",
                    )

                if self.configuration is not None:
                    raise calibration_error(
                        "configuration",
                        "cannot be combined with usage_mode=named. Use override_patch for small changes or usage_mode=explicit for full configuration.",
                    )

                if self.exchange_code_hint is not None or self.timezone_name_hint is not None:
                    raise calibration_error(
                        "usage_mode",
                        "named session references do not allow inference hints. Move timezone changes into override_patch.",
                    )
            case PresetReferenceUsageMode.EXPLICIT:
                if self.registry_scope is not None:
                    raise calibration_error(
                        "registry_scope",
                        "is not allowed when usage_mode=explicit",
                    )

                if self.preset_name is not None:
                    raise calibration_error(
                        "preset_name",
                        "is not allowed when usage_mode=explicit",
                    )

                if self.configuration is None:
                    raise calibration_error(
                        "configuration",
                        "is required when usage_mode=explicit",
                    )

                if self.override_patch is not None:
                    raise calibration_error(
                        "override_patch",
                        "cannot be combined with usage_mode=explicit because the explicit configuration is already the full replacement",
                    )

                if self.exchange_code_hint is not None or self.timezone_name_hint is not None:
                    raise calibration_error(
                        "usage_mode",
                        "explicit session references do not allow inference hints",
                    )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "session_reference",
    ) -> SessionPresetReference:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        usage_mode = _parse_usage_mode(
            raw_payload,
            field_name="usage_mode",
            field_path=f"{field_path}.usage_mode",
            allowed_usage_modes=(
                PresetReferenceUsageMode.AUTOMATIC,
                PresetReferenceUsageMode.NAMED,
                PresetReferenceUsageMode.EXPLICIT,
            ),
        )

        raw_registry_scope = raw_payload.get("registry_scope")
        raw_preset_name = raw_payload.get("preset_name")
        raw_exchange_code_hint = raw_payload.get("exchange_code_hint")
        raw_timezone_name_hint = raw_payload.get("timezone_name_hint")

        _validate_optional_raw_string(raw_registry_scope, f"{field_path}.registry_scope")
        _validate_optional_raw_string(raw_preset_name, f"{field_path}.preset_name")

        _validate_optional_raw_string(
            raw_exchange_code_hint,
            f"{field_path}.exchange_code_hint",
        )

        _validate_optional_raw_string(
            raw_timezone_name_hint,
            f"{field_path}.timezone_name_hint",
        )

        return SessionPresetReference(
            usage_mode=usage_mode,
            registry_scope=_parse_optional_registry_scope(
                raw_registry_scope,
                field_path=f"{field_path}.registry_scope",
            ),
            preset_name=raw_preset_name,
            configuration=(
                SessionExplicitConfiguration.from_mapping(
                    raw_payload["configuration"],
                    field_path=f"{field_path}.configuration",
                )
                if "configuration" in raw_payload and raw_payload["configuration"] is not None
                else None
            ),
            override_patch=(
                SessionConfigurationOverridePatch.from_mapping(
                    raw_payload["override_patch"],
                    field_path=f"{field_path}.override_patch",
                )
                if "override_patch" in raw_payload and raw_payload["override_patch"] is not None
                else None
            ),
            exchange_code_hint=raw_exchange_code_hint,
            timezone_name_hint=raw_timezone_name_hint,
        )


@dataclass(frozen=True, slots=True)
class KnownNewsPresetReference(ContractModel):
    usage_mode: PresetReferenceUsageMode
    registry_scope: Optional[PresetRegistryScope] = None
    preset_name: Optional[str] = None
    configuration: Optional[KnownNewsExplicitConfiguration] = None
    override_patch: Optional[KnownNewsConfigurationOverridePatch] = None

    def __post_init__(self) -> None:
        _validate_optional_non_empty_string(self.preset_name, field_path="preset_name")

        match self.usage_mode:
            case PresetReferenceUsageMode.AUTOMATIC:
                raise calibration_error(
                    "usage_mode",
                    (
                        "automatic is not supported for known-news references. "
                        "Use a named preset such as `none` or provide explicit configuration."
                    ),
                )
            case PresetReferenceUsageMode.NAMED:
                if self.registry_scope is None:
                    raise calibration_error(
                        "registry_scope",
                        "is required when usage_mode=named. Choose built_in or user_registered.",
                    )

                if self.preset_name is None:
                    raise calibration_error("preset_name", "is required when usage_mode=named")

                if self.configuration is not None:
                    raise calibration_error(
                        "configuration",
                        "cannot be combined with usage_mode=named. Use override_patch for small changes or usage_mode=explicit for full configuration.",
                    )
            case PresetReferenceUsageMode.EXPLICIT:
                if self.registry_scope is not None:
                    raise calibration_error(
                        "registry_scope",
                        "is not allowed when usage_mode=explicit",
                    )

                if self.preset_name is not None:
                    raise calibration_error(
                        "preset_name",
                        "is not allowed when usage_mode=explicit",
                    )

                if self.configuration is None:
                    raise calibration_error(
                        "configuration",
                        "is required when usage_mode=explicit",
                    )

                if self.override_patch is not None:
                    raise calibration_error(
                        "override_patch",
                        "cannot be combined with usage_mode=explicit because the explicit configuration is already the full replacement",
                    )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "known_news_reference",
    ) -> KnownNewsPresetReference:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        usage_mode = _parse_usage_mode(
            raw_payload,
            field_name="usage_mode",
            field_path=f"{field_path}.usage_mode",
            allowed_usage_modes=(
                PresetReferenceUsageMode.NAMED,
                PresetReferenceUsageMode.EXPLICIT,
            ),
        )

        raw_registry_scope = raw_payload.get("registry_scope")
        raw_preset_name = raw_payload.get("preset_name")

        _validate_optional_raw_string(raw_registry_scope, f"{field_path}.registry_scope")
        _validate_optional_raw_string(raw_preset_name, f"{field_path}.preset_name")

        return KnownNewsPresetReference(
            usage_mode=usage_mode,
            registry_scope=_parse_optional_registry_scope(
                raw_registry_scope,
                field_path=f"{field_path}.registry_scope",
            ),
            preset_name=raw_preset_name,
            configuration=(
                KnownNewsExplicitConfiguration.from_mapping(
                    raw_payload["configuration"],
                    field_path=f"{field_path}.configuration",
                )
                if "configuration" in raw_payload and raw_payload["configuration"] is not None
                else None
            ),
            override_patch=(
                KnownNewsConfigurationOverridePatch.from_mapping(
                    raw_payload["override_patch"],
                    field_path=f"{field_path}.override_patch",
                )
                if "override_patch" in raw_payload and raw_payload["override_patch"] is not None
                else None
            ),
        )


@dataclass(frozen=True, slots=True)
class MarketProfileExplicitConfiguration(ContractModel):
    profile_name: str
    session_reference: SessionPresetReference
    known_news_reference: Optional[KnownNewsPresetReference] = None
    known_news_policy: MarketProfileKnownNewsPolicy = MarketProfileKnownNewsPolicy.SUGGEST_ONLY
    timestamp_timezone_name: Optional[str] = None
    tick_size: Optional[float] = None
    column_alias_hints: tuple[MarketProfileColumnAliasHint, ...] = ()

    def __post_init__(self) -> None:
        if not self.profile_name.strip():
            raise calibration_error("profile_name", "must be a non-empty string")

        if self.timestamp_timezone_name is not None and not self.timestamp_timezone_name.strip():
            raise calibration_error("timestamp_timezone_name", "must be non-empty when provided")

        if self.tick_size is not None:
            require_positive_finite_scalar(self.tick_size, "tick_size", calibration_error)

        _validate_unique_string_keys(
            values=tuple(hint.canonical_name for hint in self.column_alias_hints),
            field_path="column_alias_hints",
            duplicate_message="must not contain duplicate canonical_name values",
        )

        if self.known_news_policy == MarketProfileKnownNewsPolicy.APPLY_BY_DEFAULT:
            if self.known_news_reference is None:
                raise calibration_error(
                    "known_news_policy",
                    "apply_by_default requires a known_news_reference",
                )

            if (
                self.known_news_reference.usage_mode == PresetReferenceUsageMode.NAMED
                and self.known_news_reference.preset_name == "none"
            ):
                    raise calibration_error(
                        "known_news_reference",
                        "cannot auto-apply the `none` known-news preset",
                    )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> MarketProfileExplicitConfiguration:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        raw_timestamp_timezone_name = raw_payload.get("timestamp_timezone_name")
        if raw_timestamp_timezone_name is not None and not isinstance(raw_timestamp_timezone_name, str):
            raise calibration_error(
                f"{field_path}.timestamp_timezone_name",
                "must be a string when provided",
            )

        raw_column_alias_hints = raw_payload.get("column_alias_hints", ())
        if isinstance(raw_column_alias_hints, str) or not isinstance(raw_column_alias_hints, list | tuple):
            raise calibration_error(
                f"{field_path}.column_alias_hints",
                "must be a sequence when provided",
            )

        return MarketProfileExplicitConfiguration(
            profile_name=require_non_empty_string(
                raw_payload,
                "profile_name",
                f"{field_path}.profile_name",
                calibration_error,
            ),
            session_reference=SessionPresetReference.from_mapping(
                require_field(
                    raw_payload,
                    "session_reference",
                    f"{field_path}.session_reference",
                    calibration_error,
                ),
                field_path=f"{field_path}.session_reference",
            ),
            known_news_reference=(
                KnownNewsPresetReference.from_mapping(
                    raw_payload["known_news_reference"],
                    field_path=f"{field_path}.known_news_reference",
                )
                if "known_news_reference" in raw_payload and raw_payload["known_news_reference"] is not None
                else None
            ),
            known_news_policy=_parse_known_news_policy(
                raw_payload,
                field_name="known_news_policy",
                field_path=f"{field_path}.known_news_policy",
                default_value=MarketProfileKnownNewsPolicy.SUGGEST_ONLY,
            ),
            timestamp_timezone_name=raw_timestamp_timezone_name,
            tick_size=(
                require_positive_finite_number(
                    raw_payload,
                    "tick_size",
                    f"{field_path}.tick_size",
                    calibration_error,
                )
                if "tick_size" in raw_payload and raw_payload["tick_size"] is not None
                else None
            ),
            column_alias_hints=tuple(
                MarketProfileColumnAliasHint.from_mapping(
                    raw_hint,
                    field_path=f"{field_path}.column_alias_hints[{hint_index}]",
                )
                for hint_index, raw_hint in enumerate(raw_column_alias_hints)
            ),
        )


@dataclass(frozen=True, slots=True)
class MarketProfileOverridePatch(ContractModel):
    session_reference: Optional[SessionPresetReference] = None
    known_news_reference: Optional[KnownNewsPresetReference] = None
    known_news_policy: Optional[MarketProfileKnownNewsPolicy] = None
    timestamp_timezone_name: Optional[str] = None
    tick_size: Optional[float] = None
    column_alias_hints: Optional[tuple[MarketProfileColumnAliasHint, ...]] = None

    def __post_init__(self) -> None:
        if (
            self.session_reference is None
            and self.known_news_reference is None
            and self.known_news_policy is None
            and self.timestamp_timezone_name is None
            and self.tick_size is None
            and self.column_alias_hints is None
        ):
            raise calibration_error(
                "override_patch",
                "must change at least one market-profile field",
            )

        if self.timestamp_timezone_name is not None and not self.timestamp_timezone_name.strip():
            raise calibration_error("timestamp_timezone_name", "must be non-empty when provided")

        if self.tick_size is not None:
            require_positive_finite_scalar(self.tick_size, "tick_size", calibration_error)

        if self.known_news_policy is not None and self.known_news_reference is None:
            raise calibration_error(
                "known_news_policy",
                "cannot be overridden without also providing known_news_reference because that would make the patch ambiguous",
            )

        if self.column_alias_hints is not None:
            _validate_unique_string_keys(
                values=tuple(hint.canonical_name for hint in self.column_alias_hints),
                field_path="column_alias_hints",
                duplicate_message="must not contain duplicate canonical_name values",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> MarketProfileOverridePatch:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        raw_timestamp_timezone_name = raw_payload.get("timestamp_timezone_name")
        if raw_timestamp_timezone_name is not None and not isinstance(raw_timestamp_timezone_name, str):
            raise calibration_error(
                f"{field_path}.timestamp_timezone_name",
                "must be a string when provided",
            )

        raw_column_alias_hints = raw_payload.get("column_alias_hints")
        if raw_column_alias_hints is not None and (
            isinstance(raw_column_alias_hints, str)
            or not isinstance(raw_column_alias_hints, list | tuple)
        ):
            raise calibration_error(
                f"{field_path}.column_alias_hints",
                "must be a sequence when provided",
            )

        return MarketProfileOverridePatch(
            session_reference=(
                SessionPresetReference.from_mapping(
                    raw_payload["session_reference"],
                    field_path=f"{field_path}.session_reference",
                )
                if "session_reference" in raw_payload and raw_payload["session_reference"] is not None
                else None
            ),
            known_news_reference=(
                KnownNewsPresetReference.from_mapping(
                    raw_payload["known_news_reference"],
                    field_path=f"{field_path}.known_news_reference",
                )
                if "known_news_reference" in raw_payload and raw_payload["known_news_reference"] is not None
                else None
            ),
            known_news_policy=(
                _parse_known_news_policy(
                    raw_payload,
                    field_name="known_news_policy",
                    field_path=f"{field_path}.known_news_policy",
                )
                if "known_news_policy" in raw_payload and raw_payload["known_news_policy"] is not None
                else None
            ),
            timestamp_timezone_name=raw_timestamp_timezone_name,
            tick_size=(
                require_positive_finite_number(
                    raw_payload,
                    "tick_size",
                    f"{field_path}.tick_size",
                    calibration_error,
                )
                if "tick_size" in raw_payload and raw_payload["tick_size"] is not None
                else None
            ),
            column_alias_hints=(
                tuple(
                    MarketProfileColumnAliasHint.from_mapping(
                        raw_hint,
                        field_path=f"{field_path}.column_alias_hints[{hint_index}]",
                    )
                    for hint_index, raw_hint in enumerate(raw_column_alias_hints)
                )
                if raw_column_alias_hints is not None
                else None
            ),
        )


@dataclass(frozen=True, slots=True)
class MarketProfileReference(ContractModel):
    usage_mode: PresetReferenceUsageMode
    registry_scope: Optional[PresetRegistryScope] = None
    preset_name: Optional[str] = None
    configuration: Optional[MarketProfileExplicitConfiguration] = None
    override_patch: Optional[MarketProfileOverridePatch] = None

    def __post_init__(self) -> None:
        _validate_optional_non_empty_string(self.preset_name, field_path="preset_name")

        match self.usage_mode:
            case PresetReferenceUsageMode.AUTOMATIC:
                raise calibration_error(
                    "usage_mode",
                    (
                        "automatic is not supported for market-profile references. "
                        "Use a named profile or explicit configuration."
                    ),
                )
            case PresetReferenceUsageMode.NAMED:
                if self.registry_scope is None:
                    raise calibration_error(
                        "registry_scope",
                        "is required when usage_mode=named. Choose built_in or user_registered.",
                    )

                if self.preset_name is None:
                    raise calibration_error("preset_name", "is required when usage_mode=named")

                if self.configuration is not None:
                    raise calibration_error(
                        "configuration",
                        "cannot be combined with usage_mode=named. Use override_patch for small changes or usage_mode=explicit for full configuration.",
                    )
            case PresetReferenceUsageMode.EXPLICIT:
                if self.registry_scope is not None:
                    raise calibration_error(
                        "registry_scope",
                        "is not allowed when usage_mode=explicit",
                    )

                if self.preset_name is not None:
                    raise calibration_error(
                        "preset_name",
                        "is not allowed when usage_mode=explicit",
                    )

                if self.configuration is None:
                    raise calibration_error(
                        "configuration",
                        "is required when usage_mode=explicit",
                    )

                if self.override_patch is not None:
                    raise calibration_error(
                        "override_patch",
                        "cannot be combined with usage_mode=explicit because the explicit configuration is already the full replacement",
                    )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "market_profile_reference",
    ) -> MarketProfileReference:
        if not isinstance(raw_payload, dict):
            raise calibration_error(field_path, "must be an object")

        usage_mode = _parse_usage_mode(
            raw_payload,
            field_name="usage_mode",
            field_path=f"{field_path}.usage_mode",
            allowed_usage_modes=(
                PresetReferenceUsageMode.NAMED,
                PresetReferenceUsageMode.EXPLICIT,
            ),
        )

        raw_registry_scope = raw_payload.get("registry_scope")
        raw_preset_name = raw_payload.get("preset_name")

        _validate_optional_raw_string(raw_registry_scope, f"{field_path}.registry_scope")
        _validate_optional_raw_string(raw_preset_name, f"{field_path}.preset_name")

        return MarketProfileReference(
            usage_mode=usage_mode,
            registry_scope=_parse_optional_registry_scope(
                raw_registry_scope,
                field_path=f"{field_path}.registry_scope",
            ),
            preset_name=raw_preset_name,
            configuration=(
                MarketProfileExplicitConfiguration.from_mapping(
                    raw_payload["configuration"],
                    field_path=f"{field_path}.configuration",
                )
                if "configuration" in raw_payload and raw_payload["configuration"] is not None
                else None
            ),
            override_patch=(
                MarketProfileOverridePatch.from_mapping(
                    raw_payload["override_patch"],
                    field_path=f"{field_path}.override_patch",
                )
                if "override_patch" in raw_payload and raw_payload["override_patch"] is not None
                else None
            ),
        )


def validate_session_preset_reference(raw_reference: dict[str, Any] | Any) -> SessionPresetReference:
    return SessionPresetReference.from_mapping(raw_reference)


def validate_known_news_preset_reference(raw_reference: dict[str, Any] | Any) -> KnownNewsPresetReference:
    return KnownNewsPresetReference.from_mapping(raw_reference)


def validate_market_profile_reference(raw_reference: dict[str, Any] | Any) -> MarketProfileReference:
    return MarketProfileReference.from_mapping(raw_reference)


def serialize_session_preset_reference(reference: SessionPresetReference) -> str:
    return serialize_deterministic_json(reference)


def serialize_known_news_preset_reference(reference: KnownNewsPresetReference) -> str:
    return serialize_deterministic_json(reference)


def serialize_market_profile_reference(reference: MarketProfileReference) -> str:
    return serialize_deterministic_json(reference)


def deserialize_session_preset_reference(payload_json: str) -> SessionPresetReference:
    raw_payload = parse_json_object(payload_json, "session preset reference", calibration_error)
    return validate_session_preset_reference(dict(raw_payload))


def deserialize_known_news_preset_reference(payload_json: str) -> KnownNewsPresetReference:
    raw_payload = parse_json_object(payload_json, "known-news preset reference", calibration_error)
    return validate_known_news_preset_reference(dict(raw_payload))


def deserialize_market_profile_reference(payload_json: str) -> MarketProfileReference:
    raw_payload = parse_json_object(payload_json, "market-profile reference", calibration_error)
    return validate_market_profile_reference(dict(raw_payload))


def _parse_usage_mode(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
    allowed_usage_modes: tuple[PresetReferenceUsageMode, ...],
) -> PresetReferenceUsageMode:
    raw_usage_mode = require_string(raw_payload, field_name, field_path, calibration_error)
    try:
        usage_mode = PresetReferenceUsageMode(raw_usage_mode)
    except ValueError as e:
        allowed_values = ", ".join(usage_mode.value for usage_mode in allowed_usage_modes)
        raise calibration_error(field_path, f"must be one of: {allowed_values}") from e

    if usage_mode not in allowed_usage_modes:
        allowed_values = ", ".join(allowed_mode.value for allowed_mode in allowed_usage_modes)
        raise calibration_error(field_path, f"must be one of: {allowed_values}")

    return usage_mode


def _parse_known_news_policy(
    raw_payload: dict[str, Any],
    *,
    field_name: str,
    field_path: str,
    default_value: Optional[MarketProfileKnownNewsPolicy] = None,
) -> MarketProfileKnownNewsPolicy:
    raw_policy = raw_payload.get(field_name)
    if raw_policy is None:
        if default_value is None:
            raise calibration_error(field_path, "is required")

        return default_value

    if not isinstance(raw_policy, str):
        raise calibration_error(field_path, "must be a string")

    try:
        return MarketProfileKnownNewsPolicy(raw_policy)
    except ValueError as e:
        raise calibration_error(
            field_path,
            "must be one of: suggest_only, apply_by_default",
        ) from e


def _parse_optional_registry_scope(
    raw_value: object,
    *,
    field_path: str,
) -> Optional[PresetRegistryScope]:
    if raw_value is None:
        return None

    if not isinstance(raw_value, str):
        raise calibration_error(field_path, "must be a string when provided")

    try:
        return PresetRegistryScope(raw_value)
    except ValueError as e:
        raise calibration_error(
            field_path,
            "must be one of: built_in, user_registered",
        ) from e


def _require_non_empty_string_value(raw_value: object, field_path: str) -> str:
    return require_non_empty_string_value(raw_value, field_path, calibration_error)


def _validate_optional_raw_string(raw_value: object, field_path: str) -> None:
    if raw_value is not None and not isinstance(raw_value, str):
        raise calibration_error(field_path, "must be a string when provided")


def _validate_optional_non_empty_string(raw_value: Optional[str], *, field_path: str) -> None:
    if raw_value is not None and not raw_value.strip():
        raise calibration_error(field_path, "must be non-empty when provided")


def _validate_non_empty_strings(values: tuple[str, ...], *, field_path: str) -> None:
    validate_non_empty_string_values(values, field_path, calibration_error)


def _validate_unique_string_keys(
    *,
    values: tuple[str, ...],
    field_path: str,
    duplicate_message: str,
) -> None:
    validate_unique_string_values(
        values=values,
        field_path=field_path,
        duplicate_message=duplicate_message,
        error_factory=calibration_error,
    )


__all__ = [
    "KnownNewsConfigurationOverridePatch",
    "KnownNewsEvent",
    "KnownNewsExplicitConfiguration",
    "KnownNewsPresetReference",
    "MarketProfileColumnAliasHint",
    "MarketProfileExplicitConfiguration",
    "MarketProfileKnownNewsPolicy",
    "MarketProfileOverridePatch",
    "MarketProfileReference",
    "PresetReferenceUsageMode",
    "PresetRegistryScope",
    "SessionConfigurationOverridePatch",
    "SessionExplicitConfiguration",
    "SessionPresetReference",
    "deserialize_known_news_preset_reference",
    "deserialize_market_profile_reference",
    "deserialize_session_preset_reference",
    "serialize_known_news_preset_reference",
    "serialize_market_profile_reference",
    "serialize_session_preset_reference",
    "validate_known_news_preset_reference",
    "validate_market_profile_reference",
    "validate_session_preset_reference",
]
