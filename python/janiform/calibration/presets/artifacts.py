from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from enum import StrEnum
from typing import Any, Optional, cast

from .errors import preset_registry_error
from .native import call_native_preset_object, call_native_preset_string
from .references import (
    KnownNewsExplicitConfiguration,
    MarketProfileExplicitConfiguration,
    SessionExplicitConfiguration,
)
from ..errors import calibration_error
from ..._native_api import native_module
from ...contracts.base import (
    ContractModel,
    require_field,
    require_non_empty_string,
    require_non_empty_string_value,
    require_sequence,
    validate_non_empty_string_values,
    validate_unique_string_values,
)


SESSION_PRESET_ARTIFACT_SCHEMA_VERSION = "preset.artifact.session.v1"
KNOWN_NEWS_PRESET_ARTIFACT_SCHEMA_VERSION = "preset.artifact.known_news.v1"
MARKET_PROFILE_PRESET_ARTIFACT_SCHEMA_VERSION = "preset.artifact.market_profile.v1"
PRESET_CONTENT_HASH_ALGORITHM = "sha256"


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
            raise preset_registry_error("source_name", "must be a non-empty string")
        if not self.version_label.strip():
            raise preset_registry_error("version_label", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> PresetUpstreamSourceVersion:
        if not isinstance(raw_payload, Mapping):
            raise preset_registry_error(field_path, "must be an object")

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
                raise preset_registry_error(field_name, "must be a non-empty string")

        validate_non_empty_string_values(
            self.source_manifest_ids,
            "source_manifest_ids",
            preset_registry_error,
        )
        validate_unique_string_values(
            values=self.source_manifest_ids,
            field_path="source_manifest_ids",
            duplicate_message="must not contain duplicate source manifest identifiers",
            error_factory=preset_registry_error,
        )

        if self.content_hash is not None and not is_preset_content_hash(self.content_hash):
            raise preset_registry_error(
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
            raise preset_registry_error(field_path, "must be an object")

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
            raise preset_registry_error(
                f"{field_path}.upstream_source_versions",
                "must be a sequence when provided",
            )

        raw_content_hash = raw_payload.get("content_hash")
        if raw_content_hash is not None and not isinstance(raw_content_hash, str):
            raise preset_registry_error(
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
                require_non_empty_string_value(
                    raw_manifest_id,
                    f"{field_path}.source_manifest_ids[{manifest_index}]",
                    preset_registry_error,
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
            raise preset_registry_error("display_name", "must be a non-empty string")
        if not self.description.strip():
            raise preset_registry_error("description", "must be a non-empty string")
        if not self.version_label.strip():
            raise preset_registry_error("version_label", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> PresetArtifactMetadata:
        if not isinstance(raw_payload, Mapping):
            raise preset_registry_error(field_path, "must be an object")

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
            raise preset_registry_error(
                "schema_version",
                f"must be `{SESSION_PRESET_ARTIFACT_SCHEMA_VERSION}`",
            )
        if self.category != PresetCategory.SESSION:
            raise preset_registry_error("category", "must be `session`")
        if not self.preset_name.strip():
            raise preset_registry_error("preset_name", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> SessionPresetArtifact:
        if not isinstance(raw_payload, Mapping):
            raise preset_registry_error(field_path, "must be an object")

        return SessionPresetArtifact(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                calibration_error,
            ),
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
            raise preset_registry_error(
                "schema_version",
                f"must be `{KNOWN_NEWS_PRESET_ARTIFACT_SCHEMA_VERSION}`",
            )
        if self.category != PresetCategory.KNOWN_NEWS:
            raise preset_registry_error("category", "must be `known_news`")
        if not self.preset_name.strip():
            raise preset_registry_error("preset_name", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> KnownNewsPresetArtifact:
        if not isinstance(raw_payload, Mapping):
            raise preset_registry_error(field_path, "must be an object")

        return KnownNewsPresetArtifact(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                calibration_error,
            ),
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
            raise preset_registry_error(
                "schema_version",
                f"must be `{MARKET_PROFILE_PRESET_ARTIFACT_SCHEMA_VERSION}`",
            )
        if self.category != PresetCategory.MARKET_PROFILE:
            raise preset_registry_error("category", "must be `market_profile`")
        if not self.preset_name.strip():
            raise preset_registry_error("preset_name", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> MarketProfilePresetArtifact:
        if not isinstance(raw_payload, Mapping):
            raise preset_registry_error(field_path, "must be an object")

        return MarketProfilePresetArtifact(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                calibration_error,
            ),
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


def serialize_preset_artifact(artifact: PresetArtifact) -> str:
    native = native_module()

    return call_native_preset_string(
        native,
        native.serialize_preset_artifact,
        {"artifact": artifact.to_payload()},
    )


def deserialize_preset_artifact(payload_json: str) -> PresetArtifact:
    native = native_module()
    raw_payload = call_native_preset_object(
        native,
        native.deserialize_preset_artifact,
        {"payload_json": payload_json},
    )

    return parse_preset_artifact_mapping(raw_payload, field_path="preset_artifact")


def normalize_preset_artifact(artifact: PresetArtifact) -> PresetArtifact:
    native = native_module()
    raw_payload = call_native_preset_object(
        native,
        native.normalize_preset_artifact,
        {"artifact": artifact.to_payload()},
    )

    return parse_preset_artifact_mapping(raw_payload, field_path="preset_artifact")


def compute_preset_artifact_content_hash(artifact: PresetArtifact) -> str:
    native = native_module()

    return call_native_preset_string(
        native,
        native.compute_preset_artifact_content_hash,
        {"artifact": artifact.to_payload()},
    )


def parse_preset_artifact_mapping(
    raw_payload: Mapping[str, Any],
    *,
    field_path: str,
) -> PresetArtifact:
    category = parse_preset_category(
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


def parse_preset_category(raw_value: object, *, field_path: str) -> PresetCategory:
    if not isinstance(raw_value, str):
        raise preset_registry_error(field_path, "must be a string")

    try:
        return PresetCategory(raw_value)
    except ValueError as e:
        raise preset_registry_error(
            field_path,
            "must be one of: session, known_news, market_profile",
        ) from e


def is_preset_content_hash(raw_value: str) -> bool:
    prefix = f"{PRESET_CONTENT_HASH_ALGORITHM}:"
    return raw_value.startswith(prefix) and len(raw_value) > len(prefix)


__all__ = [
    "KNOWN_NEWS_PRESET_ARTIFACT_SCHEMA_VERSION",
    "KnownNewsPresetArtifact",
    "MARKET_PROFILE_PRESET_ARTIFACT_SCHEMA_VERSION",
    "MarketProfilePresetArtifact",
    "PRESET_CONTENT_HASH_ALGORITHM",
    "PresetArtifact",
    "PresetArtifactMetadata",
    "PresetArtifactProvenance",
    "PresetCategory",
    "PresetUpstreamSourceVersion",
    "SESSION_PRESET_ARTIFACT_SCHEMA_VERSION",
    "SessionPresetArtifact",
    "compute_preset_artifact_content_hash",
    "deserialize_preset_artifact",
    "is_preset_content_hash",
    "normalize_preset_artifact",
    "parse_preset_category",
    "parse_preset_artifact_mapping",
    "serialize_preset_artifact",
]
