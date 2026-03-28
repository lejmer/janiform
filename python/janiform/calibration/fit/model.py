from __future__ import annotations

import json
from bisect import bisect_right
from collections.abc import Mapping
from copy import deepcopy
from dataclasses import asdict, dataclass, field
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any, NoReturn, Optional, cast

from ..market_data.contracts import CalibrationMarketDataWorkflowScope
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
from ..._timestamps import datetime_to_utc_nanos
from ..._workflow_outputs import (
    SimulationArtifactSink,
    generate_workflow_run_id,
    persist_coarse_simulation,
    persist_seeded_tick_simulation,
    simulation_output_preferences_from_model,
    write_pricing_artifacts,
    write_simulation_artifacts,
)
from ..._workflow_progress import JaniformOutputController
from ...contracts import (
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
    require_non_empty_string,
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
    _parse_interval_duration_nanos,
)
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
from .._errors import _calibration_error
from ...seasonality import SessionFilter, preprocess_timestamp_events
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


_HIGH_LEVEL_MODEL_PERSISTENCE_SCHEMA_VERSION = "calibration.high_level.model.v1"
_DEFAULT_HIGH_LEVEL_SIMULATION_INTERVAL = "1h"
_MEAN_REVERSION_RUNTIME_INTERVAL_ALIGNMENT_POLICY = "explicit_runtime_interval_grid_alignment.v1"
_MEAN_REVERSION_DETERMINISTIC_SOURCE_ALIGNMENT_POLICY = "piecewise_constant_utc_interval_grid.v1"


@dataclass(frozen=True, slots=True)
class _PreparedModelSimulationInputs:
    coarse_request: Optional[CoarseSimulationRunRequest]
    seeded_request: Optional[SeededTickSimulationRequest]


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

        timestamp_column = self._preferences.timestamp_column
        match self._preferences.tick_price_input_policy:
            case "explicit_price_column":
                column_names = [timestamp_column, self._preferences.price_column or "price"]
                if self._preferences.trade_sign_column is not None:
                    column_names.append(self._preferences.trade_sign_column)
                return tuple(column_names)
            case "bid_ask_midpoint":
                return (
                    timestamp_column,
                    "mid_price",
                )
            case _:
                return (
                    timestamp_column,
                    self._preferences.price_column or "price",
                )


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
        initial_excitation_state: Optional[HawkesExcitationState] = None,
        debug_trace_enabled: bool = False,
        sample_every_nth_transition: int = 1,
        max_records_per_chunk: int = 1024,
        state_snapshot_capture: SeededTickEventStateSnapshotCapture = SeededTickEventStateSnapshotCapture.DISABLED,
        silent: bool = False,
    ) -> JaniformSimulationResult:
        if output_path is not None and sink is not None:
            raise _calibration_error(
                "sink",
                "output_path and sink are mutually exclusive",
            )

        effective_interval = _DEFAULT_HIGH_LEVEL_SIMULATION_INTERVAL if interval is None else interval
        output_controller = JaniformOutputController(silent=silent)
        simulation_kind = "seeded_tick" if interval is None else "coarse_interval"

        execution_inputs = _prepare_model_simulation_inputs(
            self,
            seed=seed,
            start_time_utc=start_time_utc,
            horizon_interval_count=horizon_interval_count,
            interval=effective_interval,
            simulation_kind=simulation_kind,
            initial_excitation_state=initial_excitation_state,
            debug_trace_enabled=debug_trace_enabled,
            sample_every_nth_transition=sample_every_nth_transition,
            max_records_per_chunk=max_records_per_chunk,
            state_snapshot_capture=state_snapshot_capture,
        )
        with output_controller.progress("simulate") as progress:
            if simulation_kind == "seeded_tick":
                progress.update(detail="simulating seeded ticks")

                if execution_inputs.seeded_request is None:
                    raise RuntimeError("JaniformModel seeded simulation inputs were not prepared")

                raw_simulation_result = run_seeded_tick_event_simulation(
                    execution_inputs.seeded_request
                )
            else:
                progress.update(detail="simulating coarse intervals")

                if execution_inputs.coarse_request is None:
                    raise RuntimeError("JaniformModel coarse simulation inputs were not prepared")

                raw_simulation_result = run_coarse_simulation(
                    execution_inputs.coarse_request
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
        maturity_interval: float,
        payoff_type: VanillaOptionPayoffType | str = VanillaOptionPayoffType.CALL,
        risk_free_rate: float = 0.0,
        dividend_yield: float = 0.0,
        boundary_state: Optional[HawkesExcitationState] = None,
        observed_boundary_state: Optional[MeanReversionBoundaryState] = None,
        include_transform_diagnostics: bool = False,
        include_integration_quality_diagnostics: bool = False,
        include_calibration_hooks: bool = False,
        output_path: Optional[str | Path] = None,
        silent: bool = False,
    ) -> VanillaOptionPricingRunResult:
        output_controller = JaniformOutputController(silent=silent)
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
            observed_boundary_state=observed_boundary_state,
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

    coarse_request: Optional[CoarseSimulationRunRequest] = None
    seeded_request: Optional[SeededTickSimulationRequest] = None

    match simulation_kind:
        case "coarse_interval":
            (
                mean_reversion_fitted_parameters,
                mean_reversion_runtime_interval_anchors,
                initial_mean_reversion_boundary_state,
            ) = _prepare_model_mean_reversion_runtime_inputs(
                model,
                start_time_utc=start_time_utc,
                interval_end_times_utc=_build_runtime_interval_end_times_utc(
                    start_time_utc=start_time_utc,
                    interval_duration_nanos=runtime_factors.interval_duration_nanos,
                    horizon_interval_count=horizon_interval_count,
                ),
                interval_deltas=tuple(
                    runtime_factors.interval_delta for _ in range(horizon_interval_count)
                ),
                interval_alignment_policy=_MEAN_REVERSION_RUNTIME_INTERVAL_ALIGNMENT_POLICY,
            )
            hourly_factor_contract = provide_hourly_factor_contract(
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
                            interval_duration_nanos=_parse_interval_duration_nanos(interval),
                        )
                    ),
                ),
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
                    mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
                    mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
                    initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
                ),
                hourly_factor_contract=_zero_closed_coarse_interval_seasonality_factors(
                    hourly_factor_contract,
                    calendar_registry=runtime_factors.calendar_registry,
                    exchange_code=runtime_factors.exchange_code,
                ),
                news_process_configuration=runtime_factors.news_process_configuration,
                calendar_registry=runtime_factors.calendar_registry,
                exchange_code=runtime_factors.exchange_code,
            )
        case "seeded_tick":
            hourly_factor_contract = provide_hourly_factor_contract(
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
                            interval_duration_nanos=_parse_interval_duration_nanos(interval),
                        )
                    ),
                ),
            )

            (
                mean_reversion_fitted_parameters,
                mean_reversion_runtime_interval_anchors,
                initial_mean_reversion_boundary_state,
            ) = _prepare_model_mean_reversion_runtime_inputs(
                model,
                start_time_utc=start_time_utc,
                interval_end_times_utc=tuple(
                    _runtime_datetime_from_utc_nanos(timestamp_utc_nanos)
                    for timestamp_utc_nanos in hourly_factor_contract.interval_end_utc_nanos
                ),
                interval_deltas=tuple(
                    hourly_factor_contract.interval_duration_nanos / 3_600_000_000_000.0
                    for _ in range(hourly_factor_contract.interval_count)
                ),
                interval_alignment_policy=hourly_factor_contract.interval_alignment_policy,
            )

            seeded_request = SeededTickSimulationRequest.from_runtime_inputs(
                seed=seed,
                start_time_utc=start_time_utc,
                horizon_interval_count=horizon_interval_count,
                kernel_parameters=runtime_factors.kernel_parameters,
                nig_shared_plus_shift_configuration=runtime_factors.nig_shared_plus_shift_configuration,
                news_process_configuration=runtime_factors.news_process_configuration,
                initial_excitation_state=effective_initial_state,
                mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
                mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
                initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
                hourly_factor_contract=hourly_factor_contract,
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


def _prepare_model_mean_reversion_runtime_inputs(
    model: JaniformModel,
    *,
    start_time_utc: datetime,
    interval_end_times_utc: tuple[datetime, ...],
    interval_deltas: tuple[float, ...],
    interval_alignment_policy: str,
) -> tuple[
    MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
    MeanReversionBoundaryState,
]:
    fitted_parameters = _mean_reversion_fitted_parameters(model)
    runtime_interval_anchors = _materialize_model_mean_reversion_runtime_interval_anchors(
        fitted_parameters=fitted_parameters,
        start_time_utc=start_time_utc,
        interval_end_times_utc=interval_end_times_utc,
        interval_deltas=interval_deltas,
        interval_alignment_policy=interval_alignment_policy,
    )
    initial_boundary_state = MeanReversionBoundaryState(
        observed_log_price=0.0,
        anchor_value=runtime_interval_anchors.start_anchor_value,
        deviation=-runtime_interval_anchors.start_anchor_value,
    )

    return fitted_parameters, runtime_interval_anchors, initial_boundary_state


def _materialize_model_mean_reversion_runtime_interval_anchors(
    *,
    fitted_parameters: MeanReversionFittedParameters,
    start_time_utc: datetime,
    interval_end_times_utc: tuple[datetime, ...],
    interval_deltas: tuple[float, ...],
    interval_alignment_policy: str,
) -> MeanReversionRuntimeIntervalAnchors:
    if len(interval_end_times_utc) != len(interval_deltas):
        raise RuntimeError(
            "JaniformModel mean reversion runtime anchor materialization requires one interval "
            "end timestamp per interval delta"
        )

    return MeanReversionRuntimeIntervalAnchors(
        interval_alignment_policy=interval_alignment_policy,
        start_anchor_value=_materialize_mean_reversion_anchor_value(
            fitted_parameters=fitted_parameters,
            boundary_time_utc=start_time_utc,
        ),
        end_anchor_values=tuple(
            _materialize_mean_reversion_anchor_value(
                fitted_parameters=fitted_parameters,
                boundary_time_utc=boundary_time_utc,
            )
            for boundary_time_utc in interval_end_times_utc
        ),
        interval_deltas=interval_deltas,
    )


def _materialize_mean_reversion_anchor_value(
    *,
    fitted_parameters: MeanReversionFittedParameters,
    boundary_time_utc: datetime,
) -> float:
    normalized_boundary_time_utc = _normalize_runtime_utc_timestamp(
        boundary_time_utc,
        field_name="boundary_time_utc",
    )
    anchor_model = fitted_parameters.anchor_model

    match anchor_model.anchor_model_kind:
        case "endogenous_core_expectation_one_interval":
            raise RuntimeError(
                "JaniformModel endogenous mean reversion anchor materialization requires "
                "runtime state recursion and is not available from timestamp-only projection"
            )
        case "disabled":
            return 0.0
        case "constant":
            anchor_value = anchor_model.anchor_value
            if anchor_value is None:
                raise RuntimeError(
                    "JaniformModel mean reversion constant anchor is missing anchor_value"
                )

            return anchor_value
        case "deterministic_interval_grid":
            source_alignment_policy = anchor_model.source_alignment_policy
            if source_alignment_policy != _MEAN_REVERSION_DETERMINISTIC_SOURCE_ALIGNMENT_POLICY:
                raise RuntimeError(
                    "JaniformModel deterministic mean reversion anchor model must use "
                    f"`{_MEAN_REVERSION_DETERMINISTIC_SOURCE_ALIGNMENT_POLICY}`"
                )

            return _materialize_deterministic_interval_grid_anchor_value(
                fitted_parameters=fitted_parameters,
                boundary_time_utc=normalized_boundary_time_utc,
            )
        case _:
            raise RuntimeError("JaniformModel mean reversion anchor model kind is unsupported")


def _materialize_deterministic_interval_grid_anchor_value(
    *,
    fitted_parameters: MeanReversionFittedParameters,
    boundary_time_utc: datetime,
) -> float:
    anchor_model = fitted_parameters.anchor_model
    boundary_time_utc_nanos = datetime_to_utc_nanos(boundary_time_utc)
    source_interval_start_utc_nanos = anchor_model.source_interval_start_utc_nanos
    source_interval_end_utc_nanos = anchor_model.source_interval_end_utc_nanos

    interval_index = bisect_right(source_interval_start_utc_nanos, boundary_time_utc_nanos) - 1
    if interval_index >= 0 and boundary_time_utc_nanos < source_interval_end_utc_nanos[interval_index]:
        return anchor_model.anchor_values[interval_index]

    if (
        source_interval_end_utc_nanos
        and interval_index == len(source_interval_end_utc_nanos) - 1
        and boundary_time_utc_nanos == source_interval_end_utc_nanos[interval_index]
    ):
        return anchor_model.anchor_values[interval_index]

    raise RuntimeError(
        "JaniformModel deterministic mean reversion anchor grid does not cover runtime boundary "
        f"{boundary_time_utc.isoformat()}"
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
    maturity_interval: float,
    payoff_type: VanillaOptionPayoffType | str,
    risk_free_rate: float,
    dividend_yield: float,
    boundary_state: Optional[HawkesExcitationState],
    observed_boundary_state: Optional[MeanReversionBoundaryState],
    include_transform_diagnostics: bool,
    include_integration_quality_diagnostics: bool,
    include_calibration_hooks: bool,
) -> VanillaOptionPricingExecutionInput:
    horizon_interval_count_float = maturity_interval / 1.0
    horizon_interval_count = int(horizon_interval_count_float)
    if (
        horizon_interval_count <= 0
        or float(horizon_interval_count) != horizon_interval_count_float
    ):
        raise _calibration_error(
            "maturity_interval",
            "must be a positive exact multiple of the pricing runtime interval 1.0",
        )

    runtime_factors = _prepare_model_runtime_factors(
        model,
        start_time_utc=valuation_time_utc,
        horizon_interval_count=horizon_interval_count,
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

    (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = _prepare_model_mean_reversion_runtime_inputs(
        model,
        start_time_utc=valuation_time_utc,
        interval_end_times_utc=_build_runtime_interval_end_times_utc(
            start_time_utc=valuation_time_utc,
            interval_duration_nanos=runtime_factors.interval_duration_nanos,
            horizon_interval_count=horizon_interval_count,
        ),
        interval_deltas=tuple(
            runtime_factors.interval_delta for _ in range(horizon_interval_count)
        ),
        interval_alignment_policy=_MEAN_REVERSION_RUNTIME_INTERVAL_ALIGNMENT_POLICY,
    )
    effective_observed_boundary_state = _resolve_model_pricing_observed_boundary_state(
        mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
        runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
        observed_boundary_state=observed_boundary_state,
        default_observed_boundary_state=initial_mean_reversion_boundary_state,
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
            boundary_input=AnchoredVanillaOptionPricingBoundaryInput(
                interval_index=0,
                hawkes_boundary_state=effective_boundary_state,
                mean_reversion_fitted_parameters=mean_reversion_fitted_parameters,
                mean_reversion_runtime_interval_anchors=mean_reversion_runtime_interval_anchors,
                observed_boundary_state=effective_observed_boundary_state,
            ),
            model_parameters=VanillaOptionPricingModelParameters(
                hawkes_kernel_parameters=runtime_factors.kernel_parameters,
                gamma_severity_mark_configuration=runtime_factors.gamma_severity_mark_configuration,
                nig_shared_plus_shift_configuration=runtime_factors.nig_shared_plus_shift_configuration,
            ),
        ),
        hourly_factor_contract=provide_hourly_factor_contract(
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
        ),
        optional_output_selection=VanillaOptionPricingOptionalOutputSelection(
            include_transform_diagnostics=include_transform_diagnostics,
            include_integration_quality_diagnostics=include_integration_quality_diagnostics,
            include_calibration_hooks=include_calibration_hooks,
        ),
    )


def _resolve_model_pricing_observed_boundary_state(
    *,
    mean_reversion_fitted_parameters: MeanReversionFittedParameters,
    runtime_interval_anchors: MeanReversionRuntimeIntervalAnchors,
    observed_boundary_state: Optional[MeanReversionBoundaryState],
    default_observed_boundary_state: MeanReversionBoundaryState,
) -> MeanReversionBoundaryState:
    if observed_boundary_state is not None:
        return observed_boundary_state

    match mean_reversion_fitted_parameters.anchor_model.anchor_model_kind:
        case "disabled":
            return default_observed_boundary_state
        case _:
            raise _calibration_error(
                "observed_boundary_state",
                (
                    "is required when model mean reversion is enabled for pricing. "
                    "Pass an explicit MeanReversionBoundaryState aligned with the valuation boundary anchor "
                    f"{runtime_interval_anchors.start_anchor_value}."
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
        interval_duration_nanos=interval_duration_nanos,
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
    interval_duration = timedelta(seconds=interval_duration_nanos / 1_000_000_000)
    timestamp_event_records = _build_runtime_timestamp_event_records(
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        interval_duration=interval_duration,
    )

    if not timestamp_event_records:
        return HourlySeasonalityProviderExplicitSource(
            hourly_factors=(),
            materialization_method="runtime_compact_seasonality_parameters_mean_renormalized.v1",
        )

    preprocessed_events = preprocess_timestamp_events(
        timestamp_event_records,
        exchange_code=exchange_code,
        calendar_registry=calendar_registry,
        timestamp_field_name="timestamp",
        session_filter=SessionFilter.ALL,
        sparse_hour_event_threshold=1,
    )

    hourly_factors: list[float] = []
    for minute_of_day, day_of_week, day_of_year in zip(
        preprocessed_events.exchange_local_minute_of_day,
        preprocessed_events.exchange_local_day_of_week,
        preprocessed_events.exchange_local_day_of_year,
        strict=True,
    ):
        intraday_index = min(
            minute_of_day // intraday_minutes_per_bin,
            len(intraday_factors) - 1,
        )
        day_of_week_index = day_of_week - 1
        day_of_year_index = day_of_year - 1
        hourly_factors.append(
            intraday_factors[intraday_index]
            * day_of_week_factors[day_of_week_index]
            * day_of_year_factors[day_of_year_index]
        )

    mean_renormalized_factors = _mean_renormalize_factors(tuple(hourly_factors))

    if interval_duration_nanos < 3_600_000_000_000:
        return HourlySeasonalityProviderMaterializedSource(
            source_timestamps_utc=tuple(
                record["timestamp"]
                for record in timestamp_event_records
            ),
            source_seasonality_factors=mean_renormalized_factors,
            interpolation_method="previous_observation_carry_forward.v1",
            aggregation_method="mean_of_observations.v1",
            sparse_fallback_policy="global_mean.v1",
            sparse_hour_minimum_observation_count=1,
        )

    return HourlySeasonalityProviderExplicitSource(
        hourly_factors=mean_renormalized_factors,
        materialization_method="runtime_compact_seasonality_parameters_mean_renormalized.v1",
    )


def _build_runtime_timestamp_event_records(
    *,
    range_start_utc: datetime,
    range_end_utc: datetime,
    interval_duration: timedelta,
) -> list[dict[str, datetime]]:
    event_records: list[dict[str, datetime]] = []
    current_timestamp = range_start_utc

    while current_timestamp < range_end_utc:
        event_records.append({"timestamp": current_timestamp})
        current_timestamp += interval_duration

    return event_records


def _zero_closed_coarse_interval_seasonality_factors(
    hourly_factor_contract: HourlyFactorContract,
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
) -> HourlyFactorContract:
    if hourly_factor_contract.interval_duration_nanos >= 3_600_000_000_000:
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

    return {
        "run_id": staged_response.run_id,
        "workflow_scope": prepared.diagnostics.workflow_scope.value,
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


def _mean_renormalize_factors(factors: tuple[float, ...]) -> tuple[float, ...]:
    if not factors:
        raise RuntimeError("JaniformModel seasonality factors must not be empty")

    mean_factor = sum(factors) / len(factors)
    if mean_factor == 0.0:
        raise RuntimeError("JaniformModel seasonality factors must not average to zero")

    return tuple(factor / mean_factor for factor in factors)


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
