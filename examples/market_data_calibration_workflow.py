import json
from pathlib import Path
from tempfile import TemporaryDirectory

from janiform.calibration.market_data import (
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
    prepare_market_data_calibration,
)
from janiform.calibration.staged import CalibrationExecutionMode

from ._shared import write_market_data_fixture_files


def run_example(*, working_directory: Path) -> dict[str, object]:
    file_paths = write_market_data_fixture_files(working_directory)

    csv_request = _build_market_data_request(
        run_id="market-data-csv-primary",
        tick_data_path=file_paths["tick_csv"],
        tick_data_format=CalibrationInputFileFormat.CSV,
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        tick_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
            trade_price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
        ),
        known_news_path=file_paths["known_news"],
    )
    parquet_request = _build_market_data_request(
        run_id="market-data-parquet-primary",
        tick_data_path=file_paths["tick_parquet"],
        tick_data_format=CalibrationInputFileFormat.PARQUET,
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        tick_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
            trade_price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
        ),
        known_news_path=file_paths["known_news"],
    )
    option_surface_request = _build_market_data_request(
        run_id="market-data-option-surface-primary",
        tick_data_path=file_paths["tick_csv"],
        tick_data_format=CalibrationInputFileFormat.CSV,
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE,
        tick_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
            trade_price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
            option_quote_input_policy=CalibrationOptionQuoteInputPolicy.BID_ASK_MIDPOINT,
        ),
        known_news_path=file_paths["known_news"],
        option_surface_path=file_paths["option_surface"],
    )

    return {
        "tick_csv_primary": _summarize_market_data_workflow(csv_request),
        "tick_parquet_primary": _summarize_market_data_workflow(parquet_request),
        "tick_csv_with_option_surface": _summarize_market_data_workflow(option_surface_request),
    }


def _summarize_market_data_workflow(request: MarketDataCalibrationRequest) -> dict[str, object]:
    prepared = prepare_market_data_calibration(request)

    return {
        "preparation_schema_version": prepared.schema_version,
        "workflow_scope": prepared.request.workflow_scope.value,
        "tick_data_format": prepared.request.tick_data_source.file_format.value,
        "selected_stages": [stage.value for stage in prepared.diagnostics.selected_stages],
        "executed_preprocessing_steps": list(prepared.diagnostics.executed_preprocessing_steps),
        "tick_record_count": prepared.diagnostics.tick_record_count,
        "known_news_event_count": prepared.diagnostics.known_news_event_count,
        "option_surface_record_count": prepared.diagnostics.option_surface_record_count,
        "material_close_open_gap_count": prepared.diagnostics.material_close_open_gap_count,
        "derived_trade_events_data_reference": prepared.provenance.derived_trade_events_data_reference,
        "derived_option_surface_data_reference": prepared.provenance.derived_option_surface_data_reference,
        "prepared_execution_mode": prepared.execution_input.request.execution_mode.value,
        "prepared_stage_count": len(prepared.execution_input.request.selected_stages),
        "session_resolution_kind": prepared.session_resolution.resolution_kind,
    }


def _build_market_data_request(
    *,
    run_id: str,
    tick_data_path: Path,
    tick_data_format: CalibrationInputFileFormat,
    workflow_scope: CalibrationMarketDataWorkflowScope,
    tick_column_mapping: CalibrationTickDataColumnMapping,
    normalization_policy: CalibrationMarketDataNormalizationPolicy,
    known_news_path: Path,
    option_surface_path: Path | None = None,
) -> MarketDataCalibrationRequest:
    return MarketDataCalibrationRequest(
        schema_version="calibration.market_data.request.v1",
        run_id=run_id,
        tick_data_source=CalibrationInputFileReference(
            path=str(tick_data_path),
            file_format=tick_data_format,
            provenance_label="primary-tick-feed",
        ),
        session_configuration=CalibrationSessionConfigurationInput(
            exchange_code="XNYS",
        ),
        column_mapping=CalibrationMarketDataColumnMapping(
            tick_data=tick_column_mapping,
            option_surface=(
                CalibrationOptionSurfaceColumnMapping(
                    quote_timestamp_column="quote_timestamp_utc",
                    expiry_timestamp_column="expiry_utc",
                    strike_column="strike",
                    option_type_column="option_type",
                    bid_column="bid",
                    ask_column="ask",
                )
                if option_surface_path is not None
                else None
            ),
            known_news=CalibrationKnownNewsColumnMapping(
                event_timestamp_column="event_timestamp_utc",
                event_identifier_column="event_code",
            ),
        ),
        workflow_scope=workflow_scope,
        normalization_policy=normalization_policy,
        output_preferences=CalibrationMarketDataOutputPreferences(
            execution_mode=CalibrationExecutionMode.MEMORY,
            emit_derived_staged_execution_input=False,
            emit_input_provenance_report=True,
        ),
        option_surface_source=(
            CalibrationInputFileReference(
                path=str(option_surface_path),
                file_format=CalibrationInputFileFormat.PARQUET,
                provenance_label="options-close",
            )
            if option_surface_path is not None
            else None
        ),
        known_news_inputs=CalibrationKnownNewsInput(
            calendar_source=CalibrationInputFileReference(
                path=str(known_news_path),
                file_format=CalibrationInputFileFormat.CSV,
                provenance_label="known-news-calendar",
            ),
            calendar_timezone="UTC",
        ),
    )
def main() -> None:
    with TemporaryDirectory(prefix="janiform-market-data-example-") as working_directory:
        summary = run_example(working_directory=Path(working_directory))
        print(json.dumps(summary, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
