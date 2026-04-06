from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass, replace
from enum import StrEnum
from typing import Any, Optional

from .references import (
    KnownNewsConfigurationOverridePatch,
    KnownNewsExplicitConfiguration,
    KnownNewsPresetReference,
    MarketProfileExplicitConfiguration,
    MarketProfileKnownNewsPolicy,
    MarketProfileOverridePatch,
    MarketProfileReference,
    PresetReferenceUsageMode,
    PresetRegistryScope,
    SessionConfigurationOverridePatch,
    SessionExplicitConfiguration,
    SessionPresetReference,
)
from .registry import (
    KnownNewsPresetArtifact,
    PresetCategory,
    PresetRegistry,
    SessionPresetArtifact,
    _normalize_known_news_configuration,
    _normalize_known_news_reference,
    _normalize_market_profile_configuration,
    _normalize_market_profile_override_patch,
    _normalize_session_configuration,
    _normalize_session_reference,
)
from .._errors import _calibration_error
from ..sessions.contracts import MarketDataSessionResolution
from ...contracts.base import require_boolean
from ...contracts import ContractModel, require_mapping, require_non_empty_string, require_sequence

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
            raise _calibration_error("preset_name", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> ResolvedPresetSelection:
        if not isinstance(raw_payload, Mapping):
            raise _calibration_error(field_path, "must be an object")

        return ResolvedPresetSelection(
            category=PresetCategory(
                require_non_empty_string(
                    raw_payload,
                    "category",
                    f"{field_path}.category",
                    _calibration_error,
                )
            ),
            registry_scope=PresetRegistryScope(
                require_non_empty_string(
                    raw_payload,
                    "registry_scope",
                    f"{field_path}.registry_scope",
                    _calibration_error,
                )
            ),
            preset_name=require_non_empty_string(
                raw_payload,
                "preset_name",
                f"{field_path}.preset_name",
                _calibration_error,
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
            raise _calibration_error("code", "must be a non-empty string")
        if not self.message.strip():
            raise _calibration_error("message", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: Mapping[str, Any] | Any,
        *,
        field_path: str,
    ) -> ResolvedWorkflowDiagnostic:
        if not isinstance(raw_payload, Mapping):
            raise _calibration_error(field_path, "must be an object")

        return ResolvedWorkflowDiagnostic(
            component=ResolvedWorkflowComponent(
                require_non_empty_string(
                    raw_payload,
                    "component",
                    f"{field_path}.component",
                    _calibration_error,
                )
            ),
            severity=ResolvedWorkflowDiagnosticSeverity(
                require_non_empty_string(
                    raw_payload,
                    "severity",
                    f"{field_path}.severity",
                    _calibration_error,
                )
            ),
            code=require_non_empty_string(
                raw_payload,
                "code",
                f"{field_path}.code",
                _calibration_error,
            ),
            message=require_non_empty_string(
                raw_payload,
                "message",
                f"{field_path}.message",
                _calibration_error,
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
            raise _calibration_error(field_path, "must be an object")

        raw_selected_preset = raw_payload.get("selected_preset")

        return ResolvedMarketProfileComponent(
            configuration=MarketProfileExplicitConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "configuration",
                    f"{field_path}.configuration",
                    _calibration_error,
                ),
                field_path=f"{field_path}.configuration",
            ),
            source_kind=ResolvedWorkflowComponentSource(
                require_non_empty_string(
                    raw_payload,
                    "source_kind",
                    f"{field_path}.source_kind",
                    _calibration_error,
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
            raise _calibration_error(
                "session.inference_resolution",
                "is required when inferred_from_data is true",
            )
        if not self.inferred_from_data and self.inference_resolution is not None:
            raise _calibration_error(
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
            raise _calibration_error(field_path, "must be an object")

        raw_selected_preset = raw_payload.get("selected_preset")
        raw_inference_resolution = raw_payload.get("inference_resolution")

        return ResolvedSessionComponent(
            configuration=SessionExplicitConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "configuration",
                    f"{field_path}.configuration",
                    _calibration_error,
                ),
                field_path=f"{field_path}.configuration",
            ),
            source_kind=ResolvedWorkflowComponentSource(
                require_non_empty_string(
                    raw_payload,
                    "source_kind",
                    f"{field_path}.source_kind",
                    _calibration_error,
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
                _calibration_error,
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
            raise _calibration_error(
                "known_news.recommended_reference",
                "is required when recommendation_applied is true",
            )
        if self.recommendation_policy is None and self.recommended_reference is not None:
            raise _calibration_error(
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
            raise _calibration_error(field_path, "must be an object")

        raw_selected_preset = raw_payload.get("selected_preset")
        raw_recommended_reference = raw_payload.get("recommended_reference")
        raw_recommendation_policy = raw_payload.get("recommendation_policy")

        return ResolvedKnownNewsComponent(
            configuration=KnownNewsExplicitConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "configuration",
                    f"{field_path}.configuration",
                    _calibration_error,
                ),
                field_path=f"{field_path}.configuration",
            ),
            source_kind=ResolvedWorkflowComponentSource(
                require_non_empty_string(
                    raw_payload,
                    "source_kind",
                    f"{field_path}.source_kind",
                    _calibration_error,
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
                _calibration_error,
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
            raise _calibration_error(
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
            raise _calibration_error(field_path, "must be an object")

        raw_market_profile = raw_payload.get("market_profile")

        return ResolvedPresetWorkflowConfiguration(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                _calibration_error,
            ),
            session=ResolvedSessionComponent.from_mapping(
                require_mapping(
                    raw_payload,
                    "session",
                    f"{field_path}.session",
                    _calibration_error,
                ),
                field_path=f"{field_path}.session",
            ),
            known_news=ResolvedKnownNewsComponent.from_mapping(
                require_mapping(
                    raw_payload,
                    "known_news",
                    f"{field_path}.known_news",
                    _calibration_error,
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
                        _calibration_error,
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
        _calibration_error,
    )
    values: list[str] = []
    for value_index, raw_value in enumerate(raw_values):
        if not isinstance(raw_value, str):
            raise _calibration_error(
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
    diagnostics: list[ResolvedWorkflowDiagnostic] = []
    resolved_market_profile = _resolve_market_profile_component(
        market_profile_reference=market_profile_reference,
        registry=preset_registry,
    )

    resolved_session = _resolve_session_component(
        registry=preset_registry,
        session_reference=session_reference,
        market_profile=resolved_market_profile,
        inferred_session_resolution=inferred_session_resolution,
        diagnostics=diagnostics,
    )
    resolved_known_news = _resolve_known_news_component(
        registry=preset_registry,
        known_news_reference=known_news_reference,
        market_profile=resolved_market_profile,
        diagnostics=diagnostics,
    )

    return ResolvedPresetWorkflowConfiguration(
        schema_version=RESOLVED_PRESET_WORKFLOW_CONFIGURATION_SCHEMA_VERSION,
        market_profile=resolved_market_profile,
        session=resolved_session,
        known_news=resolved_known_news,
        diagnostics=tuple(diagnostics),
    )


def _resolve_market_profile_component(
    *,
    market_profile_reference: Optional[MarketProfileReference],
    registry: PresetRegistry,
) -> Optional[ResolvedMarketProfileComponent]:
    if market_profile_reference is None:
        return None

    normalized_reference = _normalize_market_profile_reference(market_profile_reference)
    match normalized_reference.usage_mode:
        case PresetReferenceUsageMode.NAMED:
            if normalized_reference.preset_name is None or normalized_reference.registry_scope is None:
                raise _calibration_error(
                    "market_profile_reference",
                    "named market-profile references require preset_name and registry_scope",
                )

            _, artifact = registry.resolve(
                category=PresetCategory.MARKET_PROFILE,
                preset_name=normalized_reference.preset_name,
                registry_scope=normalized_reference.registry_scope,
            )

            configuration = artifact.configuration

            override_fields: tuple[str, ...] = ()
            if normalized_reference.override_patch is not None:
                configuration = _apply_market_profile_override_patch(
                    configuration=configuration,
                    override_patch=normalized_reference.override_patch,
                )
                override_fields = _market_profile_override_fields(normalized_reference.override_patch)

            return ResolvedMarketProfileComponent(
                configuration=configuration,
                source_kind=_source_kind_for_named_reference(
                    registry_scope=normalized_reference.registry_scope,
                    override_fields=override_fields,
                ),
                selected_preset=ResolvedPresetSelection(
                    category=PresetCategory.MARKET_PROFILE,
                    registry_scope=normalized_reference.registry_scope,
                    preset_name=artifact.preset_name,
                ),
                applied_override_fields=override_fields,
            )
        case PresetReferenceUsageMode.EXPLICIT:
            if normalized_reference.configuration is None:
                raise _calibration_error(
                    "market_profile_reference.configuration",
                    "is required when usage_mode=explicit",
                )

            return ResolvedMarketProfileComponent(
                configuration=_normalize_market_profile_configuration(normalized_reference.configuration),
                source_kind=ResolvedWorkflowComponentSource.FULLY_CUSTOM,
            )
        case PresetReferenceUsageMode.AUTOMATIC:
            raise _calibration_error(
                "market_profile_reference.usage_mode",
                "automatic is not supported for market-profile resolution",
            )


def _resolve_session_component(
    *,
    registry: PresetRegistry,
    session_reference: Optional[SessionPresetReference],
    market_profile: Optional[ResolvedMarketProfileComponent],
    inferred_session_resolution: Optional[MarketDataSessionResolution],
    diagnostics: list[ResolvedWorkflowDiagnostic],
) -> ResolvedSessionComponent:
    profile_reference = (
        market_profile.configuration.session_reference if market_profile is not None else None
    )

    selected_reference = session_reference
    if selected_reference is not None:
        normalized_selected_reference = _normalize_session_reference(selected_reference)
        normalized_profile_reference = (
            None if profile_reference is None else _normalize_session_reference(profile_reference)
        )

        if (
            inferred_session_resolution is not None
            and _session_reference_replaces_inferred_baseline(normalized_selected_reference)
        ):
            diagnostics.append(
                _diagnostic(
                    component=ResolvedWorkflowComponent.SESSION,
                    severity=ResolvedWorkflowDiagnosticSeverity.WARNING,
                    code="conflicting_override",
                    message=(
                        "Explicit session selection replaced the automatically inferred session "
                        "baseline for the resolved workflow configuration."
                    ),
                )
            )

        if (
            normalized_profile_reference is not None
            and normalized_selected_reference != normalized_profile_reference
        ):
            diagnostics.append(
                _diagnostic(
                    component=ResolvedWorkflowComponent.SESSION,
                    severity=ResolvedWorkflowDiagnosticSeverity.INFO,
                    code="profile_component_resolution_outcome",
                    message=(
                        "The selected market profile provided a session baseline, but the explicit "
                        "session input took precedence."
                    ),
                )
            )
    elif inferred_session_resolution is not None:
        if profile_reference is not None:
            diagnostics.append(
                _diagnostic(
                    component=ResolvedWorkflowComponent.SESSION,
                    severity=ResolvedWorkflowDiagnosticSeverity.INFO,
                    code="profile_component_resolution_outcome",
                    message=(
                        "Session inference was available, so the resolved session used the inferred "
                        "baseline instead of the market profile session baseline."
                    ),
                )
            )
        selected_reference = SessionPresetReference(usage_mode=PresetReferenceUsageMode.AUTOMATIC)
    elif profile_reference is not None:
        selected_reference = profile_reference

    if selected_reference is None:
        raise _calibration_error(
            "session_reference",
            "could not resolve a session baseline. Provide an explicit session reference or inferred session resolution.",
        )

    normalized_reference = _normalize_session_reference(selected_reference)
    match normalized_reference.usage_mode:
        case PresetReferenceUsageMode.AUTOMATIC:
            if inferred_session_resolution is None:
                raise _calibration_error(
                    "inferred_session_resolution",
                    "is required when the resolved session reference uses automatic inference",
                )

            resolved_configuration = _session_configuration_from_inference(inferred_session_resolution)
            override_fields: tuple[str, ...] = ()

            if normalized_reference.override_patch is not None:
                resolved_configuration = _apply_session_override_patch(
                    configuration=resolved_configuration,
                    override_patch=normalized_reference.override_patch,
                )
                override_fields = _session_override_fields(normalized_reference.override_patch)

            _append_inference_diagnostics(
                inference_resolution=inferred_session_resolution,
                diagnostics=diagnostics,
            )

            return ResolvedSessionComponent(
                configuration=resolved_configuration,
                source_kind=(
                    ResolvedWorkflowComponentSource.INFERRED
                    if len(override_fields) == 0
                    else ResolvedWorkflowComponentSource.OVERRIDDEN
                ),
                inferred_from_data=True,
                inference_resolution=inferred_session_resolution,
                applied_override_fields=override_fields,
            )
        case PresetReferenceUsageMode.NAMED:
            if normalized_reference.preset_name is None or normalized_reference.registry_scope is None:
                raise _calibration_error(
                    "session_reference",
                    "named session references require preset_name and registry_scope",
                )

            _, artifact = registry.resolve(
                category=PresetCategory.SESSION,
                preset_name=normalized_reference.preset_name,
                registry_scope=normalized_reference.registry_scope,
            )

            resolved_configuration = artifact.configuration

            override_fields = ()
            if normalized_reference.override_patch is not None:
                resolved_configuration = _apply_session_override_patch(
                    configuration=resolved_configuration,
                    override_patch=normalized_reference.override_patch,
                )
                override_fields = _session_override_fields(normalized_reference.override_patch)

            return ResolvedSessionComponent(
                configuration=resolved_configuration,
                source_kind=_source_kind_for_named_reference(
                    registry_scope=normalized_reference.registry_scope,
                    override_fields=override_fields,
                ),
                selected_preset=_selection_from_session_artifact(
                    artifact=artifact,
                    registry_scope=normalized_reference.registry_scope,
                ),
                applied_override_fields=override_fields,
            )
        case PresetReferenceUsageMode.EXPLICIT:
            if normalized_reference.configuration is None:
                raise _calibration_error(
                    "session_reference.configuration",
                    "is required when usage_mode=explicit",
                )

            return ResolvedSessionComponent(
                configuration=_normalize_session_configuration(normalized_reference.configuration),
                source_kind=ResolvedWorkflowComponentSource.FULLY_CUSTOM,
            )


def _resolve_known_news_component(
    *,
    registry: PresetRegistry,
    known_news_reference: Optional[KnownNewsPresetReference],
    market_profile: Optional[ResolvedMarketProfileComponent],
    diagnostics: list[ResolvedWorkflowDiagnostic],
) -> ResolvedKnownNewsComponent:
    profile_reference = (
        market_profile.configuration.known_news_reference if market_profile is not None else None
    )
    profile_policy = (
        market_profile.configuration.known_news_policy if market_profile is not None else None
    )

    selected_reference = known_news_reference
    recommendation_reference = profile_reference
    recommendation_policy = profile_policy if profile_reference is not None else None
    recommendation_applied = False

    if selected_reference is not None:
        normalized_selected_reference = _normalize_known_news_reference(selected_reference)
        normalized_profile_reference = (
            None if profile_reference is None else _normalize_known_news_reference(profile_reference)
        )

        if (
            normalized_profile_reference is not None
            and normalized_selected_reference != normalized_profile_reference
        ):
            diagnostics.append(
                _diagnostic(
                    component=ResolvedWorkflowComponent.KNOWN_NEWS,
                    severity=ResolvedWorkflowDiagnosticSeverity.WARNING,
                    code="conflicting_override",
                    message=(
                        "Explicit known-news selection replaced the market profile known-news "
                        "baseline or recommendation."
                    ),
                )
            )
    elif profile_reference is not None and profile_policy == MarketProfileKnownNewsPolicy.APPLY_BY_DEFAULT:
        selected_reference = profile_reference
        recommendation_applied = True

        diagnostics.append(
            _diagnostic(
                component=ResolvedWorkflowComponent.KNOWN_NEWS,
                severity=ResolvedWorkflowDiagnosticSeverity.INFO,
                code="profile_component_resolution_outcome",
                message=(
                    "The selected market profile applies its known-news reference by default, so "
                    "that baseline became part of the resolved workflow configuration."
                ),
            )
        )
    else:
        selected_reference = KnownNewsPresetReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name=DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME,
        )

        if profile_reference is not None and profile_policy == MarketProfileKnownNewsPolicy.SUGGEST_ONLY:
            diagnostics.append(
                _diagnostic(
                    component=ResolvedWorkflowComponent.KNOWN_NEWS,
                    severity=ResolvedWorkflowDiagnosticSeverity.INFO,
                    code="profile_component_resolution_outcome",
                    message=(
                        "The selected market profile only recommends known-news inputs, so the "
                        "resolved configuration kept the explicit default known_news=`none` baseline."
                    ),
                )
            )

    normalized_reference = _normalize_known_news_reference(selected_reference)
    match normalized_reference.usage_mode:
        case PresetReferenceUsageMode.NAMED:
            if normalized_reference.preset_name is None or normalized_reference.registry_scope is None:
                raise _calibration_error(
                    "known_news_reference",
                    "named known-news references require preset_name and registry_scope",
                )

            _, artifact = registry.resolve(
                category=PresetCategory.KNOWN_NEWS,
                preset_name=normalized_reference.preset_name,
                registry_scope=normalized_reference.registry_scope,
            )

            resolved_configuration = artifact.configuration

            override_fields = ()
            if normalized_reference.override_patch is not None:
                resolved_configuration = _apply_known_news_override_patch(
                    configuration=resolved_configuration,
                    override_patch=normalized_reference.override_patch,
                )
                override_fields = _known_news_override_fields(normalized_reference.override_patch)

            return ResolvedKnownNewsComponent(
                configuration=resolved_configuration,
                source_kind=_source_kind_for_named_reference(
                    registry_scope=normalized_reference.registry_scope,
                    override_fields=override_fields,
                ),
                selected_preset=_selection_from_known_news_artifact(
                    artifact=artifact,
                    registry_scope=normalized_reference.registry_scope,
                ),
                applied_override_fields=override_fields,
                recommended_reference=(
                    None if recommendation_reference is None else _normalize_known_news_reference(recommendation_reference)
                ),
                recommendation_policy=recommendation_policy,
                recommendation_applied=recommendation_applied,
            )
        case PresetReferenceUsageMode.EXPLICIT:
            if normalized_reference.configuration is None:
                raise _calibration_error(
                    "known_news_reference.configuration",
                    "is required when usage_mode=explicit",
                )

            return ResolvedKnownNewsComponent(
                configuration=_normalize_known_news_configuration(normalized_reference.configuration),
                source_kind=ResolvedWorkflowComponentSource.FULLY_CUSTOM,
                recommended_reference=(
                    None if recommendation_reference is None else _normalize_known_news_reference(recommendation_reference)
                ),
                recommendation_policy=recommendation_policy,
                recommendation_applied=recommendation_applied,
            )
        case PresetReferenceUsageMode.AUTOMATIC:
            raise _calibration_error(
                "known_news_reference.usage_mode",
                "automatic known-news resolution is not supported",
            )


def _session_configuration_from_inference(
    inference_resolution: MarketDataSessionResolution,
) -> SessionExplicitConfiguration:
    return _normalize_session_configuration(
        SessionExplicitConfiguration(
            exchange_code=inference_resolution.exchange_code,
            exchange_name=inference_resolution.exchange_name,
            timezone_name=inference_resolution.resolved_timezone_name,
            regular_session_windows=inference_resolution.regular_session_windows,
            recurring_closed_windows=inference_resolution.recurring_closed_windows,
        )
    )


def _apply_session_override_patch(
    *,
    configuration: SessionExplicitConfiguration,
    override_patch: SessionConfigurationOverridePatch,
) -> SessionExplicitConfiguration:
    return _normalize_session_configuration(
        replace(
            configuration,
            timezone_name=(
                configuration.timezone_name
                if override_patch.timezone_name is None
                else override_patch.timezone_name
            ),
            regular_session_windows=(
                configuration.regular_session_windows
                if override_patch.regular_session_windows is None
                else override_patch.regular_session_windows
            ),
            recurring_closed_windows=(
                configuration.recurring_closed_windows
                if override_patch.recurring_closed_windows is None
                else override_patch.recurring_closed_windows
            ),
        )
    )


def _apply_known_news_override_patch(
    *,
    configuration: KnownNewsExplicitConfiguration,
    override_patch: KnownNewsConfigurationOverridePatch,
) -> KnownNewsExplicitConfiguration:
    events_by_id = {event.event_id: event for event in configuration.events}
    for event_id in override_patch.removed_event_ids:
        if event_id not in events_by_id:
            raise _calibration_error(
                "known_news_reference.override_patch.removed_event_ids",
                f"cannot remove unknown known-news event_id `{event_id}` from the selected baseline",
            )

        del events_by_id[event_id]

    for event in override_patch.additional_events:
        if event.event_id in events_by_id:
            raise _calibration_error(
                "known_news_reference.override_patch.additional_events",
                f"cannot add duplicate known-news event_id `{event.event_id}` to the selected baseline",
            )

        events_by_id[event.event_id] = event

    return _normalize_known_news_configuration(
        KnownNewsExplicitConfiguration(
            calendar_timezone_name=(
                configuration.calendar_timezone_name
                if override_patch.calendar_timezone_name is None
                else override_patch.calendar_timezone_name
            ),
            events=tuple(events_by_id.values()),
        )
    )


def _apply_market_profile_override_patch(
    *,
    configuration: MarketProfileExplicitConfiguration,
    override_patch: MarketProfileOverridePatch,
) -> MarketProfileExplicitConfiguration:
    return _normalize_market_profile_configuration(
        replace(
            configuration,
            session_reference=(
                configuration.session_reference
                if override_patch.session_reference is None
                else override_patch.session_reference
            ),
            known_news_reference=(
                configuration.known_news_reference
                if override_patch.known_news_reference is None
                else override_patch.known_news_reference
            ),
            known_news_policy=(
                configuration.known_news_policy
                if override_patch.known_news_policy is None
                else override_patch.known_news_policy
            ),
            timestamp_timezone_name=(
                configuration.timestamp_timezone_name
                if override_patch.timestamp_timezone_name is None
                else override_patch.timestamp_timezone_name
            ),
            column_alias_hints=(
                configuration.column_alias_hints
                if override_patch.column_alias_hints is None
                else override_patch.column_alias_hints
            ),
            default_calibration_hint_ids=(
                configuration.default_calibration_hint_ids
                if override_patch.default_calibration_hint_ids is None
                else override_patch.default_calibration_hint_ids
            ),
        )
    )


def _append_inference_diagnostics(
    *,
    inference_resolution: MarketDataSessionResolution,
    diagnostics: list[ResolvedWorkflowDiagnostic],
) -> None:
    if inference_resolution.confidence_label == "low":
        diagnostics.append(
            _diagnostic(
                component=ResolvedWorkflowComponent.SESSION,
                severity=ResolvedWorkflowDiagnosticSeverity.WARNING,
                code="session_inference_low_confidence",
                message=(
                    "The inferred session baseline reported low confidence. Review the session "
                    "diagnostics before relying on the fitted model for downstream simulation."
                ),
            )
        )

    for warning in inference_resolution.ambiguity_warnings:
        diagnostics.append(
            _diagnostic(
                component=ResolvedWorkflowComponent.SESSION,
                severity=ResolvedWorkflowDiagnosticSeverity.WARNING,
                code="session_inference_ambiguous",
                message=warning,
            )
        )

    for discrepancy in inference_resolution.metadata_discrepancies:
        diagnostics.append(
            _diagnostic(
                component=ResolvedWorkflowComponent.SESSION,
                severity=ResolvedWorkflowDiagnosticSeverity.WARNING,
                code="session_inference_metadata_discrepancy",
                message=discrepancy,
            )
        )


def _selection_from_session_artifact(
    *,
    artifact: SessionPresetArtifact,
    registry_scope: PresetRegistryScope,
) -> ResolvedPresetSelection:
    return ResolvedPresetSelection(
        category=PresetCategory.SESSION,
        registry_scope=registry_scope,
        preset_name=artifact.preset_name,
    )


def _selection_from_known_news_artifact(
    *,
    artifact: KnownNewsPresetArtifact,
    registry_scope: PresetRegistryScope,
) -> ResolvedPresetSelection:
    return ResolvedPresetSelection(
        category=PresetCategory.KNOWN_NEWS,
        registry_scope=registry_scope,
        preset_name=artifact.preset_name,
    )


def _session_reference_replaces_inferred_baseline(
    reference: SessionPresetReference,
) -> bool:
    return not (
        reference.usage_mode == PresetReferenceUsageMode.AUTOMATIC
        and reference.override_patch is None
    )


def _source_kind_for_named_reference(
    *,
    registry_scope: PresetRegistryScope,
    override_fields: tuple[str, ...],
) -> ResolvedWorkflowComponentSource:
    if len(override_fields) > 0:
        return ResolvedWorkflowComponentSource.OVERRIDDEN
    if registry_scope == PresetRegistryScope.BUILT_IN:
        return ResolvedWorkflowComponentSource.BUILT_IN

    return ResolvedWorkflowComponentSource.FULLY_CUSTOM


def _session_override_fields(override_patch: SessionConfigurationOverridePatch) -> tuple[str, ...]:
    fields: list[str] = []

    if override_patch.timezone_name is not None:
        fields.append("timezone_name")
    if override_patch.regular_session_windows is not None:
        fields.append("regular_session_windows")
    if override_patch.recurring_closed_windows is not None:
        fields.append("recurring_closed_windows")

    return tuple(fields)


def _known_news_override_fields(override_patch: KnownNewsConfigurationOverridePatch) -> tuple[str, ...]:
    fields: list[str] = []

    if override_patch.calendar_timezone_name is not None:
        fields.append("calendar_timezone_name")
    if len(override_patch.additional_events) > 0:
        fields.append("additional_events")
    if len(override_patch.removed_event_ids) > 0:
        fields.append("removed_event_ids")

    return tuple(fields)


def _market_profile_override_fields(override_patch: MarketProfileOverridePatch) -> tuple[str, ...]:
    fields: list[str] = []

    if override_patch.session_reference is not None:
        fields.append("session_reference")
    if override_patch.known_news_reference is not None:
        fields.append("known_news_reference")
    if override_patch.known_news_policy is not None:
        fields.append("known_news_policy")
    if override_patch.timestamp_timezone_name is not None:
        fields.append("timestamp_timezone_name")
    if override_patch.column_alias_hints is not None:
        fields.append("column_alias_hints")
    if override_patch.default_calibration_hint_ids is not None:
        fields.append("default_calibration_hint_ids")

    return tuple(fields)


def _normalize_market_profile_reference(reference: MarketProfileReference) -> MarketProfileReference:
    return replace(
        reference,
        configuration=(
            None
            if reference.configuration is None
            else _normalize_market_profile_configuration(reference.configuration)
        ),
        override_patch=(
            None
            if reference.override_patch is None
            else _normalize_market_profile_override_patch(reference.override_patch)
        ),
    )


def _diagnostic(
    *,
    component: ResolvedWorkflowComponent,
    severity: ResolvedWorkflowDiagnosticSeverity,
    code: str,
    message: str,
) -> ResolvedWorkflowDiagnostic:
    return ResolvedWorkflowDiagnostic(
        component=component,
        severity=severity,
        code=code,
        message=message,
    )


def _validate_override_fields(fields: tuple[str, ...], field_path: str) -> None:
    seen_fields: set[str] = set()
    for field_name in fields:
        if not field_name.strip():
            raise _calibration_error(field_path, "must not contain empty override field names")
        if field_name in seen_fields:
            raise _calibration_error(field_path, "must not contain duplicate override field names")

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
