from __future__ import annotations

import json
from copy import deepcopy
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any, Optional

from ..._workflow_progress import WorkflowProgressMode
from .pricing import (
    JaniformObservedOptionSnapshotPricingResult,
)
from .pricing.observed_snapshot import TabularFrameInput, _price_observed_option_snapshot
from .runtime.factors import (
    build_runtime_hourly_seasonality_source_from_model,
    current_state_tick_data_column_mapping_for_model,
    hawkes_kernel_parameters_for_model,
    mean_reversion_fitted_parameters_for_model,
    prepare_model_mean_reversion_runtime_inputs,
    prepare_model_runtime_factors_for_model,
    pricing_nig_shared_plus_shift_configuration_payload,
    resolved_known_news_event_timestamps,
    return_components_fitted_parameters_payload,
    seasonality_parameters_payload,
    surprise_baseline_intensity,
    surprise_deterministic_time_multiplier_by_interval_index,
)
from .runtime.metadata import (
    normalize_model_parameters,
    serialize_model_payload,
)
from .runtime.pricing import (
    build_model_pricing_hourly_factor_contract,
    model_reference_price,
    prepare_model_grouped_batch_pricing_native_input,
    prepare_model_pricing_execution_input,
    prepare_model_pricing_runtime_input,
    prepare_model_surface_pricing_native_input,
    price_observed_log_price_from_spot,
    price_option_for_model,
    price_option_groups_for_model,
    price_option_surface_for_model,
)
from .runtime.shared import (
    MODEL_PERSISTENCE_SCHEMA_VERSION as _MODEL_PERSISTENCE_SCHEMA_VERSION,
)
from .runtime.simulation import (
    JaniformSimulationResult,
    build_simulation_result,
    prepare_model_simulation_inputs,
    simulate_model,
)
from .state import CurrentStateEstimationRequest, CurrentStateEstimationResult, estimate_current_state_for_model
from ..errors import calibration_error
from ..market_data.contracts import CalibrationMarketDataWorkflowScope, CalibrationOptionSurfaceColumnMapping
from ..presets.resolution import ResolvedPresetWorkflowConfiguration
from ..sessions.contracts import MarketDataSessionResolution
from ...pricing import VanillaOptionPayoffType


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

    simulate = simulate_model
    price_option = price_option_for_model
    price_option_groups = price_option_groups_for_model
    price_option_surface = price_option_surface_for_model
    _build_simulation_result = build_simulation_result
    prepare_model_simulation_inputs = prepare_model_simulation_inputs
    prepare_model_mean_reversion_runtime_inputs = prepare_model_mean_reversion_runtime_inputs
    mean_reversion_fitted_parameters = mean_reversion_fitted_parameters_for_model
    _prepare_model_pricing_execution_input = prepare_model_pricing_execution_input
    _prepare_model_pricing_runtime_input = prepare_model_pricing_runtime_input
    _build_model_pricing_hourly_factor_contract = build_model_pricing_hourly_factor_contract
    pricing_observed_log_price_from_spot = price_observed_log_price_from_spot
    prepare_model_grouped_batch_pricing_native_input = prepare_model_grouped_batch_pricing_native_input
    prepare_model_surface_pricing_native_input = prepare_model_surface_pricing_native_input
    _model_reference_price = model_reference_price
    prepare_model_runtime_factors = prepare_model_runtime_factors_for_model
    hawkes_kernel_parameters = hawkes_kernel_parameters_for_model
    current_state_tick_data_column_mapping = current_state_tick_data_column_mapping_for_model
    build_runtime_hourly_seasonality_source_from_model = build_runtime_hourly_seasonality_source_from_model
    _seasonality_parameters_payload = seasonality_parameters_payload
    _return_components_fitted_parameters_payload = return_components_fitted_parameters_payload
    _pricing_nig_shared_plus_shift_configuration_payload = pricing_nig_shared_plus_shift_configuration_payload
    resolved_known_news_event_timestamps = resolved_known_news_event_timestamps
    surprise_baseline_intensity = surprise_baseline_intensity
    surprise_deterministic_time_multiplier_by_interval_index = surprise_deterministic_time_multiplier_by_interval_index

    def price_observed_option_snapshot(
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
        return _price_observed_option_snapshot(
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
