from __future__ import annotations

import json
from collections.abc import Callable
from collections.abc import Mapping
from copy import deepcopy
from dataclasses import asdict, dataclass, field
from datetime import UTC, date, datetime, timedelta
from math import log
from pathlib import Path
from typing import Any, NoReturn, Optional, cast

from ..market_data.contracts import (
    CalibrationMarketDataWorkflowScope,
    CalibrationTickDataColumnMapping,
)
from ..market_data.workflow import (
    PreparedMarketDataCalibration,
)
from ..presets.resolution import (
    DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME,
    ResolvedPresetWorkflowConfiguration,
    ResolvedWorkflowComponentSource,
    ResolvedPresetSelection,
)
from ..sessions.contracts import (
    MarketDataSessionResolution,
)
from ..staged.results import StagedCalibrationResponse
from ..._workflow_outputs import (
    SimulationArtifactSink,
    generate_workflow_run_id,
    persist_coarse_simulation,
    persist_seeded_tick_simulation,
    simulation_output_columns,
    simulation_output_preferences_from_model,
    write_pricing_artifacts,
    write_simulation_artifacts,
)
from ..._workflow_progress import (
    AbstractProgress,
    JaniformOutputController,
    NullProgress,
    WorkflowProgressMode,
)
from ..._timestamps import ONE_HOUR_NANOS, datetime_to_utc_nanos, nanoseconds_to_hours
from ...mean_reversion import (
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
)
from ...contracts import (
    require_non_empty_string,
    require_positive_finite_scalar,
    serialize_pretty_json,
)
from ...calendar import CalendarRegistry, MarketState
from ...hourly_factors import (
    HourlyFactorContract,
    HourlyNewsIntensityProviderMaterializedSource,
    HourlySeasonalityProviderExplicitSource,
    HourlySeasonalityProviderMaterializedSource,
    HourlySeasonalityProviderSource,
    load_hourly_factor_contract_payload,
    provide_hourly_factor_contract,
)
from ...hourly_factors.workflow import _parse_interval_duration_nanos
from ...pricing import (
    AnchoredVanillaOptionPricingBoundaryInput,
    GammaSeverityMarkConfiguration,
    NigSharedPlusShiftConfiguration,
    VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType,
    VanillaOptionPricingExecutionInput,
    VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest,
    VanillaOptionPricingRunResult,
    run_vanilla_option_pricing,
)
from ...pricing.progress import (
    PricingProgressEvent,
    pricing_progress_event_from_mapping,
)
from .._errors import (
    CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
    CalibrationValidationError,
    _calibration_error,
    _calibration_execution_error,
    _calibration_integration_error,
)
from ...risk import (
    SufficientStatisticsRiskEquationSpecification,
    SufficientStatisticsRiskModelInputs,
)
from ...simulation.coarse import (
    CoarseSimulationPath,
    CoarseSimulationRunRequest,
    NewsProcessConfiguration,
    run_coarse_simulation,
)
from ...simulation.progress import SimulationProgressEvent
from ...simulation.seeded import (
    SeededTickSimulationRequest,
    SeededTickSimulationResponse,
    run_seeded_tick_event_simulation,
)
from ...simulation.shared import (
    HawkesExcitationState,
    HawkesKernelParameters,
    SeededTickEventStateSnapshotCapture,
)
from ..._native_api import native_exception_matches, native_module
from .pricing import (
    JaniformGroupedVanillaOptionBatch,
    JaniformVanillaOptionSurface,
    JaniformVanillaOptionGroupedBatchPricingResult,
    JaniformVanillaOptionSurfacePricingResult,
)
from .maturity_inputs import (
    resolve_high_level_pricing_maturity_axis,
    resolve_single_high_level_pricing_maturity_input,
)
from .state_estimation import (
    CurrentStateEstimationRequest,
    CurrentStateEstimationResult,
    estimate_current_state_for_model,
)


_HIGH_LEVEL_MODEL_PERSISTENCE_SCHEMA_VERSION = "calibration.high_level.model.v1"
_DEFAULT_HIGH_LEVEL_SIMULATION_INTERVAL = "1h"
_MEAN_REVERSION_RUNTIME_INTERVAL_ALIGNMENT_POLICY = "explicit_runtime_interval_grid_alignment.v1"


@dataclass(frozen=True, slots=True)
class _PreparedModelSimulationInputs:
    coarse_request: Optional[CoarseSimulationRunRequest]
    seeded_request: Optional[SeededTickSimulationRequest]


@dataclass(frozen=True, slots=True)
class _ResolvedModelSimulationInitialState:
    excitation_state: HawkesExcitationState
    mean_reversion_boundary_state: Optional[MeanReversionBoundaryState]
    start_anchor_value: float


@dataclass(frozen=True, slots=True)
class _PreparedModelRuntimeFactors:
    calendar_registry: CalendarRegistry
    exchange_code: str
    interval_duration_nanos: int
    interval_delta: float
    range_end_utc: datetime
    kernel_parameters: HawkesKernelParameters
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration
    gamma_severity_mark_configuration: GammaSeverityMarkConfiguration
    news_process_configuration: NewsProcessConfiguration


@dataclass(frozen=True, slots=True)
class _PreparedModelPricingRuntimeInput:
    runtime_factors: _PreparedModelRuntimeFactors
    hourly_factor_contract: HourlyFactorContract
    effective_boundary_state: HawkesExcitationState
    effective_observed_boundary_state: MeanReversionBoundaryState
    mean_reversion_fitted_parameters: MeanReversionFittedParameters
    model_parameters: VanillaOptionPricingModelParameters


@dataclass(frozen=True, slots=True)
class _PreparedModelSimulationRuntimeInput:
    runtime_factors: _PreparedModelRuntimeFactors
    hourly_factor_contract: HourlyFactorContract
    initial_excitation_state: HawkesExcitationState
    initial_mean_reversion_boundary_state: MeanReversionBoundaryState
    mean_reversion_fitted_parameters: MeanReversionFittedParameters
    mean_reversion_runtime_interval_anchors: MeanReversionRuntimeIntervalAnchors


@dataclass(frozen=True, slots=True)
class _RuntimeStructuralSeasonalityGrid:
    interval_start_utc_nanos: tuple[int, ...]
    seasonality_factors: tuple[float, ...]


class _MemorySimulationSink:
    __slots__ = ("rows",)

    def __init__(self) -> None:
        self.rows: list[dict[str, Any]] = []

    def write_records(self, records: list[dict[str, Any]]) -> None:
        self.rows.extend(records)

    def close(self) -> None:
        return None


@dataclass(slots=True)
class JaniformSimulationResult:
    simulation_kind: str
    metadata: dict[str, Any]
    provenance: dict[str, Any]
    diagnostics: dict[str, Any]
    raw_result: CoarseSimulationPath | SeededTickSimulationResponse = field(repr=False)
    _preferences: Optional[Any] = field(repr=False, default=None)
    _calendar_registry: Optional[CalendarRegistry] = field(init=False, repr=False, default=None)
    _exchange_code: Optional[str] = field(init=False, repr=False, default=None)
    _rows_cache: Optional[list[dict[str, Any]]] = field(init=False, repr=False, default=None)

    def to_pandas(self) -> Any:
        import pandas

        rows = self._rows()
        if len(rows) == 0:
            return pandas.DataFrame(columns=self._column_names())

        return pandas.DataFrame.from_records(rows)

    def to_numpy(self) -> Any:
        import numpy

        rows = self._rows()
        if len(rows) == 0:
            return numpy.empty((0, len(self._column_names())), dtype=object)

        column_names = tuple(rows[0].keys())

        return numpy.asarray(
            [
                tuple(row[column_name] for column_name in column_names)
                for row in rows
            ],
            dtype=object,
        )

    def to_parquet(self, path: str | Path) -> None:
        output_path = Path(path)
        if self.simulation_kind == "seeded_tick":
            if self._preferences is None:
                raise RuntimeError("seeded simulation exports require output preferences")

            persist_seeded_tick_simulation(
                simulation_result=cast(SeededTickSimulationResponse, self.raw_result),
                output_path=output_path,
                preferences=self._preferences,
            )
            return

        persist_coarse_simulation(
            simulation_result=cast(CoarseSimulationPath, self.raw_result),
            output_path=output_path,
            preferences=cast(Any, self._preferences),
            calendar_registry=self._calendar_registry,
            exchange_code=self._exchange_code,
        )

    def _rows(self) -> list[dict[str, Any]]:
        if self._rows_cache is not None:
            return self._rows_cache

        sink = _MemorySimulationSink()
        if self.simulation_kind == "seeded_tick":
            if self._preferences is None:
                raise RuntimeError("seeded simulation materialization requires output preferences")

            persist_seeded_tick_simulation(
                simulation_result=cast(SeededTickSimulationResponse, self.raw_result),
                output_path=Path("simulation.parquet"),
                preferences=self._preferences,
                sink=sink,
            )
        else:
            persist_coarse_simulation(
                simulation_result=cast(CoarseSimulationPath, self.raw_result),
                output_path=Path("simulation.parquet"),
                preferences=cast(Any, self._preferences),
                sink=sink,
                calendar_registry=self._calendar_registry,
                exchange_code=self._exchange_code,
            )

        self._rows_cache = sink.rows
        return self._rows_cache

    def _column_names(self) -> tuple[str, ...]:
        if self._preferences is None:
            raise RuntimeError("simulation exports require output preferences")

        return simulation_output_columns(self._preferences)


@dataclass(frozen=True, slots=True)
class JaniformModel:
    run_id: str
    workflow_scope: CalibrationMarketDataWorkflowScope
    parameters: dict[str, Any]
    metadata: dict[str, Any]
    resolved_configuration: ResolvedPresetWorkflowConfiguration
    session_resolution: MarketDataSessionResolution
    _stage_report_payloads: tuple[dict[str, Any], ...] = field(repr=False, compare=False)

    @property
    def fitted_parameters(self) -> dict[str, Any]:
        return deepcopy(self.parameters)

    def summary(self) -> dict[str, Any]:
        summary = deepcopy(self.metadata)
        summary["fitted_parameters"] = deepcopy(self.parameters)
        return summary

    def simulate(
        self,
        *,
        seed: int,
        start_time_utc: datetime,
        horizon_interval_count: int,
        interval: Optional[str | timedelta] = None,
        output_path: Optional[str | Path] = None,
        sink: Optional[SimulationArtifactSink] = None,
        continuation_data: Optional[CurrentStateEstimationRequest] = None,
        initial_excitation_state: Optional[HawkesExcitationState] = None,
        initial_mean_reversion_boundary_state: Optional[MeanReversionBoundaryState] = None,
        debug_trace_enabled: bool = False,
        sample_every_nth_transition: int = 1,
        max_records_per_chunk: int = 1024,
        state_snapshot_capture: SeededTickEventStateSnapshotCapture = SeededTickEventStateSnapshotCapture.DISABLED,
        silent: bool = False,
        progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
        progress_callback: Optional[Callable[[SimulationProgressEvent], None]] = None,
    ) -> JaniformSimulationResult:
        if output_path is not None and sink is not None:
            raise _calibration_error(
                "sink",
                "output_path and sink are mutually exclusive",
            )

        effective_interval = _DEFAULT_HIGH_LEVEL_SIMULATION_INTERVAL if interval is None else interval
        output_controller = JaniformOutputController(
            suppress_info_output=silent,
            progress_mode=WorkflowProgressMode.NONE if silent else progress_mode,
            default_progress_mode=(
                WorkflowProgressMode.NONE if silent else WorkflowProgressMode.HUMAN
            ),
        )
        simulation_kind = "seeded_tick" if interval is None else "coarse_interval"

        execution_inputs = _prepare_model_simulation_inputs(
            self,
            seed=seed,
            start_time_utc=start_time_utc,
            horizon_interval_count=horizon_interval_count,
            interval=effective_interval,
            simulation_kind=simulation_kind,
            continuation_data=continuation_data,
            initial_excitation_state=initial_excitation_state,
            initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
            debug_trace_enabled=debug_trace_enabled,
            sample_every_nth_transition=sample_every_nth_transition,
            max_records_per_chunk=max_records_per_chunk,
            state_snapshot_capture=state_snapshot_capture,
        )
        with output_controller.progress("simulate") as progress:
            def emit_simulation_progress(event: SimulationProgressEvent) -> None:
                progress.update(
                    detail=event.detail,
                    completed_units=event.completed_units,
                    total_units=event.total_units,
                )
                if progress_callback is not None:
                    progress_callback(event)

            if simulation_kind == "seeded_tick":
                if execution_inputs.seeded_request is None:
                    raise RuntimeError("JaniformModel seeded simulation inputs were not prepared")

                raw_simulation_result = run_seeded_tick_event_simulation(
                    execution_inputs.seeded_request,
                    progress_callback=emit_simulation_progress,
                )
            else:
                if execution_inputs.coarse_request is None:
                    raise RuntimeError("JaniformModel coarse simulation inputs were not prepared")

                raw_simulation_result = run_coarse_simulation(
                    execution_inputs.coarse_request,
                    progress_callback=emit_simulation_progress,
                )

        preferences = simulation_output_preferences_from_model(self)
        simulation_result = _build_high_level_simulation_result(
            model=self,
            simulation_result=raw_simulation_result,
            preferences=preferences,
        )

        if sink is not None:
            if simulation_result.simulation_kind != "seeded_tick":
                raise _calibration_error(
                    "sink",
                    "sink output is currently supported only for seeded tick simulation",
                )

            persist_seeded_tick_simulation(
                simulation_result=cast(
                    SeededTickSimulationResponse,
                    simulation_result.raw_result,
                ),
                output_path=Path("simulation.csv"),
                preferences=preferences,
                sink=sink,
            )
        elif output_path is not None:
            simulation_run_id = generate_workflow_run_id(f"{self.run_id}_simulate")
            write_simulation_artifacts(
                model=self,
                simulation_result=raw_simulation_result,
                output_path=output_path,
                simulation_run_id=simulation_run_id,
                interval_label=None if interval is None else _normalize_interval_label(effective_interval),
            )

        return simulation_result

    def price_option(
        self,
        *,
        valuation_time_utc: datetime,
        spot: float,
        strike: float,
        expiry_timestamp_utc: Optional[datetime] = None,
        expiry_date: Optional[date] = None,
        tenor: Optional[str] = None,
        payoff_type: VanillaOptionPayoffType | str = VanillaOptionPayoffType.CALL,
        risk_free_rate: float = 0.0,
        dividend_yield: float = 0.0,
        observed_data: Optional[CurrentStateEstimationRequest] = None,
        boundary_state: Optional[HawkesExcitationState] = None,
        include_transform_diagnostics: bool = False,
        include_integration_quality_diagnostics: bool = False,
        include_calibration_hooks: bool = False,
        output_path: Optional[str | Path] = None,
        silent: bool = False,
        progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
    ) -> VanillaOptionPricingRunResult:
        output_controller = JaniformOutputController(
            suppress_info_output=silent,
            progress_mode=WorkflowProgressMode.NONE if silent else progress_mode,
            default_progress_mode=(
                WorkflowProgressMode.NONE if silent else WorkflowProgressMode.HUMAN
            ),
        )
        execution_input = _prepare_model_pricing_execution_input(
            self,
            valuation_time_utc=valuation_time_utc,
            spot=spot,
            strike=strike,
            expiry_timestamp_utc=expiry_timestamp_utc,
            expiry_date=expiry_date,
            tenor=tenor,
            payoff_type=payoff_type,
            risk_free_rate=risk_free_rate,
            dividend_yield=dividend_yield,
            observed_data=observed_data,
            boundary_state=boundary_state,
            include_transform_diagnostics=include_transform_diagnostics,
            include_integration_quality_diagnostics=include_integration_quality_diagnostics,
            include_calibration_hooks=include_calibration_hooks,
        )
        with output_controller.progress("price") as progress:
            progress.update(detail="running pricing workflow")
            pricing_result = run_vanilla_option_pricing(execution_input)

        if output_path is not None:
            pricing_run_id = generate_workflow_run_id(f"{self.run_id}_price")
            write_pricing_artifacts(
                model=self,
                pricing_result=pricing_result,
                output_path=output_path,
                pricing_run_id=pricing_run_id,
            )

        return pricing_result

    def price_option_groups(
        self,
        *,
        valuation_time_utc: datetime,
        batch: JaniformGroupedVanillaOptionBatch,
        observed_data: Optional[CurrentStateEstimationRequest] = None,
        boundary_state: Optional[HawkesExcitationState] = None,
        include_transform_diagnostics: bool = False,
        include_integration_quality_diagnostics: bool = False,
        cpu_count: Optional[int] = None,
        silent: bool = False,
        progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
    ) -> JaniformVanillaOptionGroupedBatchPricingResult:
        output_controller = JaniformOutputController(
            suppress_info_output=silent,
            progress_mode=WorkflowProgressMode.NONE if silent else progress_mode,
            default_progress_mode=(
                WorkflowProgressMode.NONE if silent else WorkflowProgressMode.HUMAN
            ),
        )
        native_execution_input = _prepare_model_grouped_batch_pricing_native_input(
            self,
            valuation_time_utc=valuation_time_utc,
            batch=batch,
            observed_data=observed_data,
            boundary_state=boundary_state,
            include_transform_diagnostics=include_transform_diagnostics,
            include_integration_quality_diagnostics=include_integration_quality_diagnostics,
            cpu_count=cpu_count,
        )
        with output_controller.progress("price") as progress:
            progress.update(detail="running grouped batch pricing workflow")
            return _run_model_vanilla_option_grouped_batch_pricing(
                native_execution_input,
                progress=progress,
            )

    def price_option_surface(
        self,
        *,
        valuation_time_utc: datetime,
        surface: JaniformVanillaOptionSurface,
        observed_data: Optional[CurrentStateEstimationRequest] = None,
        boundary_state: Optional[HawkesExcitationState] = None,
        include_transform_diagnostics: bool = False,
        include_integration_quality_diagnostics: bool = False,
        cpu_count: Optional[int] = None,
        silent: bool = False,
        progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
    ) -> JaniformVanillaOptionSurfacePricingResult:
        output_controller = JaniformOutputController(
            suppress_info_output=silent,
            progress_mode=WorkflowProgressMode.NONE if silent else progress_mode,
            default_progress_mode=(
                WorkflowProgressMode.NONE if silent else WorkflowProgressMode.HUMAN
            ),
        )
        native_execution_input = _prepare_model_surface_pricing_native_input(
            self,
            valuation_time_utc=valuation_time_utc,
            surface=surface,
            observed_data=observed_data,
            boundary_state=boundary_state,
            include_transform_diagnostics=include_transform_diagnostics,
            include_integration_quality_diagnostics=include_integration_quality_diagnostics,
            cpu_count=cpu_count,
        )
        with output_controller.progress("price") as progress:
            progress.update(detail="running surface pricing workflow")
            return _run_model_vanilla_option_surface_pricing(
                native_execution_input,
                progress=progress,
            )

    def estimate_current_state(
        self,
        *,
        state_time_utc: datetime,
        request: CurrentStateEstimationRequest,
    ) -> CurrentStateEstimationResult:
        return estimate_current_state_for_model(
            self,
            state_time_utc=state_time_utc,
            request=request,
        )

    def save(self, path: str | Path) -> None:
        model_path = Path(path)
        model_path.write_text(
            serialize_pretty_json(_build_model_persistence_payload(self)),
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
        raw_resolved_configuration = payload.get("resolved_configuration")
        raw_session_resolution = payload.get("session_resolution")

        if not isinstance(raw_parameters, dict):
            raise _calibration_error("parameters", "must be an object")
        if not isinstance(raw_metadata, dict):
            raise _calibration_error("metadata", "must be an object")
        if not isinstance(raw_resolved_configuration, dict):
            raise _calibration_error("resolved_configuration", "must be an object")
        if not isinstance(raw_session_resolution, dict):
            raise _calibration_error("session_resolution", "must be an object")

        try:
            workflow_scope = CalibrationMarketDataWorkflowScope(workflow_scope_value)
        except ValueError as e:
            raise _calibration_error(
                "workflow_scope",
                "must be a valid CalibrationMarketDataWorkflowScope value",
            ) from e

        return JaniformModel(
            run_id=require_non_empty_string(payload, "run_id", "run_id", _calibration_error),
            workflow_scope=workflow_scope,
            parameters=_normalize_model_parameters(raw_parameters),
            metadata=deepcopy(raw_metadata),
            resolved_configuration=ResolvedPresetWorkflowConfiguration.from_mapping(
                raw_resolved_configuration,
                field_path="resolved_configuration",
            ),
            session_resolution=MarketDataSessionResolution.from_mapping(
                raw_session_resolution,
                field_path="session_resolution",
            ),
            _stage_report_payloads=(),
        )


def _build_high_level_simulation_result(
    *,
    model: JaniformModel,
    simulation_result: CoarseSimulationPath | SeededTickSimulationResponse,
    preferences: Any,
) -> JaniformSimulationResult:
    if isinstance(simulation_result, SeededTickSimulationResponse):
        return JaniformSimulationResult(
            simulation_kind="seeded_tick",
            metadata=simulation_result.metadata.to_payload(),
            provenance={
                "model_run_id": model.run_id,
                "event_tape_metadata": simulation_result.event_tape_metadata.to_payload(),
                "simulation_output_preferences": asdict(preferences),
            },
            diagnostics=simulation_result.diagnostics.to_payload(),
            raw_result=simulation_result,
            _preferences=preferences,
        )

    result = JaniformSimulationResult(
        simulation_kind="coarse_interval",
        metadata=simulation_result.output_schema_metadata.to_payload(),
        provenance={
            "model_run_id": model.run_id,
            "simulation_output_preferences": asdict(preferences),
            **simulation_result.provenance.to_payload(),
        },
        diagnostics=simulation_result.diagnostics.to_payload(),
        raw_result=simulation_result,
        _preferences=preferences,
    )
    result._calendar_registry = CalendarRegistry(model.session_resolution.resolved_session_definition_json)
    result._exchange_code = model.session_resolution.exchange_code
    return result


def _prepare_model_simulation_inputs(
    model: JaniformModel,
    *,
    seed: int,
    start_time_utc: datetime,
    horizon_interval_count: int,
    interval: str | timedelta,
    simulation_kind: str,
    continuation_data: Optional[CurrentStateEstimationRequest] = None,
    initial_excitation_state: Optional[HawkesExcitationState],
    initial_mean_reversion_boundary_state: Optional[MeanReversionBoundaryState] = None,
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

    initial_state_resolution = _resolve_model_simulation_initial_state(
        model,
        start_time_utc=start_time_utc,
        continuation_data=continuation_data,
        default_initial_excitation_state=(
            runtime_factors.kernel_parameters.stationary_excitation_state()
        ),
        initial_excitation_state=initial_excitation_state,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
    )

    effective_initial_state = initial_state_resolution.excitation_state
    effective_start_anchor_value = initial_state_resolution.start_anchor_value
    effective_mean_reversion_boundary_state = initial_state_resolution.mean_reversion_boundary_state

    coarse_request: Optional[CoarseSimulationRunRequest] = None
    seeded_request: Optional[SeededTickSimulationRequest] = None

    match simulation_kind:
        case "coarse_interval":
            coarse_request = _prepare_model_coarse_interval_simulation_request(
                model,
                seed=seed,
                start_time_utc=start_time_utc,
                horizon_interval_count=horizon_interval_count,
                interval=interval,
                runtime_factors=runtime_factors,
                initial_excitation_state=effective_initial_state,
                start_anchor_value=effective_start_anchor_value,
                initial_mean_reversion_boundary_state=effective_mean_reversion_boundary_state,
            )
        case "seeded_tick":
            seeded_request = _prepare_model_seeded_tick_simulation_request(
                model,
                seed=seed,
                start_time_utc=start_time_utc,
                horizon_interval_count=horizon_interval_count,
                interval=interval,
                runtime_factors=runtime_factors,
                initial_excitation_state=effective_initial_state,
                start_anchor_value=effective_start_anchor_value,
                initial_mean_reversion_boundary_state=effective_mean_reversion_boundary_state,
                debug_trace_enabled=debug_trace_enabled,
                sample_every_nth_transition=sample_every_nth_transition,
                max_records_per_chunk=max_records_per_chunk,
                state_snapshot_capture=state_snapshot_capture,
            )
        case _:
            raise RuntimeError(
                "JaniformModel simulate must prepare either `seeded_tick` or `coarse_interval` inputs"
            )

    return _PreparedModelSimulationInputs(
        coarse_request=coarse_request,
        seeded_request=seeded_request,
    )


def _prepare_model_coarse_interval_simulation_request(
    model: JaniformModel,
    *,
    seed: int,
    start_time_utc: datetime,
    horizon_interval_count: int,
    interval: str | timedelta,
    runtime_factors: _PreparedModelRuntimeFactors,
    initial_excitation_state: HawkesExcitationState,
    start_anchor_value: float,
    initial_mean_reversion_boundary_state: Optional[MeanReversionBoundaryState],
) -> CoarseSimulationRunRequest:
    runtime_input = _prepare_model_simulation_runtime_input(
        model,
        start_time_utc=start_time_utc,
        interval=interval,
        runtime_factors=runtime_factors,
        initial_excitation_state=initial_excitation_state,
        start_anchor_value=start_anchor_value,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
    )

    return CoarseSimulationRunRequest.from_runtime_inputs(
        seed=seed,
        start_time_utc=start_time_utc,
        horizon_interval_count=horizon_interval_count,
        interval_delta=runtime_input.runtime_factors.interval_delta,
        risk_model_inputs=SufficientStatisticsRiskModelInputs(
            equation_specification=SufficientStatisticsRiskEquationSpecification(
                hawkes_kernel_parameters=runtime_input.runtime_factors.kernel_parameters,
                gamma_severity_mark_configuration=(
                    runtime_input.runtime_factors.gamma_severity_mark_configuration
                ),
            ),
            nig_shared_plus_shift_configuration=(
                runtime_input.runtime_factors.nig_shared_plus_shift_configuration
            ),
            initial_excitation_state=runtime_input.initial_excitation_state,
            mean_reversion_fitted_parameters=runtime_input.mean_reversion_fitted_parameters,
            mean_reversion_runtime_interval_anchors=(
                runtime_input.mean_reversion_runtime_interval_anchors
            ),
            initial_mean_reversion_boundary_state=(
                runtime_input.initial_mean_reversion_boundary_state
            ),
        ),
        hourly_factor_contract=_zero_closed_coarse_interval_seasonality_factors(
            runtime_input.hourly_factor_contract,
            calendar_registry=runtime_input.runtime_factors.calendar_registry,
            exchange_code=runtime_input.runtime_factors.exchange_code,
        ),
        news_process_configuration=runtime_input.runtime_factors.news_process_configuration,
        calendar_registry=runtime_input.runtime_factors.calendar_registry,
        exchange_code=runtime_input.runtime_factors.exchange_code,
    )


def _prepare_model_seeded_tick_simulation_request(
    model: JaniformModel,
    *,
    seed: int,
    start_time_utc: datetime,
    horizon_interval_count: int,
    interval: str | timedelta,
    runtime_factors: _PreparedModelRuntimeFactors,
    initial_excitation_state: HawkesExcitationState,
    start_anchor_value: float,
    initial_mean_reversion_boundary_state: Optional[MeanReversionBoundaryState],
    debug_trace_enabled: bool,
    sample_every_nth_transition: int,
    max_records_per_chunk: int,
    state_snapshot_capture: SeededTickEventStateSnapshotCapture,
) -> SeededTickSimulationRequest:
    runtime_input = _prepare_model_simulation_runtime_input(
        model,
        start_time_utc=start_time_utc,
        interval=interval,
        runtime_factors=runtime_factors,
        initial_excitation_state=initial_excitation_state,
        start_anchor_value=start_anchor_value,
        initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
    )

    return SeededTickSimulationRequest.from_runtime_inputs(
        seed=seed,
        start_time_utc=start_time_utc,
        horizon_interval_count=horizon_interval_count,
        kernel_parameters=runtime_input.runtime_factors.kernel_parameters,
        nig_shared_plus_shift_configuration=(
            runtime_input.runtime_factors.nig_shared_plus_shift_configuration
        ),
        news_process_configuration=runtime_input.runtime_factors.news_process_configuration,
        initial_excitation_state=runtime_input.initial_excitation_state,
        mean_reversion_fitted_parameters=runtime_input.mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors=(
            runtime_input.mean_reversion_runtime_interval_anchors
        ),
        initial_mean_reversion_boundary_state=(
            runtime_input.initial_mean_reversion_boundary_state
        ),
        hourly_factor_contract=runtime_input.hourly_factor_contract,
        debug_trace_enabled=debug_trace_enabled,
        sample_every_nth_transition=sample_every_nth_transition,
        max_records_per_chunk=max_records_per_chunk,
        state_snapshot_capture=state_snapshot_capture,
    )


def _prepare_model_simulation_runtime_input(
    model: JaniformModel,
    *,
    start_time_utc: datetime,
    interval: str | timedelta,
    runtime_factors: _PreparedModelRuntimeFactors,
    initial_excitation_state: HawkesExcitationState,
    start_anchor_value: float,
    initial_mean_reversion_boundary_state: Optional[MeanReversionBoundaryState],
) -> _PreparedModelSimulationRuntimeInput:
    hourly_factor_contract = _build_model_simulation_hourly_factor_contract(
        model,
        runtime_factors=runtime_factors,
        start_time_utc=start_time_utc,
        interval=interval,
    )

    (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        derived_initial_mean_reversion_boundary_state,
    ) = _prepare_model_mean_reversion_runtime_inputs(
        model,
        runtime_factors=runtime_factors,
        hourly_factor_contract=hourly_factor_contract,
        initial_excitation_state=initial_excitation_state,
        start_anchor_value=start_anchor_value,
    )

    effective_initial_mean_reversion_boundary_state = (
        derived_initial_mean_reversion_boundary_state
        if initial_mean_reversion_boundary_state is None
        else initial_mean_reversion_boundary_state
    )

    return _PreparedModelSimulationRuntimeInput(
        runtime_factors=runtime_factors,
        hourly_factor_contract=hourly_factor_contract,
        initial_excitation_state=initial_excitation_state,
        initial_mean_reversion_boundary_state=effective_initial_mean_reversion_boundary_state,
        mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
    )


def _build_model_simulation_hourly_factor_contract(
    model: JaniformModel,
    *,
    runtime_factors: _PreparedModelRuntimeFactors,
    start_time_utc: datetime,
    interval: str | timedelta,
) -> HourlyFactorContract:
    return provide_hourly_factor_contract(
        calendar_registry=runtime_factors.calendar_registry,
        exchange_code=runtime_factors.exchange_code,
        range_start_utc=start_time_utc,
        range_end_utc=runtime_factors.range_end_utc,
        interval=interval,
        seasonality_source=_build_runtime_hourly_seasonality_source_from_model(
            model,
            calendar_registry=runtime_factors.calendar_registry,
            exchange_code=runtime_factors.exchange_code,
            range_start_utc=start_time_utc,
            range_end_utc=runtime_factors.range_end_utc,
            interval=interval,
        ),
        news_intensity_source=HourlyNewsIntensityProviderMaterializedSource(
            known_scheduled_event_timestamps_utc=_resolved_known_news_event_timestamps(model),
            surprise_baseline_intensity=_surprise_baseline_intensity(model),
            surprise_deterministic_time_multiplier_by_interval_index=(
                _surprise_deterministic_time_multiplier_by_interval_index(
                    model,
                    range_start_utc=start_time_utc,
                    range_end_utc=runtime_factors.range_end_utc,
                    interval_duration_nanos=runtime_factors.interval_duration_nanos,
                )
            ),
        ),
    )


def _resolve_model_simulation_initial_state(
    model: JaniformModel,
    *,
    start_time_utc: datetime,
    continuation_data: Optional[CurrentStateEstimationRequest],
    default_initial_excitation_state: HawkesExcitationState,
    initial_excitation_state: Optional[HawkesExcitationState],
    initial_mean_reversion_boundary_state: Optional[MeanReversionBoundaryState],
) -> _ResolvedModelSimulationInitialState:
    estimated_current_state: Optional[CurrentStateEstimationResult] = None
    if continuation_data is not None and (
        initial_excitation_state is None
        or initial_mean_reversion_boundary_state is None
    ):
        estimated_current_state = model.estimate_current_state(
            state_time_utc=start_time_utc,
            request=continuation_data,
        )

    effective_initial_excitation_state = (
        default_initial_excitation_state
        if initial_excitation_state is None and estimated_current_state is None
        else (
            cast(CurrentStateEstimationResult, estimated_current_state).hawkes_excitation_state
            if initial_excitation_state is None
            else initial_excitation_state
        )
    )

    effective_initial_mean_reversion_boundary_state = (
        initial_mean_reversion_boundary_state
        if initial_mean_reversion_boundary_state is not None
        else (
            None
            if estimated_current_state is None
            else estimated_current_state.mean_reversion_boundary_state
        )
    )

    start_anchor_value = (
        0.0
        if effective_initial_mean_reversion_boundary_state is None
        else effective_initial_mean_reversion_boundary_state.anchor_value
    )

    return _ResolvedModelSimulationInitialState(
        excitation_state=effective_initial_excitation_state,
        mean_reversion_boundary_state=effective_initial_mean_reversion_boundary_state,
        start_anchor_value=start_anchor_value,
    )


def _prepare_model_mean_reversion_runtime_inputs(
    model: JaniformModel,
    *,
    runtime_factors: _PreparedModelRuntimeFactors,
    hourly_factor_contract: HourlyFactorContract,
    initial_excitation_state: HawkesExcitationState,
    start_anchor_value: float = 0.0,
) -> tuple[
    MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
    MeanReversionBoundaryState,
]:
    fitted_parameters = _mean_reversion_fitted_parameters(model)

    runtime_interval_anchors = _materialize_model_mean_reversion_runtime_interval_anchors(
        fitted_parameters=fitted_parameters,
        runtime_factors=runtime_factors,
        hourly_factor_contract=hourly_factor_contract,
        initial_excitation_state=initial_excitation_state,
        start_anchor_value=start_anchor_value,
    )

    initial_boundary_state = MeanReversionBoundaryState(
        observed_log_price=runtime_interval_anchors.start_anchor_value,
        anchor_value=runtime_interval_anchors.start_anchor_value,
        pressure_state=runtime_interval_anchors.start_pressure_state,
        deviation=0.0,
    )

    return fitted_parameters, runtime_interval_anchors, initial_boundary_state


def _materialize_model_mean_reversion_runtime_interval_anchors(
    *,
    fitted_parameters: MeanReversionFittedParameters,
    runtime_factors: _PreparedModelRuntimeFactors,
    hourly_factor_contract: HourlyFactorContract,
    initial_excitation_state: HawkesExcitationState,
    start_anchor_value: float,
) -> MeanReversionRuntimeIntervalAnchors:
    interval_end_times_utc = tuple(
        _runtime_datetime_from_utc_nanos(timestamp_utc_nanos)
        for timestamp_utc_nanos in hourly_factor_contract.interval_end_utc_nanos
    )

    interval_deltas = tuple(
        nanoseconds_to_hours(hourly_factor_contract.interval_duration_nanos)
        for _ in range(hourly_factor_contract.interval_count)
    )

    if len(interval_end_times_utc) != len(interval_deltas):
        raise RuntimeError(
            "JaniformModel mean reversion runtime anchor materialization requires one interval "
            "end timestamp per interval delta"
        )

    return _materialize_endogenous_model_mean_reversion_runtime_interval_anchors(
        fitted_parameters=fitted_parameters,
        runtime_factors=runtime_factors,
        hourly_factor_contract=hourly_factor_contract,
        initial_excitation_state=initial_excitation_state,
        interval_alignment_policy=hourly_factor_contract.interval_alignment_policy,
        start_anchor_value=start_anchor_value,
    )


def _materialize_endogenous_model_mean_reversion_runtime_interval_anchors(
    *,
    fitted_parameters: MeanReversionFittedParameters,
    runtime_factors: _PreparedModelRuntimeFactors,
    hourly_factor_contract: HourlyFactorContract,
    initial_excitation_state: HawkesExcitationState,
    interval_alignment_policy: str,
    start_anchor_value: float,
) -> MeanReversionRuntimeIntervalAnchors:
    native = native_module()

    runtime_anchor_payload = native.materialize_endogenous_mean_reversion_runtime_interval_anchors(
        runtime_factors.kernel_parameters.to_payload(),
        runtime_factors.nig_shared_plus_shift_configuration.to_payload(),
        runtime_factors.news_process_configuration.to_payload(),
        initial_excitation_state.to_payload(),
        fitted_parameters.to_payload(),
        interval_alignment_policy,
        start_anchor_value,
        tuple(hourly_factor_contract.s_h),
        tuple(hourly_factor_contract.known_component_nu_h),
        tuple(hourly_factor_contract.surprise_component_nu_h),
        tuple(
            nanoseconds_to_hours(hourly_factor_contract.interval_duration_nanos)
            for _ in range(hourly_factor_contract.interval_count)
        ),
    )

    return MeanReversionRuntimeIntervalAnchors.from_mapping(
        dict(cast(Mapping[str, Any], runtime_anchor_payload)),
        field_path="mean_reversion_runtime_interval_anchors",
    )


def _build_runtime_interval_end_times_utc(
    *,
    start_time_utc: datetime,
    interval_duration_nanos: int,
    horizon_interval_count: int,
) -> tuple[datetime, ...]:
    interval_duration = timedelta(seconds=interval_duration_nanos / 1_000_000_000)

    return tuple(
        start_time_utc + interval_duration * interval_index
        for interval_index in range(1, horizon_interval_count + 1)
    )


def _mean_reversion_fitted_parameters(model: JaniformModel) -> MeanReversionFittedParameters:
    raw_mean_reversion = model.parameters.get("mean_reversion")
    if isinstance(raw_mean_reversion, Mapping):
        fitted_parameters = raw_mean_reversion.get("fitted_parameters")
        if isinstance(fitted_parameters, Mapping):
            return MeanReversionFittedParameters.from_mapping(
                dict(fitted_parameters),
                field_path="mean_reversion.fitted_parameters",
            )

    return MeanReversionFittedParameters.from_mapping(
        dict(
            _require_payload_mapping(
                _return_components_fitted_parameters_payload(model),
                "mean_reversion_fitted_parameters",
                "return_components.fitted_parameters.mean_reversion_fitted_parameters",
            )
        ),
        field_path="return_components.fitted_parameters.mean_reversion_fitted_parameters",
    )


def _runtime_datetime_from_utc_nanos(timestamp_utc_nanos: int) -> datetime:
    seconds, nanos_remainder = divmod(timestamp_utc_nanos, 1_000_000_000)
    return datetime.fromtimestamp(seconds, tz=UTC) + timedelta(microseconds=nanos_remainder / 1000.0)


def _prepare_model_pricing_execution_input(
    model: JaniformModel,
    *,
    valuation_time_utc: datetime,
    spot: float,
    strike: float,
    expiry_timestamp_utc: Optional[datetime],
    expiry_date: Optional[date],
    tenor: Optional[str],
    payoff_type: VanillaOptionPayoffType | str,
    risk_free_rate: float,
    dividend_yield: float,
    observed_data: Optional[CurrentStateEstimationRequest] = None,
    boundary_state: Optional[HawkesExcitationState],
    include_transform_diagnostics: bool,
    include_integration_quality_diagnostics: bool,
    include_calibration_hooks: bool,
) -> VanillaOptionPricingExecutionInput:
    resolved_maturity_inputs = resolve_single_high_level_pricing_maturity_input(
        valuation_time_utc=valuation_time_utc,
        expiry_timestamp_utc=expiry_timestamp_utc,
        expiry_date=expiry_date,
        tenor=tenor,
    )

    maturity_interval = resolved_maturity_inputs.maturity_intervals[0]

    horizon_interval_count = _pricing_interval_count_from_maturity_interval(
        maturity_interval,
        field_path="maturity_input",
    )

    runtime_input = _prepare_model_pricing_runtime_input(
        model,
        valuation_time_utc=valuation_time_utc,
        spot=spot,
        horizon_interval_count=horizon_interval_count,
        observed_data=observed_data,
        boundary_state=boundary_state,
    )

    try:
        normalized_payoff_type = (
            payoff_type
            if isinstance(payoff_type, VanillaOptionPayoffType)
            else VanillaOptionPayoffType(payoff_type)
        )
    except ValueError as e:
        raise _calibration_error("payoff_type", "must be one of: call, put") from e

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
            boundary_input=AnchoredVanillaOptionPricingBoundaryInput(
                interval_index=0,
                hawkes_boundary_state=runtime_input.effective_boundary_state,
                mean_reversion_fitted_parameters=runtime_input.mean_reversion_fitted_parameters,
                interval_delta=runtime_input.runtime_factors.interval_delta,
                observed_boundary_state=runtime_input.effective_observed_boundary_state,
            ),
            model_parameters=runtime_input.model_parameters,
        ),
        hourly_factor_contract=runtime_input.hourly_factor_contract,
        optional_output_selection=VanillaOptionPricingOptionalOutputSelection(
            include_transform_diagnostics=include_transform_diagnostics,
            include_integration_quality_diagnostics=include_integration_quality_diagnostics,
            include_calibration_hooks=include_calibration_hooks,
        ),
    )


def _prepare_model_pricing_runtime_input(
    model: JaniformModel,
    *,
    valuation_time_utc: datetime,
    spot: float,
    horizon_interval_count: int,
    observed_data: Optional[CurrentStateEstimationRequest],
    boundary_state: Optional[HawkesExcitationState],
) -> _PreparedModelPricingRuntimeInput:
    runtime_factors = _prepare_model_runtime_factors(
        model,
        start_time_utc=valuation_time_utc,
        horizon_interval_count=horizon_interval_count,
        interval="1h",
        prefer_joint_pricing_block=True,
    )

    hourly_factor_contract = _build_model_pricing_hourly_factor_contract(
        model,
        runtime_factors=runtime_factors,
        valuation_time_utc=valuation_time_utc,
    )

    mean_reversion_fitted_parameters = _mean_reversion_fitted_parameters(model)

    if boundary_state is not None:
        effective_boundary_state = boundary_state
        (
            _derived_mean_reversion_fitted_parameters,
            _derived_mean_reversion_runtime_interval_anchors,
            effective_observed_boundary_state,
        ) = _prepare_model_mean_reversion_runtime_inputs(
            model,
            runtime_factors=runtime_factors,
            hourly_factor_contract=hourly_factor_contract,
            initial_excitation_state=effective_boundary_state,
            start_anchor_value=_pricing_observed_log_price_from_spot(model, spot=spot),
        )
    elif observed_data is not None:
        pricing_state = model.estimate_current_state(
            state_time_utc=valuation_time_utc,
            request=CurrentStateEstimationRequest(
                observed_tick_data=observed_data.observed_tick_data,
                tick_data_column_mapping=observed_data.tick_data_column_mapping,
                observed_price=spot if observed_data.observed_price is None else observed_data.observed_price,
            ),
        )
        effective_boundary_state = pricing_state.hawkes_excitation_state
        effective_observed_boundary_state = pricing_state.mean_reversion_boundary_state
    else:
        effective_boundary_state = runtime_factors.kernel_parameters.stationary_excitation_state()
        (
            _derived_mean_reversion_fitted_parameters,
            _derived_mean_reversion_runtime_interval_anchors,
            effective_observed_boundary_state,
        ) = _prepare_model_mean_reversion_runtime_inputs(
            model,
            runtime_factors=runtime_factors,
            hourly_factor_contract=hourly_factor_contract,
            initial_excitation_state=effective_boundary_state,
            start_anchor_value=_pricing_observed_log_price_from_spot(model, spot=spot),
        )

    return _PreparedModelPricingRuntimeInput(
        runtime_factors=runtime_factors,
        hourly_factor_contract=hourly_factor_contract,
        effective_boundary_state=effective_boundary_state,
        effective_observed_boundary_state=effective_observed_boundary_state,
        mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
        model_parameters=VanillaOptionPricingModelParameters(
            hawkes_kernel_parameters=runtime_factors.kernel_parameters,
            news_process_configuration=runtime_factors.news_process_configuration,
            nig_shared_plus_shift_configuration=runtime_factors.nig_shared_plus_shift_configuration,
        ),
    )


def _build_model_pricing_hourly_factor_contract(
    model: JaniformModel,
    *,
    runtime_factors: _PreparedModelRuntimeFactors,
    valuation_time_utc: datetime,
) -> HourlyFactorContract:
    return provide_hourly_factor_contract(
        calendar_registry=runtime_factors.calendar_registry,
        exchange_code=runtime_factors.exchange_code,
        range_start_utc=valuation_time_utc,
        range_end_utc=runtime_factors.range_end_utc,
        interval="1h",
        seasonality_source=_build_runtime_hourly_seasonality_source_from_model(
            model,
            calendar_registry=runtime_factors.calendar_registry,
            exchange_code=runtime_factors.exchange_code,
            range_start_utc=valuation_time_utc,
            range_end_utc=runtime_factors.range_end_utc,
            interval="1h",
        ),
        news_intensity_source=HourlyNewsIntensityProviderMaterializedSource(
            known_scheduled_event_timestamps_utc=_resolved_known_news_event_timestamps(model),
            surprise_baseline_intensity=_surprise_baseline_intensity(model),
            surprise_deterministic_time_multiplier_by_interval_index=(
                _surprise_deterministic_time_multiplier_by_interval_index(
                    model,
                    range_start_utc=valuation_time_utc,
                    range_end_utc=runtime_factors.range_end_utc,
                    interval_duration_nanos=runtime_factors.interval_duration_nanos,
                )
            ),
        ),
    )


def _pricing_interval_count_from_maturity_interval(
    maturity_interval: float,
    *,
    field_path: str,
) -> int:
    horizon_interval_count_float = maturity_interval / 1.0
    horizon_interval_count = int(horizon_interval_count_float)
    if (
        horizon_interval_count <= 0
        or float(horizon_interval_count) != horizon_interval_count_float
    ):
        raise _calibration_error(
            field_path,
            "must be a positive exact multiple of the pricing runtime interval 1.0",
        )

    return horizon_interval_count


def _pricing_observed_log_price_from_spot(model: JaniformModel, *, spot: float) -> float:
    normalized_spot = require_positive_finite_scalar(
        spot,
        "spot",
        _calibration_error,
    )
    reference_price = _model_reference_price(model)

    return log(normalized_spot / reference_price)


def _prepare_model_grouped_batch_pricing_native_input(
    model: JaniformModel,
    *,
    valuation_time_utc: datetime,
    batch: JaniformGroupedVanillaOptionBatch,
    observed_data: Optional[CurrentStateEstimationRequest],
    boundary_state: Optional[HawkesExcitationState],
    include_transform_diagnostics: bool,
    include_integration_quality_diagnostics: bool,
    cpu_count: Optional[int],
) -> dict[str, Any]:
    resolved_group_maturity_inputs = tuple(
        resolve_high_level_pricing_maturity_axis(
            valuation_time_utc=valuation_time_utc,
            expiry_timestamps_utc=group.expiry_timestamps_utc,
            expiry_dates=group.expiry_dates,
            tenors=group.tenors,
            field_path=f"batch.groups[{group_index}]",
        )
        for group_index, group in enumerate(batch.groups)
    )

    interval_counts = tuple(
        _pricing_interval_count_from_maturity_interval(
            maturity_interval,
            field_path=(
                f"batch.groups[{group_index}].maturity_inputs[{maturity_index}]"
            ),
        )
        for group_index, resolved_group in enumerate(resolved_group_maturity_inputs)
        for maturity_index, maturity_interval in enumerate(resolved_group.maturity_intervals)
    )

    max_interval_count = max(interval_counts)

    runtime_input = _prepare_model_pricing_runtime_input(
        model,
        valuation_time_utc=valuation_time_utc,
        spot=batch.spot,
        horizon_interval_count=max_interval_count,
        observed_data=observed_data,
        boundary_state=boundary_state,
    )

    return {
        "request": {
            "spot": batch.spot,
            "groups": [
                {
                    "payoff_type": group.payoff_type.value,
                    "risk_free_rate": group.risk_free_rate,
                    "dividend_yield": group.dividend_yield,
                    "strikes": list(group.strikes),
                    "maturity_intervals": list(resolved_group.maturity_intervals),
                }
                for group, resolved_group in zip(batch.groups, resolved_group_maturity_inputs, strict=True)
            ],
        },
        "boundary_input": AnchoredVanillaOptionPricingBoundaryInput(
            interval_index=0,
            hawkes_boundary_state=runtime_input.effective_boundary_state,
            mean_reversion_fitted_parameters=runtime_input.mean_reversion_fitted_parameters,
            interval_delta=runtime_input.runtime_factors.interval_delta,
            observed_boundary_state=runtime_input.effective_observed_boundary_state,
        ).to_payload(),
        "model_parameters": runtime_input.model_parameters.to_payload(),
        "interval_delta": runtime_input.runtime_factors.interval_delta,
        "interval_factors": _native_pricing_interval_factors(
            runtime_input.hourly_factor_contract,
            horizon_interval_count=max_interval_count,
        ),
        "optional_output_selection": VanillaOptionPricingOptionalOutputSelection(
            include_transform_diagnostics=include_transform_diagnostics,
            include_integration_quality_diagnostics=include_integration_quality_diagnostics,
            include_calibration_hooks=False,
        ).to_payload(),
        "cpu_count": cpu_count,
    }


def _prepare_model_surface_pricing_native_input(
    model: JaniformModel,
    *,
    valuation_time_utc: datetime,
    surface: JaniformVanillaOptionSurface,
    observed_data: Optional[CurrentStateEstimationRequest],
    boundary_state: Optional[HawkesExcitationState],
    include_transform_diagnostics: bool,
    include_integration_quality_diagnostics: bool,
    cpu_count: Optional[int],
) -> dict[str, Any]:
    resolved_surface_maturity_inputs = resolve_high_level_pricing_maturity_axis(
        valuation_time_utc=valuation_time_utc,
        expiry_timestamps_utc=surface.expiry_timestamps_utc,
        expiry_dates=surface.expiry_dates,
        tenors=surface.tenors,
        field_path="surface",
    )

    interval_counts = tuple(
        _pricing_interval_count_from_maturity_interval(
            maturity_interval,
            field_path=f"surface.maturity_inputs[{maturity_index}]",
        )
        for maturity_index, maturity_interval in enumerate(
            resolved_surface_maturity_inputs.maturity_intervals
        )
    )

    max_interval_count = max(interval_counts)

    runtime_input = _prepare_model_pricing_runtime_input(
        model,
        valuation_time_utc=valuation_time_utc,
        spot=surface.spot,
        horizon_interval_count=max_interval_count,
        observed_data=observed_data,
        boundary_state=boundary_state,
    )

    return {
        "request": {
            "spot": surface.spot,
            "strikes": list(surface.strikes),
            "maturity_intervals": list(resolved_surface_maturity_inputs.maturity_intervals),
            "payoff_type": surface.payoff_type.value,
            "risk_free_rate": surface.risk_free_rate,
            "dividend_yield": surface.dividend_yield,
        },
        "boundary_input": AnchoredVanillaOptionPricingBoundaryInput(
            interval_index=0,
            hawkes_boundary_state=runtime_input.effective_boundary_state,
            mean_reversion_fitted_parameters=runtime_input.mean_reversion_fitted_parameters,
            interval_delta=runtime_input.runtime_factors.interval_delta,
            observed_boundary_state=runtime_input.effective_observed_boundary_state,
        ).to_payload(),
        "model_parameters": runtime_input.model_parameters.to_payload(),
        "interval_delta": runtime_input.runtime_factors.interval_delta,
        "interval_factors": _native_pricing_interval_factors(
            runtime_input.hourly_factor_contract,
            horizon_interval_count=max_interval_count,
        ),
        "optional_output_selection": VanillaOptionPricingOptionalOutputSelection(
            include_transform_diagnostics=include_transform_diagnostics,
            include_integration_quality_diagnostics=include_integration_quality_diagnostics,
            include_calibration_hooks=False,
        ).to_payload(),
        "cpu_count": cpu_count,
    }


def _native_pricing_interval_factors(
    hourly_factor_contract: HourlyFactorContract,
    *,
    horizon_interval_count: int,
) -> list[dict[str, float]]:
    return [
        {
            "s_h": float(hourly_factor_contract.s_h[interval_index]),
            "nu_h_known": float(hourly_factor_contract.known_component_nu_h[interval_index]),
            "nu_h_surprise": float(
                hourly_factor_contract.surprise_component_nu_h[interval_index]
            ),
        }
        for interval_index in range(horizon_interval_count)
    ]


def _run_model_vanilla_option_grouped_batch_pricing(
    native_execution_input: dict[str, Any],
    *,
    progress: Optional[AbstractProgress] = None,
) -> JaniformVanillaOptionGroupedBatchPricingResult:
    return _run_model_native_pricing_workflow(
        native_method_name="run_vanilla_option_grouped_batch_pricing",
        native_execution_input=native_execution_input,
        result_parser=JaniformVanillaOptionGroupedBatchPricingResult.from_mapping,
        entrypoint="JaniformModel.price_option_groups",
        result_field_path="grouped_batch_pricing_result",
        progress=progress,
        progress_detail="running grouped batch pricing workflow",
    )


def _run_model_vanilla_option_surface_pricing(
    native_execution_input: dict[str, Any],
    *,
    progress: Optional[AbstractProgress] = None,
) -> JaniformVanillaOptionSurfacePricingResult:
    return _run_model_native_pricing_workflow(
        native_method_name="run_vanilla_option_surface_pricing",
        native_execution_input=native_execution_input,
        result_parser=JaniformVanillaOptionSurfacePricingResult.from_mapping,
        entrypoint="JaniformModel.price_option_surface",
        result_field_path="surface_pricing_result",
        progress=progress,
        progress_detail="running surface pricing workflow",
    )


def _run_model_native_pricing_workflow(
    *,
    native_method_name: str,
    native_execution_input: dict[str, Any],
    result_parser: Callable[..., Any],
    entrypoint: str,
    result_field_path: str,
    progress: Optional[AbstractProgress] = None,
    progress_detail: Optional[str] = None,
) -> Any:
    try:
        native = native_module()
    except ModuleNotFoundError as e:
        raise _calibration_integration_error(
            "native pricing module is unavailable",
            entrypoint=entrypoint,
            error_id=CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from e

    native_progress_callback: Optional[Callable[[dict[str, Any]], None]] = None
    if (
        progress is not None
        and progress_detail is not None
        and not isinstance(progress, NullProgress)
    ):
        def native_progress_callback(raw_event: dict[str, Any]) -> None:
            _update_cli_progress_from_pricing_native_event(
                progress,
                raw_event,
                detail=progress_detail,
            )

    try:
        raw_run_result = _run_native_pricing_workflow_method(
            native,
            native_method_name=native_method_name,
            native_execution_input=native_execution_input,
            native_progress_callback=native_progress_callback,
        )
    except Exception as e:
        if native_exception_matches(native, e, "PricingExecutionError"):
            raise _calibration_execution_error(
                "pricing failed during native execution",
                entrypoint=entrypoint,
                context={"source_exception_type": type(e).__name__},
            ) from e

        raise _calibration_execution_error(
            "pricing failed during native execution",
            entrypoint=entrypoint,
            context={"source_exception_type": type(e).__name__},
        ) from e

    if not isinstance(raw_run_result, dict):
        raise _calibration_integration_error(
            "native pricing returned an invalid response payload",
            entrypoint=entrypoint,
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_run_result).__name__},
        )

    try:
        return result_parser(raw_run_result, field_path=result_field_path)
    except CalibrationValidationError as e:
        raise _calibration_integration_error(
            "native pricing returned an invalid response payload",
            entrypoint=entrypoint,
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"source_error_id": e.error_id},
        ) from e


def _update_cli_progress_from_pricing_native_event(
    progress: AbstractProgress,
    raw_event: dict[str, Any],
    *,
    detail: str,
) -> PricingProgressEvent:
    event = pricing_progress_event_from_mapping(raw_event)

    progress.update(
        detail=detail,
        subdetail=event.subdetail,
        completed_units=event.completed_units,
        total_units=event.total_units,
    )

    return event


def _run_native_pricing_workflow_method(
    native: Any,
    *,
    native_method_name: str,
    native_execution_input: dict[str, Any],
    native_progress_callback: Optional[Callable[[dict[str, Any]], None]],
) -> Any:
    match native_method_name:
        case "run_vanilla_option_grouped_batch_pricing":
            if native_progress_callback is None:
                return native.run_vanilla_option_grouped_batch_pricing(native_execution_input)

            return native.run_vanilla_option_grouped_batch_pricing_with_events(
                native_execution_input,
                native_progress_callback,
            )
        case "run_vanilla_option_surface_pricing":
            if native_progress_callback is None:
                return native.run_vanilla_option_surface_pricing(native_execution_input)

            return native.run_vanilla_option_surface_pricing_with_events(
                native_execution_input,
                native_progress_callback,
            )
        case _:
            raise RuntimeError(f"unsupported native pricing method: {native_method_name}")


def _model_reference_price(model: JaniformModel) -> float:
    simulation_output_preferences = cast(
        Mapping[str, Any],
        model.metadata.get("simulation_output_preferences", {}),
    )

    raw_reference_price = simulation_output_preferences.get("reference_price")
    if raw_reference_price is None:
        raise _calibration_error(
            "metadata.simulation_output_preferences.reference_price",
            "must be present so JaniformModel can derive pricing state from spot",
        )

    return require_positive_finite_scalar(
        raw_reference_price,
        "metadata.simulation_output_preferences.reference_price",
        _calibration_error,
    )


def _prepare_model_runtime_factors(
    model: JaniformModel,
    *,
    start_time_utc: datetime,
    horizon_interval_count: int,
    interval: str | timedelta,
    prefer_joint_pricing_block: bool = False,
) -> _PreparedModelRuntimeFactors:
    _require_runtime_ready_model(model)
    _require_timezone_aware_datetime(start_time_utc, field_path="start_time_utc")

    interval_duration_nanos = _parse_interval_duration_nanos(interval, field_name="interval")

    range_end_utc = start_time_utc + timedelta(
        seconds=(interval_duration_nanos * horizon_interval_count) / 1_000_000_000
    )

    calendar_registry = CalendarRegistry(model.session_resolution.resolved_session_definition_json)

    exchange_code = model.session_resolution.exchange_code

    return_components_fitted_parameters = _return_components_fitted_parameters_payload(model)

    nig_configuration_payload = (
        _pricing_nig_shared_plus_shift_configuration_payload(model)
        if prefer_joint_pricing_block
        else _require_payload_mapping(
            return_components_fitted_parameters,
            "nig_shared_plus_shift_configuration",
            "return_components.fitted_parameters.nig_shared_plus_shift_configuration",
        )
    )

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
        interval_duration_nanos=interval_duration_nanos,
        interval_delta=nanoseconds_to_hours(interval_duration_nanos),
        range_end_utc=range_end_utc,
        kernel_parameters=_hawkes_kernel_parameters(model),
        nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration.from_mapping(
            nig_configuration_payload,
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


def _hawkes_kernel_parameters(model: JaniformModel) -> HawkesKernelParameters:
    return HawkesKernelParameters.from_mapping(
        _require_payload_mapping(
            _hawkes_fitted_parameters_payload(model),
            "kernel_parameters",
            "hawkes.fitted_parameters.kernel_parameters",
        ),
        field_path="hawkes.fitted_parameters.kernel_parameters",
    )


def _current_state_tick_data_column_mapping(
    model: JaniformModel,
    *,
    request: CurrentStateEstimationRequest,
) -> CalibrationTickDataColumnMapping:
    if request.tick_data_column_mapping is not None:
        column_mapping = request.tick_data_column_mapping
    else:
        input_inference = cast(
            Mapping[str, Any],
            model.metadata.get("input_inference", {}),
        )

        tick_price_columns = cast(
            Mapping[str, Any],
            input_inference.get("tick_price_columns", {}),
        )

        timestamp_column = input_inference.get("timestamp_column")
        price_column = tick_price_columns.get("price_column")
        trade_sign_column = tick_price_columns.get("trade_sign_column")

        if not isinstance(timestamp_column, str) or not timestamp_column.strip():
            raise _calibration_error(
                "metadata.input_inference.timestamp_column",
                "must be present so current-state estimation can read observed tick timestamps",
            )

        if not isinstance(price_column, str) or not price_column.strip():
            raise _calibration_error(
                "metadata.input_inference.tick_price_columns.price_column",
                "must be present so current-state estimation can read observed tick prices",
            )

        if not isinstance(trade_sign_column, str) or not trade_sign_column.strip():
            raise _calibration_error(
                "metadata.input_inference.tick_price_columns.trade_sign_column",
                "must be present so current-state estimation can read observed trade signs",
            )

        column_mapping = CalibrationTickDataColumnMapping(
            timestamp_column=timestamp_column,
            price_column=price_column,
            trade_sign_column=trade_sign_column,
        )

    if column_mapping.price_column is None:
        raise _calibration_error(
            "tick_data_column_mapping.price_column",
            "is required for current-state estimation",
        )

    if column_mapping.trade_sign_column is None:
        raise _calibration_error(
            "tick_data_column_mapping.trade_sign_column",
            "is required for current-state estimation",
        )

    return column_mapping


def _require_timezone_aware_datetime(value: datetime, *, field_path: str) -> None:
    if not isinstance(value, datetime):
        raise _calibration_error(field_path, "must be a datetime")

    if value.tzinfo is None:
        raise _calibration_error(field_path, "must be timezone-aware")


def _build_runtime_hourly_seasonality_source_from_model(
    model: JaniformModel,
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
    interval: str | timedelta,
) -> HourlySeasonalityProviderSource:
    seasonality_parameters = _seasonality_parameters_payload(model)
    if not _contains_structural_seasonality_parameters(seasonality_parameters):
        raise RuntimeError(
            "JaniformModel does not contain structural seasonality parameters required for runtime materialization"
        )

    intraday_minutes_per_bin = int(
        _seasonality_intraday_minutes_per_bin(seasonality_parameters)
    )
    if intraday_minutes_per_bin <= 0:
        raise RuntimeError(
            "JaniformModel seasonality.intraday_minutes_per_bin must be a positive integer"
        )

    intraday_factors = _require_payload_numeric_sequence(
        seasonality_parameters,
        "intraday_factor_by_bin",
        "seasonality.intraday_factor_by_bin",
    )
    day_of_week_factors = _require_payload_numeric_sequence(
        seasonality_parameters,
        "day_of_week_factor",
        "seasonality.day_of_week_factor",
    )
    day_of_year_factors = _require_payload_numeric_sequence(
        seasonality_parameters,
        "day_of_year_factor",
        "seasonality.day_of_year_factor",
    )

    interval_duration_nanos = _parse_interval_duration_nanos(interval, field_name="interval")
    seasonality_grid = _materialize_runtime_structural_seasonality_grid(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        interval_duration_nanos=interval_duration_nanos,
        intraday_minutes_per_bin=intraday_minutes_per_bin,
        intraday_factors=intraday_factors,
        day_of_week_factors=day_of_week_factors,
        day_of_year_factors=day_of_year_factors,
    )

    if not seasonality_grid.seasonality_factors:
        return HourlySeasonalityProviderExplicitSource(
            hourly_factors=(),
            materialization_method="runtime_compact_seasonality_parameters_mean_renormalized.v1",
        )

    if (
        interval_duration_nanos < ONE_HOUR_NANOS
        or not _runtime_interval_grid_is_hour_aligned(
            range_start_utc=range_start_utc,
            range_end_utc=range_end_utc,
        )
    ):
        return HourlySeasonalityProviderMaterializedSource(
            source_timestamps_utc=tuple(
                _runtime_datetime_from_utc_nanos(timestamp_utc_nanos)
                for timestamp_utc_nanos in seasonality_grid.interval_start_utc_nanos
            ),
            source_seasonality_factors=seasonality_grid.seasonality_factors,
            interpolation_method="previous_observation_carry_forward.v1",
            aggregation_method="mean_of_observations.v1",
            sparse_fallback_policy="global_mean.v1",
            sparse_hour_minimum_observation_count=1,
        )

    return HourlySeasonalityProviderExplicitSource(
        hourly_factors=seasonality_grid.seasonality_factors,
        materialization_method="runtime_compact_seasonality_parameters_mean_renormalized.v1",
    )


def _runtime_interval_grid_is_hour_aligned(
    *,
    range_start_utc: datetime,
    range_end_utc: datetime,
) -> bool:
    return (
        datetime_to_utc_nanos(range_start_utc) % ONE_HOUR_NANOS == 0
        and datetime_to_utc_nanos(range_end_utc) % ONE_HOUR_NANOS == 0
    )


def _materialize_runtime_structural_seasonality_grid(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
    interval_duration_nanos: int,
    intraday_minutes_per_bin: int,
    intraday_factors: tuple[float, ...],
    day_of_week_factors: tuple[float, ...],
    day_of_year_factors: tuple[float, ...],
) -> _RuntimeStructuralSeasonalityGrid:
    native_payload = native_module().materialize_runtime_structural_seasonality(
        calendar_registry._native_registry,
        exchange_code,
        range_start_utc.isoformat(),
        range_end_utc.isoformat(),
        interval_duration_nanos,
        intraday_minutes_per_bin,
        intraday_factors,
        day_of_week_factors,
        day_of_year_factors,
    )
    payload_mapping = cast(Mapping[str, Any], native_payload)

    raw_interval_start_utc_nanos = payload_mapping.get("interval_start_utc_nanos")
    if not isinstance(raw_interval_start_utc_nanos, list | tuple):
        raise RuntimeError(
            "native runtime seasonality materialization must return interval_start_utc_nanos"
        )

    interval_start_utc_nanos: list[int] = []
    for value_index, raw_value in enumerate(raw_interval_start_utc_nanos):
        if not isinstance(raw_value, int) or isinstance(raw_value, bool):
            raise RuntimeError(
                "native runtime seasonality interval_start_utc_nanos"
                f"[{value_index}] must be an integer"
            )

        interval_start_utc_nanos.append(raw_value)

    seasonality_factors = _require_payload_numeric_sequence(
        payload_mapping,
        field_name="seasonality_factors",
        field_path="native_runtime_seasonality.seasonality_factors",
    )

    if len(interval_start_utc_nanos) != len(seasonality_factors):
        raise RuntimeError(
            "native runtime seasonality materialization returned misaligned "
            "interval_start_utc_nanos and seasonality_factors"
        )

    return _RuntimeStructuralSeasonalityGrid(
        interval_start_utc_nanos=tuple(interval_start_utc_nanos),
        seasonality_factors=seasonality_factors,
    )


def _zero_closed_coarse_interval_seasonality_factors(
    hourly_factor_contract: HourlyFactorContract,
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
) -> HourlyFactorContract:
    if hourly_factor_contract.interval_duration_nanos >= ONE_HOUR_NANOS:
        return hourly_factor_contract

    payload = hourly_factor_contract.to_payload()

    gated_seasonality: list[float] = []
    contains_closed_interval = False

    for interval_start_utc_nanos, seasonality_factor in zip(
        payload["interval_start_utc_nanos"],
        payload["s_h"],
        strict=True,
    ):
        if (
            calendar_registry.resolve_market_state(
                exchange_code,
                _runtime_datetime_from_utc_nanos(interval_start_utc_nanos),
            )
            == MarketState.OPEN
        ):
            gated_seasonality.append(seasonality_factor)
            continue

        gated_seasonality.append(0.0)
        contains_closed_interval = True

    if not contains_closed_interval:
        return hourly_factor_contract

    payload["s_h"] = gated_seasonality
    return load_hourly_factor_contract_payload(payload)


def _seasonality_parameters_payload(model: JaniformModel) -> Mapping[str, Any]:
    raw_seasonality = model.parameters.get("seasonality")
    if not isinstance(raw_seasonality, Mapping):
        raise RuntimeError("JaniformModel does not contain compact seasonality parameters")

    return raw_seasonality


def _contains_structural_seasonality_parameters(
    seasonality_parameters: Mapping[str, Any],
) -> bool:
    intraday_factor_by_bin = seasonality_parameters.get("intraday_factor_by_bin")

    day_of_week_factor = seasonality_parameters.get("day_of_week_factor")
    day_of_year_factor = seasonality_parameters.get("day_of_year_factor")

    has_structural_factor_sequences = (
        isinstance(intraday_factor_by_bin, tuple | list)
        and isinstance(day_of_week_factor, tuple | list)
        and isinstance(day_of_year_factor, tuple | list)
        and len(intraday_factor_by_bin) > 0
        and len(day_of_week_factor) > 0
        and len(day_of_year_factor) > 0
    )

    if not has_structural_factor_sequences:
        return False

    try:
        _seasonality_intraday_minutes_per_bin(seasonality_parameters)
    except RuntimeError:
        return False

    return True


def _hawkes_fitted_parameters_payload(model: JaniformModel) -> Mapping[str, Any]:
    joint_refinement_parameters = model.parameters.get("joint_refinement")
    if isinstance(joint_refinement_parameters, Mapping):
        fitted_parameters = joint_refinement_parameters.get("fitted_parameters")
        if isinstance(fitted_parameters, Mapping):
            refined_parameters = fitted_parameters.get("hawkes_fitted_parameters")
            if isinstance(refined_parameters, Mapping):
                return refined_parameters

    return _fitted_parameters_from_model_parameters(model, stage_kind="hawkes")


def _return_components_fitted_parameters_payload(model: JaniformModel) -> Mapping[str, Any]:
    joint_refinement_parameters = model.parameters.get("joint_refinement")
    if isinstance(joint_refinement_parameters, Mapping):
        fitted_parameters = joint_refinement_parameters.get("fitted_parameters")
        if isinstance(fitted_parameters, Mapping):
            refined_parameters = fitted_parameters.get("return_components_fitted_parameters")
            if isinstance(refined_parameters, Mapping):
                return refined_parameters

    return _fitted_parameters_from_model_parameters(model, stage_kind="return_components")


def _pricing_nig_shared_plus_shift_configuration_payload(
    model: JaniformModel,
) -> Mapping[str, Any]:
    joint_refinement_parameters = model.parameters.get("joint_refinement")
    if isinstance(joint_refinement_parameters, Mapping):
        fitted_parameters = joint_refinement_parameters.get("fitted_parameters")
        if isinstance(fitted_parameters, Mapping):
            pricing_block = fitted_parameters.get("pricing_block_fitted_parameters")
            if isinstance(pricing_block, Mapping):
                nig_configuration = pricing_block.get("nig_shared_plus_shift_configuration")
                if isinstance(nig_configuration, Mapping):
                    return nig_configuration

    return _require_payload_mapping(
        _return_components_fitted_parameters_payload(model),
        "nig_shared_plus_shift_configuration",
        "return_components.fitted_parameters.nig_shared_plus_shift_configuration",
    )


def _fitted_parameters_from_model_parameters(
    model: JaniformModel,
    *,
    stage_kind: str,
) -> Mapping[str, Any]:
    stage_parameters = model.parameters.get(stage_kind)
    if not isinstance(stage_parameters, Mapping):
        raise RuntimeError(f"JaniformModel does not contain `{stage_kind}` parameters")

    fitted_parameters = stage_parameters.get("fitted_parameters")
    if not isinstance(fitted_parameters, Mapping):
        raise RuntimeError(
            f"JaniformModel is missing fitted parameters for stage `{stage_kind}`"
        )

    return fitted_parameters


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


def _surprise_deterministic_time_multiplier_by_interval_index(
    model: JaniformModel,
    *,
    range_start_utc: datetime,
    range_end_utc: datetime,
    interval_duration_nanos: int,
) -> dict[int, float]:
    weekly_hour_utc_multipliers = _surprise_weekly_hour_utc_multipliers(model)

    normalized_range_start_utc = _normalize_runtime_utc_timestamp(
        range_start_utc,
        field_name="range_start_utc",
    )
    normalized_range_end_utc = _normalize_runtime_utc_timestamp(
        range_end_utc,
        field_name="range_end_utc",
    )
    if normalized_range_end_utc <= normalized_range_start_utc:
        return {}

    interval_duration = timedelta(seconds=interval_duration_nanos / 1_000_000_000)
    interval_count = int(
        (normalized_range_end_utc - normalized_range_start_utc).total_seconds()
        / interval_duration.total_seconds()
    )
    deterministic_time_multiplier_by_interval_index: dict[int, float] = {}

    for interval_index in range(interval_count):
        interval_start_utc = normalized_range_start_utc + interval_duration * interval_index
        interval_end_utc = min(interval_start_utc + interval_duration, normalized_range_end_utc)

        interval_multiplier = _average_weekly_hour_multiplier_over_interval(
            interval_start_utc=interval_start_utc,
            interval_end_utc=interval_end_utc,
            weekly_hour_utc_multipliers=weekly_hour_utc_multipliers,
        )
        if abs(interval_multiplier - 1.0) > 1.0e-12:
            deterministic_time_multiplier_by_interval_index[interval_index] = interval_multiplier

    return deterministic_time_multiplier_by_interval_index


def _surprise_weekly_hour_utc_multipliers(model: JaniformModel) -> tuple[float, ...]:
    fitted_parameters = _return_components_fitted_parameters_payload(model)

    profile = _require_payload_mapping(
        fitted_parameters,
        "surprise_deterministic_intensity_profile",
        "return_components.fitted_parameters.surprise_deterministic_intensity_profile",
    )

    raw_weekly_hour_utc_multipliers = profile.get("weekly_hour_utc_multipliers")
    if not isinstance(raw_weekly_hour_utc_multipliers, list):
        raise RuntimeError(
            "JaniformModel is missing weekly surprise intensity multipliers in the fitted model payload"
        )

    weekly_hour_utc_multipliers = tuple(
        float(raw_multiplier)
        if isinstance(raw_multiplier, int | float) and not isinstance(raw_multiplier, bool)
        else _raise_invalid_weekly_surprise_multiplier(bucket_index)
        for bucket_index, raw_multiplier in enumerate(raw_weekly_hour_utc_multipliers)
    )
    if len(weekly_hour_utc_multipliers) != 168:
        raise RuntimeError(
            "JaniformModel weekly surprise intensity profile must contain exactly 168 hourly multipliers"
        )

    return weekly_hour_utc_multipliers


def _average_weekly_hour_multiplier_over_interval(
    *,
    interval_start_utc: datetime,
    interval_end_utc: datetime,
    weekly_hour_utc_multipliers: tuple[float, ...],
) -> float:
    if interval_end_utc <= interval_start_utc:
        return 1.0

    weighted_multiplier_sum = 0.0
    weighted_duration_seconds = 0.0

    current = interval_start_utc
    while current < interval_end_utc:
        hour_start_utc = current.replace(minute=0, second=0, microsecond=0)

        next_hour_boundary_utc = (
            current + timedelta(hours=1)
            if current == hour_start_utc
            else hour_start_utc + timedelta(hours=1)
        )

        segment_end_utc = min(next_hour_boundary_utc, interval_end_utc)

        segment_duration_seconds = (segment_end_utc - current).total_seconds()

        weighted_multiplier_sum += (
            weekly_hour_utc_multipliers[_weekly_hour_bucket_index(current)]
            * segment_duration_seconds
        )

        weighted_duration_seconds += segment_duration_seconds

        current = segment_end_utc

    if weighted_duration_seconds <= 0.0:
        return 1.0

    return weighted_multiplier_sum / weighted_duration_seconds


def _weekly_hour_bucket_index(timestamp_utc: datetime) -> int:
    normalized_timestamp_utc = _normalize_runtime_utc_timestamp(
        timestamp_utc,
        field_name="timestamp_utc",
    )

    return normalized_timestamp_utc.weekday() * 24 + normalized_timestamp_utc.hour


def _raise_invalid_weekly_surprise_multiplier(bucket_index: int) -> NoReturn:
    raise RuntimeError(
        "JaniformModel payload field "
        "`return_components.fitted_parameters.surprise_deterministic_intensity_profile."
        f"weekly_hour_utc_multipliers[{bucket_index}]` must be numeric"
    )


def _normalize_runtime_utc_timestamp(timestamp_utc: datetime, *, field_name: str) -> datetime:
    if timestamp_utc.tzinfo is None:
        raise _calibration_error(field_name, "must be timezone-aware")

    return timestamp_utc.astimezone(UTC)


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
    }


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
                        "intraday_minutes_per_bin": normalized_stage_payload.get("intraday_minutes_per_bin"),
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
            case "mean_reversion":
                fitted_stage_payload = stage_report.get("fitted_stage_payload")
                if isinstance(fitted_stage_payload, Mapping):
                    parameters["mean_reversion"] = {
                        "schema_version": fitted_stage_payload.get("schema_version"),
                        "fitted_parameters": deepcopy(
                            cast(dict[str, Any], fitted_stage_payload.get("fitted_parameters", {}))
                        ),
                        "diagnostics": deepcopy(
                            cast(dict[str, Any], fitted_stage_payload.get("diagnostics", {}))
                        ),
                    }
            case "joint_refinement":
                fitted_stage_payload = stage_report.get("fitted_stage_payload")
                if isinstance(fitted_stage_payload, Mapping):
                    parameters["joint_refinement"] = {
                        "schema_version": fitted_stage_payload.get("schema_version"),
                        "selected_candidate_id": fitted_stage_payload.get("selected_candidate_id"),
                        "fitted_parameters": deepcopy(
                            cast(dict[str, Any], fitted_stage_payload.get("fitted_parameters", {}))
                        ),
                        "baseline_hawkes_fitted_parameters": deepcopy(
                            cast(
                                dict[str, Any],
                                fitted_stage_payload.get(
                                    "baseline_hawkes_fitted_parameters",
                                    {},
                                ),
                            )
                        ),
                        "baseline_return_components_fitted_parameters": deepcopy(
                            cast(
                                dict[str, Any],
                                fitted_stage_payload.get(
                                    "baseline_return_components_fitted_parameters",
                                    {},
                                ),
                            )
                        ),
                        "objective_breakdown": deepcopy(
                            cast(
                                dict[str, Any],
                                fitted_stage_payload.get("objective_breakdown", {}),
                            )
                        ),
                        "optimizer_diagnostics": deepcopy(
                            cast(
                                dict[str, Any],
                                fitted_stage_payload.get("optimizer_diagnostics", {}),
                            )
                        ),
                        "option_calibration_diagnostics": deepcopy(
                            cast(
                                dict[str, Any],
                                fitted_stage_payload.get(
                                    "option_calibration_diagnostics",
                                    {},
                                ),
                            )
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
    reference_price: Optional[float],
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

    calibration_scope = (
        "joint_option_calibrated"
        if prepared.diagnostics.workflow_scope
        == CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE
        else (
            "seasonality_only"
            if prepared.diagnostics.workflow_scope
            == CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY
            else "underlying_only"
        )
    )

    return {
        "run_id": staged_response.run_id,
        "workflow_scope": prepared.diagnostics.workflow_scope.value,
        "calibration_scope": calibration_scope,
        "execution_mode": staged_response.execution_mode.value,
        "selected_stages": [
            stage.value for stage in prepared.staged_request.selected_stages
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
            "provenance_kind": prepared.session_resolution.provenance_kind,
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
        "simulation_output_preferences": {
            "reference_price": reference_price,
        },
        "diagnostic_artifacts": {
            "artifact_bundle_available": staged_response.diagnostic_artifact_bundle is not None,
            "artifact_families": [
                reference.artifact_family
                for reference in staged_response.diagnostic_artifact_references
            ],
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


def _require_payload_numeric_value(
    payload: Mapping[str, Any],
    field_name: str,
    field_path: str,
) -> float:
    raw_value = payload.get(field_name)
    if not isinstance(raw_value, int | float) or isinstance(raw_value, bool):
        raise RuntimeError(f"JaniformModel payload field `{field_path}` must be numeric")

    return float(raw_value)


def _seasonality_intraday_minutes_per_bin(seasonality_parameters: Mapping[str, Any]) -> float:
    raw_value = seasonality_parameters.get("intraday_minutes_per_bin")
    if isinstance(raw_value, int | float) and not isinstance(raw_value, bool):
        return float(raw_value)

    intraday_factors = _require_payload_numeric_sequence(
        seasonality_parameters,
        "intraday_factor_by_bin",
        "seasonality.intraday_factor_by_bin",
    )
    inferred_minutes_per_bin, remainder = divmod(1440, len(intraday_factors))

    if remainder != 0:
        raise RuntimeError(
            "JaniformModel seasonality parameters must include intraday_minutes_per_bin when "
            "intraday_factor_by_bin does not evenly partition a day"
        )

    return float(inferred_minutes_per_bin)


def _require_payload_numeric_sequence(
    payload: Mapping[str, Any],
    field_name: str,
    field_path: str,
) -> tuple[float, ...]:
    raw_value = payload.get(field_name)
    if isinstance(raw_value, str) or not isinstance(raw_value, list | tuple):
        raise RuntimeError(f"JaniformModel payload field `{field_path}` must be a sequence")

    values: list[float] = []
    for entry_index, entry_value in enumerate(raw_value):
        if not isinstance(entry_value, int | float) or isinstance(entry_value, bool):
            raise RuntimeError(
                f"JaniformModel payload field `{field_path}[{entry_index}]` must be numeric"
            )

        values.append(float(entry_value))

    if not values:
        raise RuntimeError(f"JaniformModel payload field `{field_path}` must not be empty")

    return tuple(values)


def _normalize_model_parameters(raw_parameters: Mapping[str, Any]) -> dict[str, Any]:
    parameters = deepcopy(dict(raw_parameters))
    raw_seasonality = parameters.get("seasonality")
    if isinstance(raw_seasonality, dict):
        for field_name in (
            "intraday_factor_by_bin",
            "day_of_week_factor",
            "day_of_year_factor",
        ):
            raw_value = raw_seasonality.get(field_name)
            if isinstance(raw_value, list):
                raw_seasonality[field_name] = tuple(raw_value)

    _normalize_mean_reversion_model_parameters(parameters)

    return parameters


def _normalize_mean_reversion_model_parameters(parameters: dict[str, Any]) -> None:
    raw_mean_reversion = parameters.get("mean_reversion")
    if isinstance(raw_mean_reversion, Mapping):
        fitted_parameters = raw_mean_reversion.get("fitted_parameters")
        if isinstance(fitted_parameters, Mapping):
            raw_mean_reversion["fitted_parameters"] = MeanReversionFittedParameters.from_mapping(
                dict(fitted_parameters),
                field_path="parameters.mean_reversion.fitted_parameters",
            ).to_payload()

    raw_return_components = parameters.get("return_components")
    if isinstance(raw_return_components, Mapping):
        fitted_parameters = raw_return_components.get("fitted_parameters")
        if isinstance(fitted_parameters, Mapping):
            _normalize_return_components_mean_reversion_block(
                fitted_parameters,
                field_path="parameters.return_components.fitted_parameters",
            )

    raw_joint_refinement = parameters.get("joint_refinement")
    if not isinstance(raw_joint_refinement, Mapping):
        return

    raw_fitted_parameters = raw_joint_refinement.get("fitted_parameters")
    if isinstance(raw_fitted_parameters, Mapping):
        raw_return_components_fitted_parameters = raw_fitted_parameters.get(
            "return_components_fitted_parameters"
        )
        if isinstance(raw_return_components_fitted_parameters, Mapping):
            _normalize_return_components_mean_reversion_block(
                raw_return_components_fitted_parameters,
                field_path=(
                    "parameters.joint_refinement.fitted_parameters.return_components_fitted_parameters"
                ),
            )

    raw_baseline_return_components = raw_joint_refinement.get(
        "baseline_return_components_fitted_parameters"
    )
    if isinstance(raw_baseline_return_components, Mapping):
        _normalize_return_components_mean_reversion_block(
            raw_baseline_return_components,
            field_path="parameters.joint_refinement.baseline_return_components_fitted_parameters",
        )


def _normalize_return_components_mean_reversion_block(
    fitted_parameters: Mapping[str, Any],
    *,
    field_path: str,
) -> None:
    if not isinstance(fitted_parameters, dict):
        return

    raw_mean_reversion = fitted_parameters.get("mean_reversion_fitted_parameters")
    if not isinstance(raw_mean_reversion, Mapping):
        raise _calibration_error(
            f"{field_path}.mean_reversion_fitted_parameters",
            "must be an object",
        )

    fitted_parameters["mean_reversion_fitted_parameters"] = MeanReversionFittedParameters.from_mapping(
        dict(raw_mean_reversion),
        field_path=f"{field_path}.mean_reversion_fitted_parameters",
    ).to_payload()


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


def _normalize_interval_label(interval: str | timedelta) -> str:
    if isinstance(interval, str):
        return interval.replace("/", "_")

    total_seconds = int(interval.total_seconds())
    if total_seconds % 86_400 == 0:
        return f"{total_seconds // 86_400}D"
    if total_seconds % 3_600 == 0:
        return f"{total_seconds // 3_600}h"
    if total_seconds % 60 == 0:
        return f"{total_seconds // 60}min"

    return f"{total_seconds}s"


__all__ = ["JaniformModel", "JaniformSimulationResult"]
