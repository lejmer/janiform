from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, Optional

from .errors import preset_registry_error
from .native import call_native_preset_object
from .references import PresetRegistryScope
from ..errors import calibration_error
from ..._native_api import native_module
from ...contracts.base import (
    ContractModel,
    require_field,
    require_non_empty_string,
    require_sequence,
    validate_unique_string_values,
)

if TYPE_CHECKING:
    from .artifacts import (
        KnownNewsPresetArtifact,
        MarketProfilePresetArtifact,
        PresetArtifact,
        PresetCategory,
        SessionPresetArtifact,
    )


BUNDLED_PRESET_INDEX_SCHEMA_VERSION = "preset.runtime.index.v1"
RESOLVED_PRESET_SNAPSHOT_SCHEMA_VERSION = "preset.runtime.resolved_snapshot.v1"


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
        from .artifacts import PRESET_CONTENT_HASH_ALGORITHM, is_preset_content_hash

        for field_name, field_value in (
            ("preset_name", self.preset_name),
            ("display_name", self.display_name),
            ("description", self.description),
            ("version_label", self.version_label),
        ):
            if not field_value.strip():
                raise preset_registry_error(field_name, "must be a non-empty string")

        if not is_preset_content_hash(self.content_hash):
            raise preset_registry_error(
                "content_hash",
                f"must be a {PRESET_CONTENT_HASH_ALGORITHM}:... content hash",
            )

        if self.artifact_path is not None and not self.artifact_path.strip():
            raise preset_registry_error(
                "artifact_path",
                "must be non-empty when provided",
            )

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> PresetIndexEntry:
        from .artifacts import parse_preset_category

        if not isinstance(raw_payload, Mapping):
            raise preset_registry_error(field_path, "must be an object")

        raw_artifact_path = raw_payload.get("artifact_path")
        if raw_artifact_path is not None and not isinstance(raw_artifact_path, str):
            raise preset_registry_error(
                f"{field_path}.artifact_path",
                "must be a string when provided",
            )

        return PresetIndexEntry(
            category=parse_preset_category(
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
            artifact_path=raw_artifact_path,
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
            raise preset_registry_error(
                "schema_version",
                f"must be `{BUNDLED_PRESET_INDEX_SCHEMA_VERSION}`",
            )

        for field_name, field_value in (
            ("bundle_id", self.bundle_id),
            ("bundle_version", self.bundle_version),
            ("generated_on", self.generated_on),
        ):
            if not field_value.strip():
                raise preset_registry_error(
                    field_name,
                    "must be a non-empty string",
                )

        validate_unique_string_values(
            values=tuple(
                f"{entry.registry_scope.value}|{entry.category.value}|{entry.preset_name}"
                for entry in self.entries
            ),
            field_path="entries",
            duplicate_message="must not contain duplicate registry/category/name combinations",
            error_factory=preset_registry_error,
        )

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> BundledPresetRegistryIndex:
        if not isinstance(raw_payload, Mapping):
            raise preset_registry_error(field_path, "must be an object")

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
class ResolvedPresetSnapshot(ContractModel):
    """Self-contained preset snapshot produced by resolving a requested preset.

    Market-profile snapshots include the profile artifact plus any referenced
    session and known-news artifacts, so the returned object can be serialized
    as a portable preset snapshot.
    """

    schema_version: str
    requested_category: PresetCategory
    requested_scope: PresetRegistryScope
    preset_name: str
    session_preset: Optional[SessionPresetArtifact] = None
    known_news_preset: Optional[KnownNewsPresetArtifact] = None
    market_profile_preset: Optional[MarketProfilePresetArtifact] = None

    def __post_init__(self) -> None:
        from .artifacts import PresetCategory

        if self.schema_version != RESOLVED_PRESET_SNAPSHOT_SCHEMA_VERSION:
            raise preset_registry_error(
                "schema_version",
                f"must be `{RESOLVED_PRESET_SNAPSHOT_SCHEMA_VERSION}`",
            )

        if not self.preset_name.strip():
            raise preset_registry_error(
                "preset_name",
                "must be a non-empty string",
            )

        match self.requested_category:
            case PresetCategory.SESSION:
                if self.session_preset is None:
                    raise preset_registry_error(
                        "session_preset",
                        "is required when requested_category=session",
                    )
            case PresetCategory.KNOWN_NEWS:
                if self.known_news_preset is None:
                    raise preset_registry_error(
                        "known_news_preset",
                        "is required when requested_category=known_news",
                    )
            case PresetCategory.MARKET_PROFILE:
                if self.market_profile_preset is None:
                    raise preset_registry_error(
                        "market_profile_preset",
                        "is required when requested_category=market_profile",
                    )

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> ResolvedPresetSnapshot:
        from .artifacts import parse_preset_category

        if not isinstance(raw_payload, Mapping):
            raise preset_registry_error(field_path, "must be an object")

        return ResolvedPresetSnapshot(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                calibration_error,
            ),
            requested_category=parse_preset_category(
                require_field(
                    raw_payload,
                    "requested_category",
                    f"{field_path}.requested_category",
                    calibration_error,
                ),
                field_path=f"{field_path}.requested_category",
            ),
            requested_scope=_parse_preset_registry_scope(
                require_field(
                    raw_payload,
                    "requested_scope",
                    f"{field_path}.requested_scope",
                    calibration_error,
                ),
                field_path=f"{field_path}.requested_scope",
            ),
            preset_name=require_non_empty_string(
                raw_payload,
                "preset_name",
                f"{field_path}.preset_name",
                calibration_error,
            ),
            session_preset=_optional_session_preset_artifact(
                raw_payload.get("session_preset"),
                field_path=f"{field_path}.session_preset",
            ),
            known_news_preset=_optional_known_news_preset_artifact(
                raw_payload.get("known_news_preset"),
                field_path=f"{field_path}.known_news_preset",
            ),
            market_profile_preset=_optional_market_profile_preset_artifact(
                raw_payload.get("market_profile_preset"),
                field_path=f"{field_path}.market_profile_preset",
            ),
        )


class PresetRegistry:
    def __init__(
        self,
        *,
        user_registered_presets: tuple[PresetArtifact, ...] = (),
    ) -> None:
        self._user_registered_presets: tuple[PresetArtifact, ...] = ()

        for artifact in user_registered_presets:
            self.register_user_preset(artifact)

    def list_presets(
        self,
        *,
        category: Optional[PresetCategory] = None,
        include_built_in: bool = True,
        include_user_registered: bool = True,
    ) -> tuple[PresetIndexEntry, ...]:
        entries = self._native_index().entries

        return tuple(
            entry
            for entry in entries
            if _preset_index_entry_matches_filters(
                entry,
                category=category,
                include_built_in=include_built_in,
                include_user_registered=include_user_registered,
            )
        )

    def resolve_session(
        self,
        *,
        preset_name: str,
        registry_scope: Optional[PresetRegistryScope] = None,
    ) -> tuple[PresetRegistryScope, SessionPresetArtifact]:
        from .artifacts import PresetCategory, SessionPresetArtifact

        resolved_scope, artifact = self._resolve(
            category=PresetCategory.SESSION,
            preset_name=preset_name,
            registry_scope=registry_scope,
        )
        if not isinstance(artifact, SessionPresetArtifact):
            raise preset_registry_error(
                "preset_artifact",
                "resolved session preset must be a SessionPresetArtifact",
            )

        return resolved_scope, artifact

    def resolve_known_news(
        self,
        *,
        preset_name: str,
        registry_scope: Optional[PresetRegistryScope] = None,
    ) -> tuple[PresetRegistryScope, KnownNewsPresetArtifact]:
        from .artifacts import KnownNewsPresetArtifact, PresetCategory

        resolved_scope, artifact = self._resolve(
            category=PresetCategory.KNOWN_NEWS,
            preset_name=preset_name,
            registry_scope=registry_scope,
        )
        if not isinstance(artifact, KnownNewsPresetArtifact):
            raise preset_registry_error(
                "preset_artifact",
                "resolved known-news preset must be a KnownNewsPresetArtifact",
            )

        return resolved_scope, artifact

    def resolve_market_profile(
        self,
        *,
        preset_name: str,
        registry_scope: Optional[PresetRegistryScope] = None,
    ) -> tuple[PresetRegistryScope, MarketProfilePresetArtifact]:
        from .artifacts import MarketProfilePresetArtifact, PresetCategory

        resolved_scope, artifact = self._resolve(
            category=PresetCategory.MARKET_PROFILE,
            preset_name=preset_name,
            registry_scope=registry_scope,
        )
        if not isinstance(artifact, MarketProfilePresetArtifact):
            raise preset_registry_error(
                "preset_artifact",
                "resolved market-profile preset must be a MarketProfilePresetArtifact",
            )

        return resolved_scope, artifact

    def _resolve(
        self,
        *,
        category: PresetCategory,
        preset_name: str,
        registry_scope: Optional[PresetRegistryScope] = None,
    ) -> tuple[PresetRegistryScope, PresetArtifact]:
        if not preset_name.strip():
            raise preset_registry_error(
                "preset_name",
                "must be a non-empty string",
            )

        native = native_module()
        resolved_payload = call_native_preset_object(
            native,
            native.resolve_preset_artifact,
            self._native_preset_request(
                category=category,
                preset_name=preset_name,
                registry_scope=registry_scope,
            ),
            function_label="preset artifact resolution",
        )

        return (
            _parse_native_resolved_preset_scope(resolved_payload),
            _parse_native_resolved_artifact(resolved_payload),
        )

    def register_user_preset(
        self,
        artifact: PresetArtifact,
    ) -> PresetArtifact:
        from .artifacts import normalize_preset_artifact

        normalized_artifact = normalize_preset_artifact(artifact)
        candidate_artifacts = self._user_registered_presets + (normalized_artifact,)

        self._validate_native_registry(candidate_artifacts)

        self._user_registered_presets = candidate_artifacts

        return normalized_artifact

    def resolve_preset_snapshot(
        self,
        *,
        category: PresetCategory,
        preset_name: str,
        registry_scope: Optional[PresetRegistryScope] = None,
    ) -> ResolvedPresetSnapshot:
        """
        Return a self-contained resolved preset snapshot.

        This method resolves the requested preset and includes referenced
        market-profile dependencies so callers can serialize the returned
        object when they need a portable preset snapshot.
        """

        native = native_module()
        snapshot_payload = call_native_preset_object(
            native,
            native.resolve_preset_snapshot,
            self._native_preset_request(
                category=category,
                preset_name=preset_name,
                registry_scope=registry_scope,
            ),
            function_label="resolved preset snapshot",
        )

        return ResolvedPresetSnapshot.from_mapping(
            snapshot_payload,
            field_path="resolved_preset_snapshot",
        )

    def native_resolution_payload(self) -> dict[str, Any]:
        return self._native_registry_payload(self._user_registered_presets)

    def _native_preset_request(
        self,
        *,
        category: PresetCategory,
        preset_name: str,
        registry_scope: Optional[PresetRegistryScope],
    ) -> dict[str, Any]:
        return {
            "category": category.value,
            "preset_name": preset_name,
            "registry_scope": None if registry_scope is None else registry_scope.value,
            "registry": self.native_resolution_payload(),
        }

    def _native_index(self) -> BundledPresetRegistryIndex:
        native = native_module()
        index_payload = call_native_preset_object(
            native,
            native.preset_registry_index,
            {
                "registry": self.native_resolution_payload(),
            },
            function_label="preset registry index",
        )

        return BundledPresetRegistryIndex.from_mapping(
            index_payload,
            field_path="preset_registry_index",
        )

    @staticmethod
    def _native_registry_payload(
        registered_artifacts: tuple[PresetArtifact, ...],
    ) -> dict[str, Any]:
        return {
            "user_registered_presets": [
                artifact.to_payload()
                for artifact in registered_artifacts
            ],
        }

    @staticmethod
    def _validate_native_registry(
        registered_artifacts: tuple[PresetArtifact, ...],
    ) -> None:
        native = native_module()

        call_native_preset_object(
            native,
            native.preset_registry_index,
            {
                "registry": PresetRegistry._native_registry_payload(registered_artifacts),
            },
            function_label="preset registry validation",
        )


def _preset_index_entry_matches_filters(
    entry: PresetIndexEntry,
    *,
    category: Optional[PresetCategory],
    include_built_in: bool,
    include_user_registered: bool,
) -> bool:
    if category is not None and entry.category != category:
        return False

    match entry.registry_scope:
        case PresetRegistryScope.BUILT_IN:
            return include_built_in
        case PresetRegistryScope.USER_REGISTERED:
            return include_user_registered


def _parse_preset_registry_scope(raw_value: object, *, field_path: str) -> PresetRegistryScope:
    if not isinstance(raw_value, str):
        raise preset_registry_error(field_path, "must be a string")

    try:
        return PresetRegistryScope(raw_value)
    except ValueError as e:
        raise preset_registry_error(
            field_path,
            "must be one of: built_in, user_registered",
        ) from e


def _parse_native_resolved_artifact(raw_payload: Mapping[str, Any]) -> PresetArtifact:
    from .artifacts import parse_preset_artifact_mapping

    raw_artifact = require_field(
        raw_payload,
        "artifact",
        "resolved_preset_artifact.artifact",
        calibration_error,
    )

    if not isinstance(raw_artifact, Mapping):
        raise preset_registry_error(
            "resolved_preset_artifact.artifact",
            "must be an object",
        )

    return parse_preset_artifact_mapping(
        raw_artifact,
        field_path="resolved_preset_artifact.artifact",
    )


def _parse_native_resolved_preset_scope(raw_payload: Mapping[str, Any]) -> PresetRegistryScope:
    return _parse_preset_registry_scope(
        require_field(
            raw_payload,
            "registry_scope",
            "resolved_preset_artifact.registry_scope",
            calibration_error,
        ),
        field_path="resolved_preset_artifact.registry_scope",
    )


def _optional_session_preset_artifact(
    raw_payload: Any,
    *,
    field_path: str,
) -> Optional[SessionPresetArtifact]:
    from .artifacts import SessionPresetArtifact

    if raw_payload is None:
        return None
    if not isinstance(raw_payload, Mapping):
        raise preset_registry_error(
            field_path,
            "must be an object when provided",
        )

    return SessionPresetArtifact.from_mapping(raw_payload, field_path=field_path)


def _optional_known_news_preset_artifact(
    raw_payload: Any,
    *,
    field_path: str,
) -> Optional[KnownNewsPresetArtifact]:
    from .artifacts import KnownNewsPresetArtifact

    if raw_payload is None:
        return None
    if not isinstance(raw_payload, Mapping):
        raise preset_registry_error(
            field_path,
            "must be an object when provided",
        )

    return KnownNewsPresetArtifact.from_mapping(raw_payload, field_path=field_path)


def _optional_market_profile_preset_artifact(
    raw_payload: Any,
    *,
    field_path: str,
) -> Optional[MarketProfilePresetArtifact]:
    from .artifacts import MarketProfilePresetArtifact

    if raw_payload is None:
        return None
    if not isinstance(raw_payload, Mapping):
        raise preset_registry_error(
            field_path,
            "must be an object when provided",
        )

    return MarketProfilePresetArtifact.from_mapping(raw_payload, field_path=field_path)


__all__ = [
    "BUNDLED_PRESET_INDEX_SCHEMA_VERSION",
    "RESOLVED_PRESET_SNAPSHOT_SCHEMA_VERSION",
    "BundledPresetRegistryIndex",
    "PresetIndexEntry",
    "PresetRegistry",
    "ResolvedPresetSnapshot",
]
