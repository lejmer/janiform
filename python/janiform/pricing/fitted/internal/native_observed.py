"""Native observed-options pricing boundary helpers."""

from __future__ import annotations

from collections.abc import Mapping
from contextlib import ExitStack
from dataclasses import dataclass, replace
from datetime import UTC, datetime
from math import isfinite
from pathlib import Path
from tempfile import TemporaryDirectory
from types import ModuleType
from typing import TYPE_CHECKING, Any, Optional

import pandas

from ...._native_api import native_exception_matches, native_module
from ...._native_progress import native_terminal_progress_mode
from ...._workflow_progress import WorkflowProgressMode, resolve_workflow_progress_mode
from ....calibration.fit._runtime.payloads import current_state_tick_data_column_mapping_for_model
from ....calibration.fit.state import CurrentStateEstimationRequest
from ....calibration.market_data.contracts import (
    CalibrationInputFileFormat,
    CalibrationOptionSurfaceColumnMapping,
)
from ....calibration.market_data.price_coordinates import (
    DEFAULT_OPTION_FORWARD_PARITY_WARNING_RELATIVE_THRESHOLD,
    OptionForwardParityWarningPolicy,
    PriceCoordinatePolicy,
    coerce_price_coordinate_policy,
)
from ....pricing import VanillaOptionPayoffType
from ....pricing.errors import (
    PRICING_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    PRICING_INTEGRATION_RESPONSE_ERROR_ID,
    _pricing_error,
    _pricing_execution_error,
    _pricing_integration_error,
)
from .price_coordinates import resolve_price_coordinate_from_native_mapping
from ..market_snapshot.inputs import TabularFrameInput
from ..market_snapshot.result import JaniformObservedOptionSnapshotPricingResult
from ..market_surface.result import JaniformObservedOptionSurfacePricingResult
from ..physical_distribution import (
    ObservedOptionsPhysicalDistributionDiagnosticsRequest,
    ObservedOptionsPhysicalDistributionDiagnosticsOutput,
    physical_distribution_diagnostics_request_payload,
)

if TYPE_CHECKING:
    from ....model import JaniformModel


@dataclass(frozen=True, slots=True)
class NativeObservedOptionsRequestSources:
    tick_data_source: dict[str, Any]
    option_surface_source: dict[str, Any]


def run_native_observed_options_pricing(
    model: JaniformModel,
    *,
    tick_data: TabularFrameInput,
    option_surface: TabularFrameInput,
    valuation_time_utc: Optional[datetime],
    option_type: Optional[VanillaOptionPayoffType | str],
    option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping],
    risk_free_rate: float,
    dividend_yield: float,
    valuation_count: Optional[int],
    use_all_valuation_times: bool,
    cpu_count: Optional[int],
    price_coordinate_policy: PriceCoordinatePolicy | str,
    option_forward_parity_warning_threshold: Optional[float],
    physical_distribution_diagnostics: (
        bool | ObservedOptionsPhysicalDistributionDiagnosticsRequest
    ),
    output_path: Optional[str | Path],
    silent: bool,
    progress_mode: WorkflowProgressMode | str,
    scope: str,
) -> dict[str, Any]:
    if valuation_time_utc is not None and valuation_time_utc.tzinfo is None:
        raise _pricing_error("valuation_time_utc", "must be timezone-aware when provided")
    if valuation_count is not None and valuation_count <= 0:
        raise _pricing_error("valuation_count", "must be strictly positive when provided")
    if cpu_count is not None and cpu_count <= 0:
        raise _pricing_error("cpu_count", "must be strictly positive when provided")
    if use_all_valuation_times and valuation_count is not None:
        raise _pricing_error("valuation_count", "cannot be combined with use_all_valuation_times")

    if (
        option_forward_parity_warning_threshold is not None
        and (
            not isfinite(option_forward_parity_warning_threshold)
            or option_forward_parity_warning_threshold <= 0.0
        )
    ):
        raise _pricing_error(
            "option_forward_parity_warning_threshold",
            "must be strictly positive when provided",
        )

    tick_data_column_mapping = current_state_tick_data_column_mapping_for_model(
        model,
        request=CurrentStateEstimationRequest(observed_tick_data=pandas.DataFrame()),
    )

    resolved_option_surface_column_mapping = (
        CalibrationOptionSurfaceColumnMapping()
        if option_surface_column_mapping is None
        else option_surface_column_mapping
    )

    with ExitStack() as stack:
        sources = _materialize_request_sources(
            stack,
            tick_data=tick_data,
            option_surface=option_surface,
        )

        request_payload = {
            "model_payload": model._native_model_payload(),
            "run_id": None,
            "tick_data_source": sources.tick_data_source,
            "tick_data_column_mapping": tick_data_column_mapping.to_payload(),
            "option_surface_source": sources.option_surface_source,
            "option_surface_column_mapping": resolved_option_surface_column_mapping.to_payload(),
            "normalization_policy": {
                "timestamp_timezone": "UTC",
                "sort_records_by_timestamp": True,
                "duplicate_record_policy": None,
                "tick_price_input_policy": None,
                "option_quote_input_policy": None,
            },
            "valuation_time_utc": (
                None
                if valuation_time_utc is None
                else valuation_time_utc.astimezone(UTC).isoformat()
            ),
            "option_type": _native_option_type(option_type),
            "scope": scope,
            "risk_free_rate": risk_free_rate,
            "dividend_yield": dividend_yield,
            "valuation_count": valuation_count,
            "use_all_valuation_times": use_all_valuation_times,
            "cpu_count": cpu_count,
            "price_coordinate_policy": coerce_price_coordinate_policy(
                price_coordinate_policy
            ).value,
            "option_forward_parity_warning_policy": OptionForwardParityWarningPolicy(
                relative_threshold=(
                    DEFAULT_OPTION_FORWARD_PARITY_WARNING_RELATIVE_THRESHOLD
                    if option_forward_parity_warning_threshold is None
                    else option_forward_parity_warning_threshold
                )
            ).to_payload(),
            "optional_output_selection": {
                "include_transform_diagnostics": False,
                "include_integration_quality_diagnostics": False,
                "include_calibration_hooks": False,
            },
            "pricing_interval_schedule_policy": {"policy": "auto_activity_compressed"},
            "physical_distribution_diagnostics": (
                physical_distribution_diagnostics_request_payload(
                    physical_distribution_diagnostics
                )
            ),
            "output_path": None if output_path is None else str(output_path),
            "progress_mode": _native_pricing_terminal_progress_mode(
                silent=silent,
                progress_mode=progress_mode,
            ),
        }

        native = _native_pricing_module()
        try:
            raw_result = native.run_observed_options_pricing_workflow(request_payload)
        except Exception as error:
            if native_exception_matches(native, error, "PricingExecutionError"):
                raise _pricing_execution_error(
                    "observed-options pricing failed during native execution",
                    entrypoint="run_observed_options_pricing_workflow",
                    context={"source_exception_type": type(error).__name__},
                ) from error

            raise

    if not isinstance(raw_result, dict):
        raise _pricing_integration_error(
            "native observed-options pricing returned an invalid response payload",
            entrypoint="run_observed_options_pricing_workflow",
            error_id=PRICING_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_result).__name__},
        )

    return raw_result


def observed_snapshot_result_from_native_payload(
    raw_result: dict[str, Any],
) -> JaniformObservedOptionSnapshotPricingResult:
    raw_snapshot_results = raw_result.get("snapshot_results")
    if not isinstance(raw_snapshot_results, list) or len(raw_snapshot_results) != 1:
        raise _pricing_integration_error(
            "native observed snapshot pricing returned an invalid snapshot result payload",
            entrypoint="run_observed_options_pricing_workflow",
            error_id=PRICING_INTEGRATION_RESPONSE_ERROR_ID,
            context={
                "snapshot_result_count": (
                    None
                    if not isinstance(raw_snapshot_results, list)
                    else len(raw_snapshot_results)
                )
            },
        )

    snapshot_result = JaniformObservedOptionSnapshotPricingResult.from_native_snapshot_mapping(
        raw_snapshot_results[0],
        field_path="observed_options_pricing_result.snapshot_results[0]",
    )
    physical_distribution = (
        ObservedOptionsPhysicalDistributionDiagnosticsOutput.from_native_mapping(
            raw_result.get("physical_distribution"),
            field_path="observed_options_pricing_result.physical_distribution",
        )
    )

    price_coordinate_resolution = (
        resolve_price_coordinate_from_native_mapping(
            raw_result["price_coordinate_resolution"],
            field_path="observed_options_pricing_result.price_coordinate_resolution",
        )
        if raw_result.get("price_coordinate_resolution") is not None
        else None
    )

    return replace(
        snapshot_result,
        physical_distribution=physical_distribution,
        price_coordinate_resolution=price_coordinate_resolution,
        warnings=tuple(str(warning) for warning in raw_result.get("warnings", [])),
    )


def observed_surface_result_from_native_payload(
    raw_result: dict[str, Any],
) -> JaniformObservedOptionSurfacePricingResult:
    return JaniformObservedOptionSurfacePricingResult.from_native_mapping(raw_result)


def _materialize_request_sources(
    stack: ExitStack,
    *,
    tick_data: TabularFrameInput,
    option_surface: TabularFrameInput,
) -> NativeObservedOptionsRequestSources:
    return NativeObservedOptionsRequestSources(
        tick_data_source=_source_reference_from_tabular_input(
            stack,
            tick_data,
            provenance_label="ticks",
            temporary_file_name="ticks.parquet",
            field_path="tick_data",
        ),
        option_surface_source=_source_reference_from_tabular_input(
            stack,
            option_surface,
            provenance_label="options",
            temporary_file_name="options.parquet",
            field_path="option_surface",
        ),
    )


def _source_reference_from_tabular_input(
    stack: ExitStack,
    tabular_input: TabularFrameInput,
    *,
    provenance_label: str,
    temporary_file_name: str,
    field_path: str,
) -> dict[str, Any]:
    if isinstance(tabular_input, pandas.DataFrame):
        return _temporary_parquet_source(
            stack,
            tabular_input,
            provenance_label=provenance_label,
            temporary_file_name=temporary_file_name,
        )

    if isinstance(tabular_input, Mapping):
        return _temporary_parquet_source(
            stack,
            pandas.DataFrame(tabular_input),
            provenance_label=provenance_label,
            temporary_file_name=temporary_file_name,
        )

    path = Path(tabular_input)
    file_format = _input_file_format(path, field_path=field_path)

    return {
        "path": str(path),
        "file_format": file_format.value,
        "provenance_label": provenance_label,
    }


def _temporary_parquet_source(
    stack: ExitStack,
    frame: pandas.DataFrame,
    *,
    provenance_label: str,
    temporary_file_name: str,
) -> dict[str, Any]:
    temporary_directory = stack.enter_context(TemporaryDirectory(prefix="janiform-observed-pricing-"))
    path = Path(temporary_directory) / temporary_file_name

    frame.to_parquet(path, index=False)

    return {
        "path": str(path),
        "file_format": CalibrationInputFileFormat.PARQUET.value,
        "provenance_label": provenance_label,
    }


def _input_file_format(path: Path, *, field_path: str) -> CalibrationInputFileFormat:
    suffix = path.suffix.lower()
    if suffix == ".csv":
        return CalibrationInputFileFormat.CSV
    if suffix in (".parquet", ".pq"):
        return CalibrationInputFileFormat.PARQUET

    raise _pricing_error(field_path, "must be a .csv or .parquet path")


def _native_option_type(
    option_type: Optional[VanillaOptionPayoffType | str],
) -> Optional[str]:
    if option_type is None:
        return None
    if isinstance(option_type, VanillaOptionPayoffType):
        return option_type.value

    normalized = option_type.strip().lower()
    if normalized in ("call", "c"):
        return "call"
    if normalized in ("put", "p"):
        return "put"

    raise _pricing_error("option_type", "must be one of: call, put")


def _native_pricing_terminal_progress_mode(
    *,
    silent: bool,
    progress_mode: WorkflowProgressMode | str,
) -> str:
    default_mode = WorkflowProgressMode.NONE if silent else WorkflowProgressMode.TERMINAL
    resolved_mode = resolve_workflow_progress_mode(progress_mode, default_mode=default_mode)

    return native_terminal_progress_mode(resolved_mode)


def _native_pricing_module() -> ModuleType:
    try:
        return native_module()
    except ModuleNotFoundError as error:
        raise _pricing_integration_error(
            "native pricing module is unavailable",
            entrypoint="run_observed_options_pricing_workflow",
            error_id=PRICING_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from error


__all__ = [
    "observed_snapshot_result_from_native_payload",
    "observed_surface_result_from_native_payload",
    "run_native_observed_options_pricing",
]
