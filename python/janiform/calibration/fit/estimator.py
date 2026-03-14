from datetime import UTC, datetime
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any, Literal, Optional, overload, cast

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
from ..market_data.workflow import prepare_market_data_calibration
from ..presets.references import (
    KnownNewsExplicitConfiguration,
    KnownNewsPresetReference,
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
class JaniformEstimator:
    def __init__(
        self,
        *,
        workflow_scope: CalibrationMarketDataWorkflowScope,
        tick_data_column_mapping: CalibrationTickDataColumnMapping,
        normalization_policy: CalibrationMarketDataNormalizationPolicy,
        option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping] = None,
        output_preferences: Optional[CalibrationMarketDataOutputPreferences] = None,
        market_profile: Optional[str | MarketProfileReference] = None,
        session: Optional[str | Path | SessionPresetReference] = None,
        known_news: Optional[str | KnownNewsPresetReference] = None,
        exchange_code: Optional[str] = None,
        session_timezone: Optional[str] = None,
        registry: Optional[PresetRegistry] = None,
    ) -> None:
        if exchange_code is not None and not exchange_code.strip():
            raise _calibration_error("exchange_code", "must be a non-empty string when provided")
        if session_timezone is not None and not session_timezone.strip():
            raise _calibration_error(
                "session_timezone",
                "must be a non-empty string when provided",
            )

        self._workflow_scope: CalibrationMarketDataWorkflowScope = workflow_scope
        self._tick_data_column_mapping: CalibrationTickDataColumnMapping = tick_data_column_mapping
        self._normalization_policy: CalibrationMarketDataNormalizationPolicy = normalization_policy
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
        run_id: str,
        tick_data_path: str | Path,
        tick_data_format: CalibrationInputFileFormat,
        tick_data_provenance_label: str = "tick_data",
        option_surface_path: Optional[str | Path] = None,
        option_surface_format: CalibrationInputFileFormat = CalibrationInputFileFormat.PARQUET,
        option_surface_provenance_label: str = "option_surface",
        report: Literal[False] = False,
    ) -> JaniformModel: ...

    @overload
    def fit(
        self,
        *,
        run_id: str,
        tick_data_path: str | Path,
        tick_data_format: CalibrationInputFileFormat,
        tick_data_provenance_label: str = "tick_data",
        option_surface_path: Optional[str | Path] = None,
        option_surface_format: CalibrationInputFileFormat = CalibrationInputFileFormat.PARQUET,
        option_surface_provenance_label: str = "option_surface",
        report: Literal[True],
    ) -> tuple[Optional[JaniformModel], JaniformFitReport]: ...

    def fit(
        self,
        *,
        run_id: str,
        tick_data_path: str | Path,
        tick_data_format: CalibrationInputFileFormat,
        tick_data_provenance_label: str = "tick_data",
        option_surface_path: Optional[str | Path] = None,
        option_surface_format: CalibrationInputFileFormat = CalibrationInputFileFormat.PARQUET,
        option_surface_provenance_label: str = "option_surface",
        report: bool = False,
    ) -> JaniformModel | tuple[Optional[JaniformModel], JaniformFitReport]:
        if not run_id.strip():
            raise _calibration_error("run_id", "must be a non-empty string")
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

        tick_data_source = CalibrationInputFileReference(
            path=str(Path(tick_data_path)),
            file_format=tick_data_format,
            provenance_label=tick_data_provenance_label,
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

        with TemporaryDirectory(prefix="janiform-fit-") as working_directory_text:
            working_directory = Path(working_directory_text)
            inferred_session_resolution = self._infer_session_resolution(
                run_id=run_id,
                tick_data_source=tick_data_source,
            )
            resolved_configuration = resolve_preset_workflow_configuration(
                registry=self._registry,
                market_profile_reference=self._market_profile_reference,
                session_reference=self._effective_session_reference(),
                known_news_reference=self._known_news_reference,
                inferred_session_resolution=inferred_session_resolution,
            )
            request = self._build_market_data_request(
                run_id=run_id,
                tick_data_source=tick_data_source,
                option_surface_source=option_surface_source,
                resolved_configuration=resolved_configuration,
                working_directory=working_directory,
            )
            prepared = prepare_market_data_calibration(request)
            staged_response, raw_response_payload = _run_staged_calibration_with_raw_payload(
                prepared.execution_input,
                calendar_registry=CalendarRegistry(
                    prepared.session_resolution.resolved_session_definition_json
                ),
            )

        fit_report = JaniformFitReport(
            run_id=run_id,
            workflow_scope=self._workflow_scope,
            success=staged_response.status == CalibrationStageExecutionStatus.SUCCESS,
            resolved_configuration=resolved_configuration,
            session_resolution=prepared.session_resolution,
            preparation_diagnostics=prepared.diagnostics,
            preparation_provenance=prepared.provenance,
            execution_input=prepared.execution_input,
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
                f"Janiform fit failed for run_id `{run_id}` with failure_kind `{failure_kind}`"
            )

        model = JaniformModel(
            run_id=run_id,
            workflow_scope=self._workflow_scope,
            parameters=_extract_model_parameters(fit_report.stage_report_payloads),
            metadata=_build_model_metadata(
                resolved_configuration=resolved_configuration,
                prepared=prepared,
                staged_response=staged_response,
                stage_report_payloads=fit_report.stage_report_payloads,
            ),
            resolved_configuration=resolved_configuration,
            session_resolution=prepared.session_resolution,
            _execution_input=prepared.execution_input,
            _stage_report_payloads=fit_report.stage_report_payloads,
        )
        if report:
            return model, fit_report

        return model

    def _build_market_data_request(
        self,
        *,
        run_id: str,
        tick_data_source: CalibrationInputFileReference,
        option_surface_source: Optional[CalibrationInputFileReference],
        resolved_configuration: ResolvedPresetWorkflowConfiguration,
        working_directory: Path,
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
            tick_data=self._tick_data_column_mapping,
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
            normalization_policy=self._normalization_policy,
            output_preferences=self._output_preferences,
            option_surface_source=option_surface_source,
            known_news_inputs=known_news_input,
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
                tick_data=self._tick_data_column_mapping,
            ),
            workflow_scope=self._workflow_scope,
            normalization_policy=self._normalization_policy,
            output_preferences=self._output_preferences,
        )
        loaded_inputs = load_market_data_calibration_inputs(request)
        resolved_session = resolve_market_data_session_configuration(
            request,
            loaded_inputs.tick_data,
        )

        return resolved_session.session_resolution

    def _requires_session_inference(self) -> bool:
        if self._session_reference is not None:
            return self._session_reference.usage_mode == PresetReferenceUsageMode.AUTOMATIC

        if self._market_profile_reference is None:
            return True

        if self._market_profile_reference.usage_mode == PresetReferenceUsageMode.EXPLICIT:
            configuration = self._market_profile_reference.configuration
            if configuration is None:
                raise _calibration_error(
                    "market_profile.configuration",
                    "is required when usage_mode=explicit",
                )

            return configuration.session_reference.usage_mode == PresetReferenceUsageMode.AUTOMATIC

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

        return artifact.configuration.session_reference.usage_mode == PresetReferenceUsageMode.AUTOMATIC

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
    working_directory: Path,
) -> CalibrationSessionConfigurationInput:
    if resolved_session.inferred_from_data and len(resolved_session.applied_override_fields) == 0:
        return CalibrationSessionConfigurationInput(
            exchange_code=resolved_session.configuration.exchange_code,
            session_timezone=resolved_session.configuration.timezone_name,
        )

    session_configuration = resolved_session.configuration
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
    working_directory: Path,
) -> Optional[CalibrationKnownNewsInput]:
    if len(known_news_configuration.events) == 0:
        return None

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


__all__ = ["JaniformEstimator"]
