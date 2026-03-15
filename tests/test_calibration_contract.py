import json
from typing import Any

import orjson
import pytest
from tests.calibration_test_support import (
    build_valid_execution_input_model,
    build_valid_execution_input_payload,
)

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
    deserialize_market_data_calibration_request,
    serialize_market_data_calibration_request,
    validate_market_data_calibration_request,
)
from janiform.calibration.staged import (
    CalibrationExecutionMode,
    CalibrationOptimizerSettings,
    CalibrationRunDataInputs,
    CalibrationRunSeedSettings,
    CalibrationStage,
    CalibrationStageExecutionStatus,
    CalibrationStageSeedOverride,
    CalibrationStageSolverSettings,
    StagedCalibrationRequest,
    deserialize_staged_calibration_execution_input,
    deserialize_staged_calibration_request,
    deserialize_staged_calibration_response,
    serialize_staged_calibration_execution_input,
    serialize_staged_calibration_request,
    serialize_staged_calibration_response,
    validate_staged_calibration_execution_input,
    validate_staged_calibration_request,
    validate_staged_calibration_response,
)


def test_market_data_calibration_request_defaults_python_execution_mode_to_memory() -> None:
    request_payload = _valid_market_data_request_payload()
    del request_payload["output_preferences"]["execution_mode"]

    validated_request = validate_market_data_calibration_request(request_payload)

    assert validated_request.output_preferences.execution_mode == CalibrationExecutionMode.MEMORY


def test_market_data_calibration_request_roundtrip_serialization_is_deterministic() -> None:
    request = _valid_market_data_request_model()

    serialized_request = serialize_market_data_calibration_request(request)
    deserialized_request = deserialize_market_data_calibration_request(serialized_request)

    assert deserialized_request == request
    assert orjson.loads(serialized_request)["schema_version"] == CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION


def test_market_data_calibration_request_requires_option_surface_for_option_scope() -> None:
    request_payload = _valid_market_data_request_payload()
    request_payload["workflow_scope"] = CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE.value
    del request_payload["option_surface_source"]
    del request_payload["column_mapping"]["option_surface"]
    request_payload["normalization_policy"]["option_quote_input_policy"] = None

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_market_data_calibration_request(request_payload)

    assert captured_error.value.failure.field_path == "option_surface_source"
    assert "full_with_option_surface" in captured_error.value.failure.message


def test_market_data_calibration_request_requires_tick_provenance_label() -> None:
    request_payload = _valid_market_data_request_payload()
    del request_payload["tick_data_source"]["provenance_label"]

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_market_data_calibration_request(request_payload)

    assert captured_error.value.failure.field_path == "tick_data_source.provenance_label"
    assert captured_error.value.failure.message == "is required"


def test_market_data_calibration_request_requires_known_news_column_mapping_for_csv_sources() -> None:
    request_payload = _valid_market_data_request_payload()
    del request_payload["column_mapping"]["known_news"]

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_market_data_calibration_request(request_payload)

    assert captured_error.value.failure.field_path == "column_mapping.known_news"
    assert "csv or parquet" in captured_error.value.failure.message


def test_market_data_calibration_request_rejects_unsupported_tick_data_file_formats() -> None:
    request_payload = _valid_market_data_request_payload()
    request_payload["tick_data_source"]["file_format"] = "json"

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_market_data_calibration_request(request_payload)

    assert captured_error.value.failure.field_path == "tick_data_source.file_format"
    assert captured_error.value.failure.message == "must be one of: csv, parquet"


def test_market_data_calibration_request_requires_bid_and_ask_mapping_for_tick_midpoint_policy() -> None:
    request_payload = _valid_market_data_request_payload()
    request_payload["workflow_scope"] = CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE.value
    del request_payload["option_surface_source"]
    del request_payload["column_mapping"]["option_surface"]
    request_payload["normalization_policy"]["option_quote_input_policy"] = None
    request_payload["normalization_policy"]["tick_price_input_policy"] = "bid_ask_midpoint"
    del request_payload["column_mapping"]["tick_data"]["price_column"]

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_market_data_calibration_request(request_payload)

    assert captured_error.value.failure.field_path == "column_mapping.tick_data"
    assert "bid_column and ask_column" in captured_error.value.failure.message


def test_staged_calibration_request_defaults_execution_mode_to_memory() -> None:
    request_payload = _valid_request_payload()
    del request_payload["execution_mode"]

    validated_request = validate_staged_calibration_request(request_payload)

    assert validated_request.execution_mode == CalibrationExecutionMode.MEMORY


def test_staged_calibration_request_honors_explicit_disk_override() -> None:
    request_payload = _valid_request_payload()
    request_payload["execution_mode"] = "disk"

    validated_request = validate_staged_calibration_request(request_payload)

    assert validated_request.execution_mode == CalibrationExecutionMode.DISK


def test_staged_calibration_request_roundtrip_serialization_is_deterministic() -> None:
    request = _valid_request_model()

    serialized_request = serialize_staged_calibration_request(request)
    deserialized_request = deserialize_staged_calibration_request(serialized_request)

    assert deserialized_request == request
    assert json.loads(serialized_request)["execution_mode"] == "memory"


def test_staged_calibration_request_rejects_disk_only_handoff_in_memory_mode() -> None:
    request_payload = _valid_request_payload()
    request_payload["stage_handoffs"] = [
        {
            "from_stage": "seasonality",
            "to_stage": "hawkes",
            "transport": "persisted_artifact_reference",
            "artifact_reference": "artifact://seasonality/stage-001",
        }
    ]

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_staged_calibration_request(request_payload)

    assert captured_error.value.failure.error_id == "calibration.validation.contract"
    assert captured_error.value.failure.workflow == "calibration"
    assert captured_error.value.failure.entrypoint == "contract"
    assert captured_error.value.failure.field_path == "stage_handoffs.transport"
    assert "requires execution_mode=disk" in captured_error.value.failure.message


def test_staged_calibration_execution_input_roundtrip_serialization_is_deterministic() -> None:
    execution_input = build_valid_execution_input_model()

    serialized_execution_input = serialize_staged_calibration_execution_input(execution_input)
    deserialized_execution_input = deserialize_staged_calibration_execution_input(
        serialized_execution_input
    )

    assert deserialized_execution_input == execution_input
    assert json.loads(serialized_execution_input)["request"]["execution_mode"] == "memory"


def test_staged_calibration_execution_input_requires_explicit_selected_stage_inputs() -> None:
    execution_input_payload = build_valid_execution_input_payload()
    del execution_input_payload["seasonality_stage_input"]

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_staged_calibration_execution_input(execution_input_payload)

    assert captured_error.value.failure.field_path == "seasonality_stage_input"
    assert "required when selected_stages contains seasonality" in captured_error.value.failure.message


def test_staged_calibration_execution_input_requires_disk_runtime_resources_for_disk_mode() -> None:
    execution_input_payload = build_valid_execution_input_payload()
    execution_input_payload["request"]["execution_mode"] = "disk"
    execution_input_payload["runtime_resources"] = {}

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_staged_calibration_execution_input(execution_input_payload)

    assert captured_error.value.failure.field_path == "runtime_resources.disk_checkpoint_directory"
    assert "required when execution_mode=disk" in captured_error.value.failure.message


def test_staged_calibration_execution_input_requires_close_open_boundary_price_fields() -> None:
    execution_input_payload = build_valid_execution_input_payload()
    boundary_observation = execution_input_payload["return_components_candidate_generation_input"][
        "observed_close_open_boundary_observations"
    ][0]
    del boundary_observation["close_price"]

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_staged_calibration_execution_input(execution_input_payload)

    assert (
        captured_error.value.failure.field_path
        == "return_components_candidate_generation_input.observed_close_open_boundary_observations[0].close_price"
    )
    assert captured_error.value.failure.message == "is required"


def test_staged_calibration_execution_input_requires_close_open_gap_detection_policy() -> None:
    execution_input_payload = build_valid_execution_input_payload()
    del execution_input_payload["return_components_candidate_generation_input"]["close_open_gap_detection_policy"]

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_staged_calibration_execution_input(execution_input_payload)

    assert (
        captured_error.value.failure.field_path
        == "return_components_candidate_generation_input.close_open_gap_detection_policy"
    )
    assert captured_error.value.failure.message == "is required"


def test_staged_calibration_execution_input_allows_empty_known_news_return_samples() -> None:
    execution_input_payload = build_valid_execution_input_payload()
    execution_input_payload["return_components_candidate_generation_input"][
        "realized_known_news_return_samples"
    ] = []

    validated_execution_input = validate_staged_calibration_execution_input(
        execution_input_payload
    )

    assert (
        validated_execution_input.return_components_candidate_generation_input
        is not None
    )
    assert (
        validated_execution_input.return_components_candidate_generation_input.realized_known_news_return_samples
        == ()
    )


def test_staged_calibration_response_roundtrip_serialization_is_deterministic() -> None:
    response_payload = _valid_response_payload()

    validated_response = validate_staged_calibration_response(response_payload)
    serialized_response = serialize_staged_calibration_response(validated_response)
    deserialized_response = deserialize_staged_calibration_response(serialized_response)

    assert deserialized_response == validated_response
    assert json.loads(serialized_response)["status"] == "success"


def test_staged_calibration_response_rejects_missing_stage_reports() -> None:
    response_payload = _valid_response_payload()
    del response_payload["stage_reports"]

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_staged_calibration_response(response_payload)

    assert captured_error.value.failure.error_id == "calibration.validation.contract"
    assert captured_error.value.failure.field_path == "stage_reports"
    assert captured_error.value.failure.message == "is required"


def _valid_request_model() -> StagedCalibrationRequest:
    return StagedCalibrationRequest(
        run_id="run-001",
        data_inputs=CalibrationRunDataInputs(
            exchange_code="XNYS",
            trade_events_data_reference="dataset://synthetic/trades/v1",
            option_surface_data_reference="dataset://synthetic/options/v1",
        ),
        execution_mode=CalibrationExecutionMode.MEMORY,
        selected_stages=(CalibrationStage.SEASONALITY, CalibrationStage.HAWKES),
        seed_settings=CalibrationRunSeedSettings(
            global_seed=17,
            stage_seed_overrides=(
                CalibrationStageSeedOverride(
                    stage=CalibrationStage.SEASONALITY,
                    seed=101,
                ),
            ),
        ),
        optimizer_settings_by_stage=(
            CalibrationStageSolverSettings(
                stage=CalibrationStage.SEASONALITY,
                optimizer_settings=CalibrationOptimizerSettings(
                    solver_name="lbfgs",
                    max_iterations=250,
                    absolute_tolerance=1e-8,
                    relative_tolerance=1e-6,
                    constraint_penalty_weight=10.0,
                ),
            ),
            CalibrationStageSolverSettings(
                stage=CalibrationStage.HAWKES,
                optimizer_settings=CalibrationOptimizerSettings(
                    solver_name="lbfgs",
                    max_iterations=250,
                    absolute_tolerance=1e-8,
                    relative_tolerance=1e-6,
                    constraint_penalty_weight=10.0,
                ),
            ),
        ),
    )


def _valid_market_data_request_model() -> MarketDataCalibrationRequest:
    return MarketDataCalibrationRequest(
        schema_version=CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
        run_id="market-data-run-001",
        tick_data_source=CalibrationInputFileReference(
            path="/tmp/ticks.csv",
            file_format=CalibrationInputFileFormat.CSV,
            provenance_label="tick-feed-primary",
        ),
        session_configuration=CalibrationSessionConfigurationInput(
            exchange_code="XNYS",
            session_definition_source=CalibrationInputFileReference(
                path="/tmp/sessions.json",
                file_format=CalibrationInputFileFormat.JSON,
                provenance_label="session-calendar-v1",
            ),
            session_timezone="America/New_York",
        ),
        column_mapping=CalibrationMarketDataColumnMapping(
            tick_data=CalibrationTickDataColumnMapping(
                timestamp_column="timestamp_utc",
                price_column="trade_price",
            ),
            option_surface=CalibrationOptionSurfaceColumnMapping(
                quote_timestamp_column="quote_timestamp_utc",
                expiry_timestamp_column="expiry_utc",
                strike_column="strike",
                option_type_column="option_type",
                bid_column="bid",
                ask_column="ask",
            ),
            known_news=CalibrationKnownNewsColumnMapping(
                event_timestamp_column="event_timestamp_utc",
                event_identifier_column="event_code",
            ),
        ),
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE,
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
            option_quote_input_policy=CalibrationOptionQuoteInputPolicy.BID_ASK_MIDPOINT,
        ),
        output_preferences=CalibrationMarketDataOutputPreferences(
            execution_mode=CalibrationExecutionMode.MEMORY,
            emit_derived_staged_execution_input=True,
            emit_input_provenance_report=True,
        ),
        option_surface_source=CalibrationInputFileReference(
            path="/tmp/options.parquet",
            file_format=CalibrationInputFileFormat.PARQUET,
            provenance_label="option-surface-close-001",
        ),
        known_news_inputs=CalibrationKnownNewsInput(
            calendar_source=CalibrationInputFileReference(
                path="/tmp/known-news.csv",
                file_format=CalibrationInputFileFormat.CSV,
                provenance_label="known-news-calendar",
            ),
            calendar_timezone="UTC",
        ),
    )


def _valid_request_payload() -> dict[str, Any]:
    return _valid_request_model().to_payload()


def _valid_market_data_request_payload() -> dict[str, Any]:
    return _valid_market_data_request_model().to_payload()


def _valid_execution_input_payload() -> dict[str, Any]:
    return build_valid_execution_input_payload()


def _valid_response_payload() -> dict[str, Any]:
    return {
        "schema_version": "calibration.orchestration.staged_run.report.v1",
        "failure_triage_policy": "mode_aware_failure_kind_and_checkpoint_context.v1",
        "run_id": "run-001",
        "execution_mode": "memory",
        "status": CalibrationStageExecutionStatus.SUCCESS.value,
        "stage_reports": [
            {
                "stage_report_kind": "seasonality",
                "stage": "seasonality",
                "status": "success",
                "run_id": "run-001",
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
            }
        ],
        "emitted_checkpoints": [],
    }


def _solver_settings(stage: CalibrationStage) -> CalibrationStageSolverSettings:
    return CalibrationStageSolverSettings(
        stage=stage,
        optimizer_settings=CalibrationOptimizerSettings(
            solver_name="lbfgs",
            max_iterations=250,
            absolute_tolerance=1e-8,
            relative_tolerance=1e-6,
            constraint_penalty_weight=10.0,
        ),
    )
