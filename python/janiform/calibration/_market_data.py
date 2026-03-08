from dataclasses import dataclass
from typing import Any, Optional

from ._common import (
    CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION,
    CalibrationInputFileFormat,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
    CalibrationStage,
    _calibration_error,
)
from ._requests import MarketDataCalibrationRequest, validate_market_data_calibration_request
from ._staged import StagedCalibrationExecutionInput, StagedCalibrationResponse, run_staged_calibration
from .._contracts import ContractModel

@dataclass(frozen=True, slots=True)
class MarketDataCalibrationSourceSummary(ContractModel):
    path: str
    file_format: CalibrationInputFileFormat
    provenance_label: str
    source_row_count: int
    retained_row_count: int
    dropped_duplicate_row_count: int
    sort_records_by_timestamp: bool
    applied_timestamp_timezone: Optional[str]
    canonical_value_field: Optional[str] = None
    value_mapping_policy: Optional[str] = None


@dataclass(frozen=True, slots=True)
class MarketDataCalibrationPreparationDiagnostics(ContractModel):
    workflow_scope: CalibrationMarketDataWorkflowScope
    selected_stages: tuple[CalibrationStage, ...]
    executed_preprocessing_steps: tuple[str, ...]
    tick_record_count: int
    option_surface_record_count: int
    known_news_event_count: int
    known_news_events_outside_source_window_count: int
    session_open_tick_count: int
    session_closed_tick_count: int
    known_news_open_event_count: int
    known_news_closed_event_count: int
    seasonality_overlay_count: int
    hawkes_candidate_count: int
    known_news_return_sample_count: int
    intraday_jump_candidate_count: int
    close_open_boundary_observation_count: int
    material_close_open_gap_count: int


@dataclass(frozen=True, slots=True)
class MarketDataCalibrationPreparationProvenance(ContractModel):
    tick_data_summary: MarketDataCalibrationSourceSummary
    option_surface_summary: Optional[MarketDataCalibrationSourceSummary]
    known_news_summary: Optional[MarketDataCalibrationSourceSummary]
    normalization_policy: CalibrationMarketDataNormalizationPolicy
    workflow_scope: CalibrationMarketDataWorkflowScope
    selected_stages: tuple[CalibrationStage, ...]
    session_definition_path: str
    derived_trade_events_data_reference: str
    derived_option_surface_data_reference: Optional[str]
    derivation_policies: tuple[str, ...]
    deterministic_defaults_applied: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class PreparedMarketDataCalibration(ContractModel):
    schema_version: str
    request: MarketDataCalibrationRequest
    execution_input: StagedCalibrationExecutionInput
    diagnostics: MarketDataCalibrationPreparationDiagnostics
    provenance: MarketDataCalibrationPreparationProvenance

    def __post_init__(self) -> None:
        if self.schema_version != CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION:
            raise _calibration_error(
                "schema_version",
                f"must be `{CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION}`",
            )


def _translate_market_data_calibration_request(
    request: MarketDataCalibrationRequest,
) -> Any:
    from ._market_data_translation import translate_market_data_request_to_staged_execution_input

    return translate_market_data_request_to_staged_execution_input(request)


def _build_market_data_source_summary(source_summary: Any) -> MarketDataCalibrationSourceSummary:
    return MarketDataCalibrationSourceSummary(
        path=source_summary.path,
        file_format=source_summary.file_format,
        provenance_label=source_summary.provenance_label,
        source_row_count=source_summary.source_row_count,
        retained_row_count=source_summary.retained_row_count,
        dropped_duplicate_row_count=source_summary.dropped_duplicate_row_count,
        sort_records_by_timestamp=source_summary.sort_records_by_timestamp,
        applied_timestamp_timezone=source_summary.applied_timestamp_timezone,
        canonical_value_field=source_summary.canonical_value_field,
        value_mapping_policy=source_summary.value_mapping_policy,
    )


def _build_market_data_preparation_diagnostics(diagnostics: Any) -> MarketDataCalibrationPreparationDiagnostics:
    return MarketDataCalibrationPreparationDiagnostics(
        workflow_scope=diagnostics.workflow_scope,
        selected_stages=diagnostics.selected_stages,
        executed_preprocessing_steps=diagnostics.executed_preprocessing_steps,
        tick_record_count=diagnostics.tick_record_count,
        option_surface_record_count=diagnostics.option_surface_record_count,
        known_news_event_count=diagnostics.known_news_event_count,
        known_news_events_outside_source_window_count=diagnostics.known_news_events_outside_source_window_count,
        session_open_tick_count=diagnostics.session_open_tick_count,
        session_closed_tick_count=diagnostics.session_closed_tick_count,
        known_news_open_event_count=diagnostics.known_news_open_event_count,
        known_news_closed_event_count=diagnostics.known_news_closed_event_count,
        seasonality_overlay_count=diagnostics.seasonality_overlay_count,
        hawkes_candidate_count=diagnostics.hawkes_candidate_count,
        known_news_return_sample_count=diagnostics.known_news_return_sample_count,
        intraday_jump_candidate_count=diagnostics.intraday_jump_candidate_count,
        close_open_boundary_observation_count=diagnostics.close_open_boundary_observation_count,
        material_close_open_gap_count=diagnostics.material_close_open_gap_count,
    )


def _build_market_data_preparation_provenance(provenance: Any) -> MarketDataCalibrationPreparationProvenance:
    return MarketDataCalibrationPreparationProvenance(
        tick_data_summary=_build_market_data_source_summary(provenance.tick_data_summary),
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
        normalization_policy=provenance.normalization_policy,
        workflow_scope=provenance.workflow_scope,
        selected_stages=provenance.selected_stages,
        session_definition_path=provenance.session_definition_path,
        derived_trade_events_data_reference=provenance.derived_trade_events_data_reference,
        derived_option_surface_data_reference=provenance.derived_option_surface_data_reference,
        derivation_policies=provenance.derivation_policies,
        deterministic_defaults_applied=provenance.deterministic_defaults_applied,
    )


def prepare_market_data_calibration(
    request: MarketDataCalibrationRequest,
) -> PreparedMarketDataCalibration:
    """Translate a market-data request into the canonical staged calibration input.

    Example:
        prepared = prepare_market_data_calibration(request)
        response = run_staged_calibration(
            prepared.execution_input,
            calendar_registry=CalendarRegistry(
                Path(request.session_configuration.session_definition_source.path).read_text(encoding="utf-8")
            ),
        )
    """

    if not isinstance(request, MarketDataCalibrationRequest):
        raise _calibration_error("request", "must be a MarketDataCalibrationRequest")

    normalized_request = validate_market_data_calibration_request(request.to_payload())
    translation_result = _translate_market_data_calibration_request(normalized_request)

    return PreparedMarketDataCalibration(
        schema_version=CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION,
        request=normalized_request,
        execution_input=translation_result.execution_input,
        diagnostics=_build_market_data_preparation_diagnostics(translation_result.diagnostics),
        provenance=_build_market_data_preparation_provenance(translation_result.provenance),
    )


def run_market_data_calibration(
    request: MarketDataCalibrationRequest,
) -> StagedCalibrationResponse:
    """Run the market-data-first calibration workflow through the staged engine."""

    if not isinstance(request, MarketDataCalibrationRequest):
        raise _calibration_error("request", "must be a MarketDataCalibrationRequest")

    normalized_request = validate_market_data_calibration_request(request.to_payload())
    translation_result = _translate_market_data_calibration_request(normalized_request)

    return run_staged_calibration(
        translation_result.execution_input,
        calendar_registry=translation_result.calendar_registry,
    )
