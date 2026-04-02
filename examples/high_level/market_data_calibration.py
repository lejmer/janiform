from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any

from janiform.calibration import (
    KnownNewsPresetReference,
    MarketProfileReference,
    PresetRegistry,
    SessionPresetReference,
    prepare_market_data_calibration,
    resolve_preset_workflow_configuration,
    run_market_data_calibration,
)
from janiform.calibration.market_data import (
    CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
    CalibrationInputFileFormat,
    CalibrationInputFileReference,
    CalibrationMarketDataColumnMapping,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataOutputPreferences,
    CalibrationMarketDataWorkflowScope,
    CalibrationSessionConfigurationInput,
    CalibrationTickDataColumnMapping,
    MarketDataCalibrationRequest,
    PreparedMarketDataCalibration,
)
from janiform.calibration.presets import (
    PresetReferenceUsageMode,
    PresetRegistryScope,
    ResolvedPresetWorkflowConfiguration,
)
from janiform.calibration.staged import (
    CalibrationExecutionMode,
    StagedCalibrationResponse,
)

from examples._shared import (
    SYNTHETIC_EURUSD_TICK_DATA_PATH,
    calendar_registry_payload,
    write_json_document,
)


def main() -> None:
    resolved_configuration: ResolvedPresetWorkflowConfiguration = resolve_preset_workflow_configuration(
        registry=PresetRegistry(),
        market_profile_reference=MarketProfileReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name="eurusd_spot",
        ),
        session_reference=SessionPresetReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name="XNYS",
        ),
        known_news_reference=KnownNewsPresetReference(
            usage_mode=PresetReferenceUsageMode.NAMED,
            registry_scope=PresetRegistryScope.BUILT_IN,
            preset_name="none",
        ),
    )

    with TemporaryDirectory(prefix="janiform-market-data-example-") as temp_directory:
        session_definition_path: Path = write_json_document(
            Path(temp_directory) / "calendar_registry.json",
            calendar_registry_payload(),
        )

        request: MarketDataCalibrationRequest = MarketDataCalibrationRequest(
            schema_version=CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
            run_id="example-market-data-calibration",
            tick_data_source=CalibrationInputFileReference(
                path=str(SYNTHETIC_EURUSD_TICK_DATA_PATH),
                file_format=CalibrationInputFileFormat.PARQUET,
                provenance_label="synthetic-eurusd-sample",
            ),
            session_configuration=CalibrationSessionConfigurationInput(
                exchange_code=resolved_configuration.session.configuration.exchange_code,
                session_definition_source=CalibrationInputFileReference(
                    path=str(session_definition_path),
                    file_format=CalibrationInputFileFormat.JSON,
                    provenance_label="calendar-registry-fixture",
                ),
                session_timezone=resolved_configuration.session.configuration.timezone_name,
            ),
            column_mapping=CalibrationMarketDataColumnMapping(
                tick_data=CalibrationTickDataColumnMapping(
                    timestamp_column="time",
                ),
            ),
            workflow_scope=CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY,
            normalization_policy=CalibrationMarketDataNormalizationPolicy(
                timestamp_timezone="UTC",
                sort_records_by_timestamp=True,
            ),
            output_preferences=CalibrationMarketDataOutputPreferences(
                execution_mode=CalibrationExecutionMode.MEMORY,
                emit_derived_staged_execution_input=False,
                emit_input_provenance_report=True,
            ),
        )

        prepared: PreparedMarketDataCalibration = prepare_market_data_calibration(request)
        response: StagedCalibrationResponse = run_market_data_calibration(request)

    resolved_preset_summary: dict[str, str] = {
        "market_profile": resolved_configuration.market_profile.selected_preset.preset_name,
        "session": resolved_configuration.session.selected_preset.preset_name,
        "known_news": resolved_configuration.known_news.selected_preset.preset_name,
    }
    preparation_summary: dict[str, Any] = {
        "selected_stages": [stage.value for stage in prepared.diagnostics.selected_stages],
        "tick_record_count": prepared.diagnostics.tick_record_count,
        "executed_preprocessing_steps": list(prepared.diagnostics.executed_preprocessing_steps),
    }
    run_summary: dict[str, Any] = {
        "status": response.status.value,
        "stage_reports": [report.stage.value for report in response.stage_reports],
    }

    print("resolved presets:", resolved_preset_summary)
    print("preparation summary:", preparation_summary)
    print("run summary:", run_summary)


if __name__ == "__main__":
    main()
