from contextlib import AbstractContextManager, nullcontext, redirect_stdout
from dataclasses import dataclass
from datetime import UTC, datetime
from io import StringIO
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any, Literal, Optional, overload, cast

from .input_resolution import (
    HighLevelFitInference,
    build_fit_inference_payload,
    column_overrides_from_mapping,
    infer_high_level_fit_inputs,
    normalization_overrides_from_policy,
)
from .model import (
    JaniformModel,
    _build_model_metadata,
    _extract_model_parameters,
    _raw_stage_report_payloads,
)
from .report import JaniformFitReport
from .._errors import _calibration_error
from ..market_data.contracts import (
    CalibrationExecutionMode,
    CalibrationInputFileFormat,
    CalibrationInputFileReference,
    CalibrationKnownNewsColumnMapping,
    CalibrationKnownNewsInput,
    CalibrationMarketDataColumnMapping,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataOutputPreferences,
    CalibrationMarketDataWorkflowScope,
    CalibrationOptionSurfaceColumnMapping,
    CalibrationSessionConfigurationInput,
    CalibrationTickDataColumnMapping,
)
from ..market_data.ingestion import load_market_data_calibration_inputs
from ..market_data.request import MarketDataCalibrationRequest
from ..market_data.translation import (
    _load_market_data_translation_inputs,
    _translate_loaded_market_data_request_to_staged_execution_input,
)
from ..market_data.workflow import (
    PreparedMarketDataCalibration,
    _prepare_market_data_calibration_from_translation_result,
)
from ..presets.references import (
    KnownNewsExplicitConfiguration,
    KnownNewsPresetReference,
    MarketProfileExplicitConfiguration,
    MarketProfileReference,
    PresetReferenceUsageMode,
    PresetRegistryScope,
    SessionExplicitConfiguration,
    SessionPresetReference,
)
from ..presets.registry import PresetRegistry
from ..presets.resolution import (
    ResolvedPresetWorkflowConfiguration,
    ResolvedSessionComponent,
    resolve_preset_workflow_configuration,
)
from ..sessions.contracts import MarketDataSessionResolution
from ..sessions.resolution import (
    _SessionTemplate,
    _build_calendar_registry_json_from_windows,
    _build_recurring_closed_windows,
    _looks_like_continuous_weekday_template,
    _regular_session_windows_from_exchange_artifact,
    resolve_market_data_session_configuration,
)
from ..staged.contracts import CalibrationStageExecutionStatus
from ..staged.runtime import _run_staged_calibration_with_raw_payload
from ..._workflow_outputs import write_fit_artifacts
from ..._workflow_progress import AbstractProgress, JaniformOutputController, NullProgress
from ...calendar import CalendarRegistry


_DEFAULT_HIGH_LEVEL_KNOWN_NEWS_COLUMNS = CalibrationKnownNewsColumnMapping(
    event_timestamp_column="event_timestamp_utc",
    event_identifier_column="event_id",
)
_DEFAULT_HIGH_LEVEL_OUTPUT_PREFERENCES = CalibrationMarketDataOutputPreferences(
    execution_mode=CalibrationExecutionMode.MEMORY,
    emit_derived_staged_execution_input=False,
    emit_input_provenance_report=True,
)


@dataclass(frozen=True, slots=True)
class _PreparedHighLevelFitWorkflow:
    run_id: str
    prepared: PreparedMarketDataCalibration
    resolved_configuration: ResolvedPresetWorkflowConfiguration
    input_inference: HighLevelFitInference
    fit_warning: Optional[str]
    reference_price: Optional[float]
    temporary_directory: Optional[TemporaryDirectory[str]]


class JaniformEstimator:
    def __init__(
        self,
        *,
        workflow_scope: CalibrationMarketDataWorkflowScope = CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        tick_data_column_mapping: Optional[CalibrationTickDataColumnMapping] = None,
        normalization_policy: Optional[CalibrationMarketDataNormalizationPolicy] = None,
        option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping] = None,
        output_preferences: Optional[CalibrationMarketDataOutputPreferences] = None,
        market_profile: Optional[str | MarketProfileReference] = None,
        session: Optional[str | Path | SessionPresetReference] = None,
        known_news: Optional[str | KnownNewsPresetReference] = None,
        exchange_code: Optional[str] = None,
        session_timezone: Optional[str] = None,
        registry: Optional[PresetRegistry] = None,
        silent: bool = False,
    ) -> None:
        if exchange_code is not None and not exchange_code.strip():
            raise _calibration_error("exchange_code", "must be a non-empty string when provided")
        if session_timezone is not None and not session_timezone.strip():
            raise _calibration_error(
                "session_timezone",
                "must be a non-empty string when provided",
            )

        self._workflow_scope: CalibrationMarketDataWorkflowScope = workflow_scope
        self._tick_data_column_mapping: Optional[CalibrationTickDataColumnMapping] = (
            tick_data_column_mapping
        )
        self._normalization_policy: Optional[CalibrationMarketDataNormalizationPolicy] = (
            normalization_policy
        )
        self._option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping] = (
            option_surface_column_mapping
        )
        self._output_preferences: CalibrationMarketDataOutputPreferences = (
            _DEFAULT_HIGH_LEVEL_OUTPUT_PREFERENCES
            if output_preferences is None
            else output_preferences
        )
        self._registry: PresetRegistry = PresetRegistry() if registry is None else registry
        self._exchange_code: Optional[str] = exchange_code
        self._session_timezone: Optional[str] = session_timezone
        self._silent: bool = silent
        self._market_profile_reference: Optional[MarketProfileReference] = (
            _coerce_market_profile_reference(market_profile)
        )
        self._session_reference: Optional[SessionPresetReference] = _coerce_session_reference(
            session,
            exchange_code=exchange_code,
        )
        self._known_news_reference: Optional[KnownNewsPresetReference] = (
            _coerce_known_news_reference(known_news)
        )

    @overload
    def fit(
        self,
        *,
        run_id: Optional[str] = None,
        tick_data_path: str | Path,
        tick_data_format: Optional[CalibrationInputFileFormat] = None,
        tick_data_provenance_label: str = "tick_data",
        option_surface_path: Optional[str | Path] = None,
        option_surface_format: CalibrationInputFileFormat = CalibrationInputFileFormat.PARQUET,
        option_surface_provenance_label: str = "option_surface",
        output_dir: Optional[str | Path] = None,
        silent: Optional[bool] = None,
        report: Literal[False] = False,
    ) -> JaniformModel: ...

    @overload
    def fit(
        self,
        *,
        run_id: Optional[str] = None,
        tick_data_path: str | Path,
        tick_data_format: Optional[CalibrationInputFileFormat] = None,
        tick_data_provenance_label: str = "tick_data",
        option_surface_path: Optional[str | Path] = None,
        option_surface_format: CalibrationInputFileFormat = CalibrationInputFileFormat.PARQUET,
        option_surface_provenance_label: str = "option_surface",
        output_dir: Optional[str | Path] = None,
        silent: Optional[bool] = None,
        report: Literal[True],
    ) -> tuple[Optional[JaniformModel], JaniformFitReport]: ...

    def fit(
        self,
        *,
        run_id: Optional[str] = None,
        tick_data_path: str | Path,
        tick_data_format: Optional[CalibrationInputFileFormat] = None,
        tick_data_provenance_label: str = "tick_data",
        option_surface_path: Optional[str | Path] = None,
        option_surface_format: CalibrationInputFileFormat = CalibrationInputFileFormat.PARQUET,
        option_surface_provenance_label: str = "option_surface",
        output_dir: Optional[str | Path] = None,
        silent: Optional[bool] = None,
        report: bool = False,
    ) -> JaniformModel | tuple[Optional[JaniformModel], JaniformFitReport]:
        if not tick_data_provenance_label.strip():
            raise _calibration_error(
                "tick_data_provenance_label",
                "must be a non-empty string",
            )
        if not option_surface_provenance_label.strip():
            raise _calibration_error(
                "option_surface_provenance_label",
                "must be a non-empty string",
            )

        effective_silent = self._silent if silent is None else silent
        output_controller = JaniformOutputController(silent=effective_silent)
        prepared_workflow: Optional[_PreparedHighLevelFitWorkflow] = None

        with self._stdout_suppression_context(effective_silent=effective_silent):
            try:
                with output_controller.progress(
                    "fit",
                    total_steps=self._fit_progress_total_steps(),
                ) as progress:
                    progress.update(detail="inferring workflow inputs")
                    prepared_workflow = self._prepare_fit_workflow(
                        run_id=run_id,
                        tick_data_path=tick_data_path,
                        tick_data_format=tick_data_format,
                        tick_data_provenance_label=tick_data_provenance_label,
                        option_surface_path=option_surface_path,
                        option_surface_format=option_surface_format,
                        option_surface_provenance_label=option_surface_provenance_label,
                        progress=progress,
                    )
                    if prepared_workflow.fit_warning is not None:
                        output_controller.warning(prepared_workflow.fit_warning)
                    progress.update(detail="running calibration")
                    staged_response, raw_response_payload = _run_staged_calibration_with_raw_payload(
                        prepared_workflow.prepared.execution_input,
                        calendar_registry=CalendarRegistry(
                            prepared_workflow.prepared.session_resolution.resolved_session_definition_json
                        ),
                    )
            finally:
                if (
                    prepared_workflow is not None
                    and prepared_workflow.temporary_directory is not None
                ):
                    prepared_workflow.temporary_directory.cleanup()

        fit_report = JaniformFitReport(
            run_id=prepared_workflow.run_id,
            workflow_scope=self._workflow_scope,
            success=staged_response.status == CalibrationStageExecutionStatus.SUCCESS,
            resolved_configuration=prepared_workflow.resolved_configuration,
            session_resolution=prepared_workflow.prepared.session_resolution,
            preparation_diagnostics=prepared_workflow.prepared.diagnostics,
            preparation_provenance=prepared_workflow.prepared.provenance,
            execution_input=prepared_workflow.prepared.execution_input,
            staged_response=staged_response,
            stage_report_payloads=_raw_stage_report_payloads(raw_response_payload),
        )

        if staged_response.status != CalibrationStageExecutionStatus.SUCCESS:
            if report:
                return None, fit_report

            failure_kind = (
                staged_response.failure.failure_kind
                if staged_response.failure is not None
                else "fit_failed"
            )
            raise RuntimeError(
                f"Janiform fit failed for run_id `{prepared_workflow.run_id}` with failure_kind `{failure_kind}`"
            )

        model = JaniformModel(
            run_id=prepared_workflow.run_id,
            workflow_scope=self._workflow_scope,
            parameters=_extract_model_parameters(fit_report.stage_report_payloads),
            metadata=_build_model_metadata(
                resolved_configuration=prepared_workflow.resolved_configuration,
                prepared=prepared_workflow.prepared,
                staged_response=staged_response,
                stage_report_payloads=fit_report.stage_report_payloads,
                input_inference=build_fit_inference_payload(
                    inference=prepared_workflow.input_inference,
                    workflow_scope=self._workflow_scope,
                    market_profile_name=(
                        prepared_workflow.resolved_configuration.market_profile.selected_preset.preset_name
                        if (
                            prepared_workflow.resolved_configuration.market_profile is not None
                            and prepared_workflow.resolved_configuration.market_profile.selected_preset is not None
                        )
                        else None
                    ),
                    known_news_preset_name=(
                        prepared_workflow.resolved_configuration.known_news.selected_preset.preset_name
                        if prepared_workflow.resolved_configuration.known_news.selected_preset is not None
                        else None
                    ),
                    known_news_recommended_name=(
                        prepared_workflow.resolved_configuration.known_news.recommended_reference.preset_name
                        if prepared_workflow.resolved_configuration.known_news.recommended_reference is not None
                        else None
                    ),
                    known_news_recommendation_applied=(
                        prepared_workflow.resolved_configuration.known_news.recommendation_applied
                    ),
                    session_source_kind=prepared_workflow.resolved_configuration.session.source_kind.value,
                    known_news_source_kind=prepared_workflow.resolved_configuration.known_news.source_kind.value,
                ),
                reference_price=prepared_workflow.reference_price,
            ),
            resolved_configuration=prepared_workflow.resolved_configuration,
            session_resolution=prepared_workflow.prepared.session_resolution,
            _execution_input=prepared_workflow.prepared.execution_input,
            _stage_report_payloads=fit_report.stage_report_payloads,
        )
        if output_dir is not None:
            write_fit_artifacts(
                model=model,
                fit_report=fit_report,
                output_dir=output_dir,
            )
        if report:
            return model, fit_report

        return model

    def _fit_progress_total_steps(self) -> int:
        if self._requires_session_inference():
            return 9

        return 7

    def _build_market_data_request(
        self,
        *,
        run_id: str,
        tick_data_source: CalibrationInputFileReference,
        option_surface_source: Optional[CalibrationInputFileReference],
        resolved_configuration: ResolvedPresetWorkflowConfiguration,
        tick_data_column_mapping: CalibrationTickDataColumnMapping,
        normalization_policy: CalibrationMarketDataNormalizationPolicy,
        working_directory: Optional[Path],
    ) -> MarketDataCalibrationRequest:
        session_configuration = _build_session_configuration_input(
            resolved_configuration.session,
            working_directory=working_directory,
        )
        known_news_input = _build_known_news_input(
            resolved_configuration.known_news.configuration,
            working_directory=working_directory,
        )
        column_mapping = CalibrationMarketDataColumnMapping(
            tick_data=tick_data_column_mapping,
            option_surface=self._option_surface_column_mapping,
            known_news=(
                _DEFAULT_HIGH_LEVEL_KNOWN_NEWS_COLUMNS
                if known_news_input is not None
                else None
            ),
        )

        return MarketDataCalibrationRequest(
            schema_version="calibration.market_data.request.v1",
            run_id=run_id,
            tick_data_source=tick_data_source,
            session_configuration=session_configuration,
            column_mapping=column_mapping,
            workflow_scope=self._workflow_scope,
            normalization_policy=normalization_policy,
            output_preferences=self._output_preferences,
            option_surface_source=option_surface_source,
            known_news_inputs=known_news_input,
        )

    def _prepare_fit_workflow(
        self,
        *,
        run_id: Optional[str],
        tick_data_path: str | Path,
        tick_data_format: Optional[CalibrationInputFileFormat],
        tick_data_provenance_label: str,
        option_surface_path: Optional[str | Path],
        option_surface_format: CalibrationInputFileFormat,
        option_surface_provenance_label: str,
        progress: Optional[AbstractProgress] = None,
    ) -> _PreparedHighLevelFitWorkflow:
        effective_progress = NullProgress() if progress is None else progress
        resolved_market_profile_configuration = (
            self._resolve_market_profile_configuration()
        )
        input_inference = infer_high_level_fit_inputs(
            tick_data_path=Path(tick_data_path),
            tick_data_format=tick_data_format,
            tick_data_provenance_label=tick_data_provenance_label,
            workflow_scope=self._workflow_scope,
            run_id=run_id,
            column_overrides=column_overrides_from_mapping(self._tick_data_column_mapping),
            normalization_overrides=normalization_overrides_from_policy(self._normalization_policy),
            market_profile_configuration=resolved_market_profile_configuration,
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
            effective_progress.update(detail="resolving session configuration")
            inferred_session_resolution = self._infer_session_resolution(
                run_id=input_inference.run_id,
                tick_data_source=input_inference.tick_data_source,
                tick_data_column_mapping=input_inference.tick_data_column_mapping,
                normalization_policy=input_inference.normalization_policy,
                progress=effective_progress,
            )
            effective_progress.update(detail="resolving preset configuration")
            resolved_configuration = resolve_preset_workflow_configuration(
                registry=self._registry,
                market_profile_reference=self._market_profile_reference,
                session_reference=self._effective_session_reference(),
                known_news_reference=self._known_news_reference,
                inferred_session_resolution=inferred_session_resolution,
            )
            requires_materialization = _resolved_configuration_requires_materialization(
                resolved_configuration
            )
            if requires_materialization:
                temporary_directory = TemporaryDirectory(prefix="janiform-fit-")
                working_directory = Path(temporary_directory.name)
            else:
                working_directory = None
            effective_progress.update(detail="building market-data request")
            request = self._build_market_data_request(
                run_id=input_inference.run_id,
                tick_data_source=input_inference.tick_data_source,
                option_surface_source=option_surface_source,
                resolved_configuration=resolved_configuration,
                tick_data_column_mapping=input_inference.tick_data_column_mapping,
                normalization_policy=input_inference.normalization_policy,
                working_directory=working_directory,
            )
            effective_progress.update(detail="loading market data")
            loaded_inputs_result = _load_market_data_translation_inputs(request)
            effective_progress.update(detail="translating market data")
            translation_result = _translate_loaded_market_data_request_to_staged_execution_input(
                normalized_request=request,
                loaded_inputs_result=loaded_inputs_result,
            )
            prepared = _prepare_market_data_calibration_from_translation_result(
                request=request,
                translation_result=translation_result,
            )
        except Exception:
            if temporary_directory is not None:
                temporary_directory.cleanup()
            raise

        return _PreparedHighLevelFitWorkflow(
            run_id=input_inference.run_id,
            prepared=prepared,
            resolved_configuration=resolved_configuration,
            input_inference=input_inference,
            fit_warning=_detect_interval_data_warning(
                translation_result.loaded_inputs.tick_data.records
            ),
            reference_price=_extract_reference_price(
                translation_result.loaded_inputs.tick_data.records
            ),
            temporary_directory=temporary_directory,
        )

    def _effective_session_reference(self) -> Optional[SessionPresetReference]:
        if self._session_reference is not None:
            return self._session_reference

        if not self._requires_session_inference():
            return None

        return SessionPresetReference(
            usage_mode=PresetReferenceUsageMode.AUTOMATIC,
            exchange_code_hint=self._exchange_code,
            timezone_name_hint=self._session_timezone,
        )

    def _infer_session_resolution(
        self,
        *,
        run_id: str,
        tick_data_source: CalibrationInputFileReference,
        tick_data_column_mapping: CalibrationTickDataColumnMapping,
        normalization_policy: CalibrationMarketDataNormalizationPolicy,
        progress: AbstractProgress,
    ) -> Optional[MarketDataSessionResolution]:
        if not self._requires_session_inference():
            return None

        exchange_code = self._resolve_exchange_code_for_inference()
        request = MarketDataCalibrationRequest(
            schema_version="calibration.market_data.request.v1",
            run_id=run_id,
            tick_data_source=tick_data_source,
            session_configuration=CalibrationSessionConfigurationInput(
                exchange_code=exchange_code,
                session_timezone=self._session_timezone,
            ),
            column_mapping=CalibrationMarketDataColumnMapping(
                tick_data=tick_data_column_mapping,
            ),
            workflow_scope=self._workflow_scope,
            normalization_policy=normalization_policy,
            output_preferences=self._output_preferences,
        )
        progress.update(detail="loading tick data for session inference")
        loaded_inputs = load_market_data_calibration_inputs(request)
        progress.update(detail="resolving inferred session")
        resolved_session = resolve_market_data_session_configuration(
            request,
            loaded_inputs.tick_data,
        )

        return resolved_session.session_resolution

    def _requires_session_inference(self) -> bool:
        if self._session_reference is not None:
            return self._session_reference.usage_mode == PresetReferenceUsageMode.AUTOMATIC

        market_profile_configuration = self._resolve_market_profile_configuration()
        if market_profile_configuration is None:
            return True

        return market_profile_configuration.session_reference.usage_mode == PresetReferenceUsageMode.AUTOMATIC

    def _resolve_market_profile_configuration(
        self,
    ) -> Optional[MarketProfileExplicitConfiguration]:
        if self._market_profile_reference is None:
            return None

        if self._market_profile_reference.usage_mode == PresetReferenceUsageMode.EXPLICIT:
            configuration = self._market_profile_reference.configuration
            if configuration is None:
                raise _calibration_error(
                    "market_profile.configuration",
                    "is required when usage_mode=explicit",
                )

            return configuration

        if self._market_profile_reference.preset_name is None:
            raise _calibration_error(
                "market_profile.preset_name",
                "is required for named market-profile references",
            )
        if self._market_profile_reference.registry_scope is None:
            raise _calibration_error(
                "market_profile.registry_scope",
                "is required for named market-profile references",
            )

        _, artifact = self._registry.resolve_market_profile_preset(
            self._market_profile_reference.preset_name,
            registry_scope=self._market_profile_reference.registry_scope,
        )
        return artifact.configuration

    def _resolve_exchange_code_for_inference(self) -> str:
        if self._exchange_code is not None:
            return self._exchange_code

        if self._session_reference is not None:
            return _exchange_code_from_session_reference(self._session_reference, self._registry)

        if self._market_profile_reference is not None:
            return _exchange_code_from_market_profile_reference(
                self._market_profile_reference,
                self._registry,
            )

        raise _calibration_error(
            "exchange_code",
            (
                "is required for automatic session inference when no session preset or market "
                "profile provides the exchange baseline"
            ),
        )

    def _stdout_suppression_context(
        self,
        *,
        effective_silent: Optional[bool] = None,
    ) -> AbstractContextManager[object]:
        if self._silent if effective_silent is None else effective_silent:
            return redirect_stdout(StringIO())

        return nullcontext()


def _coerce_market_profile_reference(
    market_profile: Optional[str | MarketProfileReference],
) -> Optional[MarketProfileReference]:
    if market_profile is None:
        return None
    if isinstance(market_profile, MarketProfileReference):
        return market_profile
    if isinstance(market_profile, str):
        if not market_profile.strip():
            raise _calibration_error("market_profile", "must be a non-empty string")

        return MarketProfileReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name=market_profile,
        )

    raise _calibration_error(
        "market_profile",
        "must be a preset name or MarketProfileReference",
    )


def _coerce_session_reference(
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
            raise _calibration_error("session", "must be a non-empty string")

        return SessionPresetReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name=session,
        )

    if isinstance(session, Path):
        return SessionPresetReference(
            usage_mode=PresetReferenceUsageMode.EXPLICIT,
            configuration=_load_explicit_session_configuration(
                session,
                exchange_code=exchange_code,
            ),
        )

    raise _calibration_error(
        "session",
        "must be a preset name, Path, or SessionPresetReference",
    )


def _coerce_known_news_reference(
    known_news: Optional[str | KnownNewsPresetReference],
) -> Optional[KnownNewsPresetReference]:
    if known_news is None:
        return None
    if isinstance(known_news, KnownNewsPresetReference):
        return known_news

    if isinstance(known_news, str):
        if not known_news.strip():
            raise _calibration_error("known_news", "must be a non-empty string")

        return KnownNewsPresetReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name=known_news,
        )

    raise _calibration_error(
        "known_news",
        "must be a preset name or KnownNewsPresetReference",
    )


def _load_explicit_session_configuration(
    session_definition_path: Path,
    *,
    exchange_code: Optional[str],
) -> SessionExplicitConfiguration:
    try:
        registry_json = session_definition_path.read_text(encoding="utf-8")
    except FileNotFoundError as e:
        raise _calibration_error(
            "session",
            f"session definition path `{session_definition_path}` does not exist",
        ) from e
    except OSError as e:
        raise _calibration_error(
            "session",
            f"session definition path `{session_definition_path}` could not be read: {e}",
        ) from e

    calendar_registry = CalendarRegistry(registry_json)
    registry_payload = calendar_registry._registry_payload

    raw_exchanges = registry_payload.get("exchanges")
    if not isinstance(raw_exchanges, list) or len(raw_exchanges) == 0:
        raise _calibration_error(
            "session",
            "session definition must contain at least one exchange entry",
        )

    target_exchange_code = exchange_code
    if target_exchange_code is None:
        if len(raw_exchanges) != 1:
            raise _calibration_error(
                "exchange_code",
                (
                    "is required when the explicit session definition contains more than one "
                    "exchange entry"
                ),
            )

        target_exchange_code = cast(str, cast(dict[str, Any], raw_exchanges[0])["exchange_code"])

    exchange_artifact: Optional[dict[str, Any]] = None
    for raw_exchange in raw_exchanges:
        if isinstance(raw_exchange, dict) and raw_exchange.get("exchange_code") == target_exchange_code:
            exchange_artifact = cast(dict[str, Any], raw_exchange)
            break

    if exchange_artifact is None:
        raise _calibration_error(
            "exchange_code",
            f"explicit session definition does not contain exchange `{target_exchange_code}`",
        )

    regular_session_windows = _regular_session_windows_from_exchange_artifact(exchange_artifact)
    exchange_name = cast(str, exchange_artifact["exchange_name"])
    timezone_name = cast(str, exchange_artifact["timezone_name"])
    session_template = _SessionTemplate(
        policy_id="high_level.explicit_session_definition.v1",
        exchange_name=exchange_name,
        timezone_name=timezone_name,
        timezone_source="session_definition_source",
        holiday_calendar_code=cast(
            str,
            cast(dict[str, Any], exchange_artifact["rule_metadata"])["holiday_calendar_code"],
        ),
        early_close_rule_set=cast(
            Optional[str],
            cast(dict[str, Any], exchange_artifact["rule_metadata"]).get("early_close_rule_set"),
        ),
        regular_session_windows=regular_session_windows,
        continuous_weekday_template=_looks_like_continuous_weekday_template(
            exchange_code=target_exchange_code,
            regular_session_windows=regular_session_windows,
        ),
    )

    return SessionExplicitConfiguration(
        exchange_code=target_exchange_code,
        exchange_name=exchange_name,
        timezone_name=timezone_name,
        regular_session_windows=regular_session_windows,
        recurring_closed_windows=_build_recurring_closed_windows(session_template),
    )


def _exchange_code_from_session_reference(
    session_reference: SessionPresetReference,
    registry: PresetRegistry,
) -> str:
    match session_reference.usage_mode:
        case PresetReferenceUsageMode.AUTOMATIC:
            if session_reference.exchange_code_hint is None:
                raise _calibration_error(
                    "exchange_code",
                    "is required for automatic session inference",
                )

            return session_reference.exchange_code_hint
        case PresetReferenceUsageMode.NAMED:
            if session_reference.preset_name is None or session_reference.registry_scope is None:
                raise _calibration_error(
                    "session",
                    "named session references require preset_name and registry_scope",
                )

            _, artifact = registry.resolve_session_preset(
                session_reference.preset_name,
                registry_scope=session_reference.registry_scope,
            )

            return artifact.configuration.exchange_code
        case PresetReferenceUsageMode.EXPLICIT:
            if session_reference.configuration is None:
                raise _calibration_error(
                    "session.configuration",
                    "is required when usage_mode=explicit",
                )

            return session_reference.configuration.exchange_code


def _exchange_code_from_market_profile_reference(
    market_profile_reference: MarketProfileReference,
    registry: PresetRegistry,
) -> str:
    if market_profile_reference.usage_mode == PresetReferenceUsageMode.EXPLICIT:
        if market_profile_reference.configuration is None:
            raise _calibration_error(
                "market_profile.configuration",
                "is required when usage_mode=explicit",
            )

        return _exchange_code_from_session_reference(
            market_profile_reference.configuration.session_reference,
            registry,
        )

    if market_profile_reference.preset_name is None or market_profile_reference.registry_scope is None:
        raise _calibration_error(
            "market_profile",
            "named market-profile references require preset_name and registry_scope",
        )

    _, artifact = registry.resolve_market_profile_preset(
        market_profile_reference.preset_name,
        registry_scope=market_profile_reference.registry_scope,
    )

    return _exchange_code_from_session_reference(
        artifact.configuration.session_reference,
        registry,
    )


def _build_session_configuration_input(
    resolved_session: ResolvedSessionComponent,
    *,
    working_directory: Optional[Path],
) -> CalibrationSessionConfigurationInput:
    if len(resolved_session.applied_override_fields) == 0 and (
        resolved_session.inferred_from_data
        or resolved_session.selected_preset is not None
    ):
        return CalibrationSessionConfigurationInput(
            exchange_code=resolved_session.configuration.exchange_code,
            session_timezone=resolved_session.configuration.timezone_name,
        )

    session_configuration = resolved_session.configuration
    if working_directory is None:
        raise _calibration_error(
            "session_configuration",
            "resolved session materialization requires a working directory",
        )
    session_definition_path = working_directory / "resolved-session-definition.json"
    session_definition_path.write_text(
        _calendar_registry_json_from_session_configuration(session_configuration),
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


def _calendar_registry_json_from_session_configuration(
    session_configuration: SessionExplicitConfiguration,
) -> str:
    return _build_calendar_registry_json_from_windows(
        exchange_code=session_configuration.exchange_code,
        exchange_name=(
            session_configuration.exchange_name
            if session_configuration.exchange_name is not None
            else session_configuration.exchange_code
        ),
        timezone_name=session_configuration.timezone_name,
        holiday_calendar_code="USER_DEFINED",
        early_close_rule_set=None,
        regular_session_windows=session_configuration.regular_session_windows,
        recurring_closed_windows=session_configuration.recurring_closed_windows,
    )


def _build_known_news_input(
    known_news_configuration: KnownNewsExplicitConfiguration,
    *,
    working_directory: Optional[Path],
) -> Optional[CalibrationKnownNewsInput]:
    if len(known_news_configuration.events) == 0:
        return None

    if working_directory is None:
        raise _calibration_error(
            "known_news",
            "resolved known-news materialization requires a working directory",
        )

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


def _resolved_configuration_requires_materialization(
    resolved_configuration: ResolvedPresetWorkflowConfiguration,
) -> bool:
    if len(resolved_configuration.known_news.configuration.events) > 0:
        return True

    return not (
        len(resolved_configuration.session.applied_override_fields) == 0
        and (
            resolved_configuration.session.inferred_from_data
            or resolved_configuration.session.selected_preset is not None
        )
    )


def _extract_reference_price(normalized_records: tuple[Any, ...]) -> Optional[float]:
    if len(normalized_records) == 0:
        return None

    last_record = normalized_records[-1]
    price = cast(Optional[float], last_record.price)
    return price


def _detect_interval_data_warning(normalized_records: tuple[Any, ...]) -> Optional[str]:
    if len(normalized_records) < 4:
        return None

    deltas_in_seconds: list[int] = []
    for previous_record, current_record in zip(normalized_records, normalized_records[1:]):
        previous_timestamp = cast(datetime, previous_record.timestamp)
        current_timestamp = cast(datetime, current_record.timestamp)
        delta_seconds = int((current_timestamp - previous_timestamp).total_seconds())
        if delta_seconds > 0:
            deltas_in_seconds.append(delta_seconds)

    if len(deltas_in_seconds) < 3:
        return None

    aligned_to_minute_count = sum(delta_seconds % 60 == 0 for delta_seconds in deltas_in_seconds)
    dominant_delta_seconds = max(
        set(deltas_in_seconds),
        key=deltas_in_seconds.count,
    )
    dominant_delta_share = deltas_in_seconds.count(dominant_delta_seconds) / len(deltas_in_seconds)

    if dominant_delta_seconds < 60:
        return None
    if aligned_to_minute_count / len(deltas_in_seconds) < 0.8:
        return None
    if dominant_delta_share < 0.6:
        return None

    return (
        "The supplied fit input appears to be interval data rather than tick data. "
        f"Janiform detected a dominant spacing of {_format_interval_warning_delta(dominant_delta_seconds)}, "
        "so fitting the model on this data will likely produce an invalid fit."
    )


def _format_interval_warning_delta(delta_seconds: int) -> str:
    if delta_seconds % 86_400 == 0:
        return f"{delta_seconds // 86_400}D"
    if delta_seconds % 3_600 == 0:
        return f"{delta_seconds // 3_600}h"
    if delta_seconds % 60 == 0:
        return f"{delta_seconds // 60}min"

    return f"{delta_seconds}s"


__all__ = ["JaniformEstimator"]
