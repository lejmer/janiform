from __future__ import annotations

import json
from collections.abc import Callable
from copy import deepcopy
from dataclasses import dataclass
from datetime import UTC, date, datetime, timedelta
from pathlib import Path
from typing import Any, Optional

from ..._workflow_outputs import SimulationArtifactSink
from ..._workflow_progress import WorkflowProgressMode
from ...contracts.base import require_non_empty_string
from ...mean_reversion import MeanReversionBoundaryState
from ...pricing import (
    PricingIntervalSchedulePolicy,
    VanillaOptionPayoffType,
    VanillaOptionPricingRunResult,
)
from ...pricing.progress import PricingProgressEvent
from ...simulation.progress import SimulationProgressEvent
from ...simulation.shared import HawkesExcitationState, SeededTickEventStateSnapshotCapture
from ..errors import calibration_error
from ..market_data.contracts import (
    CalibrationMarketDataWorkflowScope,
    CalibrationOptionSurfaceColumnMapping,
)
from ..presets.resolution import ResolvedPresetWorkflowConfiguration
from ..sessions.contracts import MarketDataSessionResolution
from ._runtime.metadata import (
    normalize_model_parameters,
    serialize_model_payload,
)
from ._runtime.option_pricing.workflow import (
    price_option_grid_for_model,
    price_option_for_model,
    price_option_grid_surface_for_model,
)
from ._runtime.simulation.result import (
    JaniformSimulationResult,
)
from ._runtime.simulation.workflow import (
    simulate_model,
)
from .pricing.grids import (
    JaniformVanillaOptionGridBatch,
    JaniformVanillaOptionGridBatchPricingResult,
)
from .pricing.market_snapshot._workflow.workflow import price_option_snapshot_for_model
from .pricing.market_snapshot.inputs import TabularFrameInput
from .pricing.market_snapshot.result import JaniformObservedOptionSnapshotPricingResult
from .pricing.market_surface._workflow.workflow import price_option_surface_for_model
from .pricing.market_surface.result import JaniformObservedOptionSurfacePricingResult
from .pricing.surface import (
    JaniformVanillaOptionSurface,
    JaniformVanillaOptionSurfacePricingResult,
)
from .state import CurrentStateEstimationRequest, CurrentStateEstimationResult
from .state._estimation import estimate_current_state_for_model

_DEFAULT_SIMULATION_SEED = 0


@dataclass(frozen=True, slots=True)
class JaniformModel:
    run_id: str
    workflow_scope: CalibrationMarketDataWorkflowScope
    parameters: dict[str, Any]
    metadata: dict[str, Any]
    resolved_configuration: ResolvedPresetWorkflowConfiguration
    session_resolution: MarketDataSessionResolution

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
        horizon_interval_count: int,
        seed: int = _DEFAULT_SIMULATION_SEED,
        start_time_utc: Optional[datetime] = None,
        interval: Optional[str | timedelta] = None,
        coarse: bool = False,
        output_path: Optional[str | Path] = None,
        sink: Optional[SimulationArtifactSink] = None,
        continuation_data: Optional[CurrentStateEstimationRequest] = None,
        initial_excitation_state: Optional[HawkesExcitationState] = None,
        initial_mean_reversion_boundary_state: Optional[MeanReversionBoundaryState] = None,
        debug_trace_enabled: bool = False,
        sample_every_nth_transition: int = 1,
        max_records_per_chunk: int = 1024,
        state_snapshot_capture: SeededTickEventStateSnapshotCapture = SeededTickEventStateSnapshotCapture.DISABLED,
        progress_callback: Optional[Callable[[SimulationProgressEvent], None]] = None,
        progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
    ) -> JaniformSimulationResult:
        resolved_start_time_utc = datetime.now(UTC) if start_time_utc is None else start_time_utc

        return simulate_model(
            self,
            seed=seed,
            start_time_utc=resolved_start_time_utc,
            horizon_interval_count=horizon_interval_count,
            interval=interval,
            coarse=coarse,
            output_path=output_path,
            sink=sink,
            continuation_data=continuation_data,
            initial_excitation_state=initial_excitation_state,
            initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
            debug_trace_enabled=debug_trace_enabled,
            sample_every_nth_transition=sample_every_nth_transition,
            max_records_per_chunk=max_records_per_chunk,
            state_snapshot_capture=state_snapshot_capture,
            progress_callback=progress_callback,
            progress_mode=progress_mode,
        )

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
        pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy] = None,
        output_path: Optional[str | Path] = None,
        silent: bool = False,
        progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
    ) -> VanillaOptionPricingRunResult:
        return price_option_for_model(
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
            pricing_interval_schedule_policy=pricing_interval_schedule_policy,
            output_path=output_path,
            silent=silent,
            progress_mode=progress_mode,
        )

    def price_option_grid(
        self,
        *,
        valuation_time_utc: datetime,
        batch: JaniformVanillaOptionGridBatch,
        observed_data: Optional[CurrentStateEstimationRequest] = None,
        boundary_state: Optional[HawkesExcitationState] = None,
        include_transform_diagnostics: bool = False,
        include_integration_quality_diagnostics: bool = False,
        pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy] = None,
        cpu_count: Optional[int] = None,
        silent: bool = False,
        progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
        progress_callback: Optional[Callable[[PricingProgressEvent], None]] = None,
    ) -> JaniformVanillaOptionGridBatchPricingResult:
        return price_option_grid_for_model(
            self,
            valuation_time_utc=valuation_time_utc,
            batch=batch,
            observed_data=observed_data,
            boundary_state=boundary_state,
            include_transform_diagnostics=include_transform_diagnostics,
            include_integration_quality_diagnostics=include_integration_quality_diagnostics,
            pricing_interval_schedule_policy=pricing_interval_schedule_policy,
            cpu_count=cpu_count,
            silent=silent,
            progress_mode=progress_mode,
            progress_callback=progress_callback,
        )

    def price_future(self, *args: Any, **kwargs: Any) -> Any:
        _ = args
        _ = kwargs
        raise NotImplementedError("JaniformModel.price_future is reserved but not implemented yet")

    def price_swap(self, *args: Any, **kwargs: Any) -> Any:
        _ = args
        _ = kwargs
        raise NotImplementedError("JaniformModel.price_swap is reserved but not implemented yet")

    def price_instruments(self, *args: Any, **kwargs: Any) -> Any:
        _ = args
        _ = kwargs
        raise NotImplementedError("JaniformModel.price_instruments is reserved but not implemented yet")

    def price_option_grid_surface(
        self,
        *,
        valuation_time_utc: datetime,
        surface: JaniformVanillaOptionSurface,
        observed_data: Optional[CurrentStateEstimationRequest] = None,
        boundary_state: Optional[HawkesExcitationState] = None,
        include_transform_diagnostics: bool = False,
        include_integration_quality_diagnostics: bool = False,
        pricing_interval_schedule_policy: Optional[PricingIntervalSchedulePolicy] = None,
        cpu_count: Optional[int] = None,
        silent: bool = False,
        progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
        progress_callback: Optional[Callable[[PricingProgressEvent], None]] = None,
    ) -> JaniformVanillaOptionSurfacePricingResult:
        return price_option_grid_surface_for_model(
            self,
            valuation_time_utc=valuation_time_utc,
            surface=surface,
            observed_data=observed_data,
            boundary_state=boundary_state,
            include_transform_diagnostics=include_transform_diagnostics,
            include_integration_quality_diagnostics=include_integration_quality_diagnostics,
            pricing_interval_schedule_policy=pricing_interval_schedule_policy,
            cpu_count=cpu_count,
            silent=silent,
            progress_mode=progress_mode,
            progress_callback=progress_callback,
        )

    def price_option_snapshot(
        self,
        *,
        tick_data: TabularFrameInput,
        option_surface: TabularFrameInput,
        valuation_time_utc: Optional[datetime] = None,
        option_type: Optional[VanillaOptionPayoffType | str] = None,
        option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping] = None,
        risk_free_rate: float = 0.0,
        dividend_yield: float = 0.0,
        cpu_count: Optional[int] = None,
        silent: bool = False,
        progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
    ) -> JaniformObservedOptionSnapshotPricingResult:
        return price_option_snapshot_for_model(
            self,
            tick_data=tick_data,
            option_surface=option_surface,
            valuation_time_utc=valuation_time_utc,
            option_type=option_type,
            option_surface_column_mapping=option_surface_column_mapping,
            risk_free_rate=risk_free_rate,
            dividend_yield=dividend_yield,
            cpu_count=cpu_count,
            silent=silent,
            progress_mode=progress_mode,
        )

    def price_option_surface(
        self,
        *,
        tick_data: TabularFrameInput,
        option_surface: TabularFrameInput,
        valuation_time_utc: Optional[datetime] = None,
        option_type: Optional[VanillaOptionPayoffType | str] = None,
        option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping] = None,
        risk_free_rate: float = 0.0,
        dividend_yield: float = 0.0,
        valuation_count: Optional[int] = None,
        use_all_valuation_times: bool = False,
        cpu_count: Optional[int] = None,
        silent: bool = False,
        progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
    ) -> JaniformObservedOptionSurfacePricingResult:
        return price_option_surface_for_model(
            self,
            tick_data=tick_data,
            option_surface=option_surface,
            valuation_time_utc=valuation_time_utc,
            option_type=option_type,
            option_surface_column_mapping=option_surface_column_mapping,
            risk_free_rate=risk_free_rate,
            dividend_yield=dividend_yield,
            valuation_count=valuation_count,
            use_all_valuation_times=use_all_valuation_times,
            cpu_count=cpu_count,
            silent=silent,
            progress_mode=progress_mode,
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
            serialize_model_payload(self),
            encoding="utf-8",
        )

    @staticmethod
    def load(path: str | Path) -> JaniformModel:
        model_path = Path(path)

        try:
            payload = json.loads(model_path.read_text(encoding="utf-8"))
        except FileNotFoundError as error:
            raise calibration_error(
                "path",
                f"persisted model path `{model_path}` does not exist",
            ) from error
        except OSError as error:
            raise calibration_error(
                "path",
                f"persisted model path `{model_path}` could not be read: {error}",
            ) from error
        except json.JSONDecodeError as error:
            raise calibration_error(
                "path",
                f"persisted model path `{model_path}` does not contain valid JSON: {error.msg}",
            ) from error

        if not isinstance(payload, dict):
            raise calibration_error("path", "persisted model payload must be a JSON object")

        from ._runtime.metadata import validate_model_persistence_schema_version

        validate_model_persistence_schema_version(payload.get("schema_version"))

        workflow_scope_value = payload.get("workflow_scope")
        if not isinstance(workflow_scope_value, str):
            raise calibration_error("workflow_scope", "must be a string")

        raw_parameters = payload.get("parameters")
        raw_metadata = payload.get("metadata")
        raw_resolved_configuration = payload.get("resolved_configuration")
        raw_session_resolution = payload.get("session_resolution")

        if not isinstance(raw_parameters, dict):
            raise calibration_error("parameters", "must be an object")
        if not isinstance(raw_metadata, dict):
            raise calibration_error("metadata", "must be an object")
        if not isinstance(raw_resolved_configuration, dict):
            raise calibration_error("resolved_configuration", "must be an object")
        if not isinstance(raw_session_resolution, dict):
            raise calibration_error("session_resolution", "must be an object")

        try:
            workflow_scope = CalibrationMarketDataWorkflowScope(workflow_scope_value)
        except ValueError as error:
            raise calibration_error(
                "workflow_scope",
                "must be a valid CalibrationMarketDataWorkflowScope value",
            ) from error

        return JaniformModel(
            run_id=require_non_empty_string(payload, "run_id", "run_id", calibration_error),
            workflow_scope=workflow_scope,
            parameters=normalize_model_parameters(raw_parameters),
            metadata=deepcopy(raw_metadata),
            resolved_configuration=ResolvedPresetWorkflowConfiguration.from_mapping(
                raw_resolved_configuration,
                field_path="resolved_configuration",
            ),
            session_resolution=MarketDataSessionResolution.from_mapping(
                raw_session_resolution,
                field_path="session_resolution",
            ),
        )


__all__ = ["JaniformModel", "JaniformSimulationResult"]
