from datetime import UTC, datetime
from pathlib import Path

import pyarrow as pa
import pytest
from pyarrow import parquet as pyarrow_parquet

from janiform.calibration._market_data_translation import (
    translate_market_data_request_to_staged_execution_input,
)
from janiform.calibration import (
    CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION,
    CalibrationDuplicateRecordPolicy,
    CalibrationExecutionMode,
    CalibrationInputFileFormat,
    CalibrationInputFileReference,
    CalibrationKnownNewsColumnMapping,
    CalibrationKnownNewsInput,
    CalibrationMarketDataColumnMapping,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataOutputPreferences,
    CalibrationMarketDataWorkflowScope,
    CalibrationOptionQuoteInputPolicy,
    CalibrationOptionSurfaceColumnMapping,
    CalibrationSessionConfigurationInput,
    CalibrationStage,
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
    MarketDataCalibrationRequest,
    prepare_market_data_calibration,
    run_staged_calibration,
    run_market_data_calibration,
)


def test_translate_market_data_request_seasonality_only_selects_only_seasonality_stage(
    tmp_path: Path,
    calendar_registry_json: str,
) -> None:
    session_definition_path = _write_session_definition(tmp_path, calendar_registry_json)

    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc",
                "2026-02-02T14:30:00Z",
                "2026-02-02T15:00:00Z",
                "2026-02-02T20:59:00Z",
            ]
        ),
        encoding="utf-8",
    )

    request = _build_market_data_request(
        session_definition_path=session_definition_path,
        tick_data_path=tick_data_path,
        workflow_scope=CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY,
        tick_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=None,
        ),
    )

    translation = translate_market_data_request_to_staged_execution_input(request)

    assert translation.execution_input.request.selected_stages == (
        CalibrationStage.SEASONALITY,
    )
    assert translation.execution_input.seasonality_stage_input is not None
    assert translation.execution_input.hawkes_candidate_inputs is None
    assert translation.execution_input.return_components_candidate_generation_input is None
    assert translation.diagnostics.tick_record_count == 3


def test_translate_market_data_request_derives_boundaries_known_news_and_diagnostics(
    tmp_path: Path,
    calendar_registry_json: str,
) -> None:
    session_definition_path = _write_session_definition(tmp_path, calendar_registry_json)

    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price",
                "2026-02-02T14:30:00Z,100.0",
                "2026-02-02T15:00:00Z,101.0",
                "2026-02-02T15:10:00Z,104.0",
                "2026-02-02T20:59:00Z,102.0",
                "2026-02-04T14:30:00Z,90.0",
                "2026-02-04T14:40:00Z,100.0",
            ]
        ),
        encoding="utf-8",
    )

    known_news_path = tmp_path / "known_news.csv"
    known_news_path.write_text(
        "\n".join(
            [
                "event_timestamp_utc,event_code",
                "2026-02-02T15:05:00Z,open_macro",
                "2026-02-02T22:00:00Z,closed_macro",
                "2026-02-10T15:00:00Z,outside_window",
            ]
        ),
        encoding="utf-8",
    )

    request = _build_market_data_request(
        session_definition_path=session_definition_path,
        tick_data_path=tick_data_path,
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        tick_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
            trade_price_column="trade_price",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
        ),
        known_news_inputs=CalibrationKnownNewsInput(
            calendar_source=CalibrationInputFileReference(
                path=str(known_news_path),
                file_format=CalibrationInputFileFormat.CSV,
                provenance_label="known-news",
            ),
            calendar_timezone="UTC",
        ),
        known_news_column_mapping=CalibrationKnownNewsColumnMapping(
            event_timestamp_column="event_timestamp_utc",
            event_identifier_column="event_code",
        ),
    )

    first_translation = translate_market_data_request_to_staged_execution_input(request)
    second_translation = translate_market_data_request_to_staged_execution_input(request)

    assert first_translation.execution_input.to_payload() == second_translation.execution_input.to_payload()
    assert first_translation.diagnostics == second_translation.diagnostics
    assert first_translation.execution_input.request.selected_stages == (
        CalibrationStage.SEASONALITY,
        CalibrationStage.HAWKES,
        CalibrationStage.RETURN_COMPONENTS,
    )
    assert first_translation.diagnostics.known_news_event_count == 2
    assert first_translation.diagnostics.known_news_events_outside_source_window_count == 1
    assert first_translation.diagnostics.seasonality_overlay_count == 2
    assert first_translation.diagnostics.close_open_boundary_observation_count == 1
    assert first_translation.diagnostics.material_close_open_gap_count == 1

    return_components_input = (
        first_translation.execution_input.return_components_candidate_generation_input
    )
    assert return_components_input is not None
    assert len(return_components_input.realized_known_news_return_samples) == 2

    open_sample = return_components_input.realized_known_news_return_samples[0]
    closed_sample = return_components_input.realized_known_news_return_samples[1]
    assert open_sample.event_timestamp_utc_nanos == _utc_nanos("2026-02-02T15:05:00Z")
    assert open_sample.open_window_jump_return == pytest.approx(0.0292703823)
    assert open_sample.closed_window_gap_return == pytest.approx(0.0)
    assert closed_sample.event_timestamp_utc_nanos == _utc_nanos("2026-02-02T22:00:00Z")
    assert closed_sample.open_window_jump_return == pytest.approx(0.0)
    assert closed_sample.closed_window_gap_return == pytest.approx(-0.12516314295)

    boundary_observation = return_components_input.observed_close_open_boundary_observations[0]
    assert boundary_observation.close_timestamp_utc_nanos == _utc_nanos("2026-02-02T21:00:00Z")
    assert boundary_observation.open_timestamp_utc_nanos == _utc_nanos("2026-02-04T14:30:00Z")
    assert boundary_observation.close_price == pytest.approx(102.0)
    assert boundary_observation.open_price == pytest.approx(90.0)


def test_translate_market_data_request_without_known_news_runs_staged_calibration(
    tmp_path: Path,
    calendar_registry_json: str,
) -> None:
    session_definition_path = _write_session_definition(tmp_path, calendar_registry_json)

    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price",
                "2026-02-02T14:30:00Z,100.0",
                "2026-02-02T15:00:00Z,101.0",
                "2026-02-02T15:10:00Z,103.0",
                "2026-02-02T20:59:00Z,102.0",
                "2026-02-04T14:30:00Z,95.0",
                "2026-02-04T14:40:00Z,98.0",
            ]
        ),
        encoding="utf-8",
    )

    request = _build_market_data_request(
        session_definition_path=session_definition_path,
        tick_data_path=tick_data_path,
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        tick_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
            trade_price_column="trade_price",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
        ),
    )

    translation = translate_market_data_request_to_staged_execution_input(request)
    return_components_input = (
        translation.execution_input.return_components_candidate_generation_input
    )
    assert return_components_input is not None
    assert return_components_input.realized_known_news_return_samples == ()

    response = run_staged_calibration(
        translation.execution_input,
        calendar_registry=translation.calendar_registry,
    )
    assert response.execution_mode == CalibrationExecutionMode.MEMORY
    assert response.status.value == "success"


def test_prepare_market_data_calibration_returns_public_diagnostics_and_provenance(
    tmp_path: Path,
    calendar_registry_json: str,
) -> None:
    session_definition_path = _write_session_definition(tmp_path, calendar_registry_json)

    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price",
                "2026-02-02T14:30:00Z,100.0",
                "2026-02-02T15:00:00Z,101.0",
                "2026-02-02T15:10:00Z,104.0",
                "2026-02-02T20:59:00Z,102.0",
                "2026-02-04T14:30:00Z,90.0",
                "2026-02-04T14:40:00Z,100.0",
            ]
        ),
        encoding="utf-8",
    )
    known_news_path = tmp_path / "known_news.csv"
    known_news_path.write_text(
        "\n".join(
            [
                "event_timestamp_utc,event_code",
                "2026-02-02T15:05:00Z,open_macro",
                "2026-02-02T22:00:00Z,closed_macro",
                "2026-02-10T15:00:00Z,outside_window",
            ]
        ),
        encoding="utf-8",
    )

    request = _build_market_data_request(
        session_definition_path=session_definition_path,
        tick_data_path=tick_data_path,
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        tick_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
            trade_price_column="trade_price",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
        ),
        known_news_inputs=CalibrationKnownNewsInput(
            calendar_source=CalibrationInputFileReference(
                path=str(known_news_path),
                file_format=CalibrationInputFileFormat.CSV,
                provenance_label="known-news",
            ),
            calendar_timezone="UTC",
        ),
        known_news_column_mapping=CalibrationKnownNewsColumnMapping(
            event_timestamp_column="event_timestamp_utc",
            event_identifier_column="event_code",
        ),
    )

    prepared = prepare_market_data_calibration(request)

    assert prepared.schema_version == CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION
    assert prepared.request.run_id == request.run_id
    assert prepared.execution_input.request.execution_mode == CalibrationExecutionMode.MEMORY
    assert prepared.diagnostics.selected_stages == (
        CalibrationStage.SEASONALITY,
        CalibrationStage.HAWKES,
        CalibrationStage.RETURN_COMPONENTS,
    )
    assert prepared.diagnostics.executed_preprocessing_steps == (
        "source_loading",
        "column_mapping_validation",
        "timestamp_timezone_normalization",
        "duplicate_record_resolution",
        "timestamp_sorting",
        "session_state_resolution",
        "workflow_scope_selection",
        "known_news_window_filtering",
        "known_news_overlay_derivation",
        "hawkes_candidate_generation",
        "return_components_candidate_generation",
    )
    assert prepared.diagnostics.known_news_event_count == 2
    assert prepared.diagnostics.known_news_events_outside_source_window_count == 1
    assert prepared.provenance.tick_data_summary.file_format == CalibrationInputFileFormat.CSV
    assert prepared.provenance.known_news_summary is not None
    assert prepared.provenance.derived_trade_events_data_reference == (
        "market_data://market_data_translation_run/ticks/ticks_primary"
    )


def test_run_market_data_calibration_executes_market_data_first_workflow(
    tmp_path: Path,
    calendar_registry_json: str,
) -> None:
    session_definition_path = _write_session_definition(tmp_path, calendar_registry_json)

    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price",
                "2026-02-02T14:30:00Z,100.0",
                "2026-02-02T15:00:00Z,101.0",
                "2026-02-02T15:10:00Z,103.0",
                "2026-02-02T20:59:00Z,102.0",
                "2026-02-04T14:30:00Z,95.0",
                "2026-02-04T14:40:00Z,98.0",
            ]
        ),
        encoding="utf-8",
    )

    request = _build_market_data_request(
        session_definition_path=session_definition_path,
        tick_data_path=tick_data_path,
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        tick_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
            trade_price_column="trade_price",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
        ),
    )

    response = run_market_data_calibration(request)

    assert response.execution_mode == CalibrationExecutionMode.MEMORY
    assert response.status.value == "success"
    assert len(response.stage_reports) == 3


def test_translate_market_data_request_full_with_option_surface_sets_option_reference(
    tmp_path: Path,
    calendar_registry_json: str,
) -> None:
    session_definition_path = _write_session_definition(tmp_path, calendar_registry_json)

    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price",
                "2026-02-02T14:30:00Z,100.0",
                "2026-02-02T15:00:00Z,101.0",
                "2026-02-02T15:10:00Z,103.0",
                "2026-02-02T20:59:00Z,102.0",
                "2026-02-04T14:30:00Z,95.0",
                "2026-02-04T14:40:00Z,99.0",
            ]
        ),
        encoding="utf-8",
    )
    option_surface_path = tmp_path / "options.parquet"

    pyarrow_parquet.write_table(
        pa.table(
            {
                "quote_timestamp_utc": ["2026-02-02T20:00:00Z"],
                "expiry_utc": ["2026-03-20T20:00:00Z"],
                "strike": [100.0],
                "option_type": ["C"],
                "bid": [4.0],
                "ask": [5.0],
            }
        ),
        option_surface_path,
    )

    request = _build_market_data_request(
        session_definition_path=session_definition_path,
        tick_data_path=tick_data_path,
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE,
        tick_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
            trade_price_column="trade_price",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
            option_quote_input_policy=CalibrationOptionQuoteInputPolicy.BID_ASK_MIDPOINT,
        ),
        option_surface_source=CalibrationInputFileReference(
            path=str(option_surface_path),
            file_format=CalibrationInputFileFormat.PARQUET,
            provenance_label="options-close",
        ),
        option_surface_column_mapping=CalibrationOptionSurfaceColumnMapping(
            quote_timestamp_column="quote_timestamp_utc",
            expiry_timestamp_column="expiry_utc",
            strike_column="strike",
            option_type_column="option_type",
            bid_column="bid",
            ask_column="ask",
        ),
    )

    translation = translate_market_data_request_to_staged_execution_input(request)

    assert translation.diagnostics.option_surface_record_count == 1
    assert translation.execution_input.request.data_inputs.option_surface_data_reference is not None


def _build_market_data_request(
    *,
    session_definition_path: Path,
    tick_data_path: Path,
    workflow_scope: CalibrationMarketDataWorkflowScope,
    tick_column_mapping: CalibrationTickDataColumnMapping,
    normalization_policy: CalibrationMarketDataNormalizationPolicy,
    known_news_inputs: CalibrationKnownNewsInput | None = None,
    known_news_column_mapping: CalibrationKnownNewsColumnMapping | None = None,
    option_surface_source: CalibrationInputFileReference | None = None,
    option_surface_column_mapping: CalibrationOptionSurfaceColumnMapping | None = None,
) -> MarketDataCalibrationRequest:
    return MarketDataCalibrationRequest(
        schema_version="calibration.market_data.request.v1",
        run_id="market-data-translation-run",
        tick_data_source=CalibrationInputFileReference(
            path=str(tick_data_path),
            file_format=CalibrationInputFileFormat.CSV,
            provenance_label="ticks-primary",
        ),
        session_configuration=CalibrationSessionConfigurationInput(
            exchange_code="XNYS",
            session_definition_source=CalibrationInputFileReference(
                path=str(session_definition_path),
                file_format=CalibrationInputFileFormat.JSON,
                provenance_label="session-registry",
            ),
            session_timezone="America/New_York",
        ),
        column_mapping=CalibrationMarketDataColumnMapping(
            tick_data=tick_column_mapping,
            option_surface=option_surface_column_mapping,
            known_news=known_news_column_mapping,
        ),
        workflow_scope=workflow_scope,
        normalization_policy=normalization_policy,
        output_preferences=CalibrationMarketDataOutputPreferences(
            execution_mode=CalibrationExecutionMode.MEMORY,
            emit_derived_staged_execution_input=False,
            emit_input_provenance_report=True,
        ),
        option_surface_source=option_surface_source,
        known_news_inputs=known_news_inputs,
    )


def _write_session_definition(tmp_path: Path, calendar_registry_json: str) -> Path:
    session_definition_path = tmp_path / "calendar_registry.json"
    session_definition_path.write_text(calendar_registry_json, encoding="utf-8")

    return session_definition_path


def _utc_nanos(timestamp_text: str) -> int:
    timestamp_utc = datetime.fromisoformat(timestamp_text.replace("Z", "+00:00")).astimezone(UTC)
    delta = timestamp_utc - datetime(1970, 1, 1, tzinfo=UTC)

    return (
        ((delta.days * 24 * 60 * 60) + delta.seconds) * 1_000_000_000
        + delta.microseconds * 1_000
    )
