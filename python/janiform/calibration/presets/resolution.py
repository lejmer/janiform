from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from enum import StrEnum
from typing import Any, Optional

from ..._native_api import native_module
from .artifacts import PresetCategory
from .native import call_native_preset_object
from .references import (
    KnownNewsExplicitConfiguration,
    KnownNewsPresetReference,
    MarketProfileExplicitConfiguration,
    MarketProfileKnownNewsPolicy,
    MarketProfileReference,
    PresetRegistryScope,
    SessionExplicitConfiguration,
    SessionPresetReference,
)
from .registry import PresetRegistry
from ..errors import calibration_error
from ..sessions.contracts import MarketDataSessionResolution
from ...contracts.base import (
    require_boolean,
    require_mapping,
    require_non_empty_string,
    require_sequence,
    ContractModel,
)


# Public preset-resolution default that callers can replace with an explicit preset name.
DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME = "none"
RESOLVED_PRESET_WORKFLOW_CONFIGURATION_SCHEMA_VERSION = (
    "calibration.preset_resolution.resolved_configuration.v1"
)


class ResolvedWorkflowComponent(StrEnum):
    MARKET_PROFILE = 'market_profile'
    SESSION = 'session'
    KNOWN_NEWS = 'known_news'


class ResolvedWorkflowComponentSource(StrEnum):
    BUILT_IN = 'built_in'
    INFERRED = 'inferred'
    OVERRIDDEN = 'overridden'
    FULLY_CUSTOM = 'fully_custom'


class ResolvedWorkflowDiagnosticSeverity(StrEnum):
    INFO = 'info'
    WARNING = 'warning'


@dataclass(frozen=True, slots=True)
class ResolvedPresetSelection(ContractModel):
    category: PresetCategory
    registry_scope: PresetRegistryScope
    preset_name: str

    def __post_init__(self) -> None:
        if not self.preset_name.strip():
            raise calibration_error("preset_name", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> ResolvedPresetSelection:
        if not isinstance(raw_payload, Mapping):
            raise calibration_error(field_path, "must be an object")

        return ResolvedPresetSelection(
            category=PresetCategory(
                require_non_empty_string(
                    raw_payload,
                    "category",
                    f"{field_path}.category",
                    calibration_error,
                )
            ),
            registry_scope=PresetRegistryScope(
                require_non_empty_string(
                    raw_payload,
                    "registry_scope",
                    f"{field_path}.registry_scope",
                    calibration_error,
                )
            ),
            preset_name=require_non_empty_string(
                raw_payload,
                "preset_name",
                f"{field_path}.preset_name",
                calibration_error,
            ),
        )


@dataclass(frozen=True, slots=True)
class ResolvedWorkflowDiagnostic(ContractModel):
    component: ResolvedWorkflowComponent
    severity: ResolvedWorkflowDiagnosticSeverity
    code: str
    message: str

    def __post_init__(self) -> None:
        if not self.code.strip():
            raise calibration_error("code", "must be a non-empty string")
        if not self.message.strip():
            raise calibration_error("message", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> ResolvedWorkflowDiagnostic:
        if not isinstance(raw_payload, Mapping):
            raise calibration_error(field_path, "must be an object")

        return ResolvedWorkflowDiagnostic(
            component=ResolvedWorkflowComponent(
                require_non_empty_string(
                    raw_payload,
                    "component",
                    f"{field_path}.component",
                    calibration_error,
                )
            ),
            severity=ResolvedWorkflowDiagnosticSeverity(
                require_non_empty_string(
                    raw_payload,
                    "severity",
                    f"{field_path}.severity",
                    calibration_error,
                )
            ),
            code=require_non_empty_string(
                raw_payload,
                "code",
                f"{field_path}.code",
                calibration_error,
            ),
            message=require_non_empty_string(
                raw_payload,
                "message",
                f"{field_path}.message",
                calibration_error,
            ),
        )


@dataclass(frozen=True, slots=True)
class ResolvedMarketProfileComponent(ContractModel):
    configuration: MarketProfileExplicitConfiguration
    source_kind: ResolvedWorkflowComponentSource
    selected_preset: Optional[ResolvedPresetSelection] = None
    applied_override_fields: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        _validate_override_fields(self.applied_override_fields, "market_profile.applied_override_fields")

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> ResolvedMarketProfileComponent:
        if not isinstance(raw_payload, Mapping):
            raise calibration_error(field_path, "must be an object")

        raw_selected_preset = raw_payload.get("selected_preset")

        return ResolvedMarketProfileComponent(
            configuration=MarketProfileExplicitConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "configuration",
                    f"{field_path}.configuration",
                    calibration_error,
                ),
                field_path=f"{field_path}.configuration",
            ),
            source_kind=ResolvedWorkflowComponentSource(
                require_non_empty_string(
                    raw_payload,
                    "source_kind",
                    f"{field_path}.source_kind",
                    calibration_error,
                )
            ),
            selected_preset=(
                ResolvedPresetSelection.from_mapping(
                    raw_selected_preset,
                    field_path=f"{field_path}.selected_preset",
                )
                if isinstance(raw_selected_preset, Mapping)
                else None
            ),
            applied_override_fields=tuple(
                _require_string_sequence(
                    raw_payload,
                    "applied_override_fields",
                    field_path=f"{field_path}.applied_override_fields",
                )
            ),
        )


@dataclass(frozen=True, slots=True)
class ResolvedSessionComponent(ContractModel):
    configuration: SessionExplicitConfiguration
    source_kind: ResolvedWorkflowComponentSource
    selected_preset: Optional[ResolvedPresetSelection] = None
    inferred_from_data: bool = False
    inference_resolution: Optional[MarketDataSessionResolution] = None
    applied_override_fields: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        _validate_override_fields(self.applied_override_fields, "session.applied_override_fields")
        if self.inferred_from_data and self.inference_resolution is None:
            raise calibration_error(
                "session.inference_resolution",
                "is required when inferred_from_data is true",
            )
        if not self.inferred_from_data and self.inference_resolution is not None:
            raise calibration_error(
                "session.inference_resolution",
                "is only allowed when inferred_from_data is true",
            )

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> ResolvedSessionComponent:
        if not isinstance(raw_payload, Mapping):
            raise calibration_error(field_path, "must be an object")

        raw_selected_preset = raw_payload.get("selected_preset")
        raw_inference_resolution = raw_payload.get("inference_resolution")

        return ResolvedSessionComponent(
            configuration=SessionExplicitConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "configuration",
                    f"{field_path}.configuration",
                    calibration_error,
                ),
                field_path=f"{field_path}.configuration",
            ),
            source_kind=ResolvedWorkflowComponentSource(
                require_non_empty_string(
                    raw_payload,
                    "source_kind",
                    f"{field_path}.source_kind",
                    calibration_error,
                )
            ),
            selected_preset=(
                ResolvedPresetSelection.from_mapping(
                    raw_selected_preset,
                    field_path=f"{field_path}.selected_preset",
                )
                if isinstance(raw_selected_preset, Mapping)
                else None
            ),
            inferred_from_data=require_boolean(
                raw_payload,
                "inferred_from_data",
                f"{field_path}.inferred_from_data",
                calibration_error,
            ),
            inference_resolution=(
                MarketDataSessionResolution.from_mapping(
                    raw_inference_resolution,
                    field_path=f"{field_path}.inference_resolution",
                )
                if isinstance(raw_inference_resolution, Mapping)
                else None
            ),
            applied_override_fields=tuple(
                _require_string_sequence(
                    raw_payload,
                    "applied_override_fields",
                    field_path=f"{field_path}.applied_override_fields",
                )
            ),
        )


@dataclass(frozen=True, slots=True)
class ResolvedKnownNewsComponent(ContractModel):
    configuration: KnownNewsExplicitConfiguration
    source_kind: ResolvedWorkflowComponentSource
    selected_preset: Optional[ResolvedPresetSelection] = None
    applied_override_fields: tuple[str, ...] = ()
    recommended_reference: Optional[KnownNewsPresetReference] = None
    recommendation_policy: Optional[MarketProfileKnownNewsPolicy] = None
    recommendation_applied: bool = False

    def __post_init__(self) -> None:
        _validate_override_fields(self.applied_override_fields, "known_news.applied_override_fields")
        if self.recommendation_applied and self.recommended_reference is None:
            raise calibration_error(
                "known_news.recommended_reference",
                "is required when recommendation_applied is true",
            )
        if self.recommendation_policy is None and self.recommended_reference is not None:
            raise calibration_error(
                "known_news.recommendation_policy",
                "is required when recommended_reference is provided",
            )

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> ResolvedKnownNewsComponent:
        if not isinstance(raw_payload, Mapping):
            raise calibration_error(field_path, "must be an object")

        raw_selected_preset = raw_payload.get("selected_preset")
        raw_recommended_reference = raw_payload.get("recommended_reference")
        raw_recommendation_policy = raw_payload.get("recommendation_policy")

        return ResolvedKnownNewsComponent(
            configuration=KnownNewsExplicitConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "configuration",
                    f"{field_path}.configuration",
                    calibration_error,
                ),
                field_path=f"{field_path}.configuration",
            ),
            source_kind=ResolvedWorkflowComponentSource(
                require_non_empty_string(
                    raw_payload,
                    "source_kind",
                    f"{field_path}.source_kind",
                    calibration_error,
                )
            ),
            selected_preset=(
                ResolvedPresetSelection.from_mapping(
                    raw_selected_preset,
                    field_path=f"{field_path}.selected_preset",
                )
                if isinstance(raw_selected_preset, Mapping)
                else None
            ),
            applied_override_fields=tuple(
                _require_string_sequence(
                    raw_payload,
                    "applied_override_fields",
                    field_path=f"{field_path}.applied_override_fields",
                )
            ),
            recommended_reference=(
                KnownNewsPresetReference.from_mapping(
                    raw_recommended_reference,
                    field_path=f"{field_path}.recommended_reference",
                )
                if isinstance(raw_recommended_reference, Mapping)
                else None
            ),
            recommendation_policy=(
                MarketProfileKnownNewsPolicy(raw_recommendation_policy)
                if isinstance(raw_recommendation_policy, str)
                else None
            ),
            recommendation_applied=require_boolean(
                raw_payload,
                "recommendation_applied",
                f"{field_path}.recommendation_applied",
                calibration_error,
            ),
        )


@dataclass(frozen=True, slots=True)
class ResolvedPresetWorkflowConfiguration(ContractModel):
    schema_version: str
    session: ResolvedSessionComponent
    known_news: ResolvedKnownNewsComponent
    diagnostics: tuple[ResolvedWorkflowDiagnostic, ...] = ()
    market_profile: Optional[ResolvedMarketProfileComponent] = None

    def __post_init__(self) -> None:
        if self.schema_version != RESOLVED_PRESET_WORKFLOW_CONFIGURATION_SCHEMA_VERSION:
            raise calibration_error(
                "schema_version",
                f"must be `{RESOLVED_PRESET_WORKFLOW_CONFIGURATION_SCHEMA_VERSION}`",
            )

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> ResolvedPresetWorkflowConfiguration:
        if not isinstance(raw_payload, Mapping):
            raise calibration_error(field_path, "must be an object")

        raw_market_profile = raw_payload.get("market_profile")

        return ResolvedPresetWorkflowConfiguration(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                calibration_error,
            ),
            session=ResolvedSessionComponent.from_mapping(
                require_mapping(
                    raw_payload,
                    "session",
                    f"{field_path}.session",
                    calibration_error,
                ),
                field_path=f"{field_path}.session",
            ),
            known_news=ResolvedKnownNewsComponent.from_mapping(
                require_mapping(
                    raw_payload,
                    "known_news",
                    f"{field_path}.known_news",
                    calibration_error,
                ),
                field_path=f"{field_path}.known_news",
            ),
            diagnostics=tuple(
                ResolvedWorkflowDiagnostic.from_mapping(
                    raw_diagnostic,
                    field_path=f"{field_path}.diagnostics[{diagnostic_index}]",
                )
                for diagnostic_index, raw_diagnostic in enumerate(
                    require_sequence(
                        raw_payload,
                        "diagnostics",
                        f"{field_path}.diagnostics",
                        calibration_error,
                    )
                )
            ),
            market_profile=(
                ResolvedMarketProfileComponent.from_mapping(
                    raw_market_profile,
                    field_path=f"{field_path}.market_profile",
                )
                if isinstance(raw_market_profile, Mapping)
                else None
            ),
        )


def _require_string_sequence(
    raw_payload: Mapping[str, Any],
    field_name: str,
    *,
    field_path: str,
) -> list[str]:
    raw_values = require_sequence(
        raw_payload,
        field_name,
        field_path,
        calibration_error,
    )
    values: list[str] = []
    for value_index, raw_value in enumerate(raw_values):
        if not isinstance(raw_value, str):
            raise calibration_error(
                f"{field_path}[{value_index}]",
                "must be a string",
            )
        values.append(raw_value)

    return values


def resolve_preset_workflow_configuration(
    *,
    registry: Optional[PresetRegistry] = None,
    market_profile_reference: Optional[MarketProfileReference] = None,
    session_reference: Optional[SessionPresetReference] = None,
    known_news_reference: Optional[KnownNewsPresetReference] = None,
    inferred_session_resolution: Optional[MarketDataSessionResolution] = None,
) -> ResolvedPresetWorkflowConfiguration:
    preset_registry = registry if registry is not None else PresetRegistry()
    raw_payload = _resolve_preset_workflow_configuration_native(
        {
            "registry": preset_registry.native_resolution_payload(),
            "market_profile_reference": (
                None
                if market_profile_reference is None
                else market_profile_reference.to_payload()
            ),
            "session_reference": (
                None if session_reference is None else session_reference.to_payload()
            ),
            "known_news_reference": (
                None
                if known_news_reference is None
                else known_news_reference.to_payload()
            ),
            "inferred_session_resolution": (
                None
                if inferred_session_resolution is None
                else inferred_session_resolution.to_payload()
            ),
        },
    )

    return ResolvedPresetWorkflowConfiguration.from_mapping(
        raw_payload,
        field_path="resolved_preset_workflow_configuration",
    )


def _resolve_preset_workflow_configuration_native(request: dict[str, Any]) -> dict[str, Any]:
    native = native_module()

    return call_native_preset_object(
        native,
        native.resolve_preset_workflow_configuration,
        request,
        function_label="resolve_preset_workflow_configuration",
    )


def _validate_override_fields(fields: tuple[str, ...], field_path: str) -> None:
    seen_fields: set[str] = set()
    for field_name in fields:
        if not field_name.strip():
            raise calibration_error(field_path, "must not contain empty override field names")
        if field_name in seen_fields:
            raise calibration_error(field_path, "must not contain duplicate override field names")

        seen_fields.add(field_name)


__all__ = [
    "DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME",
    "RESOLVED_PRESET_WORKFLOW_CONFIGURATION_SCHEMA_VERSION",
    "ResolvedKnownNewsComponent",
    "ResolvedMarketProfileComponent",
    "ResolvedPresetSelection",
    "ResolvedPresetWorkflowConfiguration",
    "ResolvedSessionComponent",
    "ResolvedWorkflowComponent",
    "ResolvedWorkflowComponentSource",
    "ResolvedWorkflowDiagnostic",
    "ResolvedWorkflowDiagnosticSeverity",
    "resolve_preset_workflow_configuration",
]
