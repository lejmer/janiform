from __future__ import annotations

import hashlib
from collections.abc import Mapping
from dataclasses import dataclass, replace
from enum import StrEnum
from importlib.resources import files
from typing import Any, Optional, cast

from .references import (
    KnownNewsConfigurationOverridePatch,
    KnownNewsExplicitConfiguration,
    KnownNewsPresetReference,
    MarketProfileExplicitConfiguration,
    MarketProfileOverridePatch,
    PresetReferenceUsageMode,
    PresetRegistryScope,
    SessionConfigurationOverridePatch,
    SessionExplicitConfiguration,
    SessionPresetReference,
)
from ..errors import calibration_error
from ...contracts.base import (
    ContractModel,
    parse_json_object,
    serialize_deterministic_json,
    require_field,
    require_non_empty_string,
    require_sequence,
)


SESSION_PRESET_ARTIFACT_SCHEMA_VERSION = "preset.artifact.session.v1"
KNOWN_NEWS_PRESET_ARTIFACT_SCHEMA_VERSION = "preset.artifact.known_news.v1"
MARKET_PROFILE_PRESET_ARTIFACT_SCHEMA_VERSION = "preset.artifact.market_profile.v1"
BUNDLED_PRESET_INDEX_SCHEMA_VERSION = "preset.runtime.index.v1"
RESOLVED_PRESET_EXPORT_SCHEMA_VERSION = "preset.runtime.export.v1"
PRESET_CONTENT_HASH_ALGORITHM = "sha256"

_BUNDLED_PRESET_RESOURCE_PACKAGE = "janiform.calibration._bundled_presets.generated"
_BUNDLED_PRESET_INDEX_RESOURCE_NAME = "index.json"
_WEEKDAY_ORDER: tuple[str, ...] = (
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
)
_WEEKDAY_ORDER_INDEX: dict[str, int] = {
    weekday_name: weekday_index
    for weekday_index, weekday_name in enumerate(_WEEKDAY_ORDER)
}


class PresetCategory(StrEnum):
    SESSION = 'session'
    KNOWN_NEWS = 'known_news'
    MARKET_PROFILE = 'market_profile'


@dataclass(frozen=True, slots=True)
class PresetUpstreamSourceVersion(ContractModel):
    source_name: str
    version_label: str

    def __post_init__(self) -> None:
        if not self.source_name.strip():
            raise _preset_registry_error("source_name", "must be a non-empty string")
        if not self.version_label.strip():
            raise _preset_registry_error("version_label", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> PresetUpstreamSourceVersion:
        if not isinstance(raw_payload, Mapping):
            raise _preset_registry_error(field_path, "must be an object")

        return PresetUpstreamSourceVersion(
            source_name=require_non_empty_string(
                raw_payload,
                "source_name",
                f"{field_path}.source_name",
                calibration_error,
            ),
            version_label=require_non_empty_string(
                raw_payload,
                "version_label",
                f"{field_path}.version_label",
                calibration_error,
            ),
        )


@dataclass(frozen=True, slots=True)
class PresetArtifactProvenance(ContractModel):
    bundle_id: str
    bundle_version: str
    generator_id: str
    generator_version: str
    generated_on: str
    source_manifest_ids: tuple[str, ...]
    upstream_source_versions: tuple[PresetUpstreamSourceVersion, ...] = ()
    content_hash: Optional[str] = None

    def __post_init__(self) -> None:
        for field_name, field_value in (
            ("bundle_id", self.bundle_id),
            ("bundle_version", self.bundle_version),
            ("generator_id", self.generator_id),
            ("generator_version", self.generator_version),
            ("generated_on", self.generated_on),
        ):
            if not field_value.strip():
                raise _preset_registry_error(field_name, "must be a non-empty string")

        _validate_non_empty_strings(
            values=self.source_manifest_ids,
            field_path="source_manifest_ids",
        )
        _validate_unique_strings(
            values=self.source_manifest_ids,
            field_path="source_manifest_ids",
            duplicate_message="must not contain duplicate source manifest identifiers",
        )

        if self.content_hash is not None and not _is_content_hash(self.content_hash):
            raise _preset_registry_error(
                "content_hash",
                f"must be a {PRESET_CONTENT_HASH_ALGORITHM}:... content hash when provided",
            )

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> PresetArtifactProvenance:
        if not isinstance(raw_payload, Mapping):
            raise _preset_registry_error(field_path, "must be an object")

        raw_source_manifest_ids = require_sequence(
            raw_payload,
            "source_manifest_ids",
            f"{field_path}.source_manifest_ids",
            calibration_error,
        )
        raw_upstream_source_versions = raw_payload.get("upstream_source_versions", ())

        if isinstance(raw_upstream_source_versions, str) or not isinstance(
            raw_upstream_source_versions,
            list | tuple,
        ):
            raise _preset_registry_error(
                f"{field_path}.upstream_source_versions",
                "must be a sequence when provided",
            )

        raw_content_hash = raw_payload.get("content_hash")
        if raw_content_hash is not None and not isinstance(raw_content_hash, str):
            raise _preset_registry_error(
                f"{field_path}.content_hash",
                "must be a string when provided",
            )

        return PresetArtifactProvenance(
            bundle_id=require_non_empty_string(
                raw_payload,
                "bundle_id",
                f"{field_path}.bundle_id",
                calibration_error,
            ),
            bundle_version=require_non_empty_string(
                raw_payload,
                "bundle_version",
                f"{field_path}.bundle_version",
                calibration_error,
            ),
            generator_id=require_non_empty_string(
                raw_payload,
                "generator_id",
                f"{field_path}.generator_id",
                calibration_error,
            ),
            generator_version=require_non_empty_string(
                raw_payload,
                "generator_version",
                f"{field_path}.generator_version",
                calibration_error,
            ),
            generated_on=require_non_empty_string(
                raw_payload,
                "generated_on",
                f"{field_path}.generated_on",
                calibration_error,
            ),
            source_manifest_ids=tuple(
                _require_non_empty_string_value(
                    raw_manifest_id,
                    f"{field_path}.source_manifest_ids[{manifest_index}]",
                )
                for manifest_index, raw_manifest_id in enumerate(raw_source_manifest_ids)
            ),
            upstream_source_versions=tuple(
                PresetUpstreamSourceVersion.from_mapping(
                    raw_source_version,
                    field_path=f"{field_path}.upstream_source_versions[{source_index}]",
                )
                for source_index, raw_source_version in enumerate(raw_upstream_source_versions)
            ),
            content_hash=cast(Optional[str], raw_content_hash),
        )


@dataclass(frozen=True, slots=True)
class PresetArtifactMetadata(ContractModel):
    display_name: str
    description: str
    version_label: str
    provenance: PresetArtifactProvenance

    def __post_init__(self) -> None:
        if not self.display_name.strip():
            raise _preset_registry_error("display_name", "must be a non-empty string")
        if not self.description.strip():
            raise _preset_registry_error("description", "must be a non-empty string")
        if not self.version_label.strip():
            raise _preset_registry_error("version_label", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> PresetArtifactMetadata:
        if not isinstance(raw_payload, Mapping):
            raise _preset_registry_error(field_path, "must be an object")

        raw_provenance = require_field(
            raw_payload,
            "provenance",
            f"{field_path}.provenance",
            calibration_error,
        )

        return PresetArtifactMetadata(
            display_name=require_non_empty_string(
                raw_payload,
                "display_name",
                f"{field_path}.display_name",
                calibration_error,
            ),
            description=require_non_empty_string(
                raw_payload,
                "description",
                f"{field_path}.description",
                calibration_error,
            ),
            version_label=require_non_empty_string(
                raw_payload,
                "version_label",
                f"{field_path}.version_label",
                calibration_error,
            ),
            provenance=PresetArtifactProvenance.from_mapping(
                raw_provenance,
                field_path=f"{field_path}.provenance",
            ),
        )


@dataclass(frozen=True, slots=True)
class SessionPresetArtifact(ContractModel):
    schema_version: str
    category: PresetCategory
    preset_name: str
    metadata: PresetArtifactMetadata
    configuration: SessionExplicitConfiguration

    def __post_init__(self) -> None:
        if self.schema_version != SESSION_PRESET_ARTIFACT_SCHEMA_VERSION:
            raise _preset_registry_error(
                "schema_version",
                f"must be `{SESSION_PRESET_ARTIFACT_SCHEMA_VERSION}`",
            )
        if self.category != PresetCategory.SESSION:
            raise _preset_registry_error("category", "must be `session`")
        if not self.preset_name.strip():
            raise _preset_registry_error("preset_name", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> SessionPresetArtifact:
        if not isinstance(raw_payload, Mapping):
            raise _preset_registry_error(field_path, "must be an object")

        return SessionPresetArtifact(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                calibration_error,
            ),
            category=_parse_preset_category(
                require_field(
                    raw_payload,
                    "category",
                    f"{field_path}.category",
                    calibration_error,
                ),
                field_path=f"{field_path}.category",
            ),
            preset_name=require_non_empty_string(
                raw_payload,
                "preset_name",
                f"{field_path}.preset_name",
                calibration_error,
            ),
            metadata=PresetArtifactMetadata.from_mapping(
                require_field(
                    raw_payload,
                    "metadata",
                    f"{field_path}.metadata",
                    calibration_error,
                ),
                field_path=f"{field_path}.metadata",
            ),
            configuration=SessionExplicitConfiguration.from_mapping(
                require_field(
                    raw_payload,
                    "configuration",
                    f"{field_path}.configuration",
                    calibration_error,
                ),
                field_path=f"{field_path}.configuration",
            ),
        )


@dataclass(frozen=True, slots=True)
class KnownNewsPresetArtifact(ContractModel):
    schema_version: str
    category: PresetCategory
    preset_name: str
    metadata: PresetArtifactMetadata
    configuration: KnownNewsExplicitConfiguration

    def __post_init__(self) -> None:
        if self.schema_version != KNOWN_NEWS_PRESET_ARTIFACT_SCHEMA_VERSION:
            raise _preset_registry_error(
                "schema_version",
                f"must be `{KNOWN_NEWS_PRESET_ARTIFACT_SCHEMA_VERSION}`",
            )
        if self.category != PresetCategory.KNOWN_NEWS:
            raise _preset_registry_error("category", "must be `known_news`")
        if not self.preset_name.strip():
            raise _preset_registry_error("preset_name", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> KnownNewsPresetArtifact:
        if not isinstance(raw_payload, Mapping):
            raise _preset_registry_error(field_path, "must be an object")

        return KnownNewsPresetArtifact(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                calibration_error,
            ),
            category=_parse_preset_category(
                require_field(
                    raw_payload,
                    "category",
                    f"{field_path}.category",
                    calibration_error,
                ),
                field_path=f"{field_path}.category",
            ),
            preset_name=require_non_empty_string(
                raw_payload,
                "preset_name",
                f"{field_path}.preset_name",
                calibration_error,
            ),
            metadata=PresetArtifactMetadata.from_mapping(
                require_field(
                    raw_payload,
                    "metadata",
                    f"{field_path}.metadata",
                    calibration_error,
                ),
                field_path=f"{field_path}.metadata",
            ),
            configuration=KnownNewsExplicitConfiguration.from_mapping(
                require_field(
                    raw_payload,
                    "configuration",
                    f"{field_path}.configuration",
                    calibration_error,
                ),
                field_path=f"{field_path}.configuration",
            ),
        )


@dataclass(frozen=True, slots=True)
class MarketProfilePresetArtifact(ContractModel):
    schema_version: str
    category: PresetCategory
    preset_name: str
    metadata: PresetArtifactMetadata
    configuration: MarketProfileExplicitConfiguration

    def __post_init__(self) -> None:
        if self.schema_version != MARKET_PROFILE_PRESET_ARTIFACT_SCHEMA_VERSION:
            raise _preset_registry_error(
                "schema_version",
                f"must be `{MARKET_PROFILE_PRESET_ARTIFACT_SCHEMA_VERSION}`",
            )
        if self.category != PresetCategory.MARKET_PROFILE:
            raise _preset_registry_error("category", "must be `market_profile`")
        if not self.preset_name.strip():
            raise _preset_registry_error("preset_name", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> MarketProfilePresetArtifact:
        if not isinstance(raw_payload, Mapping):
            raise _preset_registry_error(field_path, "must be an object")

        return MarketProfilePresetArtifact(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                calibration_error,
            ),
            category=_parse_preset_category(
                require_field(
                    raw_payload,
                    "category",
                    f"{field_path}.category",
                    calibration_error,
                ),
                field_path=f"{field_path}.category",
            ),
            preset_name=require_non_empty_string(
                raw_payload,
                "preset_name",
                f"{field_path}.preset_name",
                calibration_error,
            ),
            metadata=PresetArtifactMetadata.from_mapping(
                require_field(
                    raw_payload,
                    "metadata",
                    f"{field_path}.metadata",
                    calibration_error,
                ),
                field_path=f"{field_path}.metadata",
            ),
            configuration=MarketProfileExplicitConfiguration.from_mapping(
                require_field(
                    raw_payload,
                    "configuration",
                    f"{field_path}.configuration",
                    calibration_error,
                ),
                field_path=f"{field_path}.configuration",
            ),
        )


type PresetArtifact = SessionPresetArtifact | KnownNewsPresetArtifact | MarketProfilePresetArtifact


@dataclass(frozen=True, slots=True)
class PresetIndexEntry(ContractModel):
    category: PresetCategory
    preset_name: str
    display_name: str
    description: str
    version_label: str
    registry_scope: PresetRegistryScope
    content_hash: str
    artifact_path: Optional[str] = None

    def __post_init__(self) -> None:
        for field_name, field_value in (
            ("preset_name", self.preset_name),
            ("display_name", self.display_name),
            ("description", self.description),
            ("version_label", self.version_label),
        ):
            if not field_value.strip():
                raise _preset_registry_error(field_name, "must be a non-empty string")

        if not _is_content_hash(self.content_hash):
            raise _preset_registry_error(
                "content_hash",
                f"must be a {PRESET_CONTENT_HASH_ALGORITHM}:... content hash",
            )

        if self.artifact_path is not None and not self.artifact_path.strip():
            raise _preset_registry_error("artifact_path", "must be non-empty when provided")

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> PresetIndexEntry:
        if not isinstance(raw_payload, Mapping):
            raise _preset_registry_error(field_path, "must be an object")

        raw_artifact_path = raw_payload.get("artifact_path")
        if raw_artifact_path is not None and not isinstance(raw_artifact_path, str):
            raise _preset_registry_error(
                f"{field_path}.artifact_path",
                "must be a string when provided",
            )

        return PresetIndexEntry(
            category=_parse_preset_category(
                require_field(
                    raw_payload,
                    "category",
                    f"{field_path}.category",
                    calibration_error,
                ),
                field_path=f"{field_path}.category",
            ),
            preset_name=require_non_empty_string(
                raw_payload,
                "preset_name",
                f"{field_path}.preset_name",
                calibration_error,
            ),
            display_name=require_non_empty_string(
                raw_payload,
                "display_name",
                f"{field_path}.display_name",
                calibration_error,
            ),
            description=require_non_empty_string(
                raw_payload,
                "description",
                f"{field_path}.description",
                calibration_error,
            ),
            version_label=require_non_empty_string(
                raw_payload,
                "version_label",
                f"{field_path}.version_label",
                calibration_error,
            ),
            registry_scope=_parse_preset_registry_scope(
                require_field(
                    raw_payload,
                    "registry_scope",
                    f"{field_path}.registry_scope",
                    calibration_error,
                ),
                field_path=f"{field_path}.registry_scope",
            ),
            content_hash=require_non_empty_string(
                raw_payload,
                "content_hash",
                f"{field_path}.content_hash",
                calibration_error,
            ),
            artifact_path=cast(Optional[str], raw_artifact_path),
        )


@dataclass(frozen=True, slots=True)
class BundledPresetRegistryIndex(ContractModel):
    schema_version: str
    bundle_id: str
    bundle_version: str
    generated_on: str
    entries: tuple[PresetIndexEntry, ...]

    def __post_init__(self) -> None:
        if self.schema_version != BUNDLED_PRESET_INDEX_SCHEMA_VERSION:
            raise _preset_registry_error(
                "schema_version",
                f"must be `{BUNDLED_PRESET_INDEX_SCHEMA_VERSION}`",
            )

        for field_name, field_value in (
            ("bundle_id", self.bundle_id),
            ("bundle_version", self.bundle_version),
            ("generated_on", self.generated_on),
        ):
            if not field_value.strip():
                raise _preset_registry_error(field_name, "must be a non-empty string")

        _validate_unique_strings(
            values=tuple(
                f"{entry.registry_scope.value}|{entry.category.value}|{entry.preset_name}"
                for entry in self.entries
            ),
            field_path="entries",
            duplicate_message="must not contain duplicate registry/category/name combinations",
        )

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> BundledPresetRegistryIndex:
        if not isinstance(raw_payload, Mapping):
            raise _preset_registry_error(field_path, "must be an object")

        raw_entries = require_sequence(
            raw_payload,
            "entries",
            f"{field_path}.entries",
            calibration_error,
        )

        return BundledPresetRegistryIndex(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                calibration_error,
            ),
            bundle_id=require_non_empty_string(
                raw_payload,
                "bundle_id",
                f"{field_path}.bundle_id",
                calibration_error,
            ),
            bundle_version=require_non_empty_string(
                raw_payload,
                "bundle_version",
                f"{field_path}.bundle_version",
                calibration_error,
            ),
            generated_on=require_non_empty_string(
                raw_payload,
                "generated_on",
                f"{field_path}.generated_on",
                calibration_error,
            ),
            entries=tuple(
                PresetIndexEntry.from_mapping(
                    raw_entry,
                    field_path=f"{field_path}.entries[{entry_index}]",
                )
                for entry_index, raw_entry in enumerate(raw_entries)
            ),
        )


@dataclass(frozen=True, slots=True)
class ResolvedPresetExport(ContractModel):
    schema_version: str
    requested_category: PresetCategory
    requested_scope: PresetRegistryScope
    preset_name: str
    session_preset: Optional[SessionPresetArtifact] = None
    known_news_preset: Optional[KnownNewsPresetArtifact] = None
    market_profile_preset: Optional[MarketProfilePresetArtifact] = None

    def __post_init__(self) -> None:
        if self.schema_version != RESOLVED_PRESET_EXPORT_SCHEMA_VERSION:
            raise _preset_registry_error(
                "schema_version",
                f"must be `{RESOLVED_PRESET_EXPORT_SCHEMA_VERSION}`",
            )

        if not self.preset_name.strip():
            raise _preset_registry_error("preset_name", "must be a non-empty string")

        match self.requested_category:
            case PresetCategory.SESSION:
                if self.session_preset is None:
                    raise _preset_registry_error(
                        "session_preset",
                        "is required when requested_category=session",
                    )
            case PresetCategory.KNOWN_NEWS:
                if self.known_news_preset is None:
                    raise _preset_registry_error(
                        "known_news_preset",
                        "is required when requested_category=known_news",
                    )
            case PresetCategory.MARKET_PROFILE:
                if self.market_profile_preset is None:
                    raise _preset_registry_error(
                        "market_profile_preset",
                        "is required when requested_category=market_profile",
                    )


class BuiltInPresetRegistry:
    def __init__(
        self,
        *,
        index: BundledPresetRegistryIndex,
        preset_store: _PresetStore,
    ) -> None:
        self._index = index
        self._preset_store = preset_store

    @property
    def index(self) -> BundledPresetRegistryIndex:
        return self._index

    @classmethod
    def load_default(cls) -> BuiltInPresetRegistry:
        resource_root = files(_BUNDLED_PRESET_RESOURCE_PACKAGE)
        index_json = resource_root.joinpath(_BUNDLED_PRESET_INDEX_RESOURCE_NAME).read_text(encoding="utf-8")
        index_mapping = parse_json_object(index_json, "bundled preset index", calibration_error)
        index = BundledPresetRegistryIndex.from_mapping(index_mapping, field_path="bundled_preset_index")
        artifact_json_by_path: dict[str, str] = {}

        for entry in index.entries:
            if entry.artifact_path is None:
                raise _preset_registry_error(
                    "entries.artifact_path",
                    "bundled preset index entries must provide artifact_path",
                )
            artifact_json_by_path[entry.artifact_path] = resource_root.joinpath(entry.artifact_path).read_text(
                encoding="utf-8"
            )

        return cls.from_runtime_artifacts(
            index_json=index_json,
            artifact_json_by_path=artifact_json_by_path,
        )

    @classmethod
    def from_runtime_artifacts(
        cls,
        *,
        index_json: str,
        artifact_json_by_path: Mapping[str, str],
    ) -> BuiltInPresetRegistry:
        index_mapping = parse_json_object(index_json, "bundled preset index", calibration_error)
        index = BundledPresetRegistryIndex.from_mapping(index_mapping, field_path="bundled_preset_index")
        preset_store = _PresetStore()

        for entry_index, entry in enumerate(index.entries):
            if entry.registry_scope != PresetRegistryScope.BUILT_IN:
                raise _preset_registry_error(
                    f"bundled_preset_index.entries[{entry_index}].registry_scope",
                    "built-in preset index entries must use built_in scope",
                )
            if entry.artifact_path is None:
                raise _preset_registry_error(
                    f"bundled_preset_index.entries[{entry_index}].artifact_path",
                    "is required for built-in presets",
                )
            if entry.artifact_path not in artifact_json_by_path:
                raise _preset_registry_error(
                    f"bundled_preset_index.entries[{entry_index}].artifact_path",
                    f"references missing artifact payload `{entry.artifact_path}`",
                )

            artifact = deserialize_preset_artifact(artifact_json_by_path[entry.artifact_path])
            normalized_artifact = normalize_preset_artifact(artifact)

            _validate_index_entry_matches_artifact(entry=entry, artifact=normalized_artifact, entry_index=entry_index)

            preset_store.register(
                normalized_artifact,
                registry_scope=PresetRegistryScope.BUILT_IN,
                field_path=f"bundled_preset_index.entries[{entry_index}]",
            )

        for market_profile_name in preset_store.names(
            registry_scope=PresetRegistryScope.BUILT_IN,
            category=PresetCategory.MARKET_PROFILE,
        ):
            _validate_market_profile_dependencies(
                cast(
                    MarketProfilePresetArtifact,
                    preset_store.resolve(
                        category=PresetCategory.MARKET_PROFILE,
                        preset_name=market_profile_name,
                        registry_scope=PresetRegistryScope.BUILT_IN,
                    ),
                ),
                resolver=preset_store.resolve,
            )

        return BuiltInPresetRegistry(index=index, preset_store=preset_store)

    def list_presets(
        self,
        *,
        category: Optional[PresetCategory] = None,
    ) -> tuple[PresetIndexEntry, ...]:
        if category is None:
            return self._index.entries

        return tuple(entry for entry in self._index.entries if entry.category == category)

    def resolve(
        self,
        category: PresetCategory,
        preset_name: str,
    ) -> PresetArtifact:
        return self._preset_store.resolve(
            category=category,
            preset_name=preset_name,
            registry_scope=PresetRegistryScope.BUILT_IN,
        )

    def export_index_json(self) -> str:
        return serialize_deterministic_json(self._index)


class UserPresetRegistry:
    def __init__(self) -> None:
        self._preset_store = _PresetStore()

    def list_presets(
        self,
        *,
        category: Optional[PresetCategory] = None,
    ) -> tuple[PresetIndexEntry, ...]:
        return self._preset_store.index_entries(registry_scope=PresetRegistryScope.USER_REGISTERED, category=category)

    def resolve(
        self,
        category: PresetCategory,
        preset_name: str,
    ) -> PresetArtifact:
        return self._preset_store.resolve(
            category=category,
            preset_name=preset_name,
            registry_scope=PresetRegistryScope.USER_REGISTERED,
        )

    def contains(
        self,
        *,
        category: PresetCategory,
        preset_name: str,
    ) -> bool:
        return self._preset_store.contains(
            category=category,
            preset_name=preset_name,
            registry_scope=PresetRegistryScope.USER_REGISTERED,
        )

    def register_preset(self, artifact: PresetArtifact) -> PresetArtifact:
        normalized_artifact = normalize_preset_artifact(artifact)

        self._preset_store.register(
            normalized_artifact,
            registry_scope=PresetRegistryScope.USER_REGISTERED,
            field_path="user_registered_presets",
        )

        return normalized_artifact


class PresetRegistry:
    def __init__(
        self,
        *,
        built_in: Optional[BuiltInPresetRegistry] = None,
        user_registered: Optional[UserPresetRegistry] = None,
    ) -> None:
        self._built_in = built_in if built_in is not None else BuiltInPresetRegistry.load_default()
        self._user_registered = user_registered if user_registered is not None else UserPresetRegistry()

    def list_presets(
        self,
        *,
        category: Optional[PresetCategory] = None,
        include_built_in: bool = True,
        include_user_registered: bool = True,
    ) -> tuple[PresetIndexEntry, ...]:
        entries: list[PresetIndexEntry] = []
        if include_built_in:
            entries.extend(self._built_in.list_presets(category=category))
        if include_user_registered:
            entries.extend(self._user_registered.list_presets(category=category))

        return tuple(
            sorted(
                entries,
                key=lambda entry: (
                    entry.category.value,
                    entry.preset_name,
                    entry.registry_scope.value,
                ),
            )
        )

    def resolve(
        self,
        *,
        category: PresetCategory,
        preset_name: str,
        registry_scope: Optional[PresetRegistryScope] = None,
    ) -> tuple[PresetRegistryScope, PresetArtifact]:
        if not preset_name.strip():
            raise _preset_registry_error("preset_name", "must be a non-empty string")

        if registry_scope is not None:
            return (
                registry_scope,
                self._resolve_in_scope(
                    category=category,
                    preset_name=preset_name,
                    registry_scope=registry_scope,
                ),
            )

        if self._user_registered.contains(category=category, preset_name=preset_name):
            return (
                PresetRegistryScope.USER_REGISTERED,
                self._user_registered.resolve(category, preset_name),
            )

        return (
            PresetRegistryScope.BUILT_IN,
            self._built_in.resolve(category, preset_name),
        )

    def register_user_preset(self, artifact: PresetArtifact) -> PresetArtifact:
        normalized_artifact = normalize_preset_artifact(artifact)
        if isinstance(normalized_artifact, MarketProfilePresetArtifact):
            _validate_market_profile_dependencies(
                normalized_artifact,
                resolver=self._resolve_in_scope,
            )

        return self._user_registered.register_preset(normalized_artifact)

    def export_resolved_preset(
        self,
        *,
        category: PresetCategory,
        preset_name: str,
        registry_scope: Optional[PresetRegistryScope] = None,
    ) -> ResolvedPresetExport:
        resolved_scope, artifact = self.resolve(
            category=category,
            preset_name=preset_name,
            registry_scope=registry_scope,
        )

        market_profile_artifact = artifact if isinstance(artifact, MarketProfilePresetArtifact) else None

        session_artifact: Optional[SessionPresetArtifact] = (
            cast(SessionPresetArtifact, artifact)
            if isinstance(artifact, SessionPresetArtifact)
            else None
        )
        known_news_artifact: Optional[KnownNewsPresetArtifact] = (
            cast(KnownNewsPresetArtifact, artifact)
            if isinstance(artifact, KnownNewsPresetArtifact)
            else None
        )

        if market_profile_artifact is not None:
            session_artifact = self._resolve_market_profile_session_artifact(market_profile_artifact)
            known_news_artifact = self._resolve_market_profile_known_news_artifact(market_profile_artifact)

        return ResolvedPresetExport(
            schema_version=RESOLVED_PRESET_EXPORT_SCHEMA_VERSION,
            requested_category=category,
            requested_scope=resolved_scope,
            preset_name=preset_name,
            session_preset=session_artifact,
            known_news_preset=known_news_artifact,
            market_profile_preset=market_profile_artifact,
        )

    def _resolve_in_scope(
        self,
        *,
        category: PresetCategory,
        preset_name: str,
        registry_scope: PresetRegistryScope,
    ) -> PresetArtifact:
        match registry_scope:
            case PresetRegistryScope.BUILT_IN:
                return self._built_in.resolve(category, preset_name)
            case PresetRegistryScope.USER_REGISTERED:
                return self._user_registered.resolve(category, preset_name)

    def _resolve_market_profile_session_artifact(
        self,
        artifact: MarketProfilePresetArtifact,
    ) -> Optional[SessionPresetArtifact]:
        session_reference = artifact.configuration.session_reference
        if session_reference.usage_mode != PresetReferenceUsageMode.NAMED:
            return None
        if session_reference.preset_name is None or session_reference.registry_scope is None:
            raise _preset_registry_error(
                "configuration.session_reference",
                "named session references require preset_name and registry_scope",
            )

        return cast(
            SessionPresetArtifact,
            self._resolve_in_scope(
                category=PresetCategory.SESSION,
                preset_name=session_reference.preset_name,
                registry_scope=session_reference.registry_scope,
            ),
        )

    def _resolve_market_profile_known_news_artifact(
        self,
        artifact: MarketProfilePresetArtifact,
    ) -> Optional[KnownNewsPresetArtifact]:
        known_news_reference = artifact.configuration.known_news_reference
        if known_news_reference is None:
            return None
        if known_news_reference.usage_mode != PresetReferenceUsageMode.NAMED:
            return None
        if known_news_reference.preset_name is None or known_news_reference.registry_scope is None:
            raise _preset_registry_error(
                "configuration.known_news_reference",
                "named known-news references require preset_name and registry_scope",
            )

        return cast(
            KnownNewsPresetArtifact,
            self._resolve_in_scope(
                category=PresetCategory.KNOWN_NEWS,
                preset_name=known_news_reference.preset_name,
                registry_scope=known_news_reference.registry_scope,
            ),
        )


@dataclass(slots=True)
class _PresetStore:
    _artifacts_by_scope: dict[PresetRegistryScope, dict[PresetCategory, dict[str, PresetArtifact]]]

    def __init__(self) -> None:
        self._artifacts_by_scope = {
            PresetRegistryScope.BUILT_IN: {
                PresetCategory.SESSION: {},
                PresetCategory.KNOWN_NEWS: {},
                PresetCategory.MARKET_PROFILE: {},
            },
            PresetRegistryScope.USER_REGISTERED: {
                PresetCategory.SESSION: {},
                PresetCategory.KNOWN_NEWS: {},
                PresetCategory.MARKET_PROFILE: {},
            },
        }

    def register(
        self,
        artifact: PresetArtifact,
        *,
        registry_scope: PresetRegistryScope,
        field_path: str,
    ) -> None:
        artifacts_for_category = self._artifacts_by_scope[registry_scope][_artifact_category(artifact)]
        if artifact.preset_name in artifacts_for_category:
            raise _preset_registry_error(
                field_path,
                (
                    f"contains duplicate {registry_scope.value} "
                    f"{_artifact_category(artifact).value} preset `{artifact.preset_name}`"
                ),
            )

        artifacts_for_category[artifact.preset_name] = artifact

    def resolve(
        self,
        *,
        category: PresetCategory,
        preset_name: str,
        registry_scope: PresetRegistryScope,
    ) -> PresetArtifact:
        artifacts_for_category = self._artifacts_by_scope[registry_scope][category]
        if preset_name not in artifacts_for_category:
            raise _preset_registry_error(
                "preset_name",
                f"unknown {registry_scope.value} {category.value} preset `{preset_name}`",
            )

        return artifacts_for_category[preset_name]

    def contains(
        self,
        *,
        category: PresetCategory,
        preset_name: str,
        registry_scope: PresetRegistryScope,
    ) -> bool:
        return preset_name in self._artifacts_by_scope[registry_scope][category]

    def names(
        self,
        *,
        registry_scope: PresetRegistryScope,
        category: PresetCategory,
    ) -> tuple[str, ...]:
        return tuple(sorted(self._artifacts_by_scope[registry_scope][category].keys()))

    def index_entries(
        self,
        *,
        registry_scope: PresetRegistryScope,
        category: Optional[PresetCategory] = None,
    ) -> tuple[PresetIndexEntry, ...]:
        categories = (category,) if category is not None else tuple(PresetCategory)
        entries: list[PresetIndexEntry] = []
        for selected_category in categories:
            for preset_name, artifact in sorted(
                self._artifacts_by_scope[registry_scope][selected_category].items(),
                key=lambda item: item[0],
            ):
                entries.append(
                    PresetIndexEntry(
                        category=selected_category,
                        preset_name=preset_name,
                        display_name=artifact.metadata.display_name,
                        description=artifact.metadata.description,
                        version_label=artifact.metadata.version_label,
                        registry_scope=registry_scope,
                        content_hash=cast(str, artifact.metadata.provenance.content_hash),
                    )
                )

        return tuple(entries)


def serialize_preset_artifact(artifact: PresetArtifact) -> str:
    return serialize_deterministic_json(normalize_preset_artifact(artifact))


def deserialize_preset_artifact(payload_json: str) -> PresetArtifact:
    raw_payload = parse_json_object(payload_json, "preset artifact", calibration_error)
    return _parse_preset_artifact_mapping(raw_payload, field_path="preset_artifact")


def normalize_preset_artifact(artifact: PresetArtifact) -> PresetArtifact:
    normalized_artifact = _normalize_preset_artifact_fields(artifact)
    computed_content_hash = compute_preset_artifact_content_hash(normalized_artifact)
    observed_content_hash = normalized_artifact.metadata.provenance.content_hash

    if observed_content_hash is not None and observed_content_hash != computed_content_hash:
        raise _preset_registry_error(
            "metadata.provenance.content_hash",
            (
                f"must match the normalized artifact content hash `{computed_content_hash}`, "
                f"received `{observed_content_hash}`"
            ),
        )

    return _artifact_with_content_hash(normalized_artifact, computed_content_hash)


def compute_preset_artifact_content_hash(artifact: PresetArtifact) -> str:
    artifact_without_content_hash = _artifact_with_content_hash(
        _normalize_preset_artifact_fields(artifact),
        None,
    )
    artifact_payload_json = serialize_deterministic_json(artifact_without_content_hash)
    digest = hashlib.sha256(artifact_payload_json.encode("utf-8")).hexdigest()

    return f"{PRESET_CONTENT_HASH_ALGORITHM}:{digest}"


def _parse_preset_artifact_mapping(
    raw_payload: Mapping[str, Any],
    *,
    field_path: str,
) -> PresetArtifact:
    category = _parse_preset_category(
        require_field(raw_payload, "category", f"{field_path}.category", calibration_error),
        field_path=f"{field_path}.category",
    )

    match category:
        case PresetCategory.SESSION:
            return SessionPresetArtifact.from_mapping(raw_payload, field_path=field_path)
        case PresetCategory.KNOWN_NEWS:
            return KnownNewsPresetArtifact.from_mapping(raw_payload, field_path=field_path)
        case PresetCategory.MARKET_PROFILE:
            return MarketProfilePresetArtifact.from_mapping(raw_payload, field_path=field_path)


def _normalize_preset_artifact_fields(artifact: PresetArtifact) -> PresetArtifact:
    normalized_metadata = _normalize_metadata(artifact.metadata)
    match artifact:
        case SessionPresetArtifact():
            return replace(
                artifact,
                metadata=normalized_metadata,
                configuration=_normalize_session_configuration(artifact.configuration),
            )
        case KnownNewsPresetArtifact():
            return replace(
                artifact,
                metadata=normalized_metadata,
                configuration=_normalize_known_news_configuration(artifact.configuration),
            )
        case MarketProfilePresetArtifact():
            return replace(
                artifact,
                metadata=normalized_metadata,
                configuration=_normalize_market_profile_configuration(artifact.configuration),
            )


def _normalize_metadata(metadata: PresetArtifactMetadata) -> PresetArtifactMetadata:
    upstream_source_versions = tuple(
        sorted(
            metadata.provenance.upstream_source_versions,
            key=lambda version: (version.source_name, version.version_label),
        )
    )
    source_manifest_ids = tuple(sorted(metadata.provenance.source_manifest_ids))

    return replace(
        metadata,
        provenance=replace(
            metadata.provenance,
            source_manifest_ids=source_manifest_ids,
            upstream_source_versions=upstream_source_versions,
        ),
    )


def _normalize_session_configuration(configuration: SessionExplicitConfiguration) -> SessionExplicitConfiguration:
    regular_session_windows = tuple(
        sorted(
            configuration.regular_session_windows,
            key=lambda window: (
                _weekday_sort_key(window.weekday),
                window.open_time,
                window.close_time,
                window.label,
            ),
        )
    )

    recurring_closed_windows = tuple(
        sorted(
            configuration.recurring_closed_windows,
            key=lambda window: (
                _weekday_sort_key(window.start_weekday),
                window.start_time,
                _weekday_sort_key(window.end_weekday),
                window.end_time,
                window.reason,
            ),
        )
    )

    return replace(
        configuration,
        regular_session_windows=regular_session_windows,
        recurring_closed_windows=recurring_closed_windows,
    )


def _normalize_session_override_patch(
    override_patch: SessionConfigurationOverridePatch,
) -> SessionConfigurationOverridePatch:
    return replace(
        override_patch,
        regular_session_windows=(
            None
            if override_patch.regular_session_windows is None
            else tuple(
                sorted(
                    override_patch.regular_session_windows,
                    key=lambda window: (
                        _weekday_sort_key(window.weekday),
                        window.open_time,
                        window.close_time,
                        window.label,
                    ),
                )
            )
        ),
        recurring_closed_windows=(
            None
            if override_patch.recurring_closed_windows is None
            else tuple(
                sorted(
                    override_patch.recurring_closed_windows,
                    key=lambda window: (
                        _weekday_sort_key(window.start_weekday),
                        window.start_time,
                        _weekday_sort_key(window.end_weekday),
                        window.end_time,
                        window.reason,
                    ),
                )
            )
        ),
    )


def _normalize_known_news_configuration(configuration: KnownNewsExplicitConfiguration) -> KnownNewsExplicitConfiguration:
    events = tuple(
        sorted(
            (
                replace(
                    event,
                    tags=tuple(sorted(event.tags)),
                )
                for event in configuration.events
            ),
            key=lambda event: (event.scheduled_at.isoformat(), event.event_id),
        )
    )

    return replace(configuration, events=events)


def _normalize_known_news_override_patch(
    override_patch: KnownNewsConfigurationOverridePatch,
) -> KnownNewsConfigurationOverridePatch:
    normalized_additional_events = tuple(
        sorted(
            (
                replace(event, tags=tuple(sorted(event.tags)))
                for event in override_patch.additional_events
            ),
            key=lambda event: (event.scheduled_at.isoformat(), event.event_id),
        )
    )
    normalized_removed_event_ids = tuple(sorted(override_patch.removed_event_ids))

    return replace(
        override_patch,
        additional_events=normalized_additional_events,
        removed_event_ids=normalized_removed_event_ids,
    )


def _normalize_market_profile_configuration(
    configuration: MarketProfileExplicitConfiguration,
) -> MarketProfileExplicitConfiguration:
    normalized_column_alias_hints = tuple(
        sorted(
            (
                replace(hint, aliases=tuple(sorted(hint.aliases)))
                for hint in configuration.column_alias_hints
            ),
            key=lambda hint: hint.canonical_name,
        )
    )

    return replace(
        configuration,
        session_reference=_normalize_session_reference(configuration.session_reference),
        known_news_reference=(
            None
            if configuration.known_news_reference is None
            else _normalize_known_news_reference(configuration.known_news_reference)
        ),
        column_alias_hints=normalized_column_alias_hints,
        default_calibration_hint_ids=tuple(sorted(configuration.default_calibration_hint_ids)),
    )


def _normalize_market_profile_override_patch(
    override_patch: MarketProfileOverridePatch,
) -> MarketProfileOverridePatch:
    return replace(
        override_patch,
        session_reference=(
            None
            if override_patch.session_reference is None
            else _normalize_session_reference(override_patch.session_reference)
        ),
        known_news_reference=(
            None
            if override_patch.known_news_reference is None
            else _normalize_known_news_reference(override_patch.known_news_reference)
        ),
        column_alias_hints=(
            None
            if override_patch.column_alias_hints is None
            else tuple(
                sorted(
                    (
                        replace(hint, aliases=tuple(sorted(hint.aliases)))
                        for hint in override_patch.column_alias_hints
                    ),
                    key=lambda hint: hint.canonical_name,
                )
            )
        ),
        default_calibration_hint_ids=(
            None
            if override_patch.default_calibration_hint_ids is None
            else tuple(sorted(override_patch.default_calibration_hint_ids))
        ),
    )


def _normalize_session_reference(reference: SessionPresetReference) -> SessionPresetReference:
    return replace(
        reference,
        configuration=(
            None
            if reference.configuration is None
            else _normalize_session_configuration(reference.configuration)
        ),
        override_patch=(
            None
            if reference.override_patch is None
            else _normalize_session_override_patch(reference.override_patch)
        ),
    )


def _normalize_known_news_reference(reference: KnownNewsPresetReference) -> KnownNewsPresetReference:
    return replace(
        reference,
        configuration=(
            None
            if reference.configuration is None
            else _normalize_known_news_configuration(reference.configuration)
        ),
        override_patch=(
            None
            if reference.override_patch is None
            else _normalize_known_news_override_patch(reference.override_patch)
        ),
    )


def _artifact_with_content_hash(
    artifact: PresetArtifact,
    content_hash: Optional[str],
) -> PresetArtifact:
    normalized_metadata = replace(
        artifact.metadata,
        provenance=replace(
            artifact.metadata.provenance,
            content_hash=content_hash,
        ),
    )

    return replace(artifact, metadata=normalized_metadata)


def _validate_index_entry_matches_artifact(
    *,
    entry: PresetIndexEntry,
    artifact: PresetArtifact,
    entry_index: int,
) -> None:
    field_prefix = f"bundled_preset_index.entries[{entry_index}]"

    if entry.category != _artifact_category(artifact):
        raise _preset_registry_error(
            f"{field_prefix}.category",
            "must match the referenced artifact category",
        )
    if entry.preset_name != artifact.preset_name:
        raise _preset_registry_error(
            f"{field_prefix}.preset_name",
            "must match the referenced artifact preset_name",
        )
    if entry.display_name != artifact.metadata.display_name:
        raise _preset_registry_error(
            f"{field_prefix}.display_name",
            "must match the referenced artifact metadata.display_name",
        )
    if entry.description != artifact.metadata.description:
        raise _preset_registry_error(
            f"{field_prefix}.description",
            "must match the referenced artifact metadata.description",
        )
    if entry.version_label != artifact.metadata.version_label:
        raise _preset_registry_error(
            f"{field_prefix}.version_label",
            "must match the referenced artifact metadata.version_label",
        )

    artifact_content_hash = artifact.metadata.provenance.content_hash
    if artifact_content_hash != entry.content_hash:
        raise _preset_registry_error(
            f"{field_prefix}.content_hash",
            "must match the referenced artifact metadata.provenance.content_hash",
        )


def _artifact_category(artifact: PresetArtifact) -> PresetCategory:
    match artifact:
        case SessionPresetArtifact():
            return PresetCategory.SESSION
        case KnownNewsPresetArtifact():
            return PresetCategory.KNOWN_NEWS
        case MarketProfilePresetArtifact():
            return PresetCategory.MARKET_PROFILE


def _weekday_sort_key(weekday_name: str) -> tuple[int, str]:
    return _WEEKDAY_ORDER_INDEX.get(weekday_name, len(_WEEKDAY_ORDER)), weekday_name


def _is_content_hash(raw_value: str) -> bool:
    prefix = f"{PRESET_CONTENT_HASH_ALGORITHM}:"
    return raw_value.startswith(prefix) and len(raw_value) > len(prefix)


def _parse_preset_category(raw_value: object, *, field_path: str) -> PresetCategory:
    if not isinstance(raw_value, str):
        raise _preset_registry_error(field_path, "must be a string")

    try:
        return PresetCategory(raw_value)
    except ValueError as e:
        raise _preset_registry_error(
            field_path,
            "must be one of: session, known_news, market_profile",
        ) from e


def _parse_preset_registry_scope(raw_value: object, *, field_path: str) -> PresetRegistryScope:
    if not isinstance(raw_value, str):
        raise _preset_registry_error(field_path, "must be a string")

    try:
        return PresetRegistryScope(raw_value)
    except ValueError as e:
        raise _preset_registry_error(
            field_path,
            "must be one of: built_in, user_registered",
        ) from e


def _require_non_empty_string_value(raw_value: object, field_path: str) -> str:
    if not isinstance(raw_value, str):
        raise _preset_registry_error(field_path, "must be a string")
    if not raw_value.strip():
        raise _preset_registry_error(field_path, "must be a non-empty string")

    return raw_value


def _validate_non_empty_strings(*, values: tuple[str, ...], field_path: str) -> None:
    for value_index, value in enumerate(values):
        if not value.strip():
            raise _preset_registry_error(
                f"{field_path}[{value_index}]",
                "must be a non-empty string",
            )


def _validate_unique_strings(
    *,
    values: tuple[str, ...],
    field_path: str,
    duplicate_message: str,
) -> None:
    seen_values: set[str] = set()
    for value in values:
        if value in seen_values:
            raise _preset_registry_error(field_path, duplicate_message)

        seen_values.add(value)


def _preset_registry_error(field_path: str, message: str) -> Exception:
    return calibration_error(
        field_path,
        message,
        entrypoint="preset_registry",
    )


def _validate_market_profile_dependencies(
    artifact: MarketProfilePresetArtifact,
    *,
    resolver: Any,
) -> None:
    session_reference = artifact.configuration.session_reference
    if session_reference.usage_mode == PresetReferenceUsageMode.NAMED:
        if session_reference.preset_name is None or session_reference.registry_scope is None:
            raise _preset_registry_error(
                "configuration.session_reference",
                "named session references require preset_name and registry_scope",
            )

        resolver(
            category=PresetCategory.SESSION,
            preset_name=session_reference.preset_name,
            registry_scope=session_reference.registry_scope,
        )

    known_news_reference = artifact.configuration.known_news_reference
    if known_news_reference is None or known_news_reference.usage_mode != PresetReferenceUsageMode.NAMED:
        return
    if known_news_reference.preset_name is None or known_news_reference.registry_scope is None:
        raise _preset_registry_error(
            "configuration.known_news_reference",
            "named known-news references require preset_name and registry_scope",
        )

    resolver(
        category=PresetCategory.KNOWN_NEWS,
        preset_name=known_news_reference.preset_name,
        registry_scope=known_news_reference.registry_scope,
    )


__all__ = [
    "BUNDLED_PRESET_INDEX_SCHEMA_VERSION",
    "BuiltInPresetRegistry",
    "KNOWN_NEWS_PRESET_ARTIFACT_SCHEMA_VERSION",
    "KnownNewsPresetArtifact",
    "MARKET_PROFILE_PRESET_ARTIFACT_SCHEMA_VERSION",
    "MarketProfilePresetArtifact",
    "PRESET_CONTENT_HASH_ALGORITHM",
    "PresetArtifactMetadata",
    "PresetArtifactProvenance",
    "PresetCategory",
    "PresetIndexEntry",
    "PresetRegistry",
    "PresetUpstreamSourceVersion",
    "RESOLVED_PRESET_EXPORT_SCHEMA_VERSION",
    "ResolvedPresetExport",
    "SESSION_PRESET_ARTIFACT_SCHEMA_VERSION",
    "SessionPresetArtifact",
    "UserPresetRegistry",
    "BundledPresetRegistryIndex",
    "compute_preset_artifact_content_hash",
    "deserialize_preset_artifact",
    "normalize_preset_artifact",
    "serialize_preset_artifact",
]
