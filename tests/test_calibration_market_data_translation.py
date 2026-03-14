from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

import pyarrow as pa
import pytest
from pyarrow import parquet as pyarrow_parquet

from janiform.calibration.market_data.translation import (
    translate_market_data_request_to_staged_execution_input,
)
from janiform.calibration import CalibrationValidationError
from janiform.calibration.market_data import (
    CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION,
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
    run_market_data_calibration,
)
from janiform.calibration.staged import (
    CalibrationExecutionMode,
    CalibrationStage,
    CalibrationTradeSign,
    run_staged_calibration,
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
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:00:00Z,101.0,buy",
                "2026-02-02T15:10:00Z,104.0,sell",
                "2026-02-02T20:59:00Z,102.0,sell",
                "2026-02-04T14:30:00Z,90.0,buy",
                "2026-02-04T14:40:00Z,100.0,sell",
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
            trade_sign_column="trade_sign",
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
    assert (
        "native_market_data_translation_preprocessing.rust.v1"
        in first_translation.provenance.derivation_policies
    )

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
    assert open_sample.post_shock_activity_lift == pytest.approx(1.0)
    assert closed_sample.event_timestamp_utc_nanos == _utc_nanos("2026-02-02T22:00:00Z")
    assert closed_sample.open_window_jump_return == pytest.approx(0.0)
    assert closed_sample.closed_window_gap_return == pytest.approx(-0.12516314295)
    assert closed_sample.post_shock_activity_lift == pytest.approx(1.0)

    assert len(return_components_input.observed_open_session_returns) == 4
    assert return_components_input.open_session_sell_share_proxy == pytest.approx(0.5)

    boundary_observation = return_components_input.observed_close_open_boundary_observations[0]
    assert boundary_observation.close_timestamp_utc_nanos == _utc_nanos("2026-02-02T21:00:00Z")
    assert boundary_observation.open_timestamp_utc_nanos == _utc_nanos("2026-02-04T14:30:00Z")
    assert boundary_observation.close_price == pytest.approx(102.0)
    assert boundary_observation.open_price == pytest.approx(90.0)


def test_translate_market_data_request_infers_session_without_manual_definition(
    tmp_path: Path,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:00:00Z,101.0,buy",
                "2026-02-02T20:59:00Z,102.0,sell",
                "2026-02-03T14:30:00Z,103.0,buy",
                "2026-02-03T20:59:00Z,104.0,sell",
            ]
        ),
        encoding="utf-8",
    )

    request = _build_market_data_request(
        session_definition_path=None,
        tick_data_path=tick_data_path,
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
    )

    translation = translate_market_data_request_to_staged_execution_input(request)

    assert translation.session_resolution.resolution_kind == "automatic_inference"
    assert translation.session_resolution.manual_override_applied is False
    assert translation.session_resolution.selected_policy_id == (
        "exchange_code_builtin_default.xnys_regular_session.v1"
    )
    assert translation.session_resolution.resolved_timezone_name == "America/New_York"
    assert translation.provenance.session_definition_path is None
    assert len(translation.session_resolution.regular_session_windows) == 5
    assert translation.session_resolution.recurring_closed_windows != ()


def test_translate_market_data_request_rejects_ambiguous_session_inference_without_override(
    tmp_path: Path,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T08:00:00Z,1.0850,buy",
                "2026-02-02T12:00:00Z,1.0852,sell",
                "2026-02-03T08:00:00Z,1.0854,buy",
            ]
        ),
        encoding="utf-8",
    )

    request = _build_market_data_request(
        session_definition_path=None,
        tick_data_path=tick_data_path,
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
        exchange_code="BROKER_SPOT",
        session_timezone="UTC",
    )

    with pytest.raises(CalibrationValidationError) as captured_error:
        translate_market_data_request_to_staged_execution_input(request)

    assert captured_error.value.field_path == "session_configuration"
    assert "automatic 24/5 session inference is ambiguous" in str(captured_error.value)


def test_translate_market_data_request_without_known_news_runs_staged_calibration(
    tmp_path: Path,
    calendar_registry_json: str,
) -> None:
    session_definition_path = _write_session_definition(tmp_path, calendar_registry_json)

    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:00:00Z,101.0,buy",
                "2026-02-02T15:10:00Z,103.0,sell",
                "2026-02-02T20:59:00Z,102.0,sell",
                "2026-02-04T14:30:00Z,95.0,buy",
                "2026-02-04T14:40:00Z,98.0,sell",
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
            trade_sign_column="trade_sign",
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
    assert response.status.value == "failure"
    assert [report.stage.value for report in response.stage_reports] == [
        "seasonality",
        "hawkes",
    ]
    assert response.stage_reports[0].status.value == "success"
    assert response.stage_reports[1].status.value == "failure"
    assert response.stage_reports[1].failure is not None
    assert (
        response.stage_reports[1].failure.failure_kind
        == "constraint_violations_for_all_candidates"
    )
    assert response.failure is not None
    assert response.failure.failure_kind == "stage_execution_failed"
    assert response.failure.stage == CalibrationStage.HAWKES


def test_translate_market_data_request_scales_nig_candidates_to_tick_return_magnitude(
    tmp_path: Path,
    calendar_registry_json: str,
) -> None:
    session_definition_path = _write_session_definition(tmp_path, calendar_registry_json)

    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,1.085000,buy",
                "2026-02-02T14:30:01Z,1.085010,sell",
                "2026-02-02T14:30:02Z,1.085004,buy",
                "2026-02-02T14:30:03Z,1.085012,sell",
                "2026-02-02T14:30:04Z,1.085009,buy",
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
            trade_sign_column="trade_sign",
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

    assert len(return_components_input.observed_open_session_returns) == 4
    assert return_components_input.detected_intraday_jump_candidates == ()
    assert return_components_input.open_session_sell_share_proxy == pytest.approx(0.4)


def test_translate_market_data_request_applies_hourly_nig_shape_mass_floor_for_dense_ticks(
    tmp_path: Path,
    calendar_registry_json: str,
) -> None:
    session_definition_path = _write_session_definition(tmp_path, calendar_registry_json)

    tick_data_path = tmp_path / "dense_ticks.csv"
    start_time = datetime(2026, 2, 2, 14, 30, tzinfo=UTC)
    csv_lines = ["timestamp_utc,trade_price,trade_sign"]

    current_price = 1.085
    for row_index in range(1_200):
        timestamp = start_time + timedelta(seconds=0.75 * row_index)
        current_price += 0.000002 if row_index % 2 == 0 else -0.0000015
        trade_sign = "buy" if row_index % 2 == 0 else "sell"
        csv_lines.append(
            f"{timestamp.isoformat().replace('+00:00', 'Z')},{current_price:.6f},{trade_sign}"
        )

    tick_data_path.write_text("\n".join(csv_lines), encoding="utf-8")

    request = _build_market_data_request(
        session_definition_path=session_definition_path,
        tick_data_path=tick_data_path,
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
    )

    translation = translate_market_data_request_to_staged_execution_input(request)
    return_components_input = (
        translation.execution_input.return_components_candidate_generation_input
    )
    assert return_components_input is not None

    assert len(return_components_input.observed_open_session_returns) == 1199
    assert return_components_input.open_session_sell_share_proxy == pytest.approx(0.5)


def test_translate_market_data_request_infers_missing_trade_signs_from_price_direction(
    tmp_path: Path,
    calendar_registry_json: str,
) -> None:
    session_definition_path = _write_session_definition(tmp_path, calendar_registry_json)

    tick_data_path = tmp_path / "ticks_without_sign.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price",
                "2026-02-02T14:30:00Z,100.0",
                "2026-02-02T14:31:00Z,101.0",
                "2026-02-02T14:32:00Z,99.0",
                "2026-02-02T14:33:00Z,99.0",
                "2026-02-02T14:34:00Z,102.0",
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
            trade_sign_column=None,
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
        ),
    )

    translation = translate_market_data_request_to_staged_execution_input(request)

    assert (
        "unsigned_trade_direction_labels_follow_tick_price_change_with_flat_move_carryover.v1"
        in translation.provenance.derivation_policies
    )

    hawkes_candidates = translation.execution_input.hawkes_candidate_inputs
    assert hawkes_candidates is not None
    assert len(hawkes_candidates) > 1

    source_regime = hawkes_candidates[0].source_regime
    assert source_regime.open_session_tick_count == 5
    assert source_regime.open_session_buy_tick_count == 3
    assert source_regime.open_session_sell_tick_count == 2
    assert source_regime.open_session_known_news_activity_share == pytest.approx(0.0)

    label_diagnostics = hawkes_candidates[0].directional_label_diagnostics
    assert label_diagnostics.policy_id == "tick_price_change_with_flat_move_carryover.v1"
    assert label_diagnostics.inferred_label_count == 5
    assert label_diagnostics.price_up_inference_count == 2
    assert label_diagnostics.price_down_inference_count == 1
    assert label_diagnostics.flat_move_carry_count == 1


def test_translate_market_data_request_preserves_provided_trade_sign_diagnostics_when_sign_column_has_blanks(
    tmp_path: Path,
    calendar_registry_json: str,
) -> None:
    session_definition_path = _write_session_definition(tmp_path, calendar_registry_json)

    tick_data_path = tmp_path / "ticks_with_partial_signs.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T14:31:00Z,101.0,",
                "2026-02-02T14:32:00Z,99.0,",
                "2026-02-02T14:33:00Z,99.0,sell",
                "2026-02-02T14:34:00Z,102.0,",
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
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
        ),
    )

    translation = translate_market_data_request_to_staged_execution_input(request)

    assert [
        record.trade_sign
        for record in translation.loaded_inputs.tick_data.records
    ] == [
        CalibrationTradeSign.BUY,
        CalibrationTradeSign.BUY,
        CalibrationTradeSign.SELL,
        CalibrationTradeSign.SELL,
        CalibrationTradeSign.BUY,
    ]

    hawkes_candidates = translation.execution_input.hawkes_candidate_inputs
    assert hawkes_candidates is not None

    label_diagnostics = hawkes_candidates[0].directional_label_diagnostics
    assert label_diagnostics.provided_label_count == 2
    assert label_diagnostics.inferred_label_count == 3
    assert label_diagnostics.price_up_inference_count == 2
    assert label_diagnostics.price_down_inference_count == 1
    assert label_diagnostics.flat_move_carry_count == 0


def test_prepare_market_data_calibration_returns_public_diagnostics_and_provenance(
    tmp_path: Path,
    calendar_registry_json: str,
) -> None:
    session_definition_path = _write_session_definition(tmp_path, calendar_registry_json)

    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:00:00Z,101.0,buy",
                "2026-02-02T15:10:00Z,104.0,sell",
                "2026-02-02T20:59:00Z,102.0,sell",
                "2026-02-04T14:30:00Z,90.0,buy",
                "2026-02-04T14:40:00Z,100.0,sell",
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
            trade_sign_column="trade_sign",
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
    assert prepared.session_resolution.manual_override_applied is True
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
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:00:00Z,101.0,buy",
                "2026-02-02T15:10:00Z,103.0,sell",
                "2026-02-02T20:59:00Z,102.0,sell",
                "2026-02-04T14:30:00Z,95.0,buy",
                "2026-02-04T14:40:00Z,98.0,sell",
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
            trade_sign_column="trade_sign",
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
    assert response.status.value == "failure"
    assert len(response.stage_reports) == 2
    assert response.stage_reports[0].stage == CalibrationStage.SEASONALITY
    assert response.stage_reports[0].status.value == "success"
    assert response.stage_reports[1].stage == CalibrationStage.HAWKES
    assert response.stage_reports[1].status.value == "failure"
    assert response.stage_reports[1].failure is not None
    assert (
        response.stage_reports[1].failure.failure_kind
        == "constraint_violations_for_all_candidates"
    )
    assert response.failure is not None
    assert response.failure.failure_kind == "stage_execution_failed"
    assert response.failure.stage == CalibrationStage.HAWKES


def test_translate_market_data_request_full_with_option_surface_sets_option_reference(
    tmp_path: Path,
    calendar_registry_json: str,
) -> None:
    session_definition_path = _write_session_definition(tmp_path, calendar_registry_json)

    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:00:00Z,101.0,buy",
                "2026-02-02T15:10:00Z,103.0,sell",
                "2026-02-02T20:59:00Z,102.0,sell",
                "2026-02-04T14:30:00Z,95.0,buy",
                "2026-02-04T14:40:00Z,99.0,sell",
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
            trade_sign_column="trade_sign",
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
    session_definition_path: Path | None,
    tick_data_path: Path,
    workflow_scope: CalibrationMarketDataWorkflowScope,
    tick_column_mapping: CalibrationTickDataColumnMapping,
    normalization_policy: CalibrationMarketDataNormalizationPolicy,
    known_news_inputs: CalibrationKnownNewsInput | None = None,
    known_news_column_mapping: CalibrationKnownNewsColumnMapping | None = None,
    option_surface_source: CalibrationInputFileReference | None = None,
    option_surface_column_mapping: CalibrationOptionSurfaceColumnMapping | None = None,
    exchange_code: str = "XNYS",
    session_timezone: str | None = "America/New_York",
) -> MarketDataCalibrationRequest:
    session_configuration_fields: dict[str, Any] = {
        "exchange_code": exchange_code,
    }
    if session_definition_path is not None:
        session_configuration_fields["session_definition_source"] = CalibrationInputFileReference(
            path=str(session_definition_path),
            file_format=CalibrationInputFileFormat.JSON,
            provenance_label="session-registry",
        )
    if session_timezone is not None:
        session_configuration_fields["session_timezone"] = session_timezone

    return MarketDataCalibrationRequest(
        schema_version="calibration.market_data.request.v1",
        run_id="market-data-translation-run",
        tick_data_source=CalibrationInputFileReference(
            path=str(tick_data_path),
            file_format=CalibrationInputFileFormat.CSV,
            provenance_label="ticks-primary",
        ),
        session_configuration=CalibrationSessionConfigurationInput(**session_configuration_fields),
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
