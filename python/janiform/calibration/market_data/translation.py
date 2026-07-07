
from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from types import ModuleType
from typing import Any, Optional, cast

from .contracts import (
    CalibrationInputFileFormat,
    CalibrationInputFileReference,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
    CalibrationSessionConfigurationInput,
    CalibrationTickDataColumnMapping,
)
from .ingestion import LoadedMarketDataSourceSummary
from .request import MarketDataCalibrationRequest
from .timestamp_batching_surface import MarketDataTimestampBatchingSurfaceDiagnostic
from ..errors import (
    CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
    calibration_error,
    calibration_integration_error,
)
from ..sessions.contracts import (
    MarketDataSessionRecurringClosedWindow,
    MarketDataSessionRegularWindow,
    MarketDataSessionResolution,
)
from ..staged.contracts import CalibrationStage
from ..staged.request import StagedCalibrationRequest
from ..._native_api import native_error_parts, native_exception_matches, native_module
from ..._native_types import (
    NormalizedMarketDataHandleProtocol,
    PreparedCalibrationExecutionHandleProtocol,
)


CALIBRATION_MARKET_DATA_TRANSLATION_ERROR_ID = "calibration.validation.market_data_translation"
CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT = "market_data_translation"


@dataclass(frozen=True, slots=True)
class MarketDataTranslationDiagnostics:
    workflow_scope: CalibrationMarketDataWorkflowScope
    selected_stages: tuple[CalibrationStage, ...]
    executed_calibration_steps: tuple[str, ...]
    timestamp_batching_surface: MarketDataTimestampBatchingSurfaceDiagnostic
    tick_record_count: int
    coarse_data_record_count: int
    coarse_data_return_interval_count: int
    coarse_data_overlap_interval_count: int
    coarse_data_independent_interval_count: int
    option_surface_record_count: int
    known_news_event_count: int
    known_news_events_outside_source_window_count: int
    session_open_tick_count: int
    session_closed_tick_count: int
    known_news_open_event_count: int
    known_news_closed_event_count: int
    hawkes_candidate_count: int
    known_news_return_sample_count: int
    intraday_jump_candidate_count: int
    close_open_boundary_observation_count: int
    material_close_open_gap_count: int


@dataclass(frozen=True, slots=True)
class MarketDataTranslationProvenance:
    tick_data_summary: LoadedMarketDataSourceSummary
    coarse_data_summary: Optional[LoadedMarketDataSourceSummary]
    option_surface_summary: Optional[LoadedMarketDataSourceSummary]
    known_news_summary: Optional[LoadedMarketDataSourceSummary]
    calibration_evidence_routing: dict[str, Any]
    normalization_policy: CalibrationMarketDataNormalizationPolicy
    workflow_scope: CalibrationMarketDataWorkflowScope
    selected_stages: tuple[CalibrationStage, ...]
    session_definition_path: Optional[str]
    derived_trade_events_data_reference: str
    derived_coarse_data_reference: Optional[str]
    derived_option_surface_data_reference: Optional[str]
    derivation_policies: tuple[str, ...]
    deterministic_defaults_applied: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class MarketDataSourceProbe:
    path: str
    file_format: CalibrationInputFileFormat
    provenance_label: str
    available_columns: tuple[str, ...]
    source_row_count_estimate: Optional[int]


@dataclass(frozen=True, slots=True)
class NormalizedMarketDataSource:
    normalized_handle: NormalizedMarketDataHandleProtocol
    tick_data_summary: LoadedMarketDataSourceSummary
    reference_price: Optional[float]
    fit_warning: Optional[str]


def infer_bundled_market_profile_for_tick_data_path(
    tick_data_path: str | Path,
) -> Optional[dict[str, Any]]:
    native = _native_market_data_module()

    try:
        raw_payload = native.infer_bundled_market_profile_for_tick_data_path(str(tick_data_path))
    except RuntimeError as error:
        if native_exception_matches(native, error, "CalibrationExecutionError"):
            raise calibration_integration_error(str(error)) from error

        raise

    if raw_payload is None:
        return None
    if not isinstance(raw_payload, dict):
        raise calibration_integration_error("native market-profile inference returned a non-object payload")

    return dict(raw_payload)


def infer_market_data_fit_inputs(inference_request: dict[str, Any]) -> dict[str, Any]:
    native = _native_market_data_module()

    try:
        raw_payload = native.infer_market_data_fit_inputs(inference_request)
    except RuntimeError as error:
        if native_exception_matches(native, error, "CalibrationExecutionError"):
            raise calibration_integration_error(str(error)) from error

        raise

    if not isinstance(raw_payload, dict):
        raise calibration_integration_error("native fit-input inference returned a non-object payload")

    return dict(raw_payload)


@dataclass(frozen=True, slots=True)
class MarketDataTranslationResult:
    staged_request: StagedCalibrationRequest
    prepared_handle: PreparedCalibrationExecutionHandleProtocol
    session_resolution: MarketDataSessionResolution
    diagnostics: MarketDataTranslationDiagnostics
    provenance: MarketDataTranslationProvenance
    fit_warning: Optional[str]
    reference_price: Optional[float]

    def prepared_payload_json(self) -> str:
        return self.prepared_handle.serialize_execution_input_json()

    def prepared_payload(self) -> dict[str, Any]:
        from ...contracts import parse_json_object

        return dict(
            parse_json_object(
                self.prepared_payload_json(),
                "prepared calibration execution payload",
                _translation_error,
            )
        )


def translate_market_data_request_to_prepared_calibration(
    request: MarketDataCalibrationRequest,
    *,
    cpu_count: Optional[int] = None,
) -> MarketDataTranslationResult:
    if not isinstance(request, MarketDataCalibrationRequest):
        raise _translation_error("request", "must be a MarketDataCalibrationRequest")

    normalized_request = MarketDataCalibrationRequest.from_mapping(request.to_payload())

    native = _native_market_data_module()

    try:
        raw_payload = native.prepare_market_data_calibration_plan(
            normalized_request.to_payload(),
            cpu_count,
        )
    except Exception as e:
        if native_exception_matches(native, e, "CalibrationExecutionError"):
            field_path, message = native_error_parts(
                str(e),
                default_field_path="request",
            )

            raise _translation_error(
                field_path,
                f"market-data preparation failed: {message}",
            ) from e

        raise _translation_error(
            "request",
            f"market-data preparation failed: {e}",
        ) from e

    if not isinstance(raw_payload, dict):
        raise calibration_integration_error(
            "market-data preparation returned an invalid response payload",
            entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_payload).__name__},
        )

    return _market_data_translation_result_from_payload(raw_payload)


def translate_market_data_request_to_prepared_calibration_from_phases(
    *,
    request: MarketDataCalibrationRequest,
    normalized_market_data: NormalizedMarketDataSource,
    session_resolution: MarketDataSessionResolution,
    cpu_count: Optional[int] = None,
) -> MarketDataTranslationResult:
    if not isinstance(request, MarketDataCalibrationRequest):
        raise _translation_error("request", "must be a MarketDataCalibrationRequest")

    native = _native_market_data_module()

    try:
        raw_payload = native.prepare_market_data_calibration_plan_from_phases(
            request.to_payload(),
            normalized_market_data.normalized_handle,
            session_resolution.to_payload(),
            cpu_count,
        )
    except Exception as e:
        if native_exception_matches(native, e, "CalibrationExecutionError"):
            raise _translation_error(
                "request",
                f"market-data preparation failed: {e}",
            ) from e

        raise _translation_error(
            "request",
            f"market-data preparation failed: {e}",
        ) from e

    if not isinstance(raw_payload, dict):
        raise calibration_integration_error(
            "market-data preparation returned an invalid response payload",
            entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_payload).__name__},
        )

    return _market_data_translation_result_from_payload(raw_payload)


def probe_market_data_source(
    *,
    tick_data_source: CalibrationInputFileReference,
) -> MarketDataSourceProbe:
    native = _native_market_data_module()

    try:
        raw_payload = native.probe_market_data_source(
            tick_data_source.to_payload(),
        )
    except Exception as e:
        if native_exception_matches(native, e, "CalibrationExecutionError"):
            raise _translation_error(
                "tick_data_source",
                f"market-data source probe failed: {e}",
                context={"translation_step": "native_source_probe"},
            ) from e

        raise _translation_error(
            "tick_data_source",
            f"market-data source probe failed: {e}",
            context={"translation_step": "native_source_probe"},
        ) from e

    if not isinstance(raw_payload, dict):
        raise calibration_integration_error(
            "market-data source probe returned an invalid response payload",
            entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_payload).__name__},
        )

    return _market_data_source_probe_from_payload(raw_payload)


def normalize_market_data_source(
    *,
    tick_data_source: CalibrationInputFileReference,
    tick_data_column_mapping: CalibrationTickDataColumnMapping,
    normalization_policy: CalibrationMarketDataNormalizationPolicy,
    require_hawkes: bool = False,
) -> NormalizedMarketDataSource:
    native = _native_market_data_module()

    try:
        raw_payload = native.normalize_market_data_source(
            tick_data_source.to_payload(),
            tick_data_column_mapping.to_payload(),
            normalization_policy.to_payload(),
            require_hawkes,
        )
    except Exception as e:
        if native_exception_matches(native, e, "CalibrationExecutionError"):
            raise _translation_error(
                "tick_data_source",
                f"market-data normalization failed: {e}",
                context={"translation_step": "native_source_normalization"},
            ) from e

        raise _translation_error(
            "tick_data_source",
            f"market-data normalization failed: {e}",
            context={"translation_step": "native_source_normalization"},
        ) from e

    if not isinstance(raw_payload, dict):
        raise calibration_integration_error(
            "market-data normalization returned an invalid response payload",
            entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_payload).__name__},
        )

    return _normalized_market_data_source_from_payload(raw_payload)


def resolve_market_data_session(
    *,
    session_configuration: CalibrationSessionConfigurationInput,
    normalized_market_data: Optional[NormalizedMarketDataSource] = None,
) -> MarketDataSessionResolution:
    native = _native_market_data_module()

    try:
        raw_payload = native.resolve_market_data_session(
            session_configuration.to_payload(),
            (
                normalized_market_data.normalized_handle
                if normalized_market_data is not None
                else None
            ),
        )
    except Exception as e:
        if native_exception_matches(native, e, "CalibrationExecutionError"):
            raise _translation_error(
                "session_configuration",
                f"market-data session resolution failed: {e}",
                context={"translation_step": "native_session_resolution"},
            ) from e

        raise _translation_error(
            "session_configuration",
            f"market-data session resolution failed: {e}",
            context={"translation_step": "native_session_resolution"},
        ) from e

    if not isinstance(raw_payload, dict):
        raise calibration_integration_error(
            "market-data session resolution returned an invalid response payload",
            entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
            error_id=CALIBRATION_INTEGRATION_RESPONSE_ERROR_ID,
            context={"returned_type": type(raw_payload).__name__},
        )

    return _market_data_session_resolution_from_payload(raw_payload)


def _market_data_translation_result_from_payload(
    raw_payload: dict[str, Any],
) -> MarketDataTranslationResult:
    prepared_handle = raw_payload.get("prepared_handle")
    if prepared_handle is None:
        raise _translation_error("prepared_handle", "is required in the native response payload")

    staged_request_payload = raw_payload.get("staged_request")
    diagnostics_payload = raw_payload.get("diagnostics")
    provenance_payload = raw_payload.get("provenance")
    session_resolution_payload = raw_payload.get("session_resolution")

    if not isinstance(staged_request_payload, dict):
        raise _translation_error("staged_request", "must be an object")
    if not isinstance(diagnostics_payload, dict):
        raise _translation_error("diagnostics", "must be an object")
    if not isinstance(provenance_payload, dict):
        raise _translation_error("provenance", "must be an object")
    if not isinstance(session_resolution_payload, dict):
        raise _translation_error("session_resolution", "must be an object")

    return MarketDataTranslationResult(
        staged_request=StagedCalibrationRequest.from_mapping(staged_request_payload),
        prepared_handle=cast(PreparedCalibrationExecutionHandleProtocol, prepared_handle),
        session_resolution=_market_data_session_resolution_from_payload(session_resolution_payload),
        diagnostics=_market_data_translation_diagnostics_from_payload(diagnostics_payload),
        provenance=_market_data_translation_provenance_from_payload(provenance_payload),
        fit_warning=raw_payload.get("fit_warning"),
        reference_price=raw_payload.get("reference_price"),
    )


def _market_data_source_probe_from_payload(
    raw_payload: dict[str, Any],
) -> MarketDataSourceProbe:
    return MarketDataSourceProbe(
        path=str(raw_payload["path"]),
        file_format=CalibrationInputFileFormat(raw_payload["file_format"]),
        provenance_label=str(raw_payload["provenance_label"]),
        available_columns=tuple(str(column_name) for column_name in raw_payload["available_columns"]),
        source_row_count_estimate=(
            int(raw_payload["source_row_count_estimate"])
            if raw_payload.get("source_row_count_estimate") is not None
            else None
        ),
    )


def _normalized_market_data_source_from_payload(
    raw_payload: dict[str, Any],
) -> NormalizedMarketDataSource:
    normalized_handle = raw_payload.get("normalized_handle")
    if normalized_handle is None:
        raise _translation_error("normalized_handle", "is required in the native response payload")

    tick_data_summary = raw_payload.get("tick_data_summary")
    if not isinstance(tick_data_summary, dict):
        raise _translation_error("tick_data_summary", "must be an object")

    return NormalizedMarketDataSource(
        normalized_handle=cast(NormalizedMarketDataHandleProtocol, normalized_handle),
        tick_data_summary=_source_summary_from_payload(
            tick_data_summary,
            field_path="tick_data_summary",
        ),
        reference_price=raw_payload.get("reference_price"),
        fit_warning=raw_payload.get("fit_warning"),
    )


def _market_data_translation_diagnostics_from_payload(
    raw_payload: dict[str, Any],
) -> MarketDataTranslationDiagnostics:
    return MarketDataTranslationDiagnostics(
        workflow_scope=CalibrationMarketDataWorkflowScope(raw_payload["workflow_scope"]),
        selected_stages=tuple(CalibrationStage(stage) for stage in raw_payload["selected_stages"]),
        executed_calibration_steps=tuple(raw_payload["executed_calibration_steps"]),
        timestamp_batching_surface=(
            MarketDataTimestampBatchingSurfaceDiagnostic.from_mapping(
                raw_payload["timestamp_batching_surface"],
                field_path="diagnostics.timestamp_batching_surface",
            )
        ),
        tick_record_count=int(raw_payload["tick_record_count"]),
        coarse_data_record_count=int(raw_payload["coarse_data_record_count"]),
        coarse_data_return_interval_count=int(
            raw_payload["coarse_data_return_interval_count"]
        ),
        coarse_data_overlap_interval_count=int(
            raw_payload["coarse_data_overlap_interval_count"]
        ),
        coarse_data_independent_interval_count=int(
            raw_payload["coarse_data_independent_interval_count"]
        ),
        option_surface_record_count=int(raw_payload["option_surface_record_count"]),
        known_news_event_count=int(raw_payload["known_news_event_count"]),
        known_news_events_outside_source_window_count=int(
            raw_payload["known_news_events_outside_source_window_count"]
        ),
        session_open_tick_count=int(raw_payload["session_open_tick_count"]),
        session_closed_tick_count=int(raw_payload["session_closed_tick_count"]),
        known_news_open_event_count=int(raw_payload["known_news_open_event_count"]),
        known_news_closed_event_count=int(raw_payload["known_news_closed_event_count"]),
        hawkes_candidate_count=int(raw_payload["hawkes_candidate_count"]),
        known_news_return_sample_count=int(raw_payload["known_news_return_sample_count"]),
        intraday_jump_candidate_count=int(raw_payload["intraday_jump_candidate_count"]),
        close_open_boundary_observation_count=int(
            raw_payload["close_open_boundary_observation_count"]
        ),
        material_close_open_gap_count=int(raw_payload["material_close_open_gap_count"]),
    )


def _market_data_translation_provenance_from_payload(
    raw_payload: dict[str, Any],
) -> MarketDataTranslationProvenance:
    return MarketDataTranslationProvenance(
        tick_data_summary=_source_summary_from_payload(
            raw_payload["tick_data_summary"],
            field_path="provenance.tick_data_summary",
        ),
        coarse_data_summary=(
            _source_summary_from_payload(
                raw_payload["coarse_data_summary"]["source_summary"],
                field_path="provenance.coarse_data_summary.source_summary",
            )
            if raw_payload.get("coarse_data_summary") is not None
            else None
        ),
        option_surface_summary=(
            _source_summary_from_payload(
                raw_payload["option_surface_summary"],
                field_path="provenance.option_surface_summary",
            )
            if raw_payload.get("option_surface_summary") is not None
            else None
        ),
        known_news_summary=(
            _source_summary_from_payload(
                raw_payload["known_news_summary"],
                field_path="provenance.known_news_summary",
            )
            if raw_payload.get("known_news_summary") is not None
            else None
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy.from_mapping(
            raw_payload["normalization_policy"],
            field_path="provenance.normalization_policy",
        ),
        calibration_evidence_routing=dict(raw_payload["calibration_evidence_routing"]),
        workflow_scope=CalibrationMarketDataWorkflowScope(raw_payload["workflow_scope"]),
        selected_stages=tuple(CalibrationStage(stage) for stage in raw_payload["selected_stages"]),
        session_definition_path=raw_payload.get("session_definition_path"),
        derived_trade_events_data_reference=str(raw_payload["derived_trade_events_data_reference"]),
        derived_coarse_data_reference=raw_payload.get(
            "derived_coarse_data_reference"
        ),
        derived_option_surface_data_reference=raw_payload.get(
            "derived_option_surface_data_reference"
        ),
        derivation_policies=tuple(raw_payload["derivation_policies"]),
        deterministic_defaults_applied=tuple(raw_payload["deterministic_defaults_applied"]),
    )


def _source_summary_from_payload(
    raw_payload: dict[str, Any],
    *,
    field_path: str,
) -> LoadedMarketDataSourceSummary:
    if not isinstance(raw_payload, dict):
        raise _translation_error(field_path, "must be an object")

    return LoadedMarketDataSourceSummary(
        path=str(raw_payload["path"]),
        file_format=CalibrationInputFileFormat(str(raw_payload["file_format"])),
        provenance_label=str(raw_payload["provenance_label"]),
        source_row_count=int(raw_payload["source_row_count"]),
        retained_row_count=int(raw_payload["retained_row_count"]),
        dropped_duplicate_row_count=int(raw_payload["dropped_duplicate_row_count"]),
        dropped_invalid_row_count=int(raw_payload["dropped_invalid_row_count"]),
        sort_records_by_timestamp=bool(raw_payload["sort_records_by_timestamp"]),
        applied_timestamp_timezone=raw_payload.get("applied_timestamp_timezone"),
        canonical_value_field=raw_payload.get("canonical_value_field"),
        value_mapping_policy=raw_payload.get("value_mapping_policy"),
    )


def _market_data_session_resolution_from_payload(
    raw_payload: dict[str, Any],
) -> MarketDataSessionResolution:
    if not isinstance(raw_payload, dict):
        raise _translation_error("session_resolution", "must be an object")

    return MarketDataSessionResolution(
        schema_version=str(raw_payload["schema_version"]),
        resolution_kind=str(raw_payload["resolution_kind"]),
        provenance_kind=str(raw_payload["provenance_kind"]),
        selected_policy_id=str(raw_payload["selected_policy_id"]),
        manual_override_applied=bool(raw_payload["manual_override_applied"]),
        exchange_code=str(raw_payload["exchange_code"]),
        exchange_name=str(raw_payload["exchange_name"]),
        resolved_timezone_name=str(raw_payload["resolved_timezone_name"]),
        timezone_source=str(raw_payload["timezone_source"]),
        open_session_duration_hours_per_week=float(raw_payload["open_session_duration_hours_per_week"]),
        confidence_score=float(raw_payload["confidence_score"]),
        confidence_label=str(raw_payload["confidence_label"]),
        regular_session_windows=tuple(
            MarketDataSessionRegularWindow.from_mapping(
                entry,
                field_path=f"session_resolution.regular_session_windows[{entry_index}]",
            )
            for entry_index, entry in enumerate(raw_payload["regular_session_windows"])
        ),
        recurring_closed_windows=tuple(
            MarketDataSessionRecurringClosedWindow.from_mapping(
                entry,
                field_path=f"session_resolution.recurring_closed_windows[{entry_index}]",
            )
            for entry_index, entry in enumerate(raw_payload["recurring_closed_windows"])
        ),
        reasons=tuple(str(entry) for entry in raw_payload["reasons"]),
        ambiguity_warnings=tuple(str(entry) for entry in raw_payload["ambiguity_warnings"]),
        metadata_discrepancies=tuple(str(entry) for entry in raw_payload["metadata_discrepancies"]),
        resolved_session_definition_json=str(raw_payload["resolved_session_definition_json"]),
    )


def _native_market_data_module() -> ModuleType:
    try:
        return native_module()
    except ModuleNotFoundError as e:
        raise calibration_integration_error(
            "market-data loading module is unavailable",
            entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
            error_id=CALIBRATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            context={"module_name": "janiform._native"},
        ) from e


def _translation_error(
    field_path: str,
    message: str,
    *,
    context: Optional[dict[str, Optional[str | int | float | bool]]] = None,
) -> Exception:
    return calibration_error(
        field_path,
        message,
        entrypoint=CALIBRATION_MARKET_DATA_TRANSLATION_ENTRYPOINT,
        error_id=CALIBRATION_MARKET_DATA_TRANSLATION_ERROR_ID,
        context=context,
    )


__all__ = [
    "MarketDataSourceProbe",
    "MarketDataTranslationDiagnostics",
    "MarketDataTranslationProvenance",
    "MarketDataTranslationResult",
    "NormalizedMarketDataSource",
    "normalize_market_data_source",
    "probe_market_data_source",
    "infer_bundled_market_profile_for_tick_data_path",
    "infer_market_data_fit_inputs",
    "resolve_market_data_session",
    "translate_market_data_request_to_prepared_calibration",
    "translate_market_data_request_to_prepared_calibration_from_phases",
]
