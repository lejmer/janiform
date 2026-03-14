from collections.abc import Mapping
from copy import deepcopy
from dataclasses import dataclass, field
from datetime import UTC, datetime, timedelta
import json
from pathlib import Path
from typing import Any, Literal, Optional, overload, cast

from ..market_data.contracts import CalibrationMarketDataWorkflowScope
from ..market_data.workflow import (
    PreparedMarketDataCalibration,
)
from ..presets.references import (
    KnownNewsExplicitConfiguration,
    KnownNewsPresetReference,
    MarketProfileExplicitConfiguration,
    MarketProfileKnownNewsPolicy,
    PresetRegistryScope,
    SessionExplicitConfiguration,
)
from ..presets.registry import PresetCategory
from ..presets.resolution import (
    DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME,
    ResolvedKnownNewsComponent,
    ResolvedMarketProfileComponent,
    ResolvedPresetSelection,
    ResolvedPresetWorkflowConfiguration,
    ResolvedSessionComponent,
    ResolvedWorkflowComponent,
    ResolvedWorkflowComponentSource,
    ResolvedWorkflowDiagnostic,
    ResolvedWorkflowDiagnosticSeverity,
)
from ..sessions.contracts import (
    MarketDataSessionRecurringClosedWindow,
    MarketDataSessionRegularWindow,
    MarketDataSessionResolution,
)
from ..staged.inputs import StagedCalibrationExecutionInput
from ..staged.results import StagedCalibrationResponse
from ..._contracts import serialize_deterministic_json
from ...calendar import CalendarRegistry
from .._errors import _calibration_error
from ...hourly_factors import (
    HourlyNewsIntensityMaterializationSettings,
    HourlyNewsIntensityProviderMaterializedSource,
    _parse_interval_duration_nanos,
    build_runtime_hourly_seasonality_source_from_calibration_payload,
    materialize_hourly_news_intensity_factors,
)
from ...pricing import (
    GammaSeverityMarkConfiguration,
    NigSharedPlusShiftConfiguration,
    VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType,
    VanillaOptionPricingExecutionInput,
    VanillaOptionPricingIntervalFactors,
    VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest,
    VanillaOptionPricingRunResult,
    run_vanilla_option_pricing,
)
from ...risk import (
    SufficientStatisticsRiskEquationSpecification,
    SufficientStatisticsRiskIntervalFactors,
    SufficientStatisticsRiskModelInputs,
)
from ...simulation import (
    CoarseSimulationPath,
    CoarseSimulationRunRequest,
    HawkesExcitationState,
    HawkesKernelParameters,
    NewsProcessConfiguration,
    SeededTickEventStateSnapshotCapture,
    SeededTickSimulationRequest,
    SeededTickSimulationResponse,
    load_hourly_factor_contract_for_simulation,
    run_coarse_simulation,
    run_seeded_tick_event_simulation,
)


_HIGH_LEVEL_MODEL_PERSISTENCE_SCHEMA_VERSION = "calibration.high_level.model.v1"

type _JaniformModelSimulationOutputMode = Literal["coarse", "ticks", "both"]
type _JaniformModelSimulationResult = (
    CoarseSimulationPath
    | SeededTickSimulationResponse
    | tuple[CoarseSimulationPath, SeededTickSimulationResponse]
)


@dataclass(frozen=True, slots=True)
class _PreparedModelSimulationInputs:
    coarse_request: CoarseSimulationRunRequest
    seeded_request: SeededTickSimulationRequest


@dataclass(frozen=True, slots=True)
class _PreparedModelRuntimeFactors:
    calendar_registry: CalendarRegistry
    exchange_code: str
    interval_delta: float
    range_end_utc: datetime
    kernel_parameters: HawkesKernelParameters
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration
    gamma_severity_mark_configuration: GammaSeverityMarkConfiguration
    news_process_configuration: NewsProcessConfiguration
    seasonality_hourly_factors: tuple[float, ...]
    known_component_nu_h: tuple[float, ...]
    surprise_component_nu_h: tuple[float, ...]


@dataclass(frozen=True, slots=True)
class JaniformModel:
    run_id: str
    workflow_scope: CalibrationMarketDataWorkflowScope
    parameters: dict[str, Any]
    metadata: dict[str, Any]
    resolved_configuration: ResolvedPresetWorkflowConfiguration
    session_resolution: MarketDataSessionResolution
    _execution_input: StagedCalibrationExecutionInput = field(repr=False, compare=False)
    _stage_report_payloads: tuple[dict[str, Any], ...] = field(repr=False, compare=False)

    @property
    def fitted_parameters(self) -> dict[str, Any]:
        return deepcopy(self.parameters)

    def summary(self) -> dict[str, Any]:
        summary = deepcopy(self.metadata)
        summary["fitted_parameters"] = deepcopy(self.parameters)
        return summary

    @overload
    def simulate(
        self,
        *,
        seed: int,
        start_time_utc: datetime,
        horizon_interval_count: int,
        interval: str | timedelta = "1h",
        output: Literal["coarse"] = "coarse",
        initial_excitation_state: Optional[HawkesExcitationState] = None,
        debug_trace_enabled: bool = False,
        sample_every_nth_transition: int = 1,
        max_records_per_chunk: int = 1024,
        state_snapshot_capture: SeededTickEventStateSnapshotCapture = SeededTickEventStateSnapshotCapture.DISABLED,
    ) -> CoarseSimulationPath: ...

    @overload
    def simulate(
        self,
        *,
        seed: int,
        start_time_utc: datetime,
        horizon_interval_count: int,
        interval: str | timedelta = "1h",
        output: Literal["ticks"],
        initial_excitation_state: Optional[HawkesExcitationState] = None,
        debug_trace_enabled: bool = False,
        sample_every_nth_transition: int = 1,
        max_records_per_chunk: int = 1024,
        state_snapshot_capture: SeededTickEventStateSnapshotCapture = SeededTickEventStateSnapshotCapture.DISABLED,
    ) -> SeededTickSimulationResponse: ...

    @overload
    def simulate(
        self,
        *,
        seed: int,
        start_time_utc: datetime,
        horizon_interval_count: int,
        interval: str | timedelta = "1h",
        output: Literal["both"],
        initial_excitation_state: Optional[HawkesExcitationState] = None,
        debug_trace_enabled: bool = False,
        sample_every_nth_transition: int = 1,
        max_records_per_chunk: int = 1024,
        state_snapshot_capture: SeededTickEventStateSnapshotCapture = SeededTickEventStateSnapshotCapture.DISABLED,
    ) -> tuple[CoarseSimulationPath, SeededTickSimulationResponse]: ...

    def simulate(
        self,
        *,
        seed: int,
        start_time_utc: datetime,
        horizon_interval_count: int,
        interval: str | timedelta = "1h",
        output: _JaniformModelSimulationOutputMode = "coarse",
        initial_excitation_state: Optional[HawkesExcitationState] = None,
        debug_trace_enabled: bool = False,
        sample_every_nth_transition: int = 1,
        max_records_per_chunk: int = 1024,
        state_snapshot_capture: SeededTickEventStateSnapshotCapture = SeededTickEventStateSnapshotCapture.DISABLED,
    ) -> _JaniformModelSimulationResult:
        if output not in ("coarse", "ticks", "both"):
            raise _calibration_error(
                "output",
                "must be one of: coarse, ticks, both",
            )

        execution_inputs = _prepare_model_simulation_inputs(
            self,
            seed=seed,
            start_time_utc=start_time_utc,
            horizon_interval_count=horizon_interval_count,
            interval=interval,
            initial_excitation_state=initial_excitation_state,
            debug_trace_enabled=debug_trace_enabled,
            sample_every_nth_transition=sample_every_nth_transition,
            max_records_per_chunk=max_records_per_chunk,
            state_snapshot_capture=state_snapshot_capture,
        )

        match output:
            case "coarse":
                return run_coarse_simulation(execution_inputs.coarse_request)
            case "ticks":
                return run_seeded_tick_event_simulation(execution_inputs.seeded_request)
            case "both":
                return (
                    run_coarse_simulation(execution_inputs.coarse_request),
                    run_seeded_tick_event_simulation(execution_inputs.seeded_request),
                )

    def price_option(
        self,
        *,
        valuation_time_utc: datetime,
        spot: float,
        strike: float,
        maturity_interval: float,
        payoff_type: VanillaOptionPayoffType | str = VanillaOptionPayoffType.CALL,
        risk_free_rate: float = 0.0,
        dividend_yield: float = 0.0,
        boundary_state: Optional[HawkesExcitationState] = None,
        include_transform_diagnostics: bool = False,
        include_integration_quality_diagnostics: bool = False,
        include_calibration_hooks: bool = False,
    ) -> VanillaOptionPricingRunResult:
        execution_input = _prepare_model_pricing_execution_input(
            self,
            valuation_time_utc=valuation_time_utc,
            spot=spot,
            strike=strike,
            maturity_interval=maturity_interval,
            payoff_type=payoff_type,
            risk_free_rate=risk_free_rate,
            dividend_yield=dividend_yield,
            boundary_state=boundary_state,
            include_transform_diagnostics=include_transform_diagnostics,
            include_integration_quality_diagnostics=include_integration_quality_diagnostics,
            include_calibration_hooks=include_calibration_hooks,
        )

        return run_vanilla_option_pricing(execution_input)

    def save(self, path: str | Path) -> None:
        model_path = Path(path)
        model_path.write_text(
            serialize_deterministic_json(_build_model_persistence_payload(self)),
            encoding="utf-8",
        )

    @staticmethod
    def load(path: str | Path) -> JaniformModel:
        model_path = Path(path)

        try:
            payload = json.loads(model_path.read_text(encoding="utf-8"))
        except FileNotFoundError as e:
            raise _calibration_error(
                "path",
                f"persisted model path `{model_path}` does not exist",
            ) from e
        except OSError as e:
            raise _calibration_error(
                "path",
                f"persisted model path `{model_path}` could not be read: {e}",
            ) from e
        except json.JSONDecodeError as e:
            raise _calibration_error(
                "path",
                f"persisted model path `{model_path}` does not contain valid JSON: {e.msg}",
            ) from e

        if not isinstance(payload, dict):
            raise _calibration_error("path", "persisted model payload must be a JSON object")

        schema_version = payload.get("schema_version")
        if schema_version != _HIGH_LEVEL_MODEL_PERSISTENCE_SCHEMA_VERSION:
            raise _calibration_error(
                "schema_version",
                f"must be `{_HIGH_LEVEL_MODEL_PERSISTENCE_SCHEMA_VERSION}`",
            )

        workflow_scope_value = payload.get("workflow_scope")
        if not isinstance(workflow_scope_value, str):
            raise _calibration_error("workflow_scope", "must be a string")

        raw_parameters = payload.get("parameters")
        raw_metadata = payload.get("metadata")
        raw_execution_input = payload.get("execution_input")
        raw_stage_report_payloads = payload.get("stage_report_payloads")
        raw_resolved_configuration = payload.get("resolved_configuration")
        raw_session_resolution = payload.get("session_resolution")

        if not isinstance(raw_parameters, dict):
            raise _calibration_error("parameters", "must be an object")
        if not isinstance(raw_metadata, dict):
            raise _calibration_error("metadata", "must be an object")
        if not isinstance(raw_execution_input, dict):
            raise _calibration_error("execution_input", "must be an object")
        if not isinstance(raw_stage_report_payloads, list):
            raise _calibration_error("stage_report_payloads", "must be a sequence")
        if not isinstance(raw_resolved_configuration, dict):
            raise _calibration_error("resolved_configuration", "must be an object")
        if not isinstance(raw_session_resolution, dict):
            raise _calibration_error("session_resolution", "must be an object")

        stage_report_payloads = _stage_report_payloads_from_sequence(raw_stage_report_payloads)

        try:
            workflow_scope = CalibrationMarketDataWorkflowScope(workflow_scope_value)
        except ValueError as e:
            raise _calibration_error(
                "workflow_scope",
                "must be a valid CalibrationMarketDataWorkflowScope value",
            ) from e

        return JaniformModel(
            run_id=_require_model_persistence_string(payload, "run_id"),
            workflow_scope=workflow_scope,
            parameters=_extract_model_parameters(stage_report_payloads),
            metadata=deepcopy(raw_metadata),
            resolved_configuration=_resolved_preset_workflow_configuration_from_mapping(
                raw_resolved_configuration
            ),
            session_resolution=_market_data_session_resolution_from_mapping(raw_session_resolution),
            _execution_input=StagedCalibrationExecutionInput.from_mapping(raw_execution_input),
            _stage_report_payloads=stage_report_payloads,
        )


def _prepare_model_simulation_inputs(
    model: JaniformModel,
    *,
    seed: int,
    start_time_utc: datetime,
    horizon_interval_count: int,
    interval: str | timedelta,
    initial_excitation_state: Optional[HawkesExcitationState],
    debug_trace_enabled: bool,
    sample_every_nth_transition: int,
    max_records_per_chunk: int,
    state_snapshot_capture: SeededTickEventStateSnapshotCapture,
) -> _PreparedModelSimulationInputs:
    runtime_factors = _prepare_model_runtime_factors(
        model,
        start_time_utc=start_time_utc,
        horizon_interval_count=horizon_interval_count,
        interval=interval,
    )
    effective_initial_state = (
        _zero_hawkes_excitation_state()
        if initial_excitation_state is None
        else initial_excitation_state
    )

    coarse_request = CoarseSimulationRunRequest.from_runtime_inputs(
        seed=seed,
        start_time_utc=start_time_utc,
        horizon_interval_count=horizon_interval_count,
        interval_delta=runtime_factors.interval_delta,
        risk_model_inputs=SufficientStatisticsRiskModelInputs(
            equation_specification=SufficientStatisticsRiskEquationSpecification(
                hawkes_kernel_parameters=runtime_factors.kernel_parameters,
                gamma_severity_mark_configuration=runtime_factors.gamma_severity_mark_configuration,
            ),
            nig_shared_plus_shift_configuration=runtime_factors.nig_shared_plus_shift_configuration,
            initial_excitation_state=effective_initial_state,
        ),
        interval_factors=SufficientStatisticsRiskIntervalFactors(
            interval_delta=runtime_factors.interval_delta,
            s_h=runtime_factors.seasonality_hourly_factors,
            nu_h_known=runtime_factors.known_component_nu_h,
            nu_h_surprise=runtime_factors.surprise_component_nu_h,
        ),
        news_process_configuration=runtime_factors.news_process_configuration,
        calendar_registry=runtime_factors.calendar_registry,
        exchange_code=runtime_factors.exchange_code,
    )
    seeded_request = SeededTickSimulationRequest.from_runtime_inputs(
        seed=seed,
        start_time_utc=start_time_utc,
        horizon_interval_count=horizon_interval_count,
        kernel_parameters=runtime_factors.kernel_parameters,
        nig_shared_plus_shift_configuration=runtime_factors.nig_shared_plus_shift_configuration,
        news_process_configuration=runtime_factors.news_process_configuration,
        initial_excitation_state=effective_initial_state,
        hourly_factor_contract=load_hourly_factor_contract_for_simulation(
            calendar_registry=runtime_factors.calendar_registry,
            exchange_code=runtime_factors.exchange_code,
            range_start_utc=start_time_utc,
            range_end_utc=runtime_factors.range_end_utc,
            interval=interval,
            seasonality_source=build_runtime_hourly_seasonality_source_from_calibration_payload(
                calendar_registry=runtime_factors.calendar_registry,
                range_start_utc=start_time_utc,
                range_end_utc=runtime_factors.range_end_utc,
                interval=interval,
                seasonality_stage_payload=_seasonality_stage_payload(model),
            ),
            news_intensity_source=HourlyNewsIntensityProviderMaterializedSource(
                known_scheduled_event_timestamps_utc=_resolved_known_news_event_timestamps(model),
                surprise_baseline_intensity=_surprise_baseline_intensity(model),
            ),
        ),
        debug_trace_enabled=debug_trace_enabled,
        sample_every_nth_transition=sample_every_nth_transition,
        max_records_per_chunk=max_records_per_chunk,
        state_snapshot_capture=state_snapshot_capture,
    )

    return _PreparedModelSimulationInputs(
        coarse_request=coarse_request,
        seeded_request=seeded_request,
    )


def _prepare_model_pricing_execution_input(
    model: JaniformModel,
    *,
    valuation_time_utc: datetime,
    spot: float,
    strike: float,
    maturity_interval: float,
    payoff_type: VanillaOptionPayoffType | str,
    risk_free_rate: float,
    dividend_yield: float,
    boundary_state: Optional[HawkesExcitationState],
    include_transform_diagnostics: bool,
    include_integration_quality_diagnostics: bool,
    include_calibration_hooks: bool,
) -> VanillaOptionPricingExecutionInput:
    runtime_factors = _prepare_model_runtime_factors(
        model,
        start_time_utc=valuation_time_utc,
        horizon_interval_count=1,
        interval="1h",
    )

    try:
        normalized_payoff_type = (
            payoff_type
            if isinstance(payoff_type, VanillaOptionPayoffType)
            else VanillaOptionPayoffType(payoff_type)
        )
    except ValueError as e:
        raise _calibration_error(
            "payoff_type",
            "must be one of: call, put",
        ) from e

    effective_boundary_state = (
        _zero_hawkes_excitation_state() if boundary_state is None else boundary_state
    )

    return VanillaOptionPricingExecutionInput(
        request=VanillaOptionPricingRequest(
            market_inputs=VanillaOptionPricingMarketInputs(
                spot=spot,
                strike=strike,
                maturity_interval=maturity_interval,
                risk_free_rate=risk_free_rate,
                dividend_yield=dividend_yield,
            ),
            payoff=VanillaOptionPayoffConfiguration(payoff_type=normalized_payoff_type),
            boundary_state=effective_boundary_state,
            model_parameters=VanillaOptionPricingModelParameters(
                hawkes_kernel_parameters=runtime_factors.kernel_parameters,
                gamma_severity_mark_configuration=runtime_factors.gamma_severity_mark_configuration,
                nig_shared_plus_shift_configuration=runtime_factors.nig_shared_plus_shift_configuration,
            ),
        ),
        interval_factors=VanillaOptionPricingIntervalFactors(
            s_h=runtime_factors.seasonality_hourly_factors[0],
            nu_h_known=runtime_factors.known_component_nu_h[0],
            nu_h_surprise=runtime_factors.surprise_component_nu_h[0],
        ),
        optional_output_selection=VanillaOptionPricingOptionalOutputSelection(
            include_transform_diagnostics=include_transform_diagnostics,
            include_integration_quality_diagnostics=include_integration_quality_diagnostics,
            include_calibration_hooks=include_calibration_hooks,
        ),
    )


def _prepare_model_runtime_factors(
    model: JaniformModel,
    *,
    start_time_utc: datetime,
    horizon_interval_count: int,
    interval: str | timedelta,
) -> _PreparedModelRuntimeFactors:
    _require_runtime_ready_model(model)
    _require_timezone_aware_datetime(start_time_utc, field_path="start_time_utc")

    interval_duration_nanos = _parse_interval_duration_nanos(interval, field_name="interval")
    range_end_utc = start_time_utc + timedelta(
        seconds=(interval_duration_nanos * horizon_interval_count) / 1_000_000_000
    )

    calendar_registry = CalendarRegistry(model.session_resolution.resolved_session_definition_json)
    exchange_code = model.session_resolution.exchange_code
    seasonality_source = build_runtime_hourly_seasonality_source_from_calibration_payload(
        calendar_registry=calendar_registry,
        range_start_utc=start_time_utc,
        range_end_utc=range_end_utc,
        interval=interval,
        seasonality_stage_payload=_seasonality_stage_payload(model),
    )
    news_materialization = materialize_hourly_news_intensity_factors(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        range_start_utc=start_time_utc,
        range_end_utc=range_end_utc,
        interval=interval,
        settings=HourlyNewsIntensityMaterializationSettings(
            known_scheduled_event_timestamps_utc=_resolved_known_news_event_timestamps(model),
            surprise_baseline_intensity=_surprise_baseline_intensity(model),
        ),
    )
    hawkes_fitted_parameters = _hawkes_fitted_parameters_payload(model)
    return_components_fitted_parameters = _return_components_fitted_parameters_payload(model)
    news_process_configuration = NewsProcessConfiguration.from_mapping(
        _require_payload_mapping(
            return_components_fitted_parameters,
            "news_process_configuration",
            "return_components.fitted_parameters.news_process_configuration",
        )
    )
    shared_mark_law = _require_payload_mapping(
        news_process_configuration.to_payload(),
        "shared_mark_law",
        "return_components.fitted_parameters.news_process_configuration.shared_mark_law",
    )

    return _PreparedModelRuntimeFactors(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        interval_delta=interval_duration_nanos / 3_600_000_000_000.0,
        range_end_utc=range_end_utc,
        kernel_parameters=HawkesKernelParameters.from_mapping(
            _require_payload_mapping(
                hawkes_fitted_parameters,
                "kernel_parameters",
                "hawkes.fitted_parameters.kernel_parameters",
            ),
            field_path="hawkes.fitted_parameters.kernel_parameters",
        ),
        nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration.from_mapping(
            _require_payload_mapping(
                return_components_fitted_parameters,
                "nig_shared_plus_shift_configuration",
                "return_components.fitted_parameters.nig_shared_plus_shift_configuration",
            ),
            field_path="return_components.fitted_parameters.nig_shared_plus_shift_configuration",
        ),
        gamma_severity_mark_configuration=GammaSeverityMarkConfiguration.from_mapping(
            _require_payload_mapping(
                shared_mark_law,
                "gamma_severity_marks",
                "return_components.fitted_parameters.news_process_configuration.shared_mark_law.gamma_severity_marks",
            ),
            field_path=(
                "return_components.fitted_parameters.news_process_configuration."
                "shared_mark_law.gamma_severity_marks"
            ),
        ),
        news_process_configuration=news_process_configuration,
        seasonality_hourly_factors=tuple(float(value) for value in seasonality_source.hourly_factors),
        known_component_nu_h=tuple(float(value) for value in news_materialization.known_component_nu_h),
        surprise_component_nu_h=tuple(float(value) for value in news_materialization.surprise_component_nu_h),
    )


def _require_runtime_ready_model(model: JaniformModel) -> None:
    if model.workflow_scope == CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY:
        raise RuntimeError(
            "JaniformModel runtime methods require a fitted model with Hawkes and return-components stages"
        )

    _hawkes_fitted_parameters_payload(model)
    _return_components_fitted_parameters_payload(model)


def _zero_hawkes_excitation_state() -> HawkesExcitationState:
    return HawkesExcitationState(
        y_plus_plus=0.0,
        y_plus_minus=0.0,
        y_minus_plus=0.0,
        y_minus_minus=0.0,
    )


def _require_timezone_aware_datetime(value: datetime, *, field_path: str) -> None:
    if not isinstance(value, datetime):
        raise _calibration_error(field_path, "must be a datetime")
    if value.tzinfo is None:
        raise _calibration_error(field_path, "must be timezone-aware")


def _seasonality_stage_payload(model: JaniformModel) -> Mapping[str, Any]:
    for stage_report_payload in model._stage_report_payloads:
        if stage_report_payload.get("stage_report_kind") != "seasonality":
            continue

        normalized_stage_payload = stage_report_payload.get("normalized_stage_payload")
        if isinstance(normalized_stage_payload, Mapping):
            return normalized_stage_payload

        raise RuntimeError("JaniformModel is missing the normalized seasonality stage payload")

    raise RuntimeError("JaniformModel does not contain a seasonality stage payload")


def _hawkes_fitted_parameters_payload(model: JaniformModel) -> Mapping[str, Any]:
    return _stage_fitted_parameters_payload(model, stage_kind="hawkes")


def _return_components_fitted_parameters_payload(model: JaniformModel) -> Mapping[str, Any]:
    return _stage_fitted_parameters_payload(model, stage_kind="return_components")


def _stage_fitted_parameters_payload(
    model: JaniformModel,
    *,
    stage_kind: str,
) -> Mapping[str, Any]:
    for stage_report_payload in model._stage_report_payloads:
        if stage_report_payload.get("stage_report_kind") != stage_kind:
            continue

        raw_fitted_stage_payload = stage_report_payload.get("fitted_stage_payload")
        if not isinstance(raw_fitted_stage_payload, Mapping):
            raise RuntimeError(
                f"JaniformModel is missing the fitted payload for stage `{stage_kind}`"
            )

        fitted_parameters = raw_fitted_stage_payload.get("fitted_parameters")
        if not isinstance(fitted_parameters, Mapping):
            raise RuntimeError(
                f"JaniformModel is missing fitted parameters for stage `{stage_kind}`"
            )

        return fitted_parameters

    raise RuntimeError(f"JaniformModel does not contain the `{stage_kind}` stage payload")


def _resolved_known_news_event_timestamps(model: JaniformModel) -> tuple[datetime, ...]:
    return tuple(
        event.scheduled_at.astimezone(UTC)
        for event in model.resolved_configuration.known_news.configuration.events
    )


def _surprise_baseline_intensity(model: JaniformModel) -> float:
    news_process_configuration = _require_payload_mapping(
        _return_components_fitted_parameters_payload(model),
        "news_process_configuration",
        "return_components.fitted_parameters.news_process_configuration",
    )
    arrivals = _require_payload_mapping(
        news_process_configuration,
        "arrivals",
        "return_components.fitted_parameters.news_process_configuration.arrivals",
    )
    surprise = _require_payload_mapping(
        arrivals,
        "surprise",
        "return_components.fitted_parameters.news_process_configuration.arrivals.surprise",
    )
    baseline_intensity = surprise.get("baseline_intensity")
    if not isinstance(baseline_intensity, int | float) or isinstance(baseline_intensity, bool):
        raise RuntimeError(
            "JaniformModel is missing a numeric surprise baseline intensity in the fitted model payload"
        )

    return float(baseline_intensity)


def _require_payload_mapping(
    payload: Mapping[str, Any],
    field_name: str,
    field_path: str,
) -> Mapping[str, Any]:
    raw_value = payload.get(field_name)
    if not isinstance(raw_value, Mapping):
        raise RuntimeError(f"JaniformModel payload field `{field_path}` must be an object")

    return raw_value


def _build_model_persistence_payload(model: JaniformModel) -> dict[str, Any]:
    return {
        "schema_version": _HIGH_LEVEL_MODEL_PERSISTENCE_SCHEMA_VERSION,
        "run_id": model.run_id,
        "workflow_scope": model.workflow_scope.value,
        "parameters": deepcopy(model.parameters),
        "metadata": deepcopy(model.metadata),
        "resolved_configuration": model.resolved_configuration.to_payload(),
        "session_resolution": model.session_resolution.to_payload(),
        "execution_input": model._execution_input.to_payload(),
        "stage_report_payloads": [deepcopy(payload) for payload in model._stage_report_payloads],
    }


def _require_model_persistence_string(
    payload: Mapping[str, Any],
    field_name: str,
) -> str:
    raw_value = payload.get(field_name)
    if not isinstance(raw_value, str) or not raw_value.strip():
        raise _calibration_error(field_name, "must be a non-empty string")

    return raw_value


def _stage_report_payloads_from_sequence(
    raw_stage_report_payloads: list[Any],
) -> tuple[dict[str, Any], ...]:
    payloads: list[dict[str, Any]] = []
    for stage_index, raw_stage_report in enumerate(raw_stage_report_payloads):
        if not isinstance(raw_stage_report, dict):
            raise _calibration_error(
                f"stage_report_payloads[{stage_index}]",
                "must be an object",
            )

        payloads.append(deepcopy(raw_stage_report))

    return tuple(payloads)


def _market_data_session_resolution_from_mapping(
    raw_payload: Mapping[str, Any],
) -> MarketDataSessionResolution:
    return MarketDataSessionResolution(
        schema_version=_require_model_persistence_string(raw_payload, "schema_version"),
        resolution_kind=_require_model_persistence_string(raw_payload, "resolution_kind"),
        selected_policy_id=_require_model_persistence_string(raw_payload, "selected_policy_id"),
        manual_override_applied=_require_model_persistence_boolean(
            raw_payload,
            "manual_override_applied",
        ),
        exchange_code=_require_model_persistence_string(raw_payload, "exchange_code"),
        exchange_name=_require_model_persistence_string(raw_payload, "exchange_name"),
        resolved_timezone_name=_require_model_persistence_string(raw_payload, "resolved_timezone_name"),
        timezone_source=_require_model_persistence_string(raw_payload, "timezone_source"),
        open_session_duration_hours_per_week=_require_model_persistence_float(
            raw_payload,
            "open_session_duration_hours_per_week",
        ),
        confidence_score=_require_model_persistence_float(raw_payload, "confidence_score"),
        confidence_label=_require_model_persistence_string(raw_payload, "confidence_label"),
        regular_session_windows=tuple(
            MarketDataSessionRegularWindow.from_mapping(
                raw_window,
                field_path=f"session_resolution.regular_session_windows[{window_index}]",
            )
            for window_index, raw_window in enumerate(
                _require_model_persistence_sequence(raw_payload, "regular_session_windows")
            )
        ),
        recurring_closed_windows=tuple(
            MarketDataSessionRecurringClosedWindow.from_mapping(
                raw_window,
                field_path=f"session_resolution.recurring_closed_windows[{window_index}]",
            )
            for window_index, raw_window in enumerate(
                _require_model_persistence_sequence(raw_payload, "recurring_closed_windows")
            )
        ),
        reasons=tuple(
            _require_model_persistence_sequence_of_strings(raw_payload, "reasons")
        ),
        ambiguity_warnings=tuple(
            _require_model_persistence_sequence_of_strings(raw_payload, "ambiguity_warnings")
        ),
        metadata_discrepancies=tuple(
            _require_model_persistence_sequence_of_strings(raw_payload, "metadata_discrepancies")
        ),
        resolved_session_definition_json=_require_model_persistence_string(
            raw_payload,
            "resolved_session_definition_json",
        ),
    )


def _resolved_preset_workflow_configuration_from_mapping(
    raw_payload: Mapping[str, Any],
) -> ResolvedPresetWorkflowConfiguration:
    raw_market_profile = raw_payload.get("market_profile")

    return ResolvedPresetWorkflowConfiguration(
        schema_version=_require_model_persistence_string(raw_payload, "schema_version"),
        session=_resolved_session_component_from_mapping(
            _require_model_persistence_mapping(raw_payload, "session")
        ),
        known_news=_resolved_known_news_component_from_mapping(
            _require_model_persistence_mapping(raw_payload, "known_news")
        ),
        diagnostics=tuple(
            _resolved_workflow_diagnostic_from_mapping(raw_diagnostic)
            for raw_diagnostic in _require_model_persistence_sequence(raw_payload, "diagnostics")
        ),
        market_profile=(
            _resolved_market_profile_component_from_mapping(raw_market_profile)
            if isinstance(raw_market_profile, Mapping)
            else None
        ),
    )


def _resolved_market_profile_component_from_mapping(
    raw_payload: Mapping[str, Any],
) -> ResolvedMarketProfileComponent:
    raw_selected_preset = raw_payload.get("selected_preset")

    return ResolvedMarketProfileComponent(
        configuration=_market_profile_explicit_configuration_from_mapping(
            _require_model_persistence_mapping(raw_payload, "configuration")
        ),
        source_kind=ResolvedWorkflowComponentSource(
            _require_model_persistence_string(raw_payload, "source_kind")
        ),
        selected_preset=(
            _resolved_preset_selection_from_mapping(raw_selected_preset)
            if isinstance(raw_selected_preset, Mapping)
            else None
        ),
        applied_override_fields=tuple(
            _require_model_persistence_sequence_of_strings(raw_payload, "applied_override_fields")
        ),
    )


def _resolved_session_component_from_mapping(
    raw_payload: Mapping[str, Any],
) -> ResolvedSessionComponent:
    raw_selected_preset = raw_payload.get("selected_preset")
    raw_inference_resolution = raw_payload.get("inference_resolution")

    return ResolvedSessionComponent(
        configuration=SessionExplicitConfiguration.from_mapping(
            _require_model_persistence_mapping(raw_payload, "configuration"),
            field_path="resolved_configuration.session.configuration",
        ),
        source_kind=ResolvedWorkflowComponentSource(
            _require_model_persistence_string(raw_payload, "source_kind")
        ),
        selected_preset=(
            _resolved_preset_selection_from_mapping(raw_selected_preset)
            if isinstance(raw_selected_preset, Mapping)
            else None
        ),
        inferred_from_data=_require_model_persistence_boolean(raw_payload, "inferred_from_data"),
        inference_resolution=(
            _market_data_session_resolution_from_mapping(raw_inference_resolution)
            if isinstance(raw_inference_resolution, Mapping)
            else None
        ),
        applied_override_fields=tuple(
            _require_model_persistence_sequence_of_strings(raw_payload, "applied_override_fields")
        ),
    )


def _resolved_known_news_component_from_mapping(
    raw_payload: Mapping[str, Any],
) -> ResolvedKnownNewsComponent:
    raw_selected_preset = raw_payload.get("selected_preset")
    raw_recommended_reference = raw_payload.get("recommended_reference")
    raw_recommendation_policy = raw_payload.get("recommendation_policy")

    return ResolvedKnownNewsComponent(
        configuration=KnownNewsExplicitConfiguration.from_mapping(
            _require_model_persistence_mapping(raw_payload, "configuration"),
            field_path="resolved_configuration.known_news.configuration",
        ),
        source_kind=ResolvedWorkflowComponentSource(
            _require_model_persistence_string(raw_payload, "source_kind")
        ),
        selected_preset=(
            _resolved_preset_selection_from_mapping(raw_selected_preset)
            if isinstance(raw_selected_preset, Mapping)
            else None
        ),
        applied_override_fields=tuple(
            _require_model_persistence_sequence_of_strings(raw_payload, "applied_override_fields")
        ),
        recommended_reference=(
            KnownNewsPresetReference.from_mapping(
                raw_recommended_reference,
                field_path="resolved_configuration.known_news.recommended_reference",
            )
            if isinstance(raw_recommended_reference, Mapping)
            else None
        ),
        recommendation_policy=(
            MarketProfileKnownNewsPolicy(cast(str, raw_recommendation_policy))
            if isinstance(raw_recommendation_policy, str)
            else None
        ),
        recommendation_applied=_require_model_persistence_boolean(
            raw_payload,
            "recommendation_applied",
        ),
    )


def _resolved_workflow_diagnostic_from_mapping(
    raw_payload: object,
) -> ResolvedWorkflowDiagnostic:
    if not isinstance(raw_payload, Mapping):
        raise _calibration_error("diagnostics", "must contain objects")

    return ResolvedWorkflowDiagnostic(
        component=ResolvedWorkflowComponent(
            _require_model_persistence_string(raw_payload, "component")
        ),
        severity=ResolvedWorkflowDiagnosticSeverity(
            _require_model_persistence_string(raw_payload, "severity")
        ),
        code=_require_model_persistence_string(raw_payload, "code"),
        message=_require_model_persistence_string(raw_payload, "message"),
    )


def _resolved_preset_selection_from_mapping(
    raw_payload: Mapping[str, Any],
) -> ResolvedPresetSelection:
    return ResolvedPresetSelection(
        category=PresetCategory(_require_model_persistence_string(raw_payload, "category")),
        registry_scope=PresetRegistryScope(
            _require_model_persistence_string(raw_payload, "registry_scope")
        ),
        preset_name=_require_model_persistence_string(raw_payload, "preset_name"),
    )


def _market_profile_explicit_configuration_from_mapping(
    raw_payload: Mapping[str, Any],
) -> MarketProfileExplicitConfiguration:
    return MarketProfileExplicitConfiguration.from_mapping(
        raw_payload,
        field_path="resolved_configuration.market_profile.configuration",
    )


def _require_model_persistence_mapping(
    payload: Mapping[str, Any],
    field_name: str,
) -> Mapping[str, Any]:
    raw_value = payload.get(field_name)
    if not isinstance(raw_value, Mapping):
        raise _calibration_error(field_name, "must be an object")

    return raw_value


def _require_model_persistence_sequence(
    payload: Mapping[str, Any],
    field_name: str,
) -> list[Any]:
    raw_value = payload.get(field_name)
    if not isinstance(raw_value, list):
        raise _calibration_error(field_name, "must be a sequence")

    return list(raw_value)


def _require_model_persistence_sequence_of_strings(
    payload: Mapping[str, Any],
    field_name: str,
) -> list[str]:
    raw_values = _require_model_persistence_sequence(payload, field_name)
    values: list[str] = []
    for position, raw_value in enumerate(raw_values):
        if not isinstance(raw_value, str):
            raise _calibration_error(
                f"{field_name}[{position}]",
                "must be a string",
            )

        values.append(raw_value)

    return values


def _require_model_persistence_boolean(
    payload: Mapping[str, Any],
    field_name: str,
) -> bool:
    raw_value = payload.get(field_name)
    if not isinstance(raw_value, bool):
        raise _calibration_error(field_name, "must be a boolean")

    return raw_value


def _require_model_persistence_float(
    payload: Mapping[str, Any],
    field_name: str,
) -> float:
    raw_value = payload.get(field_name)
    if not isinstance(raw_value, int | float) or isinstance(raw_value, bool):
        raise _calibration_error(field_name, "must be a number")

    return float(raw_value)


def _raw_stage_report_payloads(raw_response_payload: dict[str, Any]) -> tuple[dict[str, Any], ...]:
    raw_stage_reports = raw_response_payload.get("stage_reports", ())
    if not isinstance(raw_stage_reports, list):
        raise _calibration_error("stage_reports", "must be a sequence")

    stage_payloads: list[dict[str, Any]] = []
    for stage_index, raw_stage_report in enumerate(raw_stage_reports):
        if not isinstance(raw_stage_report, dict):
            raise _calibration_error(
                f"stage_reports[{stage_index}]",
                "must be an object",
            )
        stage_payloads.append(dict(raw_stage_report))

    return tuple(stage_payloads)


def _extract_model_parameters(stage_report_payloads: tuple[dict[str, Any], ...]) -> dict[str, Any]:
    parameters: dict[str, Any] = {}
    for stage_report in stage_report_payloads:
        stage_kind = cast(str, stage_report["stage_report_kind"])
        match stage_kind:
            case "seasonality":
                normalized_stage_payload = stage_report.get("normalized_stage_payload")
                if isinstance(normalized_stage_payload, Mapping):
                    parameters["seasonality"] = {
                        "schema_version": normalized_stage_payload.get("composition_schema_version"),
                        "exchange_code": normalized_stage_payload.get("exchange_code"),
                        "intraday_factor_by_bin": tuple(
                            cast(list[float], normalized_stage_payload.get("intraday_factor_by_bin", []))
                        ),
                        "day_of_week_factor": tuple(
                            cast(list[float], normalized_stage_payload.get("day_of_week_factor", []))
                        ),
                        "day_of_year_factor": tuple(
                            cast(list[float], normalized_stage_payload.get("day_of_year_factor", []))
                        ),
                    }
            case "hawkes":
                fitted_stage_payload = stage_report.get("fitted_stage_payload")
                if isinstance(fitted_stage_payload, Mapping):
                    parameters["hawkes"] = {
                        "schema_version": fitted_stage_payload.get("schema_version"),
                        "selected_candidate_id": fitted_stage_payload.get("selected_candidate_id"),
                        "fitted_parameters": deepcopy(
                            cast(dict[str, Any], fitted_stage_payload.get("fitted_parameters", {}))
                        ),
                    }
            case "return_components":
                fitted_stage_payload = stage_report.get("fitted_stage_payload")
                if isinstance(fitted_stage_payload, Mapping):
                    parameters["return_components"] = {
                        "schema_version": fitted_stage_payload.get("schema_version"),
                        "selected_candidate_id": fitted_stage_payload.get("selected_candidate_id"),
                        "fitted_parameters": deepcopy(
                            cast(dict[str, Any], fitted_stage_payload.get("fitted_parameters", {}))
                        ),
                    }
            case _:
                raise _calibration_error(
                    "stage_report_kind",
                    f"unsupported stage report kind `{stage_kind}`",
                )

    return parameters


def _build_model_metadata(
    *,
    resolved_configuration: ResolvedPresetWorkflowConfiguration,
    prepared: PreparedMarketDataCalibration,
    staged_response: StagedCalibrationResponse,
    stage_report_payloads: tuple[dict[str, Any], ...],
    input_inference: dict[str, Any],
) -> dict[str, Any]:
    market_profile_name = (
        resolved_configuration.market_profile.selected_preset.preset_name
        if (
            resolved_configuration.market_profile is not None
            and resolved_configuration.market_profile.selected_preset is not None
        )
        else None
    )
    session_preset_name = (
        resolved_configuration.session.selected_preset.preset_name
        if resolved_configuration.session.selected_preset is not None
        else None
    )
    known_news_preset_name = (
        resolved_configuration.known_news.selected_preset.preset_name
        if resolved_configuration.known_news.selected_preset is not None
        else None
    )
    known_news_event_count = len(resolved_configuration.known_news.configuration.events)

    return {
        "run_id": staged_response.run_id,
        "workflow_scope": prepared.diagnostics.workflow_scope.value,
        "execution_mode": staged_response.execution_mode.value,
        "selected_stages": [
            stage.value for stage in prepared.execution_input.request.selected_stages
        ],
        "market_profile": market_profile_name,
        "session_preset": session_preset_name,
        "known_news_preset": known_news_preset_name,
        "session_source_kind": resolved_configuration.session.source_kind.value,
        "known_news_source_kind": resolved_configuration.known_news.source_kind.value,
        "fitted_parameter_summary": {
            "stage_keys": sorted(_extract_model_parameters(stage_report_payloads).keys()),
            "selected_candidate_ids": _selected_candidate_ids(stage_report_payloads),
        },
        "preset_provenance": {
            "market_profile": (
                _resolved_component_provenance(
                    resolved_configuration.market_profile.source_kind,
                    resolved_configuration.market_profile.selected_preset,
                    resolved_configuration.market_profile.applied_override_fields,
                )
                if resolved_configuration.market_profile is not None
                else None
            ),
            "session": {
                **_resolved_component_provenance(
                    resolved_configuration.session.source_kind,
                    resolved_configuration.session.selected_preset,
                    resolved_configuration.session.applied_override_fields,
                ),
                "inferred_from_data": resolved_configuration.session.inferred_from_data,
            },
            "known_news": {
                **_resolved_component_provenance(
                    resolved_configuration.known_news.source_kind,
                    resolved_configuration.known_news.selected_preset,
                    resolved_configuration.known_news.applied_override_fields,
                ),
                "recommended_preset": (
                    resolved_configuration.known_news.recommended_reference.preset_name
                    if resolved_configuration.known_news.recommended_reference is not None
                    else None
                ),
                "recommendation_policy": (
                    resolved_configuration.known_news.recommendation_policy.value
                    if resolved_configuration.known_news.recommendation_policy is not None
                    else None
                ),
                "recommendation_applied": resolved_configuration.known_news.recommendation_applied,
            },
        },
        "session_resolution": {
            "resolution_kind": prepared.session_resolution.resolution_kind,
            "selected_policy_id": prepared.session_resolution.selected_policy_id,
            "exchange_code": prepared.session_resolution.exchange_code,
            "exchange_name": prepared.session_resolution.exchange_name,
            "resolved_timezone_name": prepared.session_resolution.resolved_timezone_name,
            "confidence_score": prepared.session_resolution.confidence_score,
            "confidence_label": prepared.session_resolution.confidence_label,
            "manual_override_applied": prepared.session_resolution.manual_override_applied,
        },
        "known_news_activation": {
            "selected_preset": known_news_preset_name or DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME,
            "event_count": known_news_event_count,
            "active": known_news_event_count > 0,
        },
        "input_inference": input_inference,
        "diagnostics": [
            {
                "component": diagnostic.component.value,
                "severity": diagnostic.severity.value,
                "code": diagnostic.code,
                "message": diagnostic.message,
            }
            for diagnostic in resolved_configuration.diagnostics
        ],
    }


def _selected_candidate_ids(stage_report_payloads: tuple[dict[str, Any], ...]) -> dict[str, str]:
    selected_candidate_ids: dict[str, str] = {}
    for stage_report in stage_report_payloads:
        stage_kind = stage_report.get("stage_report_kind")
        fitted_stage_payload = stage_report.get("fitted_stage_payload")
        if not isinstance(stage_kind, str) or not isinstance(fitted_stage_payload, Mapping):
            continue

        selected_candidate_id = fitted_stage_payload.get("selected_candidate_id")
        if isinstance(selected_candidate_id, str):
            selected_candidate_ids[stage_kind] = selected_candidate_id

    return selected_candidate_ids


def _resolved_component_provenance(
    source_kind: ResolvedWorkflowComponentSource,
    selected_preset: Optional[ResolvedPresetSelection],
    applied_override_fields: tuple[str, ...],
) -> dict[str, Any]:
    return {
        "source_kind": source_kind.value,
        "selected_preset": (
            None
            if selected_preset is None
            else {
                "category": selected_preset.category.value,
                "registry_scope": selected_preset.registry_scope.value,
                "preset_name": selected_preset.preset_name,
            }
        ),
        "applied_override_fields": list(applied_override_fields),
    }


__all__ = ["JaniformModel"]
