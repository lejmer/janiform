from datetime import UTC, datetime, timedelta
import math
from pathlib import Path
from typing import Any, Optional

import pyarrow as pa
import pytest
from pyarrow import parquet

from janiform.calibration.market_data.translation import (
    normalize_market_data_source,
    resolve_market_data_session,
    translate_market_data_request_to_prepared_calibration,
    translate_market_data_request_to_prepared_calibration_from_phases,
)
from janiform.calibration import CalibrationValidationError
from janiform.calibration.market_data import (
    CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION,
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
    prepare_market_data_calibration,
)
from janiform.calibration.staged import (
    CalibrationExecutionMode,
    CalibrationStage,
)
from janiform.seasonality import IntradaySmoothingKernel, SessionFilter


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
            tick_price_input_policy=None,
        ),
    )

    translation = translate_market_data_request_to_prepared_calibration(request)
    prepared_payload = _prepared_payload(translation)

    assert translation.staged_request.selected_stages == (
        CalibrationStage.SEASONALITY,
    )
    assert set(prepared_payload) == {
        "schema_version",
        "request",
        "seasonality_stage_input",
        "joint_refinement_audit_option_pricing_enabled",
    }
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
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
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

    first_translation = translate_market_data_request_to_prepared_calibration(request)
    second_translation = translate_market_data_request_to_prepared_calibration(request)
    first_prepared_payload = _prepared_payload(first_translation)

    assert first_prepared_payload == _prepared_payload(second_translation)
    assert first_translation.diagnostics == second_translation.diagnostics
    assert first_translation.staged_request.selected_stages == (
        CalibrationStage.SEASONALITY,
        CalibrationStage.HAWKES,
        CalibrationStage.RETURN_COMPONENTS,
        CalibrationStage.MEAN_REVERSION,
        CalibrationStage.JOINT_REFINEMENT,
    )
    assert first_translation.diagnostics.known_news_event_count == 2
    assert first_translation.diagnostics.known_news_events_outside_source_window_count == 1
    assert first_translation.diagnostics.seasonality_overlay_count == 2
    assert first_translation.diagnostics.close_open_boundary_observation_count == 1
    assert first_translation.diagnostics.material_close_open_gap_count == 1
    assert (
        "market_data_loading_and_translation.rust.v1"
        in first_translation.provenance.derivation_policies
    )

    return_components_input = first_prepared_payload["return_components_candidate_generation_input"]
    assert len(return_components_input["realized_known_news_return_samples"]) == 2

    open_sample = return_components_input["realized_known_news_return_samples"][0]
    closed_sample = return_components_input["realized_known_news_return_samples"][1]
    assert open_sample["event_timestamp_utc_nanos"] == _utc_nanos("2026-02-02T15:05:00Z")
    assert open_sample["open_window_jump_return"] == pytest.approx(0.0292703823)
    assert open_sample["closed_window_gap_return"] == pytest.approx(0.0)
    assert open_sample["post_shock_activity_lift"] == pytest.approx(1.0)
    assert closed_sample["event_timestamp_utc_nanos"] == _utc_nanos("2026-02-02T22:00:00Z")
    assert closed_sample["open_window_jump_return"] == pytest.approx(0.0)
    assert closed_sample["closed_window_gap_return"] == pytest.approx(-0.12516314295)
    assert closed_sample["post_shock_activity_lift"] == pytest.approx(1.0)

    assert len(return_components_input["observed_open_session_returns"]) == 4
    assert return_components_input["open_session_sell_share_proxy"] == pytest.approx(0.5)

    boundary_observation = return_components_input["observed_close_open_boundary_observations"][0]
    assert boundary_observation["close_timestamp_utc_nanos"] == _utc_nanos("2026-02-02T21:00:00Z")
    assert boundary_observation["open_timestamp_utc_nanos"] == _utc_nanos("2026-02-04T14:30:00Z")
    assert boundary_observation["close_price"] == pytest.approx(102.0)
    assert boundary_observation["open_price"] == pytest.approx(90.0)


def test_translate_market_data_request_materializes_mean_reversion_on_interval_grid(
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
                "2026-02-02T20:59:00Z,101.0,sell",
                "2026-02-03T14:30:00Z,102.0,buy",
                "2026-02-03T20:59:00Z,103.0,sell",
                "2026-02-04T14:30:00Z,104.0,buy",
                "2026-02-04T20:59:00Z,105.0,sell",
                "2026-02-05T14:30:00Z,106.0,buy",
                "2026-02-05T20:59:00Z,107.0,sell",
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
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        ),
        mean_reversion=CalibrationMeanReversionConfiguration(
        ),
    )

    translation = translate_market_data_request_to_prepared_calibration(request)
    prepared_payload = _prepared_payload(translation)

    assert translation.staged_request.selected_stages == (
        CalibrationStage.SEASONALITY,
        CalibrationStage.HAWKES,
        CalibrationStage.RETURN_COMPONENTS,
        CalibrationStage.MEAN_REVERSION,
        CalibrationStage.JOINT_REFINEMENT,
    )

    mean_reversion_stage_input = prepared_payload["mean_reversion_stage_input"]
    assert set(mean_reversion_stage_input) == {
        "calibration_grid_kind",
        "calibration_grid_interval_hours",
        "source_observation_count",
        "coverage_ratio",
        "observations",
    }
    assert mean_reversion_stage_input["source_observation_count"] == 3
    assert mean_reversion_stage_input["coverage_ratio"] == pytest.approx(0.25)
    assert mean_reversion_stage_input["calibration_grid_interval_hours"] == pytest.approx(6.5)
    assert mean_reversion_stage_input["calibration_grid_kind"] == (
        "market_data_open_session_log_price_interval_grid.v1:"
        "last_observed_open_session_log_price_per_complete_utc_interval.v1:"
        "median_observed_session_duration_hours_from_retained_open_session_ticks.v1"
    )
    assert tuple(
        observation["observation_timestamp_utc_nanos"]
        for observation in mean_reversion_stage_input["observations"]
    ) == (
        _utc_nanos("2026-02-02T21:00:00Z"),
        _utc_nanos("2026-02-03T03:30:00Z"),
        _utc_nanos("2026-02-03T10:00:00Z"),
        _utc_nanos("2026-02-03T16:30:00Z"),
        _utc_nanos("2026-02-03T23:00:00Z"),
        _utc_nanos("2026-02-04T05:30:00Z"),
        _utc_nanos("2026-02-04T12:00:00Z"),
        _utc_nanos("2026-02-04T18:30:00Z"),
        _utc_nanos("2026-02-05T01:00:00Z"),
        _utc_nanos("2026-02-05T07:30:00Z"),
        _utc_nanos("2026-02-05T14:00:00Z"),
        _utc_nanos("2026-02-05T20:30:00Z"),
    )
    assert tuple(
        observation["observed_log_price"]
        for observation in mean_reversion_stage_input["observations"]
    ) == pytest.approx(
        (
            math.log(101.0 / 107.0),
            math.log(101.0 / 107.0),
            math.log(101.0 / 107.0),
            math.log(101.0 / 107.0),
            math.log(101.0 / 107.0),
            math.log(101.0 / 107.0),
            math.log(101.0 / 107.0),
            math.log(104.0 / 107.0),
            math.log(104.0 / 107.0),
            math.log(104.0 / 107.0),
            math.log(104.0 / 107.0),
            math.log(106.0 / 107.0),
        )
    )


def test_translate_market_data_request_reports_direct_mean_reversion_coverage_on_sparse_grid(
    tmp_path: Path,
    calendar_registry_json: str,
) -> None:
    session_definition_path = _write_session_definition(tmp_path, calendar_registry_json)

    tick_data_path = tmp_path / "ticks_sparse.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T20:59:00Z,101.0,sell",
                "2026-02-05T14:30:00Z,106.0,buy",
                "2026-02-05T20:59:00Z,107.0,sell",
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
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        ),
        mean_reversion=CalibrationMeanReversionConfiguration(
            calibration_grid_interval_hours=24.0,
        ),
    )

    translation = translate_market_data_request_to_prepared_calibration(request)
    prepared_payload = _prepared_payload(translation)

    mean_reversion_stage_input = prepared_payload["mean_reversion_stage_input"]

    assert mean_reversion_stage_input["source_observation_count"] == 1
    assert mean_reversion_stage_input["coverage_ratio"] == pytest.approx(1.0 / 3.0)
    assert len(mean_reversion_stage_input["observations"]) == 3
    assert tuple(
        observation["observation_timestamp_utc_nanos"]
        for observation in mean_reversion_stage_input["observations"]
    ) == (
        _utc_nanos("2026-02-03T14:30:00Z"),
        _utc_nanos("2026-02-04T14:30:00Z"),
        _utc_nanos("2026-02-05T14:30:00Z"),
    )


def test_translate_market_data_request_applies_explicit_preprocessing_settings(
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
                "2026-02-02T14:35:00Z,103.0,sell",
                "2026-02-02T20:59:00Z,101.0,buy",
                "2026-02-03T14:30:00Z,90.0,sell",
                "2026-02-03T14:35:00Z,100.0,buy",
            ]
        ),
        encoding="utf-8",
    )

    known_news_path = tmp_path / "known_news.csv"
    known_news_path.write_text(
        "\n".join(
            [
                "event_timestamp_utc,event_code",
                "2026-02-02T15:00:00Z,macro",
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
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
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
        preprocessing_settings=CalibrationMarketDataPreprocessingSettings(
            seasonality=CalibrationMarketDataSeasonalityPreprocessingSettings(
                session_filter=SessionFilter.ALL,
                sparse_hour_event_threshold=2,
                known_news_overlay_log_adjustment=-0.2,
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
    )

    default_request = _build_market_data_request(
        session_definition_path=session_definition_path,
        tick_data_path=tick_data_path,
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        tick_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
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

    translation = translate_market_data_request_to_prepared_calibration(request)
    default_translation = translate_market_data_request_to_prepared_calibration(default_request)
    prepared_payload = _prepared_payload(translation)
    default_prepared_payload = _prepared_payload(default_translation)
    seasonality_settings = prepared_payload["seasonality_stage_input"]["settings"]
    overlay = prepared_payload["seasonality_stage_input"]["scheduled_event_overlays"][0]
    return_components_input = prepared_payload["return_components_candidate_generation_input"]
    default_return_components_input = default_prepared_payload[
        "return_components_candidate_generation_input"
    ]

    assert seasonality_settings["session_filter"] == "all"
    assert seasonality_settings["sparse_hour_event_threshold"] == 2
    assert seasonality_settings["intraday_settings"]["minutes_per_bin"] == 5
    assert seasonality_settings["intraday_settings"]["smoothing_kernel"] == "uniform"
    assert seasonality_settings["intraday_settings"]["smoothing_bandwidth_bins"] == pytest.approx(2.5)
    assert seasonality_settings["intraday_settings"]["minimum_bin_events"] == 3
    assert seasonality_settings["day_of_week_settings"]["minimum_bucket_events"] == 4
    assert seasonality_settings["day_of_year_settings"]["minimum_bucket_events"] == 5
    assert overlay["log_adjustment"] == pytest.approx(-0.2)
    assert (
        default_prepared_payload["seasonality_stage_input"]["scheduled_event_overlays"][0][
            "log_adjustment"
        ]
        == pytest.approx(0.0)
    )
    assert (
        return_components_input["close_open_gap_detection_policy"][
            "material_gap_absolute_return_tolerance"
        ]
        == pytest.approx(
            default_return_components_input["close_open_gap_detection_policy"][
                "material_gap_absolute_return_tolerance"
            ]
            * (2.0 / 3.0),
            abs=1e-6,
        )
    )
    assert translation.provenance.deterministic_defaults_applied == ()


def test_translate_market_data_request_honors_explicit_mean_reversion_grid_interval_override(
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
                "2026-02-02T20:59:00Z,101.0,sell",
                "2026-02-03T14:30:00Z,102.0,buy",
                "2026-02-03T20:59:00Z,103.0,sell",
                "2026-02-04T14:30:00Z,104.0,buy",
                "2026-02-04T20:59:00Z,105.0,sell",
                "2026-02-05T14:30:00Z,106.0,buy",
                "2026-02-05T20:59:00Z,107.0,sell",
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
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        ),
        mean_reversion=CalibrationMeanReversionConfiguration(
            calibration_grid_interval_hours=36.0,
        ),
    )

    translation = translate_market_data_request_to_prepared_calibration(request)
    prepared_payload = _prepared_payload(translation)

    mean_reversion_stage_input = prepared_payload["mean_reversion_stage_input"]
    assert set(mean_reversion_stage_input) == {
        "calibration_grid_kind",
        "calibration_grid_interval_hours",
        "source_observation_count",
        "coverage_ratio",
        "observations",
    }
    assert mean_reversion_stage_input["calibration_grid_interval_hours"] == pytest.approx(36.0)
    assert (
        "explicit_mean_reversion_grid_interval_hours.v1"
        in mean_reversion_stage_input["calibration_grid_kind"]
    )


def test_translate_market_data_request_derives_fxspot_mean_reversion_interval_from_observed_session_duration(
    tmp_path: Path,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T00:00:00Z,100.0,buy",
                "2026-02-02T12:00:00Z,101.0,sell",
                "2026-02-03T00:00:00Z,102.0,buy",
                "2026-02-03T12:00:00Z,103.0,sell",
                "2026-02-04T00:00:00Z,104.0,buy",
                "2026-02-04T12:00:00Z,105.0,sell",
                "2026-02-05T00:00:00Z,106.0,buy",
                "2026-02-05T12:00:00Z,107.0,sell",
                "2026-02-06T00:00:00Z,108.0,buy",
                "2026-02-06T18:00:00Z,109.0,sell",
                "2026-02-08T21:00:00Z,110.0,buy",
                "2026-02-09T00:00:00Z,111.0,sell",
                "2026-02-09T12:00:00Z,112.0,buy",
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
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        ),
        mean_reversion=CalibrationMeanReversionConfiguration(
        ),
        exchange_code="FXSPOT",
        session_timezone="UTC",
    )

    translation = translate_market_data_request_to_prepared_calibration(request)
    prepared_payload = _prepared_payload(translation)

    mean_reversion_stage_input = prepared_payload["mean_reversion_stage_input"]
    assert set(mean_reversion_stage_input) == {
        "calibration_grid_kind",
        "calibration_grid_interval_hours",
        "source_observation_count",
        "coverage_ratio",
        "observations",
    }
    assert mean_reversion_stage_input["calibration_grid_interval_hours"] == pytest.approx(24.0)
    assert (
        "median_observed_session_duration_hours_from_retained_open_session_ticks.v1"
        in mean_reversion_stage_input["calibration_grid_kind"]
    )
    assert mean_reversion_stage_input["source_observation_count"] == 6
    assert mean_reversion_stage_input["coverage_ratio"] == pytest.approx(6.0 / 7.0)
    assert tuple(
        observation["observation_timestamp_utc_nanos"]
        for observation in mean_reversion_stage_input["observations"]
    ) == (
        _utc_nanos("2026-02-03T00:00:00Z"),
        _utc_nanos("2026-02-04T00:00:00Z"),
        _utc_nanos("2026-02-05T00:00:00Z"),
        _utc_nanos("2026-02-06T00:00:00Z"),
        _utc_nanos("2026-02-07T00:00:00Z"),
        _utc_nanos("2026-02-08T00:00:00Z"),
        _utc_nanos("2026-02-09T00:00:00Z"),
    )


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
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        ),
    )

    translation = translate_market_data_request_to_prepared_calibration(request)

    assert translation.session_resolution.resolution_kind == "automatic_inference"
    assert translation.session_resolution.manual_override_applied is False
    assert translation.session_resolution.selected_policy_id == "sources.sessions.XNYS.v1"
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
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        ),
        exchange_code="BROKER_SPOT",
        session_timezone="UTC",
    )

    with pytest.raises(CalibrationValidationError) as captured_error:
        translate_market_data_request_to_prepared_calibration(request)

    assert captured_error.value.field_path == "session_configuration"
    assert "automatic 24/5 session inference is ambiguous" in str(captured_error.value)


def test_built_in_session_resolution_does_not_require_source_file_access() -> None:
    session_resolution = resolve_market_data_session(
        session_configuration=CalibrationSessionConfigurationInput(
            exchange_code="FXSPOT",
            session_timezone="UTC",
        ),
    )

    assert session_resolution.exchange_code == "FXSPOT"
    assert session_resolution.resolved_timezone_name == "UTC"
    assert session_resolution.selected_policy_id == "sources.sessions.fx_spot_24_5.v1"


def test_built_in_xcme_session_resolution_does_not_require_source_file_access() -> None:
    session_resolution = resolve_market_data_session(
        session_configuration=CalibrationSessionConfigurationInput(
            exchange_code="XCME",
            session_timezone="America/Chicago",
        ),
    )

    assert session_resolution.exchange_code == "XCME"
    assert session_resolution.resolved_timezone_name == "America/Chicago"
    assert session_resolution.selected_policy_id == "sources.sessions.XCME.v1"
    assert len(session_resolution.regular_session_windows) == 10


def test_translate_market_data_request_without_known_news_omits_realized_known_news_samples(
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
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        ),
    )

    translation = translate_market_data_request_to_prepared_calibration(request)
    prepared_payload = _prepared_payload(translation)
    return_components_input = prepared_payload["return_components_candidate_generation_input"]
    assert return_components_input["realized_known_news_return_samples"] == []


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
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        ),
    )

    translation = translate_market_data_request_to_prepared_calibration(request)
    return_components_input = _prepared_payload(translation)[
        "return_components_candidate_generation_input"
    ]

    assert len(return_components_input["observed_open_session_returns"]) == 4
    assert return_components_input["detected_intraday_jump_candidates"] == []
    assert return_components_input["open_session_sell_share_proxy"] == pytest.approx(0.4)


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
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        ),
    )

    translation = translate_market_data_request_to_prepared_calibration(request)
    return_components_input = _prepared_payload(translation)[
        "return_components_candidate_generation_input"
    ]

    assert len(return_components_input["observed_open_session_returns"]) == 1199
    assert return_components_input["open_session_sell_share_proxy"] == pytest.approx(0.5)


def test_translate_market_data_request_preserves_isolated_data_driven_intraday_jump_candidate(
    tmp_path: Path,
    calendar_registry_json: str,
) -> None:
    session_definition_path = _write_session_definition(tmp_path, calendar_registry_json)

    tick_data_path = tmp_path / "isolated_jump_ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,1.085000,buy",
                "2026-02-02T14:30:01Z,1.085001,sell",
                "2026-02-02T14:30:02Z,1.085002,buy",
                "2026-02-02T14:30:03Z,1.086500,sell",
                "2026-02-02T14:30:04Z,1.086501,buy",
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
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        ),
    )

    translation = translate_market_data_request_to_prepared_calibration(request)
    return_components_input = _prepared_payload(translation)[
        "return_components_candidate_generation_input"
    ]

    assert len(return_components_input["detected_intraday_jump_candidates"]) == 1
    detected_jump_candidate = return_components_input["detected_intraday_jump_candidates"][0]
    assert detected_jump_candidate["jump_timestamp_utc_nanos"] == _utc_nanos("2026-02-02T14:30:03Z")
    assert detected_jump_candidate["jump_return"] > 0.0


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
            price_column="trade_price",
            trade_sign_column=None,
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        ),
    )

    translation = translate_market_data_request_to_prepared_calibration(request)

    assert (
        "unsigned_trade_direction_labels_follow_tick_price_change_with_flat_move_carryover.v1"
        in translation.provenance.derivation_policies
    )

    hawkes_candidates = _prepared_payload(translation)["hawkes_candidate_inputs"]
    assert len(hawkes_candidates) > 1

    source_regime = hawkes_candidates[0]["source_regime"]
    assert source_regime["open_session_tick_count"] == 5
    assert source_regime["open_session_buy_tick_count"] == 3
    assert source_regime["open_session_sell_tick_count"] == 2
    assert source_regime["open_session_known_news_activity_share"] == pytest.approx(0.0)

    label_diagnostics = hawkes_candidates[0]["directional_label_diagnostics"]
    assert label_diagnostics["policy_id"] == "tick_price_change_with_flat_move_carryover.v1"
    assert label_diagnostics["inferred_label_count"] == 5
    assert label_diagnostics["price_up_inference_count"] == 2
    assert label_diagnostics["price_down_inference_count"] == 1
    assert label_diagnostics["flat_move_carry_count"] == 1


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
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        ),
    )

    translation = translate_market_data_request_to_prepared_calibration(request)

    hawkes_candidates = _prepared_payload(translation)["hawkes_candidate_inputs"]

    label_diagnostics = hawkes_candidates[0]["directional_label_diagnostics"]
    assert label_diagnostics["provided_label_count"] == 2
    assert label_diagnostics["inferred_label_count"] == 3
    assert label_diagnostics["price_up_inference_count"] == 2
    assert label_diagnostics["price_down_inference_count"] == 1
    assert label_diagnostics["flat_move_carry_count"] == 0
    assert hawkes_candidates[0]["source_regime"]["open_session_buy_tick_count"] == 3
    assert hawkes_candidates[0]["source_regime"]["open_session_sell_tick_count"] == 2


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
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
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
    assert prepared.staged_request.execution_mode == CalibrationExecutionMode.MEMORY
    assert prepared.prepared_data_reference is None
    assert prepared.prepared_payload_path is None
    assert prepared.staged_execution_input_path is None
    assert set(prepared.to_payload()) == {
        "schema_version",
        "request",
        "staged_request",
        "diagnostics",
        "provenance",
        "session_resolution",
        "fit_warning",
        "reference_price",
        "prepared_data_reference",
        "prepared_payload_path",
        "staged_execution_input_path",
    }
    assert prepared.staged_request.selected_stages == (
        CalibrationStage.SEASONALITY,
        CalibrationStage.HAWKES,
        CalibrationStage.RETURN_COMPONENTS,
        CalibrationStage.MEAN_REVERSION,
        CalibrationStage.JOINT_REFINEMENT,
    )
    assert prepared.diagnostics.selected_stages == (
        CalibrationStage.SEASONALITY,
        CalibrationStage.HAWKES,
        CalibrationStage.RETURN_COMPONENTS,
        CalibrationStage.MEAN_REVERSION,
        CalibrationStage.JOINT_REFINEMENT,
    )
    assert prepared.staged_request.selected_stages == prepared.diagnostics.selected_stages
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

    parquet.write_table(
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
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
            option_quote_input_policy=None,
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

    translation = translate_market_data_request_to_prepared_calibration(request)

    assert translation.diagnostics.option_surface_record_count == 1
    assert translation.staged_request.data_inputs.option_surface_data_reference is not None


def test_translate_market_data_request_from_phases_resolves_common_column_names(
    tmp_path: Path,
    calendar_registry_json: str,
) -> None:
    session_definition_path = _write_session_definition(tmp_path, calendar_registry_json)

    tick_data_path = tmp_path / "ticks.parquet"
    parquet.write_table(
        pa.table(
            {
                "ts_recv": [
                    "2026-02-02T14:30:00Z",
                    "2026-02-02T15:00:00Z",
                    "2026-02-02T20:59:00Z",
                ],
                "trade_price": [100.0, 101.0, 102.0],
                "side": ["buy", "sell", "buy"],
                "ignored_label": ["first", "second", "third"],
            }
        ),
        tick_data_path,
    )

    option_surface_path = tmp_path / "options.parquet"
    parquet.write_table(
        pa.table(
            {
                "quote_time": ["2026-02-02T20:00:00Z"],
                "expiry_utc": ["2026-03-20T20:00:00Z"],
                "strike_price": [100.0],
                "right": ["C"],
                "best_bid": [4.0],
                "best_ask": [5.0],
                "ignored_label": ["snapshot"],
            }
        ),
        option_surface_path,
    )

    request = _build_market_data_request(
        session_definition_path=session_definition_path,
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.PARQUET,
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE,
        tick_column_mapping=CalibrationTickDataColumnMapping(),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
            option_quote_input_policy=None,
        ),
        option_surface_source=CalibrationInputFileReference(
            path=str(option_surface_path),
            file_format=CalibrationInputFileFormat.PARQUET,
            provenance_label="options-close",
        ),
        option_surface_column_mapping=CalibrationOptionSurfaceColumnMapping(),
    )

    normalized_market_data = normalize_market_data_source(
        tick_data_source=request.tick_data_source,
        tick_data_column_mapping=request.column_mapping.tick_data,
        normalization_policy=request.normalization_policy,
        require_hawkes=True,
    )
    session_resolution = resolve_market_data_session(
        session_configuration=request.session_configuration,
        normalized_market_data=normalized_market_data,
    )

    translation = translate_market_data_request_to_prepared_calibration_from_phases(
        request=request,
        normalized_market_data=normalized_market_data,
        session_resolution=session_resolution,
    )

    assert translation.diagnostics.tick_record_count == 3
    assert translation.diagnostics.option_surface_record_count == 1
    assert translation.staged_request.data_inputs.option_surface_data_reference is not None


def _build_market_data_request(
    *,
    session_definition_path: Optional[Path],
    tick_data_path: Path,
    tick_data_format: CalibrationInputFileFormat = CalibrationInputFileFormat.CSV,
    workflow_scope: CalibrationMarketDataWorkflowScope,
    tick_column_mapping: CalibrationTickDataColumnMapping,
    normalization_policy: CalibrationMarketDataNormalizationPolicy,
    known_news_inputs: Optional[CalibrationKnownNewsInput] = None,
    known_news_column_mapping: Optional[CalibrationKnownNewsColumnMapping] = None,
    option_surface_source: Optional[CalibrationInputFileReference] = None,
    option_surface_column_mapping: Optional[CalibrationOptionSurfaceColumnMapping] = None,
    mean_reversion: Optional[CalibrationMeanReversionConfiguration] = None,
    preprocessing_settings: Optional[CalibrationMarketDataPreprocessingSettings] = None,
    exchange_code: str = "XNYS",
    session_timezone: Optional[str] = "America/New_York",
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
            file_format=tick_data_format,
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
            emit_input_provenance_report=True,
        ),
        preprocessing_settings=preprocessing_settings,
        option_surface_source=option_surface_source,
        known_news_inputs=known_news_inputs,
        mean_reversion=mean_reversion,
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


def _prepared_payload(translation_result: Any) -> dict[str, Any]:
    return translation_result.prepared_payload()
