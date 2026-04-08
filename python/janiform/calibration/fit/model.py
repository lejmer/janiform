from __future__ import annotations

import json
from copy import deepcopy
from dataclasses import dataclass, field
from datetime import date, datetime, timedelta
from pathlib import Path
from typing import Any, Optional

from ._runtime.metadata import (
    normalize_model_parameters,
    serialize_model_payload,
)
from ._runtime.shared import (
    MODEL_PERSISTENCE_SCHEMA_VERSION as _MODEL_PERSISTENCE_SCHEMA_VERSION,
)
from ._runtime.option_pricing.workflow import (
    price_option_grids_for_model,
    price_option_for_model,
    price_option_surface_for_model,
)
from ._runtime.simulation.result import (
    JaniformSimulationResult,
)
from ._runtime.simulation.workflow import (
    simulate_model,
)
from .pricing import (
    JaniformGroupedVanillaOptionBatch,
    JaniformObservedOptionSnapshotPricingResult,
    JaniformVanillaOptionGroupedBatchPricingResult,
    JaniformVanillaOptionSurface,
    JaniformVanillaOptionSurfacePricingResult,
)
from .pricing.market_snapshot.loading import TabularFrameInput
from .pricing.market_snapshot.workflow import _price_option_market_snapshot
from .state import CurrentStateEstimationRequest, CurrentStateEstimationResult
from .state._estimation import estimate_current_state_for_model
from ..errors import calibration_error
from ..market_data.contracts import CalibrationMarketDataWorkflowScope, CalibrationOptionSurfaceColumnMapping
from ..presets.resolution import ResolvedPresetWorkflowConfiguration
from ..sessions.contracts import MarketDataSessionResolution
from ..._workflow_progress import WorkflowProgressMode
from ...mean_reversion import MeanReversionBoundaryState
from ...pricing import VanillaOptionPayoffType, VanillaOptionPricingRunResult
from ...simulation.shared import HawkesExcitationState, SeededTickEventStateSnapshotCapture


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
        sink: Optional[Any] = None,
        continuation_data: Optional[CurrentStateEstimationRequest] = None,
        initial_excitation_state: Optional[HawkesExcitationState] = None,
        initial_mean_reversion_boundary_state: Optional[MeanReversionBoundaryState] = None,
        debug_trace_enabled: bool = False,
        sample_every_nth_transition: int = 1,
        max_records_per_chunk: int = 1024,
        state_snapshot_capture: SeededTickEventStateSnapshotCapture = SeededTickEventStateSnapshotCapture.DISABLED,
        silent: bool = False,
        progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
        progress_callback: Optional[Any] = None,
    ) -> JaniformSimulationResult:
        return simulate_model(
            self,
            seed=seed,
            start_time_utc=start_time_utc,
            horizon_interval_count=horizon_interval_count,
            interval=interval,
            output_path=output_path,
            sink=sink,
            continuation_data=continuation_data,
            initial_excitation_state=initial_excitation_state,
            initial_mean_reversion_boundary_state=initial_mean_reversion_boundary_state,
            debug_trace_enabled=debug_trace_enabled,
            sample_every_nth_transition=sample_every_nth_transition,
            max_records_per_chunk=max_records_per_chunk,
            state_snapshot_capture=state_snapshot_capture,
            silent=silent,
            progress_mode=progress_mode,
            progress_callback=progress_callback,
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
            output_path=output_path,
            silent=silent,
            progress_mode=progress_mode,
        )

    def price_option_grids(
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
        return price_option_grids_for_model(
            self,
            valuation_time_utc=valuation_time_utc,
            batch=batch,
            observed_data=observed_data,
            boundary_state=boundary_state,
            include_transform_diagnostics=include_transform_diagnostics,
            include_integration_quality_diagnostics=include_integration_quality_diagnostics,
            cpu_count=cpu_count,
            silent=silent,
            progress_mode=progress_mode,
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
        return price_option_surface_for_model(
            self,
            valuation_time_utc=valuation_time_utc,
            surface=surface,
            observed_data=observed_data,
            boundary_state=boundary_state,
            include_transform_diagnostics=include_transform_diagnostics,
            include_integration_quality_diagnostics=include_integration_quality_diagnostics,
            cpu_count=cpu_count,
            silent=silent,
            progress_mode=progress_mode,
        )

    def price_option_market_snapshot(
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
        return _price_option_market_snapshot(
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

        schema_version = payload.get("schema_version")
        if schema_version != _MODEL_PERSISTENCE_SCHEMA_VERSION:
            raise calibration_error(
                "schema_version",
                f"must be `{_MODEL_PERSISTENCE_SCHEMA_VERSION}`",
            )

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

        from ...contracts import require_non_empty_string

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
            _stage_report_payloads=(),
        )


__all__ = ["JaniformModel", "JaniformSimulationResult"]
