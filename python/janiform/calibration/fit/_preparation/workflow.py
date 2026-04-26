from __future__ import annotations

from dataclasses import replace
from datetime import datetime
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any, Callable, Optional

from ...._workflow_progress import AbstractProgress, NullProgress
from ...errors import calibration_error
from .._inference import (
    column_overrides_from_mapping,
    infer_fit_inputs,
    normalization_overrides_from_policy,
)
from .contracts import (
    DEFAULT_KNOWN_NEWS_COLUMNS,
    FitPreparationConfiguration,
    PreparedFitWorkflow,
)
from ...market_data.contracts import (
    CalibrationFlatCarryInput,
    CalibrationInputFileFormat,
    CalibrationInputFileReference,
    CalibrationMarketDataColumnMapping,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
    CalibrationSessionConfigurationInput,
    CalibrationTickDataColumnMapping,
)
from ...market_data.request import MarketDataCalibrationRequest
from ...market_data.translation import (
    NormalizedMarketDataSource,
    infer_bundled_market_profile_for_tick_data_path,
    normalize_market_data_source,
    resolve_market_data_session,
    translate_market_data_request_to_prepared_calibration_from_phases,
)
from ...market_data.workflow import (
    PreparedMarketDataCalibration,
    _prepare_market_data_calibration_from_translation_result,
)
from ...presets.references import (
    KnownNewsPresetReference,
    MarketProfileColumnAliasHint,
    MarketProfileExplicitConfiguration,
    MarketProfileKnownNewsPolicy,
    MarketProfileReference,
    PresetReferenceUsageMode,
    PresetRegistryScope,
    SessionPresetReference,
)
from ...presets.resolution import (
    ResolvedPresetWorkflowConfiguration,
    resolve_preset_workflow_configuration,
)
from ...sessions.contracts import MarketDataSessionResolution
from .configuration import (
    build_known_news_input,
    build_session_configuration_input,
    resolved_configuration_requires_materialization,
)


def fit_progress_total_steps(configuration: FitPreparationConfiguration) -> int:
    if configuration.requires_session_inference:
        return 9

    return 8


def effective_workflow_scope(
    configuration: FitPreparationConfiguration,
    *,
    option_surface_path: Optional[str | Path],
) -> CalibrationMarketDataWorkflowScope:
    if configuration.workflow_scope_override is not None:
        return configuration.workflow_scope_override
    if option_surface_path is not None:
        return CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE

    return CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE


def build_market_data_request(
    configuration: FitPreparationConfiguration,
    *,
    run_id: str,
    workflow_scope: CalibrationMarketDataWorkflowScope,
    tick_data_source: CalibrationInputFileReference,
    option_surface_source: Optional[CalibrationInputFileReference],
    option_surface_valuation_time_utc: Optional[datetime],
    carry: Optional[CalibrationFlatCarryInput],
    resolved_configuration: ResolvedPresetWorkflowConfiguration,
    tick_data_column_mapping: CalibrationTickDataColumnMapping,
    normalization_policy: CalibrationMarketDataNormalizationPolicy,
    working_directory: Optional[Path],
) -> MarketDataCalibrationRequest:
    session_configuration = build_session_configuration_input(
        resolved_configuration.session,
        working_directory=working_directory,
    )
    known_news_input = build_known_news_input(
        resolved_configuration.known_news.configuration,
        working_directory=working_directory,
    )

    column_mapping = CalibrationMarketDataColumnMapping(
        tick_data=tick_data_column_mapping,
        option_surface=configuration.option_surface_column_mapping,
        known_news=(DEFAULT_KNOWN_NEWS_COLUMNS if known_news_input is not None else None),
    )

    return MarketDataCalibrationRequest(
        schema_version="calibration.market_data.request.v1",
        run_id=run_id,
        tick_data_source=tick_data_source,
        session_configuration=session_configuration,
        column_mapping=column_mapping,
        workflow_scope=workflow_scope,
        normalization_policy=normalization_policy,
        output_preferences=configuration.output_preferences,
        optimizer_settings_by_stage=configuration.optimizer_settings_by_stage,
        preprocessing_settings=configuration.preprocessing_settings,
        option_surface_source=option_surface_source,
        option_surface_valuation_time_utc=option_surface_valuation_time_utc,
        carry=carry,
        pricing_interval_schedule_policy=configuration.pricing_interval_schedule_policy,
        dynamics_search_restart_candidate_budget=(
            configuration.dynamics_search_restart_candidate_budget
        ),
        known_news_inputs=known_news_input,
        mean_reversion=(
            None
            if workflow_scope == CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY
            else (
                configuration.mean_reversion
                if configuration.mean_reversion is not None
                else configuration.default_endogenous_mean_reversion_configuration
            )
        ),
    )


def infer_session_resolution(
    configuration: FitPreparationConfiguration,
    *,
    run_id: str,
    workflow_scope: CalibrationMarketDataWorkflowScope,
    tick_data_source: CalibrationInputFileReference,
    option_surface_source: Optional[CalibrationInputFileReference],
    option_surface_valuation_time_utc: Optional[datetime],
    carry: Optional[CalibrationFlatCarryInput],
    tick_data_column_mapping: CalibrationTickDataColumnMapping,
    normalization_policy: CalibrationMarketDataNormalizationPolicy,
    normalized_market_data: NormalizedMarketDataSource,
    progress: AbstractProgress,
) -> Optional[MarketDataSessionResolution]:
    if not configuration.requires_session_inference:
        return None

    if configuration.exchange_code_for_inference is None:
        raise _missing_automatic_session_exchange_context_error()

    request = MarketDataCalibrationRequest(
        schema_version="calibration.market_data.request.v1",
        run_id=run_id,
        tick_data_source=tick_data_source,
        session_configuration=CalibrationSessionConfigurationInput(
            exchange_code=configuration.exchange_code_for_inference,
            session_timezone=configuration.session_timezone,
        ),
        column_mapping=CalibrationMarketDataColumnMapping(
            tick_data=tick_data_column_mapping,
            option_surface=configuration.option_surface_column_mapping,
        ),
        workflow_scope=workflow_scope,
        normalization_policy=normalization_policy,
        output_preferences=configuration.output_preferences,
        optimizer_settings_by_stage=configuration.optimizer_settings_by_stage,
        preprocessing_settings=configuration.preprocessing_settings,
        option_surface_source=option_surface_source,
        option_surface_valuation_time_utc=option_surface_valuation_time_utc,
        carry=carry,
        pricing_interval_schedule_policy=configuration.pricing_interval_schedule_policy,
        dynamics_search_restart_candidate_budget=(
            configuration.dynamics_search_restart_candidate_budget
        ),
    )

    progress.update(detail="resolving session from market data")

    return resolve_market_data_session(
        session_configuration=request.session_configuration,
        normalized_market_data=normalized_market_data,
    )


def configuration_with_native_market_profile_inference(
    configuration: FitPreparationConfiguration,
    *,
    tick_data_path: Path,
) -> FitPreparationConfiguration:
    session_reference_blocks_market_profile_inference = (
        configuration.effective_session_reference is not None
        and configuration.effective_session_reference.usage_mode != PresetReferenceUsageMode.AUTOMATIC
    )

    if (
        configuration.market_profile_reference is not None
        or session_reference_blocks_market_profile_inference
        or configuration.exchange_code_for_inference is not None
    ):
        return configuration

    raw_profile = infer_bundled_market_profile_for_tick_data_path(tick_data_path)
    if raw_profile is None:
        if (
            configuration.requires_session_inference
            and configuration.exchange_code_for_inference is None
        ):
            raise _missing_automatic_session_exchange_context_error()

        return configuration

    market_profile_reference = MarketProfileReference(
        usage_mode=PresetReferenceUsageMode.EXPLICIT,
        configuration=_market_profile_configuration_from_native_payload(raw_profile),
    )

    return replace(
        configuration,
        market_profile_reference=market_profile_reference,
        resolved_market_profile_configuration=market_profile_reference.configuration,
        requires_session_inference=False,
        exchange_code_for_inference=None,
    )


def _missing_automatic_session_exchange_context_error() -> Exception:
    return calibration_error(
        "exchange_code",
        (
            "is required for automatic session inference when no session preset, market "
            "profile, or native tick-data profile inference provides the exchange baseline"
        ),
    )


def _market_profile_configuration_from_native_payload(
    raw_profile: dict[str, Any],
) -> MarketProfileExplicitConfiguration:
    known_news_preset_name = raw_profile.get("known_news_preset_name")

    raw_column_alias_hints = raw_profile.get("column_alias_hints", ())

    raw_default_calibration_hint_ids = raw_profile.get(
        "default_calibration_hint_ids",
        (),
    )

    raw_known_news_policy = raw_profile.get(
        "known_news_policy",
        MarketProfileKnownNewsPolicy.SUGGEST_ONLY,
    )

    if not isinstance(raw_column_alias_hints, list | tuple):
        raise calibration_error(
            "market_profile.column_alias_hints",
            "must be a sequence in the native market-profile inference payload",
        )

    if not isinstance(raw_default_calibration_hint_ids, list | tuple):
        raise calibration_error(
            "market_profile.default_calibration_hint_ids",
            "must be a sequence in the native market-profile inference payload",
        )

    return MarketProfileExplicitConfiguration(
        profile_name=str(raw_profile["profile_name"]),
        session_reference=SessionPresetReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name=str(raw_profile["session_preset_name"]),
        ),
        known_news_reference=(
            KnownNewsPresetReference(
                usage_mode=PresetReferenceUsageMode.NAMED,
                registry_scope=PresetRegistryScope.BUILT_IN,
                preset_name=str(known_news_preset_name),
            )
            if known_news_preset_name is not None
            else None
        ),
        known_news_policy=MarketProfileKnownNewsPolicy(
            raw_known_news_policy or MarketProfileKnownNewsPolicy.SUGGEST_ONLY
        ),
        timestamp_timezone_name=raw_profile.get("timestamp_timezone_name"),
        column_alias_hints=tuple(
            MarketProfileColumnAliasHint.from_mapping(
                raw_hint,
                field_path=f"market_profile.column_alias_hints[{hint_index}]",
            )
            for hint_index, raw_hint in enumerate(raw_column_alias_hints)
        ),
        default_calibration_hint_ids=tuple(
            str(hint_id) for hint_id in raw_default_calibration_hint_ids
        ),
    )


def prepare_fit_workflow(
    configuration: FitPreparationConfiguration,
    *,
    run_id: Optional[str],
    tick_data_path: str | Path,
    tick_data_format: Optional[CalibrationInputFileFormat],
    tick_data_provenance_label: str,
    option_surface_path: Optional[str | Path],
    option_surface_format: CalibrationInputFileFormat,
    option_surface_provenance_label: str,
    option_surface_valuation_time_utc: Optional[datetime] = None,
    carry: Optional[CalibrationFlatCarryInput] = None,
    progress: Optional[AbstractProgress] = None,
    temporary_directory_factory: Callable[..., TemporaryDirectory[str]] = TemporaryDirectory,
    normalize_market_data_source_method: Callable[..., NormalizedMarketDataSource] = normalize_market_data_source,
    resolve_market_data_session_method: Callable[..., MarketDataSessionResolution] = resolve_market_data_session,
    resolve_preset_workflow_configuration_method: Callable[..., ResolvedPresetWorkflowConfiguration] = resolve_preset_workflow_configuration,
    translate_market_data_request_to_prepared_calibration_from_phases_method: Callable[..., Any] = translate_market_data_request_to_prepared_calibration_from_phases,
    prepare_market_data_calibration_from_translation_result_method: Callable[..., PreparedMarketDataCalibration] = _prepare_market_data_calibration_from_translation_result,
) -> PreparedFitWorkflow:
    effective_progress = NullProgress() if progress is None else progress
    tick_data_path = Path(tick_data_path)

    configuration = configuration_with_native_market_profile_inference(
        configuration,
        tick_data_path=tick_data_path,
    )

    workflow_scope = effective_workflow_scope(
        configuration,
        option_surface_path=option_surface_path,
    )

    input_inference = infer_fit_inputs(
        tick_data_path=tick_data_path,
        tick_data_format=tick_data_format,
        tick_data_provenance_label=tick_data_provenance_label,
        workflow_scope=workflow_scope,
        run_id=run_id,
        column_overrides=column_overrides_from_mapping(configuration.tick_data_column_mapping),
        normalization_overrides=normalization_overrides_from_policy(configuration.normalization_policy),
        market_profile_configuration=configuration.resolved_market_profile_configuration,
    )

    option_surface_source = (
        CalibrationInputFileReference(
            path=str(Path(option_surface_path)),
            file_format=option_surface_format,
            provenance_label=option_surface_provenance_label,
        )
        if option_surface_path is not None
        else None
    )

    temporary_directory: Optional[TemporaryDirectory[str]] = None
    try:
        effective_progress.update(detail="normalize source")
        normalized_market_data = normalize_market_data_source_method(
            tick_data_source=input_inference.tick_data_source,
            tick_data_column_mapping=input_inference.tick_data_column_mapping,
            normalization_policy=input_inference.normalization_policy,
            require_hawkes=(workflow_scope != CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY),
        )

        effective_progress.update(detail="infer session")
        inferred_session_resolution = infer_session_resolution(
            configuration,
            run_id=input_inference.run_id,
            workflow_scope=workflow_scope,
            tick_data_source=input_inference.tick_data_source,
            option_surface_source=option_surface_source,
            option_surface_valuation_time_utc=option_surface_valuation_time_utc,
            carry=carry,
            tick_data_column_mapping=input_inference.tick_data_column_mapping,
            normalization_policy=input_inference.normalization_policy,
            normalized_market_data=normalized_market_data,
            progress=effective_progress,
        )

        effective_progress.update(detail="resolve presets")
        resolved_configuration = resolve_preset_workflow_configuration_method(
            registry=configuration.registry,
            market_profile_reference=configuration.market_profile_reference,
            session_reference=configuration.effective_session_reference,
            known_news_reference=configuration.known_news_reference,
            inferred_session_resolution=inferred_session_resolution,
        )

        if resolved_configuration_requires_materialization(resolved_configuration):
            temporary_directory = temporary_directory_factory(prefix="janiform-fit-")
            working_directory = Path(temporary_directory.name)
        else:
            working_directory: Optional[Path] = None

        effective_progress.update(detail="build market-data request")
        request = build_market_data_request(
            configuration,
            run_id=input_inference.run_id,
            workflow_scope=workflow_scope,
            tick_data_source=input_inference.tick_data_source,
            option_surface_source=option_surface_source,
            option_surface_valuation_time_utc=option_surface_valuation_time_utc,
            carry=carry,
            resolved_configuration=resolved_configuration,
            tick_data_column_mapping=input_inference.tick_data_column_mapping,
            normalization_policy=input_inference.normalization_policy,
            working_directory=working_directory,
        )

        effective_progress.update(detail="resolve final session")
        session_resolution = resolve_market_data_session_method(
            session_configuration=request.session_configuration,
            normalized_market_data=normalized_market_data,
        )

        effective_progress.update(detail="prepare stage inputs")
        translation_result = translate_market_data_request_to_prepared_calibration_from_phases_method(
            request=request,
            normalized_market_data=normalized_market_data,
            session_resolution=session_resolution,
            cpu_count=configuration.cpu_count,
        )

        prepared = prepare_market_data_calibration_from_translation_result_method(
            request=request,
            translation_result=translation_result,
        )
    except Exception:
        if temporary_directory is not None:
            temporary_directory.cleanup()

        raise

    return PreparedFitWorkflow(
        run_id=input_inference.run_id,
        workflow_scope=workflow_scope,
        prepared=prepared,
        resolved_configuration=resolved_configuration,
        input_inference=input_inference,
        fit_warning=translation_result.fit_warning,
        reference_price=translation_result.reference_price,
        temporary_directory=temporary_directory,
    )
