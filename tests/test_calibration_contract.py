import json
from typing import Any

import orjson
import pytest
from tests.support.calibration import (
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
    CalibrationMeanReversionConfiguration,
    CalibrationMarketDataColumnMapping,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataOutputPreferences,
    CalibrationMarketDataPreprocessingSettings,
    CalibrationMarketDataReturnComponentsPreprocessingSettings,
    CalibrationMarketDataSeasonalityPreprocessingSettings,
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
from janiform.calibration import (
    MeanReversionAnchorModel,
    MeanReversionAnchorModelKind,
    MeanReversionFittedParameters,
)
from janiform.calibration.staged import (
    CalibrationExecutionMode,
    CalibrationOptimizerSettings,
    CalibrationPreparedExecutionPayloadFileReference,
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
from janiform.seasonality import IntradaySmoothingKernel, SessionFilter


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


def test_market_data_calibration_request_roundtrip_preserves_mean_reversion_configuration() -> None:
    request = _valid_market_data_request_model()

    assert request.mean_reversion is not None
    assert (
        request.mean_reversion.anchor_model_kind
        == MeanReversionAnchorModelKind.ENDOGENOUS_CORE_PRESSURE_LEVEL
    )
    assert request.mean_reversion.calibration_grid_interval_hours == pytest.approx(24.0)

    serialized_request = serialize_market_data_calibration_request(request)
    deserialized_request = deserialize_market_data_calibration_request(serialized_request)

    assert deserialized_request == request


def test_market_data_calibration_request_roundtrip_preserves_optimizer_settings_by_stage() -> None:
    base_request = _valid_market_data_request_model()
    request = MarketDataCalibrationRequest(
        schema_version=CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
        run_id="run-001",
        tick_data_source=base_request.tick_data_source,
        session_configuration=base_request.session_configuration,
        column_mapping=base_request.column_mapping,
        workflow_scope=base_request.workflow_scope,
        normalization_policy=base_request.normalization_policy,
        output_preferences=base_request.output_preferences,
        optimizer_settings_by_stage=(
            _solver_settings(CalibrationStage.SEASONALITY),
            _solver_settings(CalibrationStage.HAWKES),
        ),
        option_surface_source=base_request.option_surface_source,
        known_news_inputs=base_request.known_news_inputs,
        mean_reversion=base_request.mean_reversion,
    )

    serialized_request = serialize_market_data_calibration_request(request)
    deserialized_request = deserialize_market_data_calibration_request(serialized_request)

    assert deserialized_request == request


def test_market_data_calibration_request_roundtrip_preserves_preprocessing_settings() -> None:
    base_request = _valid_market_data_request_model()
    request = MarketDataCalibrationRequest(
        schema_version=base_request.schema_version,
        run_id=base_request.run_id,
        tick_data_source=base_request.tick_data_source,
        session_configuration=base_request.session_configuration,
        column_mapping=base_request.column_mapping,
        workflow_scope=base_request.workflow_scope,
        normalization_policy=base_request.normalization_policy,
        output_preferences=base_request.output_preferences,
        preprocessing_settings=CalibrationMarketDataPreprocessingSettings(
            seasonality=CalibrationMarketDataSeasonalityPreprocessingSettings(
                session_filter=SessionFilter.ALL,
                sparse_hour_event_threshold=2,
                known_news_overlay_log_adjustment=-0.25,
                intraday_minutes_per_bin=5,
                intraday_smoothing_kernel=IntradaySmoothingKernel.UNIFORM,
                intraday_smoothing_bandwidth_bins=2.5,
                intraday_minimum_bin_events=3,
                day_of_week_minimum_bucket_events=4,
                day_of_year_minimum_bucket_events=5,
            ),
            return_components=CalibrationMarketDataReturnComponentsPreprocessingSettings(
                intraday_jump_threshold_multiplier=6.0,
                close_open_gap_tolerance_multiplier=2.0,
            ),
        ),
        optimizer_settings_by_stage=base_request.optimizer_settings_by_stage,
        option_surface_source=base_request.option_surface_source,
        known_news_inputs=base_request.known_news_inputs,
        mean_reversion=base_request.mean_reversion,
    )

    serialized_request = serialize_market_data_calibration_request(request)
    deserialized_request = deserialize_market_data_calibration_request(serialized_request)

    assert deserialized_request == request


def test_market_data_calibration_request_rejects_enabled_mean_reversion_for_seasonality_only() -> None:
    request_payload = _valid_market_data_request_payload()
    request_payload["workflow_scope"] = CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY.value
    request_payload["normalization_policy"]["tick_price_input_policy"] = None
    request_payload["normalization_policy"]["option_quote_input_policy"] = None
    del request_payload["option_surface_source"]
    del request_payload["column_mapping"]["option_surface"]

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_market_data_calibration_request(request_payload)

    assert captured_error.value.failure.field_path == "mean_reversion"
    assert "seasonality_only" in captured_error.value.failure.message


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


def test_mean_reversion_fitted_parameters_roundtrip_serialization_is_deterministic() -> None:
    fitted_parameters = MeanReversionFittedParameters(
        kappa=0.25,
        anchor_model=MeanReversionAnchorModel.zero_gain_endogenous_core_pressure_level(),
    )

    serialized = fitted_parameters.to_json()
    deserialized = MeanReversionFittedParameters.from_mapping(json.loads(serialized))

    assert deserialized == fitted_parameters
    assert json.loads(serialized)["anchor_model"]["anchor_model_kind"] == (
        MeanReversionAnchorModelKind.ENDOGENOUS_CORE_PRESSURE_LEVEL.value
    )


def test_mean_reversion_fitted_parameters_reject_negative_kappa() -> None:
    with pytest.raises(ValueError, match="kappa: must be non-negative"):
        MeanReversionFittedParameters(
            kappa=-0.01,
            anchor_model=MeanReversionAnchorModel.zero_gain_endogenous_core_pressure_level(),
        )


def test_endogenous_mean_reversion_anchor_model_from_mapping_rejects_unknown_anchor_value() -> None:
    with pytest.raises(ValueError, match="unsupported field 'anchor_value'"):
        MeanReversionAnchorModel.from_mapping(
            {
                "anchor_model_kind": MeanReversionAnchorModelKind.ENDOGENOUS_CORE_PRESSURE_LEVEL.value,
                "anchor_value": 0.0,
            }
        )


def test_staged_calibration_execution_input_requires_explicit_selected_stage_inputs() -> None:
    execution_input_payload = build_valid_execution_input_payload()
    del execution_input_payload["prepared_payload_file"]

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_staged_calibration_execution_input(execution_input_payload)

    assert captured_error.value.failure.field_path == "prepared_payload_file"
    assert captured_error.value.failure.message == "is required"


def test_staged_calibration_execution_input_requires_mean_reversion_stage_input_when_selected(
) -> None:
    execution_input_payload = build_valid_execution_input_payload()
    execution_input_payload["prepared_payload_file"] = {
        "path": "",
        "file_format": "json",
    }

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_staged_calibration_execution_input(execution_input_payload)

    assert captured_error.value.failure.field_path == "prepared_payload_file.path"
    assert "non-empty string" in captured_error.value.failure.message


def test_staged_calibration_execution_input_rejects_non_positive_mean_reversion_grid_interval(
) -> None:
    execution_input_payload = build_valid_execution_input_payload()
    execution_input_payload["prepared_payload_file"]["file_format"] = "parquet"

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_staged_calibration_execution_input(execution_input_payload)

    assert captured_error.value.failure.field_path == "prepared_payload_file.file_format"
    assert "CalibrationPreparedExecutionPayloadFileFormat" in captured_error.value.failure.message


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
    execution_input_payload["runtime_resources"] = {
        "disk_checkpoint_directory": 123,
    }

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_staged_calibration_execution_input(execution_input_payload)

    assert captured_error.value.failure.field_path == "runtime_resources.disk_checkpoint_directory"
    assert captured_error.value.failure.message == "must be a string when provided"


def test_staged_calibration_execution_input_requires_close_open_gap_detection_policy() -> None:
    execution_input_payload = build_valid_execution_input_payload()
    execution_input_payload["runtime_resources"] = {
        "disk_checkpoint_directory": "/tmp/should-not-be-used",
    }

    with pytest.raises(CalibrationValidationError) as captured_error:
        validate_staged_calibration_execution_input(execution_input_payload)

    assert captured_error.value.failure.field_path == "runtime_resources.disk_checkpoint_directory"
    assert "only supported when execution_mode=disk" in captured_error.value.failure.message


def test_staged_calibration_execution_input_allows_empty_known_news_return_samples() -> None:
    execution_input = build_valid_execution_input_model()
    validated_execution_input = validate_staged_calibration_execution_input(
        execution_input.to_payload()
    )

    assert (
        validated_execution_input.prepared_payload_file
        == CalibrationPreparedExecutionPayloadFileReference(
            path=execution_input.prepared_payload_file.path
        )
    )
    assert validated_execution_input.runtime_resources is None


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


def test_staged_calibration_response_accepts_diagnostic_artifact_bundle() -> None:
    response_payload = _valid_response_payload()
    response_payload["diagnostic_artifact_references"] = [
        {
            "artifact_family": "observed_targets",
            "relative_path": "diagnostics/observed_targets.json",
        }
    ]
    response_payload["diagnostic_artifact_bundle"] = {
        "observed_targets": {
            "schema_version": "calibration.diagnostics.observed_targets.v1",
            "target_builder_version": "test",
            "targets": [],
        },
        "candidate_frontier": {
            "schema_version": "calibration.diagnostics.candidate_frontier.v1",
            "frontier_entries": [],
        },
        "coarse_tick_consistency": {
            "schema_version": "calibration.diagnostics.coarse_tick_consistency.v1",
            "metrics": [],
        },
        "selection_rationale": {
            "schema_version": "calibration.diagnostics.selection_rationale.v1",
            "decision": {
                "selection_stage": "joint_refinement",
                "selected_candidate_id": "candidate-001",
                "selection_policy": "test",
                "validity_screened_candidate_count": 1,
                "admissible_candidate_count": 1,
                "pareto_frontier_count": 1,
                "tie_break_candidate_count": 1,
                "decisive_metric_ids": [],
                "rationale": "test",
            },
        },
        "simulation_summary": {
            "schema_version": "calibration.diagnostics.simulation_summary.v1",
            "summaries": [],
        },
        "search_surface_manifest": {
            "schema_version": "calibration.diagnostics.search_surface_manifest.v1",
            "surfaces": [],
        },
        "runtime_profile": {
            "schema_version": "calibration.diagnostics.runtime_profile.v1",
            "cpu_count": 24,
            "entries": [],
        },
    }

    validated_response = validate_staged_calibration_response(response_payload)

    assert validated_response.diagnostic_artifact_references == (
        {
            "artifact_family": "observed_targets",
            "relative_path": "diagnostics/observed_targets.json",
        },
    )
    assert validated_response.diagnostic_artifact_bundle is not None
    assert (
        validated_response.diagnostic_artifact_bundle["selection_rationale"]["decision"][
            "selected_candidate_id"
        ]
        == "candidate-001"
    )


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
                    cpu_count=1,
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
                    cpu_count=1,
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
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
            option_quote_input_policy=CalibrationOptionQuoteInputPolicy.BID_ASK_MIDPOINT,
        ),
        output_preferences=CalibrationMarketDataOutputPreferences(
            execution_mode=CalibrationExecutionMode.MEMORY,
            emit_derived_staged_execution_input=False,
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
        mean_reversion=CalibrationMeanReversionConfiguration(
            anchor_model_kind=MeanReversionAnchorModelKind.ENDOGENOUS_CORE_PRESSURE_LEVEL,
            calibration_grid_interval_hours=24.0,
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
            cpu_count=1,
            absolute_tolerance=1e-8,
            relative_tolerance=1e-6,
            constraint_penalty_weight=10.0,
        ),
    )
