import json
from datetime import UTC, datetime
from pathlib import Path

import pytest

from janiform.calibration import CalibrationValidationError
from janiform.calibration.presets import (
    KnownNewsEvent,
    KnownNewsExplicitConfiguration,
    KnownNewsPresetArtifact,
    KnownNewsPresetReference,
    MarketProfileExplicitConfiguration,
    MarketProfilePresetArtifact,
    PresetArtifactMetadata,
    PresetArtifactProvenance,
    PresetCategory,
    PresetReferenceUsageMode,
    PresetRegistry,
    PresetRegistryScope,
    PresetUpstreamSourceVersion,
    RESOLVED_PRESET_SNAPSHOT_SCHEMA_VERSION,
    SESSION_PRESET_ARTIFACT_SCHEMA_VERSION,
    SessionExplicitConfiguration,
    SessionPresetArtifact,
    SessionPresetReference,
    deserialize_preset_artifact,
    serialize_preset_artifact,
)
from janiform.calibration.sessions import (
    MarketDataSessionRecurringClosedWindow,
    MarketDataSessionRegularWindow,
)
from janiform.contracts import serialize_deterministic_json

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
BUNDLED_PRESET_ROOT = (
    REPOSITORY_ROOT / "python" / "janiform" / "calibration" / "_bundled_presets" / "generated"
)


def test_bundled_registry_loads_minimal_v1_preset_set_and_fx_session_hours() -> None:
    registry = PresetRegistry()

    assert {entry.preset_name for entry in registry.list_presets(category=PresetCategory.SESSION)} == {
        "XCME",
        "XNAS",
        "XNYS",
        "crypto_24_7",
        "fx_spot_24_5",
    }
    assert {entry.preset_name for entry in registry.list_presets(category=PresetCategory.KNOWN_NEWS)} == {
        "eu_us_macro_fx_core",
        "none",
        "us_macro_core",
    }
    assert {
        entry.preset_name for entry in registry.list_presets(category=PresetCategory.MARKET_PROFILE)
    } == {"btcusd_spot", "es_futures_globex", "eurusd_spot", "us_equity_cash"}

    _, fx_session = registry.resolve_session(preset_name="fx_spot_24_5")

    assert fx_session.configuration.timezone_name == "UTC"
    assert fx_session.configuration.recurring_closed_windows == (
        MarketDataSessionRecurringClosedWindow(
            start_weekday="Friday",
            start_time="19:00",
            end_weekday="Sunday",
            end_time="21:00",
            reason="weekend_close",
        ),
    )


def test_bundled_registry_runtime_artifacts_match_checked_in_snapshots() -> None:
    registry = PresetRegistry()
    expected_index = json.loads((BUNDLED_PRESET_ROOT / "index.json").read_text(encoding="utf-8"))
    observed_entries = [
        entry.to_payload()
        for entry in registry.list_presets(include_user_registered=False)
    ]

    assert observed_entries == expected_index["entries"]

    for entry in registry.list_presets(include_user_registered=False):
        match entry.category:
            case PresetCategory.SESSION:
                _, artifact = registry.resolve_session(
                    preset_name=entry.preset_name,
                    registry_scope=PresetRegistryScope.BUILT_IN,
                )
            case PresetCategory.KNOWN_NEWS:
                _, artifact = registry.resolve_known_news(
                    preset_name=entry.preset_name,
                    registry_scope=PresetRegistryScope.BUILT_IN,
                )
            case PresetCategory.MARKET_PROFILE:
                _, artifact = registry.resolve_market_profile(
                    preset_name=entry.preset_name,
                    registry_scope=PresetRegistryScope.BUILT_IN,
                )
        artifact_path = entry.artifact_path

        assert artifact_path is not None
        assert json.loads(serialize_preset_artifact(artifact)) == json.loads(
            (BUNDLED_PRESET_ROOT / artifact_path).read_text(encoding="utf-8")
        )


def test_user_registry_can_shadow_built_in_name_without_mutating_built_in_store() -> None:
    registry = PresetRegistry()
    user_session = _build_user_session_artifact(preset_name="XNYS", display_name="Desk XNYS")

    registry.register_user_preset(user_session)

    resolved_scope, resolved_session = registry.resolve_session(
        preset_name="XNYS",
    )
    _, bundled_session = registry.resolve_session(
        preset_name="XNYS",
        registry_scope=PresetRegistryScope.BUILT_IN,
    )

    assert resolved_scope == PresetRegistryScope.USER_REGISTERED
    assert resolved_session.metadata.display_name == "Desk XNYS"
    assert bundled_session.metadata.display_name == "New York Stock Exchange Cash Session"

    with pytest.raises(CalibrationValidationError) as captured_error:
        registry.register_user_preset(user_session)

    assert captured_error.value.failure.field_path == "registry.user_registered_presets[1]"
    assert "duplicate" in captured_error.value.failure.message


def test_user_preset_artifact_load_normalizes_order_and_exports_deterministically() -> None:
    registry = PresetRegistry()
    user_known_news = KnownNewsPresetArtifact(
        schema_version="preset.artifact.known_news.v1",
        category=PresetCategory.KNOWN_NEWS,
        preset_name="desk_macro",
        metadata=_build_metadata(display_name="Desk Macro"),
        configuration=KnownNewsExplicitConfiguration(
            calendar_timezone_name="UTC",
            events=(
                KnownNewsEvent(
                    event_id="late_event",
                    event_name="Late Event",
                    scheduled_at=datetime(2026, 4, 10, 12, 30, tzinfo=UTC),
                    tags=("macro", "desk"),
                ),
                KnownNewsEvent(
                    event_id="early_event",
                    event_name="Early Event",
                    scheduled_at=datetime(2026, 4, 1, 11, 0, tzinfo=UTC),
                    tags=("desk", "alpha"),
                ),
            ),
        ),
    )

    loaded_artifact = registry.register_user_preset(deserialize_preset_artifact(user_known_news.to_json()))
    first_export = serialize_deterministic_json(
        registry.resolve_preset_snapshot(
            category=PresetCategory.KNOWN_NEWS,
            preset_name="desk_macro",
            registry_scope=PresetRegistryScope.USER_REGISTERED,
        )
    )
    second_export = serialize_deterministic_json(
        registry.resolve_preset_snapshot(
            category=PresetCategory.KNOWN_NEWS,
            preset_name="desk_macro",
            registry_scope=PresetRegistryScope.USER_REGISTERED,
        )
    )

    assert first_export == second_export
    assert serialize_preset_artifact(loaded_artifact) == serialize_preset_artifact(
        registry.resolve_known_news(
            preset_name="desk_macro",
            registry_scope=PresetRegistryScope.USER_REGISTERED,
        )[1]
    )
    assert loaded_artifact.metadata.provenance.content_hash is not None
    assert loaded_artifact.configuration.events[0].event_id == "early_event"


def test_user_preset_artifact_rejects_malformed_payload_and_dangling_market_profile_reference() -> None:
    registry = PresetRegistry()

    with pytest.raises(CalibrationValidationError) as malformed_error:
        registry.register_user_preset(deserialize_preset_artifact("[]"))

    assert malformed_error.value.failure.field_path == "preset artifact"
    assert "JSON object" in malformed_error.value.failure.message

    dangling_profile = MarketProfilePresetArtifact(
        schema_version="preset.artifact.market_profile.v1",
        category=PresetCategory.MARKET_PROFILE,
        preset_name="desk_fx_profile",
        metadata=_build_metadata(display_name="Desk FX Profile"),
        configuration=MarketProfileExplicitConfiguration(
            profile_name="desk_fx_profile",
            session_reference=SessionPresetReference(
                usage_mode=PresetReferenceUsageMode.NAMED,
                registry_scope=PresetRegistryScope.BUILT_IN,
                preset_name="fx_spot_24_5",
            ),
            known_news_reference=KnownNewsPresetReference(
                usage_mode=PresetReferenceUsageMode.NAMED,
                registry_scope=PresetRegistryScope.BUILT_IN,
                preset_name="none",
            ),
        ),
    )

    dangling_profile_payload = json.loads(dangling_profile.to_json())
    dangling_profile_payload["configuration"]["known_news_reference"] = {
        "usage_mode": "named",
        "registry_scope": "built_in",
        "preset_name": "missing_news",
    }

    registry.register_user_preset(
        deserialize_preset_artifact(json.dumps(dangling_profile_payload, sort_keys=True))
    )

    with pytest.raises(CalibrationValidationError) as dangling_error:
        registry.resolve_preset_snapshot(
            category=PresetCategory.MARKET_PROFILE,
            preset_name="desk_fx_profile",
            registry_scope=PresetRegistryScope.USER_REGISTERED,
        )

    assert dangling_error.value.failure.field_path == "preset_resolution"
    assert "missing_news" in dangling_error.value.failure.message


def test_market_profile_snapshot_includes_nested_session_and_known_news_artifacts() -> None:
    registry = PresetRegistry()
    snapshot_payload = json.loads(
        serialize_deterministic_json(
            registry.resolve_preset_snapshot(
                category=PresetCategory.MARKET_PROFILE,
                preset_name="eurusd_spot",
            )
        )
    )

    assert snapshot_payload["schema_version"] == RESOLVED_PRESET_SNAPSHOT_SCHEMA_VERSION
    assert snapshot_payload["requested_scope"] == "built_in"
    assert snapshot_payload["market_profile_preset"]["preset_name"] == "eurusd_spot"
    assert snapshot_payload["session_preset"]["preset_name"] == "fx_spot_24_5"
    assert snapshot_payload["known_news_preset"]["preset_name"] == "eu_us_macro_fx_core"


def _build_metadata(*, display_name: str) -> PresetArtifactMetadata:
    return PresetArtifactMetadata(
        display_name=display_name,
        description=f"{display_name} description",
        version_label="user.v1",
        provenance=PresetArtifactProvenance(
            bundle_id="user_presets",
            bundle_version="desk",
            generator_id="manual_test",
            generator_version="1.0.0",
            generated_on="2026-03-12",
            source_manifest_ids=("tests.user_preset",),
            upstream_source_versions=(
                PresetUpstreamSourceVersion(
                    source_name="manual_input",
                    version_label="test",
                ),
            ),
        ),
    )


def _build_user_session_artifact(*, preset_name: str, display_name: str) -> SessionPresetArtifact:
    return SessionPresetArtifact(
        schema_version=SESSION_PRESET_ARTIFACT_SCHEMA_VERSION,
        category=PresetCategory.SESSION,
        preset_name=preset_name,
        metadata=_build_metadata(display_name=display_name),
        configuration=SessionExplicitConfiguration(
            exchange_code="XNYS",
            exchange_name="Desk New York Session",
            timezone_name="America/New_York",
            regular_session_windows=(
                MarketDataSessionRegularWindow(
                    weekday="Friday",
                    label="cash",
                    open_time="09:30",
                    close_time="16:00",
                ),
                MarketDataSessionRegularWindow(
                    weekday="Monday",
                    label="cash",
                    open_time="09:30",
                    close_time="16:00",
                ),
            ),
        ),
    )
