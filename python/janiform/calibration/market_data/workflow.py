from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional

from .contracts import (
    CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION,
    CalibrationInputFileFormat,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
)
from .ingestion import LoadedMarketDataSourceSummary
from .request import MarketDataCalibrationRequest, validate_market_data_calibration_request
from .translation import (
    MarketDataTranslationDiagnostics,
    MarketDataTranslationProvenance,
    MarketDataTranslationResult,
    translate_market_data_request_to_prepared_calibration,
)
from ..errors import calibration_error
from ..sessions.contracts import MarketDataSessionResolution
from ..staged.contracts import CalibrationStage
from ..staged.request import StagedCalibrationRequest
from ..staged.results import StagedCalibrationResponse
from ..staged.runtime import run_prepared_staged_calibration_with_raw_payload
from ..._native_types import PreparedCalibrationExecutionHandleProtocol
from ...calendar import CalendarRegistry
from ...contracts import ContractModel


@dataclass(frozen=True, slots=True)
class MarketDataCalibrationSourceSummary(ContractModel):
    path: str
    file_format: CalibrationInputFileFormat
    provenance_label: str
    source_row_count: int
    retained_row_count: int
    dropped_duplicate_row_count: int
    dropped_invalid_row_count: int
    sort_records_by_timestamp: bool
    applied_timestamp_timezone: Optional[str]
    canonical_value_field: Optional[str] = None
    value_mapping_policy: Optional[str] = None

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> MarketDataCalibrationSourceSummary:
        if not isinstance(raw_payload, dict):
            raise calibration_error("source_summary", "must be an object")

        return MarketDataCalibrationSourceSummary(
            path=str(raw_payload["path"]),
            file_format=CalibrationInputFileFormat(str(raw_payload["file_format"])),
            provenance_label=str(raw_payload["provenance_label"]),
            source_row_count=int(raw_payload["source_row_count"]),
            retained_row_count=int(raw_payload["retained_row_count"]),
            dropped_duplicate_row_count=int(raw_payload["dropped_duplicate_row_count"]),
            dropped_invalid_row_count=int(raw_payload["dropped_invalid_row_count"]),
            sort_records_by_timestamp=bool(raw_payload["sort_records_by_timestamp"]),
            applied_timestamp_timezone=(
                str(raw_payload["applied_timestamp_timezone"])
                if raw_payload.get("applied_timestamp_timezone") is not None
                else None
            ),
            canonical_value_field=(
                str(raw_payload["canonical_value_field"])
                if raw_payload.get("canonical_value_field") is not None
                else None
            ),
            value_mapping_policy=(
                str(raw_payload["value_mapping_policy"])
                if raw_payload.get("value_mapping_policy") is not None
                else None
            ),
        )


@dataclass(frozen=True, slots=True)
class MarketDataCalibrationPreparationDiagnostics(ContractModel):
    workflow_scope: CalibrationMarketDataWorkflowScope
    selected_stages: tuple[CalibrationStage, ...]
    executed_preprocessing_steps: tuple[str, ...]
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

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> MarketDataCalibrationPreparationDiagnostics:
        if not isinstance(raw_payload, dict):
            raise calibration_error("preparation_diagnostics", "must be an object")

        return MarketDataCalibrationPreparationDiagnostics(
            workflow_scope=CalibrationMarketDataWorkflowScope(str(raw_payload["workflow_scope"])),
            selected_stages=tuple(
                CalibrationStage(str(stage))
                for stage in raw_payload["selected_stages"]
            ),
            executed_preprocessing_steps=tuple(
                str(step)
                for step in raw_payload["executed_preprocessing_steps"]
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


@dataclass(frozen=True, slots=True)
class MarketDataCalibrationPreparationProvenance(ContractModel):
    tick_data_summary: MarketDataCalibrationSourceSummary
    coarse_data_summary: Optional[MarketDataCalibrationSourceSummary]
    option_surface_summary: Optional[MarketDataCalibrationSourceSummary]
    known_news_summary: Optional[MarketDataCalibrationSourceSummary]
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

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> MarketDataCalibrationPreparationProvenance:
        if not isinstance(raw_payload, dict):
            raise calibration_error("preparation_provenance", "must be an object")

        return MarketDataCalibrationPreparationProvenance(
            tick_data_summary=MarketDataCalibrationSourceSummary.from_mapping(
                raw_payload["tick_data_summary"]
            ),
            coarse_data_summary=(
                MarketDataCalibrationSourceSummary.from_mapping(
                    raw_payload["coarse_data_summary"]["source_summary"]
                )
                if raw_payload.get("coarse_data_summary") is not None
                else None
            ),
            option_surface_summary=(
                MarketDataCalibrationSourceSummary.from_mapping(
                    raw_payload["option_surface_summary"]
                )
                if raw_payload.get("option_surface_summary") is not None
                else None
            ),
            known_news_summary=(
                MarketDataCalibrationSourceSummary.from_mapping(
                    raw_payload["known_news_summary"]
                )
                if raw_payload.get("known_news_summary") is not None
                else None
            ),
            calibration_evidence_routing=dict(raw_payload["calibration_evidence_routing"]),
            normalization_policy=CalibrationMarketDataNormalizationPolicy.from_mapping(
                raw_payload["normalization_policy"],
                field_path="preparation_provenance.normalization_policy",
            ),
            workflow_scope=CalibrationMarketDataWorkflowScope(str(raw_payload["workflow_scope"])),
            selected_stages=tuple(
                CalibrationStage(str(stage))
                for stage in raw_payload["selected_stages"]
            ),
            session_definition_path=(
                str(raw_payload["session_definition_path"])
                if raw_payload.get("session_definition_path") is not None
                else None
            ),
            derived_trade_events_data_reference=str(
                raw_payload["derived_trade_events_data_reference"]
            ),
            derived_coarse_data_reference=(
                str(raw_payload["derived_coarse_data_reference"])
                if raw_payload.get("derived_coarse_data_reference") is not None
                else None
            ),
            derived_option_surface_data_reference=(
                str(raw_payload["derived_option_surface_data_reference"])
                if raw_payload.get("derived_option_surface_data_reference") is not None
                else None
            ),
            derivation_policies=tuple(str(policy) for policy in raw_payload["derivation_policies"]),
            deterministic_defaults_applied=tuple(
                str(default_id)
                for default_id in raw_payload["deterministic_defaults_applied"]
            ),
        )


@dataclass(frozen=True, slots=True)
class PreparedMarketDataCalibration(ContractModel):
    schema_version: str
    request: MarketDataCalibrationRequest
    staged_request: StagedCalibrationRequest
    diagnostics: MarketDataCalibrationPreparationDiagnostics
    provenance: MarketDataCalibrationPreparationProvenance
    session_resolution: MarketDataSessionResolution
    fit_warning: Optional[str] = None
    reference_price: Optional[float] = None
    prepared_data_reference: Optional[str] = None
    prepared_payload_path: Optional[str] = None
    staged_execution_input_path: Optional[str] = None
    _prepared_handle: Optional[PreparedCalibrationExecutionHandleProtocol] = field(
        repr=False,
        compare=False,
        default=None,
    )

    def __post_init__(self) -> None:
        if self.schema_version != CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION:
            raise calibration_error(
                "schema_version",
                f"must be `{CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION}`",
            )
        if self._prepared_handle is None:
            raise calibration_error("_prepared_handle", "must be provided")

    @property
    def prepared_handle(self) -> PreparedCalibrationExecutionHandleProtocol:
        if self._prepared_handle is None:
            raise calibration_error("_prepared_handle", "must be provided")

        return self._prepared_handle

    def to_payload(self) -> dict[str, Any]:
        payload = {
            "schema_version": self.schema_version,
            "request": self.request.to_payload(),
            "staged_request": self.staged_request.to_payload(),
            "diagnostics": self.diagnostics.to_payload(),
            "provenance": self.provenance.to_payload(),
            "session_resolution": self.session_resolution.to_payload(),
            "fit_warning": self.fit_warning,
            "reference_price": self.reference_price,
            "prepared_data_reference": self.prepared_data_reference,
            "prepared_payload_path": self.prepared_payload_path,
            "staged_execution_input_path": self.staged_execution_input_path,
        }
        return payload

def _build_market_data_source_summary(
    source_summary: LoadedMarketDataSourceSummary,
) -> MarketDataCalibrationSourceSummary:
    return MarketDataCalibrationSourceSummary(
        path=source_summary.path,
        file_format=source_summary.file_format,
        provenance_label=source_summary.provenance_label,
        source_row_count=source_summary.source_row_count,
        retained_row_count=source_summary.retained_row_count,
        dropped_duplicate_row_count=source_summary.dropped_duplicate_row_count,
        dropped_invalid_row_count=source_summary.dropped_invalid_row_count,
        sort_records_by_timestamp=source_summary.sort_records_by_timestamp,
        applied_timestamp_timezone=source_summary.applied_timestamp_timezone,
        canonical_value_field=source_summary.canonical_value_field,
        value_mapping_policy=source_summary.value_mapping_policy,
    )


def _build_market_data_preparation_diagnostics(
    diagnostics: MarketDataTranslationDiagnostics,
) -> MarketDataCalibrationPreparationDiagnostics:
    return MarketDataCalibrationPreparationDiagnostics(
        workflow_scope=diagnostics.workflow_scope,
        selected_stages=diagnostics.selected_stages,
        executed_preprocessing_steps=diagnostics.executed_preprocessing_steps,
        tick_record_count=diagnostics.tick_record_count,
        coarse_data_record_count=diagnostics.coarse_data_record_count,
        coarse_data_return_interval_count=(
            diagnostics.coarse_data_return_interval_count
        ),
        coarse_data_overlap_interval_count=(
            diagnostics.coarse_data_overlap_interval_count
        ),
        coarse_data_independent_interval_count=(
            diagnostics.coarse_data_independent_interval_count
        ),
        option_surface_record_count=diagnostics.option_surface_record_count,
        known_news_event_count=diagnostics.known_news_event_count,
        known_news_events_outside_source_window_count=diagnostics.known_news_events_outside_source_window_count,
        session_open_tick_count=diagnostics.session_open_tick_count,
        session_closed_tick_count=diagnostics.session_closed_tick_count,
        known_news_open_event_count=diagnostics.known_news_open_event_count,
        known_news_closed_event_count=diagnostics.known_news_closed_event_count,
        hawkes_candidate_count=diagnostics.hawkes_candidate_count,
        known_news_return_sample_count=diagnostics.known_news_return_sample_count,
        intraday_jump_candidate_count=diagnostics.intraday_jump_candidate_count,
        close_open_boundary_observation_count=diagnostics.close_open_boundary_observation_count,
        material_close_open_gap_count=diagnostics.material_close_open_gap_count,
    )


def _build_market_data_preparation_provenance(
    provenance: MarketDataTranslationProvenance,
) -> MarketDataCalibrationPreparationProvenance:
    return MarketDataCalibrationPreparationProvenance(
        tick_data_summary=_build_market_data_source_summary(provenance.tick_data_summary),
        coarse_data_summary=(
            _build_market_data_source_summary(provenance.coarse_data_summary)
            if provenance.coarse_data_summary is not None
            else None
        ),
        option_surface_summary=(
            _build_market_data_source_summary(provenance.option_surface_summary)
            if provenance.option_surface_summary is not None
            else None
        ),
        known_news_summary=(
            _build_market_data_source_summary(provenance.known_news_summary)
            if provenance.known_news_summary is not None
            else None
        ),
        calibration_evidence_routing=provenance.calibration_evidence_routing,
        normalization_policy=provenance.normalization_policy,
        workflow_scope=provenance.workflow_scope,
        selected_stages=provenance.selected_stages,
        session_definition_path=provenance.session_definition_path,
        derived_trade_events_data_reference=provenance.derived_trade_events_data_reference,
        derived_coarse_data_reference=(
            provenance.derived_coarse_data_reference
        ),
        derived_option_surface_data_reference=provenance.derived_option_surface_data_reference,
        derivation_policies=provenance.derivation_policies,
        deterministic_defaults_applied=provenance.deterministic_defaults_applied,
    )


def _build_market_data_session_resolution(
    session_resolution: MarketDataSessionResolution,
) -> MarketDataSessionResolution:
    if not isinstance(session_resolution, MarketDataSessionResolution):
        raise calibration_error(
            "session_resolution",
            "must be a MarketDataSessionResolution",
        )

    return session_resolution


def prepare_market_data_calibration(
    request: MarketDataCalibrationRequest,
    *,
    cpu_count: Optional[int] = None,
) -> PreparedMarketDataCalibration:
    if not isinstance(request, MarketDataCalibrationRequest):
        raise calibration_error("request", "must be a MarketDataCalibrationRequest")

    normalized_request = validate_market_data_calibration_request(request.to_payload())
    translation_result = translate_market_data_request_to_prepared_calibration(
        normalized_request,
        cpu_count=cpu_count,
    )

    return _prepare_market_data_calibration_from_translation_result(
        request=normalized_request,
        translation_result=translation_result,
    )


def _prepare_market_data_calibration_from_translation_result(
    *,
    request: MarketDataCalibrationRequest,
    translation_result: MarketDataTranslationResult,
) -> PreparedMarketDataCalibration:
    return PreparedMarketDataCalibration(
        schema_version=CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION,
        request=request,
        staged_request=translation_result.staged_request,
        diagnostics=_build_market_data_preparation_diagnostics(translation_result.diagnostics),
        provenance=_build_market_data_preparation_provenance(translation_result.provenance),
        session_resolution=_build_market_data_session_resolution(
            translation_result.session_resolution
        ),
        fit_warning=translation_result.fit_warning,
        reference_price=translation_result.reference_price,
        _prepared_handle=translation_result.prepared_handle,
    )


def run_market_data_calibration(request: MarketDataCalibrationRequest) -> StagedCalibrationResponse:
    if not isinstance(request, MarketDataCalibrationRequest):
        raise calibration_error("request", "must be a MarketDataCalibrationRequest")

    prepared = prepare_market_data_calibration(request)
    response, _ = run_prepared_staged_calibration_with_raw_payload(
        prepared.prepared_handle,
        calendar_registry=CalendarRegistry(
            prepared.session_resolution.resolved_session_definition_json
        ),
    )
    return response


__all__ = [
    "MarketDataCalibrationPreparationDiagnostics",
    "MarketDataCalibrationPreparationProvenance",
    "MarketDataCalibrationSourceSummary",
    "PreparedMarketDataCalibration",
    "prepare_market_data_calibration",
    "run_market_data_calibration",
]
