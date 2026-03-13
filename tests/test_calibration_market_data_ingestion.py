from datetime import UTC, datetime
from pathlib import Path

import pyarrow as pa
import pytest
from pyarrow import parquet as pyarrow_parquet

from janiform.calibration.market_data.ingestion import load_market_data_calibration_inputs
from janiform.calibration import CalibrationValidationError
from janiform.calibration.market_data import (
    CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
    CalibrationDuplicateRecordPolicy,
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
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
    MarketDataCalibrationRequest,
)
from janiform.calibration.staged import CalibrationExecutionMode


def test_load_market_data_calibration_inputs_normalizes_csv_tick_midpoints_and_provenance(
    tmp_path: Path,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,bid,ask",
                "2026-01-02T14:31:00Z,100.0,102.0",
                "2026-01-02T14:30:00Z,99.0,101.0",
                "2026-01-02T14:31:00Z,100.0,102.0",
            ]
        ),
        encoding="utf-8",
    )

    request = _build_market_data_request(
        tick_data_source=CalibrationInputFileReference(
            path=str(tick_data_path),
            file_format=CalibrationInputFileFormat.CSV,
            provenance_label="ticks-primary",
        ),
        column_mapping=CalibrationMarketDataColumnMapping(
            tick_data=CalibrationTickDataColumnMapping(
                timestamp_column="timestamp_utc",
                bid_column="bid",
                ask_column="ask",
            ),
            known_news=None,
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.KEEP_FIRST,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.BID_ASK_MIDPOINT,
        ),
    )

    loaded_inputs = load_market_data_calibration_inputs(request)

    assert [record.timestamp for record in loaded_inputs.tick_data.records] == [
        datetime(2026, 1, 2, 14, 30, tzinfo=UTC),
        datetime(2026, 1, 2, 14, 31, tzinfo=UTC),
    ]
    assert [record.price for record in loaded_inputs.tick_data.records] == pytest.approx([100.0, 101.0])
    assert loaded_inputs.tick_data.records[1].source_row_number == 1
    assert loaded_inputs.tick_data.summary.path == str(tick_data_path)
    assert loaded_inputs.tick_data.summary.file_format == CalibrationInputFileFormat.CSV
    assert loaded_inputs.tick_data.summary.provenance_label == "ticks-primary"
    assert loaded_inputs.tick_data.summary.source_row_count == 3
    assert loaded_inputs.tick_data.summary.retained_row_count == 2
    assert loaded_inputs.tick_data.summary.dropped_duplicate_row_count == 1
    assert loaded_inputs.tick_data.summary.canonical_value_field == "price"
    assert loaded_inputs.tick_data.summary.value_mapping_policy == "bid_ask_midpoint"


def test_load_market_data_calibration_inputs_normalizes_parquet_option_surface_quotes(
    tmp_path: Path,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price",
                "2026-01-02T14:30:00Z,100.5",
            ]
        ),
        encoding="utf-8",
    )
    option_surface_path = tmp_path / "options.parquet"
    option_surface_table = pa.table(
        {
            "quote_timestamp_utc": [datetime(2026, 1, 2, 14, 30, tzinfo=UTC)],
            "expiry_utc": [datetime(2026, 2, 20, 21, 0, tzinfo=UTC)],
            "strike": [102.0],
            "option_type": ["C"],
            "bid": [3.5],
            "ask": [4.5],
        }
    )
    pyarrow_parquet.write_table(option_surface_table, option_surface_path)

    request = _build_market_data_request(
        tick_data_source=CalibrationInputFileReference(
            path=str(tick_data_path),
            file_format=CalibrationInputFileFormat.CSV,
            provenance_label="ticks-primary",
        ),
        option_surface_source=CalibrationInputFileReference(
            path=str(option_surface_path),
            file_format=CalibrationInputFileFormat.PARQUET,
            provenance_label="options-close",
        ),
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE,
        column_mapping=CalibrationMarketDataColumnMapping(
            tick_data=CalibrationTickDataColumnMapping(
                timestamp_column="timestamp_utc",
                trade_price_column="trade_price",
            ),
            option_surface=CalibrationOptionSurfaceColumnMapping(
                quote_timestamp_column="quote_timestamp_utc",
                expiry_timestamp_column="expiry_utc",
                strike_column="strike",
                option_type_column="option_type",
                bid_column="bid",
                ask_column="ask",
            ),
            known_news=None,
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
            option_quote_input_policy=CalibrationOptionQuoteInputPolicy.BID_ASK_MIDPOINT,
        ),
    )

    loaded_inputs = load_market_data_calibration_inputs(request)

    assert loaded_inputs.option_surface is not None
    assert len(loaded_inputs.option_surface.records) == 1
    assert loaded_inputs.option_surface.records[0].option_type == "call"
    assert loaded_inputs.option_surface.records[0].quote_value == pytest.approx(4.0)
    assert loaded_inputs.option_surface.summary.file_format == CalibrationInputFileFormat.PARQUET
    assert loaded_inputs.option_surface.summary.retained_row_count == 1


def test_load_market_data_calibration_inputs_supports_close_price_only_tick_inputs(
    tmp_path: Path,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,close_price",
                "2026-01-02T14:30:00Z,101.25",
            ]
        ),
        encoding="utf-8",
    )

    request = _build_market_data_request(
        tick_data_source=CalibrationInputFileReference(
            path=str(tick_data_path),
            file_format=CalibrationInputFileFormat.CSV,
            provenance_label="close-only",
        ),
        column_mapping=CalibrationMarketDataColumnMapping(
            tick_data=CalibrationTickDataColumnMapping(
                timestamp_column="timestamp_utc",
                close_price_column="close_price",
            ),
            known_news=None,
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.CLOSE_PRICE_COLUMN,
        ),
    )

    loaded_inputs = load_market_data_calibration_inputs(request)

    assert [record.price for record in loaded_inputs.tick_data.records] == pytest.approx([101.25])
    assert loaded_inputs.tick_data.summary.value_mapping_policy == "close_price_column"


def test_load_market_data_calibration_inputs_rejects_malformed_tick_timestamp(
    tmp_path: Path,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price",
                "not-a-timestamp,101.25",
            ]
        ),
        encoding="utf-8",
    )

    request = _build_market_data_request(
        tick_data_source=CalibrationInputFileReference(
            path=str(tick_data_path),
            file_format=CalibrationInputFileFormat.CSV,
            provenance_label="ticks-primary",
        ),
        column_mapping=CalibrationMarketDataColumnMapping(
            tick_data=CalibrationTickDataColumnMapping(
                timestamp_column="timestamp_utc",
                trade_price_column="trade_price",
            ),
            known_news=None,
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
        ),
    )

    with pytest.raises(CalibrationValidationError) as captured_error:
        load_market_data_calibration_inputs(request)

    assert captured_error.value.failure.error_id == "calibration.validation.market_data_ingestion"
    assert captured_error.value.failure.field_path == "tick_data_source.rows[1].timestamp_utc"
    assert "supported timestamp string" in captured_error.value.failure.message
    assert captured_error.value.failure.context == {
        "normalization_step": "timestamp_normalization",
        "raw_value": "not-a-timestamp",
        "source_path": str(tick_data_path),
    }


def test_load_market_data_calibration_inputs_accepts_common_day_first_timestamp_strings(
    tmp_path: Path,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_local,trade_price",
                "21.03.2023 19:00:00.154,1.07649",
                "21.03.2023 19:00:00.257,1.076485",
            ]
        ),
        encoding="utf-8",
    )

    request = _build_market_data_request(
        tick_data_source=CalibrationInputFileReference(
            path=str(tick_data_path),
            file_format=CalibrationInputFileFormat.CSV,
            provenance_label="ticks-primary",
        ),
        column_mapping=CalibrationMarketDataColumnMapping(
            tick_data=CalibrationTickDataColumnMapping(
                timestamp_column="timestamp_local",
                trade_price_column="trade_price",
            ),
            known_news=None,
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
        ),
    )

    loaded_inputs = load_market_data_calibration_inputs(request)

    assert [record.timestamp for record in loaded_inputs.tick_data.records] == [
        datetime(2023, 3, 21, 19, 0, 0, 154000, tzinfo=UTC),
        datetime(2023, 3, 21, 19, 0, 0, 257000, tzinfo=UTC),
    ]


def test_load_market_data_calibration_inputs_accepts_unix_epoch_millisecond_strings(
    tmp_path: Path,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_epoch_millis,trade_price",
                "1704205800000,100.5",
                "1704205860000,100.75",
            ]
        ),
        encoding="utf-8",
    )

    request = _build_market_data_request(
        tick_data_source=CalibrationInputFileReference(
            path=str(tick_data_path),
            file_format=CalibrationInputFileFormat.CSV,
            provenance_label="ticks-primary",
        ),
        column_mapping=CalibrationMarketDataColumnMapping(
            tick_data=CalibrationTickDataColumnMapping(
                timestamp_column="timestamp_epoch_millis",
                trade_price_column="trade_price",
            ),
            known_news=None,
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
        ),
    )

    loaded_inputs = load_market_data_calibration_inputs(request)

    assert [record.timestamp for record in loaded_inputs.tick_data.records] == [
        datetime(2024, 1, 2, 14, 30, tzinfo=UTC),
        datetime(2024, 1, 2, 14, 31, tzinfo=UTC),
    ]


def test_load_market_data_calibration_inputs_keep_last_duplicate_policy_uses_last_source_row(
    tmp_path: Path,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price",
                "2026-01-02T14:30:00Z,100.0",
                "2026-01-02T14:31:00Z,101.0",
                "2026-01-02T14:30:00Z,100.0",
            ]
        ),
        encoding="utf-8",
    )

    request = _build_market_data_request(
        tick_data_source=CalibrationInputFileReference(
            path=str(tick_data_path),
            file_format=CalibrationInputFileFormat.CSV,
            provenance_label="ticks-primary",
        ),
        column_mapping=CalibrationMarketDataColumnMapping(
            tick_data=CalibrationTickDataColumnMapping(
                timestamp_column="timestamp_utc",
                trade_price_column="trade_price",
            ),
            known_news=None,
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=False,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.KEEP_LAST,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
        ),
    )

    loaded_inputs = load_market_data_calibration_inputs(request)

    assert [record.source_row_number for record in loaded_inputs.tick_data.records] == [2, 3]
    assert loaded_inputs.tick_data.summary.dropped_duplicate_row_count == 1


def test_load_market_data_calibration_inputs_rejects_missing_mapped_columns_with_actionable_guidance(
    tmp_path: Path,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,last_trade",
                "2026-01-02T14:30:00Z,100.0",
            ]
        ),
        encoding="utf-8",
    )

    request = _build_market_data_request(
        tick_data_source=CalibrationInputFileReference(
            path=str(tick_data_path),
            file_format=CalibrationInputFileFormat.CSV,
            provenance_label="ticks-primary",
        ),
        column_mapping=CalibrationMarketDataColumnMapping(
            tick_data=CalibrationTickDataColumnMapping(
                timestamp_column="timestamp_utc",
                trade_price_column="trade_price",
            ),
            known_news=None,
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
        ),
    )

    with pytest.raises(CalibrationValidationError) as captured_error:
        load_market_data_calibration_inputs(request)

    assert captured_error.value.failure.field_path == "column_mapping.tick_data"
    assert "Missing columns: trade_price" in captured_error.value.failure.message
    assert "Update the explicit column mapping" in captured_error.value.failure.message
    assert captured_error.value.failure.context == {
        "normalization_step": "column_mapping_validation",
        "source_path": str(tick_data_path),
    }


def test_load_market_data_calibration_inputs_normalizes_known_news_parquet_sources(
    tmp_path: Path,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price",
                "2026-01-02T14:30:00Z,100.5",
            ]
        ),
        encoding="utf-8",
    )
    known_news_path = tmp_path / "known_news.parquet"
    known_news_table = pa.table(
        {
            "event_timestamp_utc": [datetime(2026, 1, 2, 13, 0, tzinfo=UTC)],
            "event_code": ["NFP"],
        }
    )
    pyarrow_parquet.write_table(known_news_table, known_news_path)

    request = _build_market_data_request(
        tick_data_source=CalibrationInputFileReference(
            path=str(tick_data_path),
            file_format=CalibrationInputFileFormat.CSV,
            provenance_label="ticks-primary",
        ),
        known_news_inputs=CalibrationKnownNewsInput(
            calendar_source=CalibrationInputFileReference(
                path=str(known_news_path),
                file_format=CalibrationInputFileFormat.PARQUET,
                provenance_label="known-news",
            ),
            calendar_timezone="UTC",
        ),
        column_mapping=CalibrationMarketDataColumnMapping(
            tick_data=CalibrationTickDataColumnMapping(
                timestamp_column="timestamp_utc",
                trade_price_column="trade_price",
            ),
            known_news=CalibrationKnownNewsColumnMapping(
                event_timestamp_column="event_timestamp_utc",
                event_identifier_column="event_code",
            ),
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
        ),
    )

    loaded_inputs = load_market_data_calibration_inputs(request)

    assert loaded_inputs.known_news is not None
    assert loaded_inputs.known_news.records[0].event_identifier == "NFP"
    assert loaded_inputs.known_news.summary.file_format == CalibrationInputFileFormat.PARQUET


def _build_market_data_request(
    *,
    tick_data_source: CalibrationInputFileReference,
    column_mapping: CalibrationMarketDataColumnMapping,
    normalization_policy: CalibrationMarketDataNormalizationPolicy,
    workflow_scope: CalibrationMarketDataWorkflowScope = CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
    option_surface_source: CalibrationInputFileReference | None = None,
    known_news_inputs: CalibrationKnownNewsInput | None = None,
) -> MarketDataCalibrationRequest:
    return MarketDataCalibrationRequest(
        schema_version=CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
        run_id="market-data-run-001",
        tick_data_source=tick_data_source,
        session_configuration=CalibrationSessionConfigurationInput(
            exchange_code="XNYS",
            session_definition_source=CalibrationInputFileReference(
                path="/tmp/sessions.json",
                file_format=CalibrationInputFileFormat.JSON,
                provenance_label="session-calendar",
            ),
            session_timezone="America/New_York",
        ),
        column_mapping=column_mapping,
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
