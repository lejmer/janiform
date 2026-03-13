import json
from datetime import UTC, datetime

import pytest

from janiform.calibration import CalibrationValidationError
from janiform.calibration.presets import (
    KnownNewsConfigurationOverridePatch,
    KnownNewsEvent,
    KnownNewsExplicitConfiguration,
    KnownNewsPresetReference,
    MarketProfileColumnAliasHint,
    MarketProfileExplicitConfiguration,
    MarketProfileKnownNewsPolicy,
    MarketProfileOverridePatch,
    MarketProfileReference,
    PresetReferenceUsageMode,
    PresetRegistryScope,
    SessionConfigurationOverridePatch,
    SessionExplicitConfiguration,
    SessionPresetReference,
    deserialize_known_news_preset_reference,
    deserialize_market_profile_reference,
    deserialize_session_preset_reference,
    serialize_known_news_preset_reference,
    serialize_market_profile_reference,
    serialize_session_preset_reference,
    validate_known_news_preset_reference,
    validate_market_profile_reference,
    validate_session_preset_reference,
)
from janiform.calibration.sessions import (
    MarketDataSessionRecurringClosedWindow,
    MarketDataSessionRegularWindow,
)


def test_session_reference_supports_automatic_inference_with_override_patch() -> None:
    reference = validate_session_preset_reference(
        {
            "usage_mode": "automatic",
            "exchange_code_hint": "XNYS",
            "override_patch": {
                "timezone_name": "America/New_York",
            },
        }
    )

    assert reference == SessionPresetReference(
        usage_mode=PresetReferenceUsageMode.AUTOMATIC,
        exchange_code_hint="XNYS",
        override_patch=SessionConfigurationOverridePatch(
            timezone_name="America/New_York",
        ),
    )


def test_session_named_reference_roundtrip_serialization_is_deterministic() -> None:
    reference = SessionPresetReference(
        usage_mode=PresetReferenceUsageMode.NAMED,
        registry_scope=PresetRegistryScope.BUILT_IN,
        preset_name="XNYS",
        override_patch=SessionConfigurationOverridePatch(
            recurring_closed_windows=(
                MarketDataSessionRecurringClosedWindow(
                    start_weekday="Thursday",
                    start_time="13:00",
                    end_weekday="Thursday",
                    end_time="14:00",
                    reason="half_day_override",
                ),
            ),
        ),
    )

    serialized_reference = serialize_session_preset_reference(reference)
    deserialized_reference = deserialize_session_preset_reference(serialized_reference)

    assert deserialized_reference == reference
    assert serialized_reference == serialize_session_preset_reference(reference)
    assert json.loads(serialized_reference)["usage_mode"] == "named"


def test_session_explicit_configuration_roundtrip_serialization_is_deterministic() -> None:
    reference = SessionPresetReference(
        usage_mode=PresetReferenceUsageMode.EXPLICIT,
        configuration=SessionExplicitConfiguration(
            exchange_code="XLON",
            exchange_name="London Stock Exchange",
            timezone_name="Europe/London",
            regular_session_windows=(
                MarketDataSessionRegularWindow(
                    weekday="Monday",
                    label="cash",
                    open_time="08:00",
                    close_time="16:30",
                ),
            ),
        ),
    )

    serialized_reference = serialize_session_preset_reference(reference)

    assert deserialize_session_preset_reference(serialized_reference) == reference
    assert json.loads(serialized_reference)["configuration"]["exchange_code"] == "XLON"


def test_session_reference_rejects_named_usage_without_registry_scope() -> None:
    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_session_preset_reference(
            {
                "usage_mode": "named",
                "preset_name": "XNYS",
            }
        )

    assert captured_error.value.failure.field_path == "registry_scope"
    assert "built_in or user_registered" in captured_error.value.failure.message


def test_session_reference_rejects_explicit_configuration_with_override_patch() -> None:
    with pytest.raises(CalibrationValidationError) as captured_error:
        SessionPresetReference(
            usage_mode=PresetReferenceUsageMode.EXPLICIT,
            configuration=SessionExplicitConfiguration(
                exchange_code="XNYS",
                timezone_name="America/New_York",
                regular_session_windows=(
                    MarketDataSessionRegularWindow(
                        weekday="Monday",
                        label="regular",
                        open_time="09:30",
                        close_time="16:00",
                    ),
                ),
            ),
            override_patch=SessionConfigurationOverridePatch(timezone_name="UTC"),
        )

    assert captured_error.value.failure.field_path == "override_patch"
    assert "full replacement" in captured_error.value.failure.message


def test_known_news_named_reference_with_override_patch_roundtrips() -> None:
    reference = KnownNewsPresetReference(
        usage_mode=PresetReferenceUsageMode.NAMED,
        registry_scope=PresetRegistryScope.USER_REGISTERED,
        preset_name="desk_macro_watch",
        override_patch=KnownNewsConfigurationOverridePatch(
            additional_events=(
                KnownNewsEvent(
                    event_id="cpi_2026_04",
                    event_name="US CPI",
                    scheduled_at=datetime(2026, 4, 10, 12, 30, tzinfo=UTC),
                    tags=("macro", "cpi"),
                ),
            ),
        ),
    )

    serialized_reference = serialize_known_news_preset_reference(reference)

    assert deserialize_known_news_preset_reference(serialized_reference) == reference
    assert json.loads(serialized_reference)["preset_name"] == "desk_macro_watch"


def test_known_news_explicit_configuration_roundtrips() -> None:
    reference = KnownNewsPresetReference(
        usage_mode=PresetReferenceUsageMode.EXPLICIT,
        configuration=KnownNewsExplicitConfiguration(
            calendar_timezone_name="UTC",
            events=(
                KnownNewsEvent(
                    event_id="nfp_2026_03",
                    event_name="US Nonfarm Payrolls",
                    scheduled_at=datetime(2026, 3, 6, 13, 30, tzinfo=UTC),
                    tags=("macro",),
                ),
            ),
        ),
    )

    serialized_reference = serialize_known_news_preset_reference(reference)

    assert deserialize_known_news_preset_reference(serialized_reference) == reference
    assert json.loads(serialized_reference)["configuration"]["calendar_timezone_name"] == "UTC"


def test_known_news_reference_rejects_automatic_usage_mode() -> None:
    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_known_news_preset_reference(
            {
                "usage_mode": "automatic",
            }
        )

    assert captured_error.value.failure.field_path == "known_news_reference.usage_mode"
    assert captured_error.value.failure.message == "must be one of: named, explicit"


def test_known_news_override_patch_rejects_add_and_remove_same_event_id() -> None:
    with pytest.raises(CalibrationValidationError) as captured_error:
        KnownNewsConfigurationOverridePatch(
            additional_events=(
                KnownNewsEvent(
                    event_id="fomc_2026_03",
                    scheduled_at=datetime(2026, 3, 18, 18, 0, tzinfo=UTC),
                ),
            ),
            removed_event_ids=("fomc_2026_03",),
        )

    assert captured_error.value.failure.field_path == "override_patch"
    assert "same event_id" in captured_error.value.failure.message


def test_market_profile_explicit_configuration_roundtrip_serialization_is_deterministic() -> None:
    reference = MarketProfileReference(
        usage_mode=PresetReferenceUsageMode.EXPLICIT,
        configuration=MarketProfileExplicitConfiguration(
            profile_name="eurusd_spot_custom",
            session_reference=SessionPresetReference(
                usage_mode=PresetReferenceUsageMode.NAMED,
                registry_scope=PresetRegistryScope.BUILT_IN,
                preset_name="fx_spot_24_5",
            ),
            known_news_reference=KnownNewsPresetReference(
                usage_mode=PresetReferenceUsageMode.NAMED,
                registry_scope=PresetRegistryScope.BUILT_IN,
                preset_name="eu_us_macro_fx_core",
            ),
            known_news_policy=MarketProfileKnownNewsPolicy.SUGGEST_ONLY,
            timestamp_timezone_name="UTC",
            column_alias_hints=(
                MarketProfileColumnAliasHint(
                    canonical_name="timestamp",
                    aliases=("event_timestamp_utc", "ts_utc"),
                ),
            ),
            default_calibration_hint_ids=("fx_defaults",),
        ),
    )

    serialized_reference = serialize_market_profile_reference(reference)

    assert deserialize_market_profile_reference(serialized_reference) == reference
    assert json.loads(serialized_reference)["configuration"]["profile_name"] == "eurusd_spot_custom"


def test_market_profile_override_patch_rejects_policy_without_known_news_reference() -> None:
    with pytest.raises(CalibrationValidationError) as captured_error:
        MarketProfileOverridePatch(
            known_news_policy=MarketProfileKnownNewsPolicy.APPLY_BY_DEFAULT,
        )

    assert captured_error.value.failure.field_path == "known_news_policy"
    assert "ambiguous" in captured_error.value.failure.message


def test_market_profile_explicit_configuration_rejects_auto_apply_of_none_known_news() -> None:
    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_market_profile_reference(
            {
                "usage_mode": "explicit",
                "configuration": {
                    "profile_name": "us_equity_cash_custom",
                    "session_reference": {
                        "usage_mode": "named",
                        "registry_scope": "built_in",
                        "preset_name": "XNYS",
                    },
                    "known_news_reference": {
                        "usage_mode": "named",
                        "registry_scope": "built_in",
                        "preset_name": "none",
                    },
                    "known_news_policy": "apply_by_default",
                },
            }
        )

    assert captured_error.value.failure.field_path == "known_news_reference"
    assert "cannot auto-apply" in captured_error.value.failure.message
