import json
import sys
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, cast
from zoneinfo import ZoneInfo

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]

PYTHON_ROOT = REPOSITORY_ROOT / "python"
if str(PYTHON_ROOT) not in sys.path:
    sys.path.insert(0, str(PYTHON_ROOT))

from janiform.calibration.presets.artifacts import (
    KNOWN_NEWS_PRESET_ARTIFACT_SCHEMA_VERSION,
    MARKET_PROFILE_PRESET_ARTIFACT_SCHEMA_VERSION,
    SESSION_PRESET_ARTIFACT_SCHEMA_VERSION,
    KnownNewsPresetArtifact,
    MarketProfilePresetArtifact,
    PresetArtifactMetadata,
    PresetArtifactProvenance,
    PresetCategory,
    PresetUpstreamSourceVersion,
    SessionPresetArtifact,
    normalize_preset_artifact,
)
from janiform.calibration.presets.references import (
    KnownNewsEvent,
    KnownNewsExplicitConfiguration,
    KnownNewsPresetReference,
    MarketProfileColumnAliasHint,
    MarketProfileExplicitConfiguration,
    MarketProfileKnownNewsPolicy,
    PresetReferenceUsageMode,
    PresetRegistryScope,
    SessionExplicitConfiguration,
    SessionPresetReference,
)
from janiform.calibration.presets.registry import (
    BUNDLED_PRESET_INDEX_SCHEMA_VERSION,
    BundledPresetRegistryIndex,
    PresetIndexEntry,
)
from janiform.contracts import serialize_deterministic_json

SOURCE_ROOT = REPOSITORY_ROOT / "presets" / "sources"
GENERATED_ROOT = REPOSITORY_ROOT / "python" / "janiform" / "calibration" / "_bundled_presets" / "generated"


def main() -> None:
    session_source = _load_json(SOURCE_ROOT / "sessions" / "session_presets.json")
    known_news_source = _load_json(SOURCE_ROOT / "known_news" / "known_news_presets.json")
    market_profile_source = _load_json(SOURCE_ROOT / "market_profiles" / "market_profile_presets.json")

    session_artifacts = _build_session_artifacts(session_source)
    known_news_artifacts = _build_known_news_artifacts(known_news_source)
    market_profile_artifacts = _build_market_profile_artifacts(market_profile_source)

    bundle_id = _require_non_empty_string(session_source, "bundle_id")
    bundle_version = _require_non_empty_string(session_source, "bundle_version")
    generated_on = _require_non_empty_string(session_source, "generated_on")

    entries: list[PresetIndexEntry] = []
    entries.extend(_write_category_artifacts("sessions", session_artifacts, PresetCategory.SESSION))
    entries.extend(_write_category_artifacts("known_news", known_news_artifacts, PresetCategory.KNOWN_NEWS))
    entries.extend(_write_category_artifacts("market_profiles", market_profile_artifacts, PresetCategory.MARKET_PROFILE))

    index = BundledPresetRegistryIndex(
        schema_version=BUNDLED_PRESET_INDEX_SCHEMA_VERSION,
        bundle_id=bundle_id,
        bundle_version=bundle_version,
        generated_on=generated_on,
        entries=tuple(
            sorted(
                entries,
                key=_preset_index_entry_sort_key,
            )
        ),
    )

    (GENERATED_ROOT / "index.json").write_text(
        _serialize_pretty_json(index),
        encoding="utf-8",
    )


def _preset_index_entry_sort_key(entry: PresetIndexEntry) -> tuple[int, str]:
    return _preset_category_sort_rank(entry.category), entry.preset_name


def _preset_category_sort_rank(category: PresetCategory) -> int:
    match category:
        case PresetCategory.MARKET_PROFILE:
            return 0
        case PresetCategory.KNOWN_NEWS:
            return 1
        case PresetCategory.SESSION:
            return 2
        case _:
            raise ValueError(f"unsupported preset category: {category}")


def _build_session_artifacts(source_document: dict[str, Any]) -> tuple[SessionPresetArtifact, ...]:
    root_metadata = _root_metadata(source_document)
    artifacts: list[SessionPresetArtifact] = []

    for entry in _require_sequence(source_document, "presets"):
        if not isinstance(entry, dict):
            raise TypeError("session preset source entry must be an object")

        configuration = SessionExplicitConfiguration.from_mapping(
            _require_mapping(entry, "configuration"),
            field_path="configuration",
        )

        artifacts.append(
            normalize_preset_artifact(
                SessionPresetArtifact(
                    schema_version=SESSION_PRESET_ARTIFACT_SCHEMA_VERSION,
                    category=PresetCategory.SESSION,
                    preset_name=_require_non_empty_string(entry, "preset_name"),
                    metadata=_build_metadata(entry, root_metadata),
                    configuration=configuration,
                )
            )
        )

    return tuple(artifacts)


def _build_known_news_artifacts(source_document: dict[str, Any]) -> tuple[KnownNewsPresetArtifact, ...]:
    root_metadata = _root_metadata(source_document)
    artifacts: list[KnownNewsPresetArtifact] = []

    for entry in _require_sequence(source_document, "presets"):
        if not isinstance(entry, dict):
            raise TypeError("known-news preset source entry must be an object")

        configuration_source = _require_mapping(entry, "configuration")
        raw_events = _require_sequence(configuration_source, "events")

        configuration = KnownNewsExplicitConfiguration(
            calendar_timezone_name=configuration_source.get("calendar_timezone_name"),
            events=tuple(_build_known_news_event(raw_event) for raw_event in raw_events),
        )

        artifacts.append(
            normalize_preset_artifact(
                KnownNewsPresetArtifact(
                    schema_version=KNOWN_NEWS_PRESET_ARTIFACT_SCHEMA_VERSION,
                    category=PresetCategory.KNOWN_NEWS,
                    preset_name=_require_non_empty_string(entry, "preset_name"),
                    metadata=_build_metadata(entry, root_metadata),
                    configuration=configuration,
                )
            )
        )

    return tuple(artifacts)


def _build_market_profile_artifacts(
    source_document: dict[str, Any],
) -> tuple[MarketProfilePresetArtifact, ...]:
    root_metadata = _root_metadata(source_document)

    artifacts: list[MarketProfilePresetArtifact] = []
    for entry in _require_sequence(source_document, "presets"):
        if not isinstance(entry, dict):
            raise TypeError("market-profile preset source entry must be an object")

        configuration_source = _require_mapping(entry, "configuration")

        known_news_preset_name = configuration_source.get("known_news_preset_name")
        if known_news_preset_name is not None and not isinstance(known_news_preset_name, str):
            raise TypeError("market-profile known_news_preset_name must be a string when provided")

        configuration = MarketProfileExplicitConfiguration(
            profile_name=_require_non_empty_string(configuration_source, "profile_name"),
            session_reference=SessionPresetReference(
                usage_mode=PresetReferenceUsageMode.NAMED,
                registry_scope=PresetRegistryScope.BUILT_IN,
                preset_name=_require_non_empty_string(configuration_source, "session_preset_name"),
            ),
            known_news_reference=(
                None
                if known_news_preset_name is None
                else KnownNewsPresetReference(
                    usage_mode=PresetReferenceUsageMode.NAMED,
                    registry_scope=PresetRegistryScope.BUILT_IN,
                    preset_name=known_news_preset_name,
                )
            ),
            known_news_policy=MarketProfileKnownNewsPolicy(
                _require_non_empty_string(configuration_source, "known_news_policy")
            ),
            timestamp_timezone_name=configuration_source.get("timestamp_timezone_name"),
            tick_size=configuration_source.get("tick_size"),
            column_alias_hints=tuple(
                MarketProfileColumnAliasHint.from_mapping(raw_hint, field_path="column_alias_hints")
                for raw_hint in configuration_source.get("column_alias_hints", ())
            ),
            default_calibration_hint_ids=tuple(configuration_source.get("default_calibration_hint_ids", ())),
        )

        artifacts.append(
            normalize_preset_artifact(
                MarketProfilePresetArtifact(
                    schema_version=MARKET_PROFILE_PRESET_ARTIFACT_SCHEMA_VERSION,
                    category=PresetCategory.MARKET_PROFILE,
                    preset_name=_require_non_empty_string(entry, "preset_name"),
                    metadata=_build_metadata(entry, root_metadata),
                    configuration=configuration,
                )
            )
        )

    return tuple(artifacts)


def _build_known_news_event(raw_event: object) -> KnownNewsEvent:
    if not isinstance(raw_event, dict):
        raise TypeError("known-news source event must be an object")

    scheduled_local = _require_non_empty_string(raw_event, "scheduled_local")
    timezone_name = _require_non_empty_string(raw_event, "timezone_name")

    localized_timestamp = datetime.fromisoformat(scheduled_local).replace(tzinfo=ZoneInfo(timezone_name))
    raw_tags = raw_event.get("tags", ())

    if isinstance(raw_tags, str) or not isinstance(raw_tags, list | tuple):
        raise TypeError("known-news source tags must be a sequence when provided")

    return KnownNewsEvent(
        event_id=_require_non_empty_string(raw_event, "event_id"),
        event_name=raw_event.get("event_name"),
        scheduled_at=localized_timestamp.astimezone(UTC),
        tags=tuple(_require_non_empty_string_value(tag) for tag in raw_tags),
    )


def _build_metadata(
    entry: dict[str, Any],
    root_metadata: dict[str, str],
) -> PresetArtifactMetadata:
    raw_source_manifest_ids = _require_sequence(entry, "source_manifest_ids")
    raw_upstream_source_versions = entry.get("upstream_source_versions", ())
    if isinstance(raw_upstream_source_versions, str) or not isinstance(raw_upstream_source_versions, list | tuple):
        raise TypeError("upstream_source_versions must be a sequence when provided")

    return PresetArtifactMetadata(
        display_name=_require_non_empty_string(entry, "display_name"),
        description=_require_non_empty_string(entry, "description"),
        version_label=_require_non_empty_string(entry, "version_label"),
        provenance=PresetArtifactProvenance(
            bundle_id=root_metadata["bundle_id"],
            bundle_version=root_metadata["bundle_version"],
            generator_id=root_metadata["generator_id"],
            generator_version=root_metadata["generator_version"],
            generated_on=root_metadata["generated_on"],
            source_manifest_ids=tuple(
                _require_non_empty_string_value(raw_value)
                for raw_value in raw_source_manifest_ids
            ),
            upstream_source_versions=tuple(
                PresetUpstreamSourceVersion(
                    source_name=_require_non_empty_string(cast(dict[str, Any], raw_source), "source_name"),
                    version_label=_require_non_empty_string(cast(dict[str, Any], raw_source), "version_label"),
                )
                for raw_source in raw_upstream_source_versions
            ),
        ),
    )


def _root_metadata(source_document: dict[str, Any]) -> dict[str, str]:
    return {
        "bundle_id": _require_non_empty_string(source_document, "bundle_id"),
        "bundle_version": _require_non_empty_string(source_document, "bundle_version"),
        "generator_id": _require_non_empty_string(source_document, "generator_id"),
        "generator_version": _require_non_empty_string(source_document, "generator_version"),
        "generated_on": _require_non_empty_string(source_document, "generated_on"),
    }


def _write_category_artifacts(
    category_directory_name: str,
    artifacts: tuple[SessionPresetArtifact | KnownNewsPresetArtifact | MarketProfilePresetArtifact, ...],
    category: PresetCategory,
) -> tuple[PresetIndexEntry, ...]:
    category_directory = GENERATED_ROOT / category_directory_name
    category_directory.mkdir(parents=True, exist_ok=True)

    entries: list[PresetIndexEntry] = []
    for artifact in sorted(artifacts, key=lambda item: item.preset_name):
        artifact_relative_path = f"{category_directory_name}/{artifact.preset_name}.json"
        (GENERATED_ROOT / artifact_relative_path).write_text(
            _serialize_pretty_json(artifact),
            encoding="utf-8",
        )

        entries.append(
            PresetIndexEntry(
                category=category,
                preset_name=artifact.preset_name,
                display_name=artifact.metadata.display_name,
                description=artifact.metadata.description,
                version_label=artifact.metadata.version_label,
                registry_scope=PresetRegistryScope.BUILT_IN,
                content_hash=artifact.metadata.provenance.content_hash or "",
                artifact_path=artifact_relative_path,
            )
        )

    return tuple(entries)


def _load_json(path: Path) -> dict[str, Any]:
    document = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(document, dict):
        raise TypeError(f"{path} must decode to an object")

    return document


def _require_mapping(mapping: dict[str, Any], field_name: str) -> dict[str, Any]:
    raw_value = mapping[field_name]
    if not isinstance(raw_value, dict):
        raise TypeError(f"{field_name} must be an object")

    return raw_value


def _require_sequence(mapping: dict[str, Any], field_name: str) -> list[Any] | tuple[Any, ...]:
    raw_value = mapping[field_name]
    if isinstance(raw_value, str) or not isinstance(raw_value, list | tuple):
        raise TypeError(f"{field_name} must be a sequence")

    return raw_value


def _require_non_empty_string(mapping: dict[str, Any], field_name: str) -> str:
    raw_value = mapping[field_name]
    if not isinstance(raw_value, str) or not raw_value.strip():
        raise TypeError(f"{field_name} must be a non-empty string")

    return raw_value


def _require_non_empty_string_value(raw_value: object) -> str:
    if not isinstance(raw_value, str) or not raw_value.strip():
        raise TypeError("expected a non-empty string value")

    return raw_value


def _serialize_pretty_json(value: object) -> str:
    return json.dumps(
        json.loads(serialize_deterministic_json(value)),
        indent=2,
        sort_keys=True,
    ) + "\n"


if __name__ == "__main__":
    main()
