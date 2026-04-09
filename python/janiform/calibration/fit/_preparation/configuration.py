from __future__ import annotations

from datetime import UTC
from pathlib import Path
from typing import Any, Optional, cast

from ....calendar import CalendarRegistry
from ...errors import calibration_error
from ...market_data.contracts import (
    CalibrationInputFileFormat,
    CalibrationInputFileReference,
    CalibrationKnownNewsInput,
    CalibrationSessionConfigurationInput,
)
from ...presets.references import (
    KnownNewsExplicitConfiguration,
    KnownNewsPresetReference,
    MarketProfileReference,
    PresetReferenceUsageMode,
    PresetRegistryScope,
    SessionExplicitConfiguration,
    SessionPresetReference,
)
from ...presets.registry import PresetCategory, PresetRegistry
from ...presets.resolution import ResolvedSessionComponent, ResolvedWorkflowComponentSource
from ...sessions._resolution import (
    SessionTemplate,
    build_calendar_registry_json_from_windows,
    build_recurring_closed_windows,
    looks_like_continuous_weekday_template,
    regular_session_windows_from_exchange_artifact,
)


def coerce_market_profile_reference(
    market_profile: Optional[str | MarketProfileReference],
) -> Optional[MarketProfileReference]:
    if market_profile is None:
        return None
    if isinstance(market_profile, MarketProfileReference):
        return market_profile
    if isinstance(market_profile, str):
        if not market_profile.strip():
            raise calibration_error("market_profile", "must be a non-empty string")
        return MarketProfileReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name=market_profile,
        )
    raise calibration_error("market_profile", "must be a preset name or MarketProfileReference")


def coerce_session_reference(
    session: Optional[str | Path | SessionPresetReference],
    *,
    exchange_code: Optional[str],
) -> Optional[SessionPresetReference]:
    if session is None:
        return None
    if isinstance(session, SessionPresetReference):
        return session
    if isinstance(session, str):
        if not session.strip():
            raise calibration_error("session", "must be a non-empty string")
        return SessionPresetReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name=session,
        )
    if isinstance(session, Path):
        return SessionPresetReference(
            usage_mode=PresetReferenceUsageMode.EXPLICIT,
            configuration=load_explicit_session_configuration(session, exchange_code=exchange_code),
        )
    raise calibration_error("session", "must be a preset name, Path, or SessionPresetReference")


def coerce_known_news_reference(
    known_news: Optional[str | KnownNewsPresetReference],
) -> Optional[KnownNewsPresetReference]:
    if known_news is None:
        return None
    if isinstance(known_news, KnownNewsPresetReference):
        return known_news
    if isinstance(known_news, str):
        if not known_news.strip():
            raise calibration_error("known_news", "must be a non-empty string")
        return KnownNewsPresetReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name=known_news,
        )
    raise calibration_error("known_news", "must be a preset name or KnownNewsPresetReference")


def load_explicit_session_configuration(
    session_definition_path: Path,
    *,
    exchange_code: Optional[str],
) -> SessionExplicitConfiguration:
    try:
        registry_json = session_definition_path.read_text(encoding="utf-8")
    except FileNotFoundError as error:
        raise calibration_error(
            "session",
            f"session definition path `{session_definition_path}` does not exist",
        ) from error
    except OSError as error:
        raise calibration_error(
            "session",
            f"session definition path `{session_definition_path}` could not be read: {error}",
        ) from error

    calendar_registry = CalendarRegistry(registry_json)
    registry_payload = calendar_registry._registry_payload
    raw_exchanges = registry_payload.get("exchanges")
    if not isinstance(raw_exchanges, list) or len(raw_exchanges) == 0:
        raise calibration_error("session", "session definition must contain at least one exchange entry")

    target_exchange_code = exchange_code
    if target_exchange_code is None:
        if len(raw_exchanges) != 1:
            raise calibration_error(
                "exchange_code",
                "is required when the explicit session definition contains more than one exchange entry",
            )
        target_exchange_code = cast(str, cast(dict[str, Any], raw_exchanges[0])["exchange_code"])

    exchange_artifact: Optional[dict[str, Any]] = None
    for raw_exchange in raw_exchanges:
        if isinstance(raw_exchange, dict) and raw_exchange.get("exchange_code") == target_exchange_code:
            exchange_artifact = cast(dict[str, Any], raw_exchange)
            break

    if exchange_artifact is None:
        raise calibration_error(
            "exchange_code",
            f"explicit session definition does not contain exchange `{target_exchange_code}`",
        )

    regular_session_windows = regular_session_windows_from_exchange_artifact(exchange_artifact)

    exchange_name = cast(str, exchange_artifact["exchange_name"])
    timezone_name = cast(str, exchange_artifact["timezone_name"])

    session_template = SessionTemplate(
        policy_id="high_level.explicit_session_definition.v1",
        exchange_name=exchange_name,
        timezone_name=timezone_name,
        timezone_source="session_definition_source",
        holiday_calendar_code=cast(str, cast(dict[str, Any], exchange_artifact["rule_metadata"])["holiday_calendar_code"]),
        early_close_rule_set=cast(dict[str, Any], exchange_artifact["rule_metadata"]).get("early_close_rule_set"),
        regular_session_windows=regular_session_windows,
        continuous_weekday_template=looks_like_continuous_weekday_template(
            exchange_code=target_exchange_code,
            regular_session_windows=regular_session_windows,
        ),
    )

    return SessionExplicitConfiguration(
        exchange_code=target_exchange_code,
        exchange_name=exchange_name,
        timezone_name=timezone_name,
        regular_session_windows=regular_session_windows,
        recurring_closed_windows=build_recurring_closed_windows(session_template),
    )


def exchange_code_from_session_reference(session_reference: SessionPresetReference, registry: PresetRegistry) -> str:
    match session_reference.usage_mode:
        case PresetReferenceUsageMode.AUTOMATIC:
            if session_reference.exchange_code_hint is None:
                raise calibration_error("exchange_code", "is required for automatic session inference")

            return session_reference.exchange_code_hint
        case PresetReferenceUsageMode.NAMED:
            if session_reference.preset_name is None or session_reference.registry_scope is None:
                raise calibration_error("session", "named session references require preset_name and registry_scope")

            _, artifact = registry.resolve(
                category=PresetCategory.SESSION,
                preset_name=session_reference.preset_name,
                registry_scope=session_reference.registry_scope,
            )

            return artifact.configuration.exchange_code
        case PresetReferenceUsageMode.EXPLICIT:
            if session_reference.configuration is None:
                raise calibration_error("session.configuration", "is required when usage_mode=explicit")

            return session_reference.configuration.exchange_code


def exchange_code_from_market_profile_reference(market_profile_reference: MarketProfileReference, registry: PresetRegistry) -> str:
    if market_profile_reference.usage_mode == PresetReferenceUsageMode.EXPLICIT:
        if market_profile_reference.configuration is None:
            raise calibration_error("market_profile.configuration", "is required when usage_mode=explicit")

        return exchange_code_from_session_reference(
            market_profile_reference.configuration.session_reference,
            registry,
        )

    if market_profile_reference.preset_name is None or market_profile_reference.registry_scope is None:
        raise calibration_error("market_profile", "named market-profile references require preset_name and registry_scope")

    _, artifact = registry.resolve(
        category=PresetCategory.MARKET_PROFILE,
        preset_name=market_profile_reference.preset_name,
        registry_scope=market_profile_reference.registry_scope,
    )

    return exchange_code_from_session_reference(artifact.configuration.session_reference, registry)


def build_session_configuration_input(
    resolved_session: ResolvedSessionComponent,
    *,
    working_directory: Optional[Path],
) -> CalibrationSessionConfigurationInput:
    if (
        resolved_session.source_kind == ResolvedWorkflowComponentSource.BUILT_IN
        and len(resolved_session.applied_override_fields) == 0
    ) or (
        len(resolved_session.applied_override_fields) == 0 and resolved_session.inferred_from_data
    ):
        return CalibrationSessionConfigurationInput(
            exchange_code=resolved_session.configuration.exchange_code,
            session_timezone=resolved_session.configuration.timezone_name,
        )

    if working_directory is None:
        raise calibration_error(
            "session_configuration",
            "resolved session materialization requires a working directory",
        )

    session_configuration = resolved_session.configuration

    session_definition_path = working_directory / "resolved-session-definition.json"
    session_definition_path.write_text(
        calendar_registry_json_from_session_configuration(session_configuration),
        encoding="utf-8",
    )

    return CalibrationSessionConfigurationInput(
        exchange_code=session_configuration.exchange_code,
        session_definition_source=CalibrationInputFileReference(
            path=str(session_definition_path),
            file_format=CalibrationInputFileFormat.JSON,
            provenance_label="resolved-session-configuration",
        ),
        session_timezone=session_configuration.timezone_name,
    )


def calendar_registry_json_from_session_configuration(session_configuration: SessionExplicitConfiguration) -> str:
    return build_calendar_registry_json_from_windows(
        exchange_code=session_configuration.exchange_code,
        exchange_name=session_configuration.exchange_name if session_configuration.exchange_name is not None else session_configuration.exchange_code,
        timezone_name=session_configuration.timezone_name,
        holiday_calendar_code="USER_DEFINED",
        early_close_rule_set=None,
        regular_session_windows=session_configuration.regular_session_windows,
        recurring_closed_windows=session_configuration.recurring_closed_windows,
    )


def build_known_news_input(
    known_news_configuration: KnownNewsExplicitConfiguration,
    *,
    working_directory: Optional[Path],
) -> Optional[CalibrationKnownNewsInput]:
    if len(known_news_configuration.events) == 0:
        return None
    if working_directory is None:
        raise calibration_error("known_news", "resolved known-news materialization requires a working directory")

    known_news_path = working_directory / "resolved-known-news.csv"
    rows = [
        "event_timestamp_utc,event_id",
        *[
            f"{event.scheduled_at.astimezone(UTC).isoformat()},{event.event_id}"
            for event in known_news_configuration.events
        ],
    ]
    known_news_path.write_text("\n".join(rows), encoding="utf-8")

    return CalibrationKnownNewsInput(
        calendar_source=CalibrationInputFileReference(
            path=str(known_news_path),
            file_format=CalibrationInputFileFormat.CSV,
            provenance_label="resolved-known-news-configuration",
        ),
        calendar_timezone=known_news_configuration.calendar_timezone_name or "UTC",
    )


def resolved_configuration_requires_materialization(resolved_configuration: Any) -> bool:
    if len(resolved_configuration.known_news.configuration.events) > 0:
        return True

    session_requires_materialization = not (
        len(resolved_configuration.session.applied_override_fields) == 0
        and (
            resolved_configuration.session.inferred_from_data
            or resolved_configuration.session.source_kind == ResolvedWorkflowComponentSource.BUILT_IN
        )
    )

    return session_requires_materialization
