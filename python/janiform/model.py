"""Public fitted-model representation and workflow methods."""

from __future__ import annotations

import json
from collections.abc import Callable, Mapping
from copy import deepcopy
from dataclasses import dataclass, field
from datetime import UTC, date, datetime, timedelta
from pathlib import Path
from typing import TYPE_CHECKING, Any, Optional

from ._workflow_outputs import SimulationArtifactSink
from ._workflow_progress import WorkflowProgressMode
from .contracts.base import require_non_empty_string
from .mean_reversion import MeanReversionBoundaryState
from .pricing import (
    PricingIntervalSchedulePolicy,
    VanillaOptionPayoffType,
    VanillaOptionPricingRunResult,
)
from .pricing.fitted import (
    JaniformObservedOptionSnapshotPricingResult,
    JaniformObservedOptionSurfacePricingResult,
    JaniformVanillaOptionGridBatch,
    JaniformVanillaOptionGridBatchPricingResult,
    JaniformVanillaOptionSurface,
    JaniformVanillaOptionSurfacePricingResult,
)
from .pricing.fitted.market_snapshot.inputs import TabularFrameInput
from .simulation import JaniformSimulationResult
from .simulation.shared import HawkesExcitationState, SeededTickEventStateSnapshotCapture

if TYPE_CHECKING:
    from .calibration.fit.state import CurrentStateEstimationRequest, CurrentStateEstimationResult
    from .calibration.market_data.contracts import (
        CalibrationMarketDataWorkflowScope,
        CalibrationOptionSurfaceColumnMapping,
    )
    from .calibration.presets.resolution import ResolvedPresetWorkflowConfiguration
    from .calibration.sessions.contracts import MarketDataSessionResolution

_DEFAULT_SIMULATION_SEED = 0


@dataclass(frozen=True, slots=True)
class JaniformModel:
    run_id: str
    workflow_scope: CalibrationMarketDataWorkflowScope
    parameters: dict[str, Any]
    metadata: dict[str, Any]
    resolved_configuration: ResolvedPresetWorkflowConfiguration
    session_resolution: MarketDataSessionResolution
    _model_payload: Optional[dict[str, Any]] = field(
        default=None,
        repr=False,
        compare=False,
    )

    @property
    def fitted_parameters(self) -> dict[str, Any]:
        return deepcopy(self.parameters)

    def summary(self) -> dict[str, Any]:
        summary = deepcopy(self.metadata)
        summary["fitted_parameters"] = deepcopy(self.parameters)

        return summary

    def _native_model_payload(self) -> dict[str, Any]:
        if self._model_payload is None:
            raise RuntimeError(
                "JaniformModel does not carry a Rust model payload. Refit through the "
                "native fit workflow or load a Rust-written model.jf artifact."
            )

        return deepcopy(self._model_payload)

    def save(self, path: Optional[str | Path] = None) -> Path:
        from ._native_api import native_module
        from .calibration.errors import calibration_execution_error

        raw_output = native_module().save_model_artifact(
            {
                "model_payload": self._native_model_payload(),
                "path": None if path is None else str(Path(path)),
            }
        )
        if not isinstance(raw_output, dict):
            raise calibration_execution_error(
                "native model artifact save returned an invalid output payload",
                entrypoint="model.save",
                context={"field": "output"},
            )

        raw_path = raw_output.get("path")
        if not isinstance(raw_path, str):
            raise calibration_execution_error(
                "native model artifact save returned an invalid path",
                entrypoint="model.save",
                context={"field": "output.path"},
            )

        return Path(raw_path)

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
        progress_callback: Optional[Callable[[Mapping[str, Any]], None]] = None,
        progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
    ) -> JaniformSimulationResult:
        from .calibration.fit._runtime.simulation.workflow import simulate_model

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
        from .calibration.fit._runtime.option_pricing.workflow import price_option_for_model

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
        progress_callback: Optional[Callable[[Mapping[str, Any]], None]] = None,
    ) -> JaniformVanillaOptionGridBatchPricingResult:
        from .calibration.fit._runtime.option_pricing.workflow import price_option_grid_for_model

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
        progress_callback: Optional[Callable[[Mapping[str, Any]], None]] = None,
    ) -> JaniformVanillaOptionSurfacePricingResult:
        from .calibration.fit._runtime.option_pricing.workflow import price_option_grid_surface_for_model

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
        output_path: Optional[str | Path] = None,
        silent: bool = False,
        progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
    ) -> JaniformObservedOptionSnapshotPricingResult:
        from .pricing.fitted.market_snapshot._workflow.workflow import price_option_snapshot_for_model

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
            output_path=output_path,
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
        output_path: Optional[str | Path] = None,
        silent: bool = False,
        progress_mode: WorkflowProgressMode | str = WorkflowProgressMode.AUTO,
    ) -> JaniformObservedOptionSurfacePricingResult:
        from .pricing.fitted.market_surface._workflow.workflow import price_option_surface_for_model

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
            output_path=output_path,
            silent=silent,
            progress_mode=progress_mode,
        )

    def estimate_current_state(
        self,
        *,
        state_time_utc: datetime,
        request: CurrentStateEstimationRequest,
    ) -> CurrentStateEstimationResult:
        from .calibration.fit.state._estimation import estimate_current_state_for_model

        return estimate_current_state_for_model(
            self,
            state_time_utc=state_time_utc,
            request=request,
        )

    @staticmethod
    def load(path: str | Path) -> JaniformModel:
        from .calibration.errors import calibration_error
        from .calibration.fit._runtime.metadata import (
            normalize_model_parameters,
            validate_model_persistence_schema_version,
        )
        from .calibration.market_data.contracts import CalibrationMarketDataWorkflowScope
        from .calibration.presets.resolution import ResolvedPresetWorkflowConfiguration
        from .calibration.sessions.contracts import MarketDataSessionResolution

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

        validate_model_persistence_schema_version(payload.get("schema_version"))

        workflow_scope_value = payload.get("workflow_scope")
        if not isinstance(workflow_scope_value, str):
            raise calibration_error("workflow_scope", "must be a string")

        raw_parameters = payload.get("parameters")
        raw_metadata = payload.get("metadata")
        raw_resolved_configuration = payload.get("resolved_configuration")
        raw_session_resolution = payload.get("session_resolution")
        report_preparation_payload = (
            _model_sibling_report_preparation_payload(model_path)
            if not isinstance(raw_resolved_configuration, dict)
            or not isinstance(raw_session_resolution, dict)
            else None
        )
        if not isinstance(raw_session_resolution, dict):
            raw_session_resolution = _required_preparation_payload_field(
                report_preparation_payload,
                "session_resolution",
            )
        if not isinstance(raw_resolved_configuration, dict) and isinstance(
            raw_session_resolution,
            dict,
        ):
            raw_resolved_configuration = _resolved_configuration_payload_from_session_resolution(
                raw_session_resolution
            )
        if not isinstance(raw_resolved_configuration, dict):
            raw_resolved_configuration = _required_preparation_payload_field(
                report_preparation_payload,
                "resolved_configuration",
            )

        if not isinstance(raw_parameters, dict):
            raise calibration_error("parameters", "must be an object")
        if not isinstance(raw_metadata, dict):
            raise calibration_error("metadata", "must be an object")
        raw_metadata = _model_metadata_with_recovered_input_inference(
            raw_metadata,
            report_preparation_payload,
        )
        if not isinstance(raw_resolved_configuration, dict):
            raise calibration_error("resolved_configuration", "must be an object")
        if not isinstance(raw_session_resolution, dict):
            raise calibration_error("session_resolution", "must be an object")

        model_payload = deepcopy(payload)
        model_payload["parameters"] = deepcopy(raw_parameters)
        model_payload["metadata"] = deepcopy(raw_metadata)
        model_payload["resolved_configuration"] = deepcopy(raw_resolved_configuration)
        model_payload["session_resolution"] = deepcopy(raw_session_resolution)

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
            _model_payload=model_payload,
        )


def _model_sibling_report_preparation_payload(model_path: Path) -> Optional[dict[str, Any]]:
    from .calibration.errors import calibration_error

    report_path = model_path.with_name("report.json")
    try:
        report_payload = json.loads(report_path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return None
    except OSError as error:
        raise calibration_error(
            "path",
            f"sibling fit report path `{report_path}` could not be read: {error}",
        ) from error
    except json.JSONDecodeError as error:
        raise calibration_error(
            "path",
            f"sibling fit report path `{report_path}` does not contain valid JSON: {error.msg}",
        ) from error

    if not isinstance(report_payload, dict):
        raise calibration_error("path", "sibling fit report payload must be a JSON object")

    raw_preparation_payload = report_payload.get("preparation")
    if not isinstance(raw_preparation_payload, dict):
        return None

    return raw_preparation_payload


def _required_preparation_payload_field(
    preparation_payload: Optional[dict[str, Any]],
    field_name: str,
) -> Any:
    from .calibration.errors import calibration_error

    if preparation_payload is None:
        raise calibration_error(field_name, "must be an object")

    raw_field_payload = preparation_payload.get(field_name)
    if not isinstance(raw_field_payload, dict):
        raise calibration_error(field_name, "must be an object")

    return raw_field_payload


def _resolved_configuration_payload_from_session_resolution(
    raw_session_resolution: dict[str, Any],
) -> dict[str, Any]:
    from .calibration.presets.resolution import resolve_preset_workflow_configuration
    from .calibration.sessions.contracts import MarketDataSessionResolution

    session_resolution = MarketDataSessionResolution.from_mapping(
        raw_session_resolution,
        field_path="session_resolution",
    )

    return resolve_preset_workflow_configuration(
        inferred_session_resolution=session_resolution,
    ).to_payload()


def _model_metadata_with_recovered_input_inference(
    raw_metadata: dict[str, Any],
    preparation_payload: Optional[dict[str, Any]],
) -> dict[str, Any]:
    if isinstance(raw_metadata.get("input_inference"), dict):
        return raw_metadata

    recovered_input_inference = _recovered_input_inference_payload(preparation_payload)
    if recovered_input_inference is None:
        return raw_metadata

    metadata = deepcopy(raw_metadata)
    metadata["input_inference"] = recovered_input_inference

    return metadata


def _recovered_input_inference_payload(
    preparation_payload: Optional[dict[str, Any]],
) -> Optional[dict[str, Any]]:
    if preparation_payload is None:
        return None

    raw_input_inference = preparation_payload.get("input_inference")
    if isinstance(raw_input_inference, dict):
        return raw_input_inference

    request = preparation_payload.get("request")
    if not isinstance(request, dict):
        return None

    tick_data_source = request.get("tick_data_source")
    column_mapping = request.get("column_mapping")
    normalization_policy = request.get("normalization_policy")
    if (
        not isinstance(tick_data_source, dict)
        or not isinstance(column_mapping, dict)
        or not isinstance(normalization_policy, dict)
    ):
        return None

    tick_data_column_mapping = column_mapping.get("tick_data")
    if not isinstance(tick_data_column_mapping, dict):
        return None

    recovered_input_inference = {
        "tick_data_source": {
            "file_format": tick_data_source.get("file_format", "parquet"),
        },
        "tick_data_column_mapping": tick_data_column_mapping,
        "timestamp_timezone_source": "explicit",
        "normalization_policy": normalization_policy,
    }
    if isinstance(request.get("run_id"), str):
        recovered_input_inference["run_id"] = request["run_id"]
    if isinstance(request.get("workflow_scope"), str):
        recovered_input_inference["workflow_scope"] = request["workflow_scope"]

    return recovered_input_inference


__all__ = ["JaniformModel"]
