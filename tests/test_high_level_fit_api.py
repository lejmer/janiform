from __future__ import annotations

import json
import math
from collections.abc import Mapping
from datetime import UTC, datetime, timedelta
from pathlib import Path
from types import SimpleNamespace
from typing import Any, NoReturn, Self, cast

import pyarrow as pa
import pandas
import pytest
from pyarrow import parquet

import janiform.calibration.fit.estimator as fit_estimator_module
import janiform.calibration.fit.model as fit_model_module
import janiform.calibration.fit.state_estimation as state_estimation_module
import janiform.calibration.market_data.ingestion as market_data_ingestion_module
from janiform._timestamps import datetime_to_utc_nanos
from janiform.calibration import CalibrationValidationError
from janiform.calibration.fit import (
    CurrentStateEstimationRequest,
    CurrentStateEstimationResult,
    JaniformEstimator,
    JaniformFitReport,
    JaniformModel,
    JaniformSimulationResult,
    JaniformVanillaOptionBatch,
    JaniformVanillaOptionBatchPricingResult,
    JaniformVanillaOptionSurface,
    JaniformVanillaOptionSurfacePricingResult,
)
from janiform.calibration.market_data import (
    CalibrationDuplicateRecordPolicy,
    CalibrationInputFileFormat,
    CalibrationMeanReversionConfiguration,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
    CalibrationMarketDataPreprocessingSettings,
    CalibrationMarketDataReturnComponentsPreprocessingSettings,
    CalibrationMarketDataSeasonalityPreprocessingSettings,
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
)
from janiform.calibration.presets import PresetCategory
from janiform.calibration.staged import (
    CalibrationOptimizerSettings,
    CalibrationStage,
    CalibrationStageSolverSettings,
    StagedCalibrationResponse,
    validate_staged_calibration_response,
)
from janiform.calendar import CalendarRegistry, MarketState
from janiform.hourly_factors import HourlyFactorContract
from janiform.mean_reversion import (
    MeanReversionAnchorModel,
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
)
from janiform.pricing import (
    VanillaOptionImpliedVolatilityInversionStatus,
    VanillaOptionPayoffType,
)
from janiform.seasonality import IntradaySmoothingKernel, SessionFilter
from janiform.simulation import SeededTickEventSide, SeededTickEventStateSnapshotCapture
from tests.support.calibration import (
    build_endogenous_mean_reversion_fitted_parameters_payload,
    build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload,
    update_model_mean_reversion_payload,
)


def test_janiform_estimator_fit_returns_model_for_default_high_level_workflow(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc",
                "2026-02-02T14:30:00Z",
                "2026-02-02T15:00:00Z",
                "2026-02-02T20:59:00Z",
                "2026-02-03T14:30:00Z",
                "2026-02-03T20:59:00Z",
            ]
        ),
        encoding="utf-8",
    )

    estimator = JaniformEstimator(
        workflow_scope=CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY,
        tick_data_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
        ),
        market_profile="us_equity_cash",
    )

    _patch_successful_staged_runner(
        monkeypatch,
        run_id="high-level-fit-success",
    )

    model = estimator.fit(
        run_id="high-level-fit-success",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
    )

    assert isinstance(model, JaniformModel)
    assert "seasonality" in model.parameters
    assert model.metadata["market_profile"] == "us_equity_cash"
    assert model.metadata["selected_stages"] == ["seasonality"]


def test_janiform_estimator_supports_simple_zero_config_tick_fit(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "simple_ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:00:00Z,101.0,sell",
                "2026-02-02T20:59:00Z,102.0,buy",
                "2026-02-03T14:30:00Z,99.0,sell",
            ]
        ),
        encoding="utf-8",
    )

    monkeypatch.setattr(
        fit_estimator_module,
        "_run_prepared_staged_calibration_with_raw_payload",
        lambda prepared_handle, **kwargs: _successful_full_model_staged_runner_payload(
            _prepared_runner_run_id(prepared_handle)
        ),
    )

    estimator = JaniformEstimator(
        market_profile="us_equity_cash",
    )

    model = estimator.fit(
        tick_data_path=tick_data_path,
    )

    assert isinstance(model, JaniformModel)
    assert model.run_id.startswith("simple_ticks_")
    assert model.metadata["workflow_scope"] == "full_without_option_surface"
    assert model.metadata["input_inference"]["tick_data_format"] == "csv"
    assert model.metadata["input_inference"]["timestamp_column"] == "timestamp_utc"
    assert model.metadata["input_inference"]["tick_price_input_policy"] == "explicit_price_column"
    assert model.metadata["input_inference"]["tick_price_columns"]["price_column"] == "trade_price"
    assert model.metadata["input_inference"]["tick_price_columns"]["trade_sign_column"] == "trade_sign"


def test_prepare_fit_workflow_uses_column_only_parquet_schema_discovery(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.parquet"
    parquet.write_table(
        pa.table(
            {
                "time": [datetime(2026, 2, 2, 14, 30, tzinfo=UTC)],
                "close": [1.0850],
            }
        ),
        tick_data_path,
    )

    class _StopAfterInference(Exception):
        pass

    def _unexpected_read_table(*args: object, **kwargs: object) -> NoReturn:
        del args, kwargs
        raise AssertionError("fit input inference should not materialize a parquet table")

    def _stop_after_inference(*args: object, **kwargs: object) -> NoReturn:
        del args, kwargs
        raise _StopAfterInference()

    monkeypatch.setattr(market_data_ingestion_module.parquet, "read_table", _unexpected_read_table)
    monkeypatch.setattr(
        fit_estimator_module,
        "translate_market_data_request_to_prepared_calibration_from_phases",
        _stop_after_inference,
    )

    estimator = JaniformEstimator(market_profile="eurusd_spot")

    with pytest.raises(_StopAfterInference):
        estimator._prepare_fit_workflow(
            run_id="high-level-parquet-schema-only",
            tick_data_path=tick_data_path,
            tick_data_format=CalibrationInputFileFormat.PARQUET,
            tick_data_provenance_label="tick_data",
            option_surface_path=None,
            option_surface_format=CalibrationInputFileFormat.PARQUET,
            option_surface_provenance_label="option_surface",
        )


def test_prepare_fit_workflow_passes_explicit_mean_reversion_request_to_translation(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:00:00Z,101.0,sell",
                "2026-02-02T20:59:00Z,102.0,buy",
                "2026-02-03T14:30:00Z,99.0,sell",
            ]
        ),
        encoding="utf-8",
    )

    class _StopAfterTranslation(Exception):
        pass

    captured_request: dict[str, Any] = {}

    def _capture_request(request, **kwargs):
        captured_request["request"] = request
        del kwargs
        raise _StopAfterTranslation()

    monkeypatch.setattr(
        fit_estimator_module,
        "translate_market_data_request_to_prepared_calibration_from_phases",
        _capture_request,
    )

    estimator = JaniformEstimator(
        market_profile="us_equity_cash",
        mean_reversion=CalibrationMeanReversionConfiguration(
            calibration_grid_interval_hours=36.0,
        ),
    )

    with pytest.raises(_StopAfterTranslation):
        estimator._prepare_fit_workflow(
            run_id="high-level-mean-reversion-request",
            tick_data_path=tick_data_path,
            tick_data_format=CalibrationInputFileFormat.CSV,
            tick_data_provenance_label="tick_data",
            option_surface_path=None,
            option_surface_format=CalibrationInputFileFormat.PARQUET,
            option_surface_provenance_label="option_surface",
        )

    request = captured_request["request"]
    assert request.mean_reversion is not None
    assert request.mean_reversion.calibration_grid_interval_hours == pytest.approx(36.0)


def test_prepare_fit_workflow_passes_optimizer_settings_by_stage_to_translation(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:00:00Z,101.0,sell",
                "2026-02-02T20:59:00Z,102.0,buy",
                "2026-02-03T14:30:00Z,99.0,sell",
            ]
        ),
        encoding="utf-8",
    )

    class _StopAfterTranslation(Exception):
        pass

    captured_request: dict[str, Any] = {}

    def _capture_request(request, **kwargs):
        captured_request["request"] = request
        del kwargs
        raise _StopAfterTranslation()

    monkeypatch.setattr(
        fit_estimator_module,
        "translate_market_data_request_to_prepared_calibration_from_phases",
        _capture_request,
    )

    optimizer_settings_by_stage = (
        CalibrationStageSolverSettings(
            stage=CalibrationStage.HAWKES,
            optimizer_settings=CalibrationOptimizerSettings(
                solver_name="lbfgs",
                max_iterations=321,
                cpu_count=2,
                absolute_tolerance=1e-9,
                relative_tolerance=1e-7,
                constraint_penalty_weight=15.0,
            ),
        ),
    )
    estimator = JaniformEstimator(
        market_profile="us_equity_cash",
        optimizer_settings_by_stage=optimizer_settings_by_stage,
    )

    with pytest.raises(_StopAfterTranslation):
        estimator._prepare_fit_workflow(
            run_id="high-level-optimizer-settings-request",
            tick_data_path=tick_data_path,
            tick_data_format=CalibrationInputFileFormat.CSV,
            tick_data_provenance_label="tick_data",
            option_surface_path=None,
            option_surface_format=CalibrationInputFileFormat.PARQUET,
            option_surface_provenance_label="option_surface",
        )

    request = captured_request["request"]
    assert request.optimizer_settings_by_stage == optimizer_settings_by_stage


def test_prepare_fit_workflow_passes_preprocessing_settings_to_translation(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:00:00Z,101.0,sell",
                "2026-02-02T20:59:00Z,102.0,buy",
                "2026-02-03T14:30:00Z,99.0,sell",
            ]
        ),
        encoding="utf-8",
    )

    class _StopAfterTranslation(Exception):
        pass

    captured_request: dict[str, Any] = {}

    def _capture_request(request, **kwargs):
        captured_request["request"] = request
        del kwargs
        raise _StopAfterTranslation()

    monkeypatch.setattr(
        fit_estimator_module,
        "translate_market_data_request_to_prepared_calibration_from_phases",
        _capture_request,
    )

    preprocessing_settings = CalibrationMarketDataPreprocessingSettings(
        seasonality=CalibrationMarketDataSeasonalityPreprocessingSettings(
            session_filter=SessionFilter.ALL,
            sparse_hour_event_threshold=2,
            known_news_overlay_log_adjustment=-0.1,
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
    )
    estimator = JaniformEstimator(
        market_profile="us_equity_cash",
        preprocessing_settings=preprocessing_settings,
    )

    with pytest.raises(_StopAfterTranslation):
        estimator._prepare_fit_workflow(
            run_id="high-level-preprocessing-settings-request",
            tick_data_path=tick_data_path,
            tick_data_format=CalibrationInputFileFormat.CSV,
            tick_data_provenance_label="tick_data",
            option_surface_path=None,
            option_surface_format=CalibrationInputFileFormat.PARQUET,
            option_surface_provenance_label="option_surface",
        )

    request = captured_request["request"]
    assert request.preprocessing_settings == preprocessing_settings


def test_prepare_model_runtime_factors_uses_fitted_surprise_intensity_profile(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:00:00Z,100.0,buy",
                "2026-02-02T14:30:00Z,100.1,sell",
                "2026-02-02T15:00:00Z,100.2,buy",
                "2026-02-02T15:30:00Z,100.3,sell",
            ]
        ),
        encoding="utf-8",
    )

    monkeypatch.setattr(
        fit_estimator_module,
        "_run_prepared_staged_calibration_with_raw_payload",
        lambda prepared_handle, **kwargs: _successful_full_model_staged_runner_payload(
            _prepared_runner_run_id(prepared_handle)
        ),
    )

    estimator = JaniformEstimator(market_profile="us_equity_cash")
    model = estimator.fit(tick_data_path=tick_data_path)
    prepared_inputs = fit_model_module._prepare_model_simulation_inputs(
        model,
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 14, 0, tzinfo=UTC),
        horizon_interval_count=2,
        interval="1h",
        simulation_kind="coarse_interval",
        initial_excitation_state=None,
        debug_trace_enabled=False,
        sample_every_nth_transition=1,
        max_records_per_chunk=1_000,
        state_snapshot_capture=SeededTickEventStateSnapshotCapture.DISABLED,
    )
    assert prepared_inputs.coarse_request is not None

    assert prepared_inputs.coarse_request.hourly_factor_contract.surprise_component_nu_h[0] == pytest.approx(0.04)
    assert prepared_inputs.coarse_request.hourly_factor_contract.surprise_component_nu_h[1] == pytest.approx(0.02)


def test_janiform_estimator_preserves_duplicate_ticks_by_default(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "duplicate_ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:00:00Z,101.0,sell",
                "2026-02-02T20:59:00Z,102.0,buy",
                "2026-02-03T14:30:00Z,99.0,sell",
            ]
        ),
        encoding="utf-8",
    )

    monkeypatch.setattr(
        fit_estimator_module,
        "_run_prepared_staged_calibration_with_raw_payload",
        lambda prepared_handle, **kwargs: _successful_full_model_staged_runner_payload(
            _prepared_runner_run_id(prepared_handle)
        ),
    )

    estimator = JaniformEstimator(
        market_profile="us_equity_cash",
    )

    model = estimator.fit(
        tick_data_path=tick_data_path,
    )

    assert isinstance(model, JaniformModel)
    assert model.run_id.startswith("duplicate_ticks_")


def test_high_level_session_calendar_registry_preserves_fx_weekend_closure() -> None:
    registry = fit_estimator_module.PresetRegistry()
    _, session_artifact = registry.resolve(
        category=PresetCategory.SESSION,
        preset_name="fx_spot_24_5",
    )

    calendar_registry = CalendarRegistry(
        fit_estimator_module._calendar_registry_json_from_session_configuration(
            session_artifact.configuration
        )
    )

    assert (
        calendar_registry.resolve_market_state(
            "FXSPOT",
            datetime(2026, 2, 6, 18, 59, tzinfo=UTC),
        )
        == MarketState.OPEN
    )
    assert (
        calendar_registry.resolve_market_state(
            "FXSPOT",
            datetime(2026, 2, 6, 19, 0, tzinfo=UTC),
        )
        == MarketState.CLOSED
    )
    assert (
        calendar_registry.resolve_market_state(
            "FXSPOT",
            datetime(2026, 2, 8, 20, 59, tzinfo=UTC),
        )
        == MarketState.CLOSED
    )
    assert (
        calendar_registry.resolve_market_state(
            "FXSPOT",
            datetime(2026, 2, 8, 21, 0, tzinfo=UTC),
        )
        == MarketState.OPEN
    )


def test_janiform_estimator_fit_report_true_returns_model_and_report(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc",
                "2026-02-02T14:30:00Z",
                "2026-02-02T15:00:00Z",
                "2026-02-02T20:59:00Z",
                "2026-02-03T14:30:00Z",
                "2026-02-03T20:59:00Z",
            ]
        ),
        encoding="utf-8",
    )

    estimator = JaniformEstimator(
        workflow_scope=CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY,
        tick_data_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
        ),
        market_profile="us_equity_cash",
    )

    _patch_successful_staged_runner(
        monkeypatch,
        run_id="high-level-fit-with-report",
    )

    model, fit_report = estimator.fit(
        run_id="high-level-fit-with-report",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
        report=True,
    )

    assert isinstance(model, JaniformModel)
    assert isinstance(fit_report, JaniformFitReport)
    assert fit_report.success is True
    assert fit_report.resolved_configuration.market_profile is not None
    assert fit_report.resolved_configuration.market_profile.selected_preset is not None
    assert (
        fit_report.resolved_configuration.market_profile.selected_preset.preset_name
        == "us_equity_cash"
    )
    assert fit_report.resolved_configuration.known_news.selected_preset is not None
    assert fit_report.resolved_configuration.known_news.selected_preset.preset_name == "none"
    assert fit_report.resolved_configuration.known_news.recommended_reference is not None
    assert (
        fit_report.resolved_configuration.known_news.recommended_reference.preset_name
        == "us_macro_core"
    )


def test_high_level_fit_does_not_retain_raw_market_data_paths_on_model(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
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

    estimator = JaniformEstimator(
        workflow_scope=CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY,
        tick_data_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
        ),
        market_profile="us_equity_cash",
    )

    _patch_successful_staged_runner(
        monkeypatch,
        run_id="high-level-fit-no-retention",
    )

    model = estimator.fit(
        run_id="high-level-fit-no-retention",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
    )
    serialized_model = json.dumps(
        {
            "parameters": model.parameters,
            "metadata": model.metadata,
        },
        sort_keys=True,
    )

    assert str(tick_data_path) not in serialized_model
    assert not hasattr(estimator, "_tick_data_path")


def test_high_level_model_summary_exposes_fitted_parameters_and_provenance(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-summary",
    )

    summary = model.summary()

    assert "hawkes" in summary["fitted_parameters"]
    assert "return_components" in summary["fitted_parameters"]
    assert "mean_reversion" in summary["fitted_parameters"]
    assert (
        summary["preset_provenance"]["known_news"]["selected_preset"]["preset_name"]
        == "none"
    )
    assert summary["known_news_activation"] == {
        "selected_preset": "none",
        "event_count": 0,
        "active": False,
    }
    assert summary["session_resolution"]["resolution_kind"] == "automatic_inference"
    assert summary["session_resolution"]["provenance_kind"] == "bundled_preset"
    assert summary["fitted_parameter_summary"]["selected_candidate_ids"] == {
        "hawkes": "candidate_stable_low_loss",
        "return_components": "return_candidate_balanced",
        "joint_refinement": "joint_refinement_baseline",
    }


def test_high_level_model_save_and_load_roundtrip_preserves_deterministic_metadata(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-persistence",
    )
    first_path = tmp_path / "model-first.json"
    second_path = tmp_path / "model-second.json"

    model.save(first_path)
    loaded_model = JaniformModel.load(first_path)
    loaded_model.save(second_path)
    persisted_payload = json.loads(first_path.read_text(encoding="utf-8"))

    assert loaded_model.parameters == model.parameters
    assert loaded_model.metadata == model.metadata
    assert loaded_model.summary() == model.summary()
    assert "execution_input" not in persisted_payload
    assert "stage_report_payloads" not in persisted_payload
    assert (
        persisted_payload["parameters"]["return_components"]["fitted_parameters"][
            "mean_reversion_fitted_parameters"
        ]
        == build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload()
    )
    assert (
        persisted_payload["parameters"]["mean_reversion"]["fitted_parameters"]
        == build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload()
    )
    assert "timestamp_utc_nanos" not in persisted_payload["parameters"]["seasonality"]
    assert "normalized_combined_factor_by_event" not in persisted_payload["parameters"]["seasonality"]
    assert first_path.read_text(encoding="utf-8").startswith("{\n  ")
    assert first_path.read_text(encoding="utf-8") == second_path.read_text(encoding="utf-8")


def test_high_level_model_save_and_load_roundtrip_preserves_endogenous_mean_reversion_block(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-mean-reversion-roundtrip",
    )

    mean_reversion_payload = MeanReversionFittedParameters(
        kappa=0.25,
        anchor_model=MeanReversionAnchorModel.zero_gain_endogenous_core_pressure_level(),
    ).to_payload()

    model.parameters["return_components"]["fitted_parameters"][
        "mean_reversion_fitted_parameters"
    ] = mean_reversion_payload
    model.parameters["mean_reversion"]["fitted_parameters"] = mean_reversion_payload
    model.parameters["joint_refinement"]["fitted_parameters"]["return_components_fitted_parameters"][
        "mean_reversion_fitted_parameters"
    ] = mean_reversion_payload
    model.parameters["joint_refinement"]["baseline_return_components_fitted_parameters"][
        "mean_reversion_fitted_parameters"
    ] = mean_reversion_payload

    persisted_model_path = tmp_path / "model-mean-reversion.json"

    model.save(persisted_model_path)
    loaded_model = JaniformModel.load(persisted_model_path)

    assert (
        loaded_model.parameters["return_components"]["fitted_parameters"][
            "mean_reversion_fitted_parameters"
        ]
        == mean_reversion_payload
    )
    assert (
        loaded_model.parameters["mean_reversion"]["fitted_parameters"] == mean_reversion_payload
    )
    assert (
        loaded_model.parameters["joint_refinement"]["fitted_parameters"][
            "return_components_fitted_parameters"
        ]["mean_reversion_fitted_parameters"]
        == mean_reversion_payload
    )
    assert (
        loaded_model.parameters["joint_refinement"]["baseline_return_components_fitted_parameters"][
            "mean_reversion_fitted_parameters"
        ]
        == mean_reversion_payload
    )


def test_loaded_high_level_model_runtime_seasonality_prefers_compact_parameters(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-native-seasonality",
    )

    persisted_model_path = tmp_path / "model.json"

    model.save(persisted_model_path)
    loaded_model = JaniformModel.load(persisted_model_path)

    calendar_registry = CalendarRegistry(
        loaded_model.session_resolution.resolved_session_definition_json
    )

    def _materialize_runtime_structural_seasonality(
        native_calendar_registry: Any,
        exchange_code: str,
        range_start_utc: str,
        range_end_utc: str,
        interval_duration_nanos: int,
        intraday_minutes_per_bin: int,
        intraday_factor_by_bin: tuple[float, ...],
        day_of_week_factor: tuple[float, ...],
        day_of_year_factor: tuple[float, ...],
    ) -> dict[str, Any]:
        assert native_calendar_registry is calendar_registry._native_registry
        assert exchange_code == loaded_model.session_resolution.exchange_code
        assert range_start_utc == "2026-02-02T14:00:00+00:00"
        assert range_end_utc == "2026-02-02T16:00:00+00:00"
        assert interval_duration_nanos == 3_600_000_000_000
        assert intraday_minutes_per_bin == 480
        assert len(intraday_factor_by_bin) == 3
        assert len(day_of_week_factor) == 7
        assert len(day_of_year_factor) == 366
        return {
            "interval_start_utc_nanos": [
                datetime_to_utc_nanos(datetime(2026, 2, 2, 14, tzinfo=UTC)),
                datetime_to_utc_nanos(datetime(2026, 2, 2, 15, tzinfo=UTC)),
            ],
            "seasonality_factors": [1.0, 1.0],
        }

    monkeypatch.setattr(
        fit_model_module,
        "native_module",
        lambda: SimpleNamespace(
            materialize_runtime_structural_seasonality=(
                _materialize_runtime_structural_seasonality
            )
        ),
    )

    seasonality_source = fit_model_module._build_runtime_hourly_seasonality_source_from_model(
        loaded_model,
        calendar_registry=calendar_registry,
        exchange_code=loaded_model.session_resolution.exchange_code,
        range_start_utc=datetime(2026, 2, 2, 14, tzinfo=UTC),
        range_end_utc=datetime(2026, 2, 2, 16, tzinfo=UTC),
        interval="1h",
    )

    assert seasonality_source.hourly_factors == (1.0, 1.0)


def test_loaded_high_level_model_runtime_seasonality_infers_intraday_bin_width(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-infer-seasonality-bin-width",
    )
    persisted_model_path = tmp_path / "model.json"

    model.save(persisted_model_path)
    loaded_model = JaniformModel.load(persisted_model_path)

    loaded_model.parameters["seasonality"] = {
        **loaded_model.parameters["seasonality"],
        "intraday_minutes_per_bin": None,
    }

    calendar_registry = CalendarRegistry(
        loaded_model.session_resolution.resolved_session_definition_json
    )

    def _materialize_runtime_structural_seasonality(
        native_calendar_registry: Any,
        exchange_code: str,
        range_start_utc: str,
        range_end_utc: str,
        interval_duration_nanos: int,
        intraday_minutes_per_bin: int,
        intraday_factor_by_bin: tuple[float, ...],
        day_of_week_factor: tuple[float, ...],
        day_of_year_factor: tuple[float, ...],
    ) -> dict[str, Any]:
        assert native_calendar_registry is calendar_registry._native_registry
        assert exchange_code == loaded_model.session_resolution.exchange_code
        assert range_start_utc == "2026-02-02T14:00:00+00:00"
        assert range_end_utc == "2026-02-02T16:00:00+00:00"
        assert interval_duration_nanos == 3_600_000_000_000
        assert intraday_minutes_per_bin == 480
        assert len(intraday_factor_by_bin) == 3
        assert len(day_of_week_factor) == 7
        assert len(day_of_year_factor) == 366
        return {
            "interval_start_utc_nanos": [
                datetime_to_utc_nanos(datetime(2026, 2, 2, 14, tzinfo=UTC)),
                datetime_to_utc_nanos(datetime(2026, 2, 2, 15, tzinfo=UTC)),
            ],
            "seasonality_factors": [1.0, 1.0],
        }

    monkeypatch.setattr(
        fit_model_module,
        "native_module",
        lambda: SimpleNamespace(
            materialize_runtime_structural_seasonality=(
                _materialize_runtime_structural_seasonality
            )
        ),
    )

    seasonality_source = fit_model_module._build_runtime_hourly_seasonality_source_from_model(
        loaded_model,
        calendar_registry=calendar_registry,
        exchange_code=loaded_model.session_resolution.exchange_code,
        range_start_utc=datetime(2026, 2, 2, 14, tzinfo=UTC),
        range_end_utc=datetime(2026, 2, 2, 16, tzinfo=UTC),
        interval="1h",
    )

    assert seasonality_source.hourly_factors == (1.0, 1.0)


def test_loaded_high_level_model_runtime_seasonality_requires_structural_parameters(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-structural-seasonality-required",
    )

    persisted_model_path = tmp_path / "missing-structural-seasonality-model.json"

    model.save(persisted_model_path)
    loaded_model = JaniformModel.load(persisted_model_path)

    loaded_model.parameters["seasonality"] = {
        "schema_version": loaded_model.parameters["seasonality"]["schema_version"],
        "exchange_code": loaded_model.parameters["seasonality"]["exchange_code"],
    }

    registry = CalendarRegistry(
        loaded_model.session_resolution.resolved_session_definition_json
    )

    with pytest.raises(
        RuntimeError,
        match="JaniformModel does not contain structural seasonality parameters required for runtime materialization",
    ):
        fit_model_module._build_runtime_hourly_seasonality_source_from_model(
            loaded_model,
            calendar_registry=registry,
            exchange_code=loaded_model.session_resolution.exchange_code,
            range_start_utc=datetime(2026, 2, 2, 14, tzinfo=UTC),
            range_end_utc=datetime(2026, 2, 2, 16, tzinfo=UTC),
            interval="1h",
        )


def test_high_level_runtime_seasonality_zeroes_closed_intervals_for_coarse_inputs(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-closed-seasonality-gating",
    )

    prepared_inputs = fit_model_module._prepare_model_simulation_inputs(
        model,
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 14, 0, tzinfo=UTC),
        horizon_interval_count=4,
        interval="15min",
        simulation_kind="coarse_interval",
        initial_excitation_state=None,
        debug_trace_enabled=False,
        sample_every_nth_transition=1,
        max_records_per_chunk=1_000,
        state_snapshot_capture=SeededTickEventStateSnapshotCapture.DISABLED,
    )
    assert prepared_inputs.coarse_request is not None

    assert prepared_inputs.coarse_request.hourly_factor_contract.s_h == pytest.approx(
        (0.0, 0.0, 1.0, 1.0)
    )


def test_high_level_model_simulate_dispatches_by_interval_mode(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-simulate",
    )

    prepared_inputs = fit_model_module._PreparedModelSimulationInputs(
        coarse_request=object(),
        seeded_request=object(),
    )

    captured_calls: list[tuple[str, Any]] = []
    coarse_result = object()
    tick_result = object()

    monkeypatch.setattr(
        fit_model_module,
        "_prepare_model_simulation_inputs",
        lambda *args, **kwargs: prepared_inputs,
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_coarse_simulation",
        lambda request, **kwargs: captured_calls.append(("coarse", request)) or coarse_result,
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_seeded_tick_event_simulation",
        lambda request, **kwargs: captured_calls.append(("ticks", request)) or tick_result,
    )
    monkeypatch.setattr(
        fit_model_module,
        "_build_high_level_simulation_result",
        lambda **kwargs: JaniformSimulationResult(
            simulation_kind=(
                "seeded_tick"
                if kwargs["simulation_result"] is tick_result
                else "coarse_interval"
            ),
            metadata={},
            provenance={},
            diagnostics={},
            raw_result=kwargs["simulation_result"],
        ),
    )

    observed_seeded = model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=2,
    )
    observed_coarse = model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=2,
        interval="15min",
    )

    assert isinstance(observed_seeded, JaniformSimulationResult)
    assert isinstance(observed_coarse, JaniformSimulationResult)
    assert observed_seeded.raw_result is tick_result
    assert observed_coarse.raw_result is coarse_result
    assert observed_seeded.simulation_kind == "seeded_tick"
    assert observed_coarse.simulation_kind == "coarse_interval"
    assert captured_calls == [
        ("ticks", prepared_inputs.seeded_request),
        ("coarse", prepared_inputs.coarse_request),
    ]


def test_high_level_model_simulate_forwards_progress_events_to_callback_and_renderer(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-progress",
    )

    prepared_inputs = fit_model_module._PreparedModelSimulationInputs(
        coarse_request=None,
        seeded_request=object(),
    )
    simulated_result = object()
    observed_progress_updates: list[tuple[str | None, int | None, int | None]] = []
    observed_callback_events: list[tuple[str, int, int]] = []

    class _CapturingProgress:
        def __enter__(self) -> Self:
            return self

        def __exit__(self, exc_type, exc_value, traceback) -> None:
            _ = exc_type
            _ = exc_value
            _ = traceback
            return None

        def update(
            self,
            *,
            detail: str | None = None,
            completed_units: int | None = None,
            total_units: int | None = None,
        ) -> None:
            observed_progress_updates.append((detail, completed_units, total_units))

    monkeypatch.setattr(
        fit_model_module,
        "_prepare_model_simulation_inputs",
        lambda *args, **kwargs: prepared_inputs,
    )
    monkeypatch.setattr(
        fit_model_module.JaniformOutputController,
        "progress",
        lambda self, label, total_steps=None: _CapturingProgress(),
    )

    def fake_seeded_runtime(
        request: Any,
        *,
        progress_callback: Any = None,
    ) -> Any:
        del request
        assert progress_callback is not None
        progress_callback(
            fit_model_module.SimulationProgressEvent(
                phase="seeded_tick_intervals",
                completed_units=2,
                total_units=5,
            )
        )
        return simulated_result

    monkeypatch.setattr(
        fit_model_module,
        "run_seeded_tick_event_simulation",
        fake_seeded_runtime,
    )
    monkeypatch.setattr(
        fit_model_module,
        "_build_high_level_simulation_result",
        lambda **kwargs: JaniformSimulationResult(
            simulation_kind="seeded_tick",
            metadata={},
            provenance={},
            diagnostics={},
            raw_result=kwargs["simulation_result"],
        ),
    )

    result = model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=5,
        progress_callback=lambda event: observed_callback_events.append(
            (event.detail, event.completed_units, event.total_units)
        ),
    )

    assert result.raw_result is simulated_result
    assert observed_progress_updates == [("seeded tick path generation", 2, 5)]
    assert observed_callback_events == [("seeded tick path generation", 2, 5)]


def test_high_level_model_simulate_uses_estimated_current_state_when_continuation_data_is_supplied(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-simulate-estimated-current-state",
    )
    captured_requests: dict[str, Any] = {}
    captured_estimation_calls: list[tuple[datetime, CurrentStateEstimationRequest]] = []
    seeded_result = object()
    coarse_result = object()
    estimated_state = CurrentStateEstimationResult(
        hawkes_excitation_state=fit_model_module.HawkesExcitationState(
            y_plus_plus=0.45,
            y_plus_minus=0.15,
            y_minus_plus=0.3,
            y_minus_minus=0.2,
        ),
        mean_reversion_boundary_state=MeanReversionBoundaryState(
            observed_log_price=0.05,
            anchor_value=0.05,
            pressure_state=0.0,
            deviation=0.0,
        ),
    )
    continuation_data = CurrentStateEstimationRequest(
        observed_tick_data=pandas.DataFrame(
            {
                "timestamp_utc": [datetime(2026, 2, 2, 14, 30, tzinfo=UTC)],
                "trade_price": [100.0],
                "trade_sign": ["buy"],
            }
        ),
    )

    monkeypatch.setattr(
        fit_model_module,
        "estimate_current_state_for_model",
        lambda model_value, *, state_time_utc, request: (
            captured_estimation_calls.append((state_time_utc, request)) or estimated_state
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "provide_hourly_factor_contract",
        lambda **kwargs: _build_test_hourly_factor_contract(
            start_time_utc=cast(datetime, kwargs["range_start_utc"]),
            interval_end_times_utc=(cast(datetime, kwargs["range_end_utc"]),),
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_seeded_tick_event_simulation",
        lambda request, **kwargs: _capture_request(
            captured_requests,
            "seeded",
            request,
            seeded_result,
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_coarse_simulation",
        lambda request, **kwargs: _capture_request(
            captured_requests,
            "coarse",
            request,
            coarse_result,
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "_build_high_level_simulation_result",
        lambda **kwargs: JaniformSimulationResult(
            simulation_kind="seeded_tick",
            metadata={},
            provenance={},
            diagnostics={},
            raw_result=kwargs["simulation_result"],
        ),
    )

    model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=1,
        continuation_data=continuation_data,
    )
    model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=1,
        interval="15min",
        continuation_data=continuation_data,
    )

    assert captured_estimation_calls == [
        (datetime(2026, 2, 2, 15, tzinfo=UTC), continuation_data),
        (datetime(2026, 2, 2, 15, tzinfo=UTC), continuation_data),
    ]
    assert captured_requests["seeded"].initial_excitation_state == estimated_state.hawkes_excitation_state
    assert (
        captured_requests["seeded"].initial_mean_reversion_boundary_state
        == estimated_state.mean_reversion_boundary_state
    )
    coarse_model_inputs = captured_requests["coarse"].model_inputs.risk_model_inputs
    assert coarse_model_inputs.initial_excitation_state == estimated_state.hawkes_excitation_state
    assert (
        coarse_model_inputs.initial_mean_reversion_boundary_state
        == estimated_state.mean_reversion_boundary_state
    )


def test_high_level_model_simulate_estimates_mean_reversion_boundary_when_hawkes_override_is_supplied(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-simulate-hawkes-override-with-continuation",
    )
    captured_estimation_calls: list[tuple[datetime, CurrentStateEstimationRequest]] = []
    captured_requests: dict[str, Any] = {}
    estimated_state = CurrentStateEstimationResult(
        hawkes_excitation_state=fit_model_module.HawkesExcitationState(
            y_plus_plus=0.45,
            y_plus_minus=0.15,
            y_minus_plus=0.3,
            y_minus_minus=0.2,
        ),
        mean_reversion_boundary_state=MeanReversionBoundaryState(
            observed_log_price=0.05,
            anchor_value=0.05,
            pressure_state=0.1,
            deviation=0.0,
        ),
    )
    explicit_initial_excitation_state = fit_model_module.HawkesExcitationState(
        y_plus_plus=0.9,
        y_plus_minus=0.8,
        y_minus_plus=0.7,
        y_minus_minus=0.6,
    )
    continuation_data = CurrentStateEstimationRequest(
        observed_tick_data=pandas.DataFrame(
            {
                "timestamp_utc": [datetime(2026, 2, 2, 14, 30, tzinfo=UTC)],
                "trade_price": [100.0],
                "trade_sign": ["buy"],
            }
        ),
    )

    monkeypatch.setattr(
        fit_model_module,
        "estimate_current_state_for_model",
        lambda model_value, *, state_time_utc, request: (
            captured_estimation_calls.append((state_time_utc, request)) or estimated_state
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "provide_hourly_factor_contract",
        lambda **kwargs: _build_test_hourly_factor_contract(
            start_time_utc=cast(datetime, kwargs["range_start_utc"]),
            interval_end_times_utc=(cast(datetime, kwargs["range_end_utc"]),),
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_seeded_tick_event_simulation",
        lambda request, **kwargs: _capture_request(
            captured_requests,
            "seeded",
            request,
            object(),
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "_build_high_level_simulation_result",
        lambda **kwargs: JaniformSimulationResult(
            simulation_kind="seeded_tick",
            metadata={},
            provenance={},
            diagnostics={},
            raw_result=kwargs["simulation_result"],
        ),
    )

    model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=1,
        continuation_data=continuation_data,
        initial_excitation_state=explicit_initial_excitation_state,
    )

    assert captured_estimation_calls == [
        (datetime(2026, 2, 2, 15, tzinfo=UTC), continuation_data)
    ]
    assert captured_requests["seeded"].initial_excitation_state == explicit_initial_excitation_state
    assert (
        captured_requests["seeded"].initial_mean_reversion_boundary_state
        == estimated_state.mean_reversion_boundary_state
    )


def test_high_level_model_simulate_uses_explicit_mean_reversion_boundary_override(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-simulate-boundary-override",
    )
    captured_estimation_calls: list[tuple[datetime, CurrentStateEstimationRequest]] = []
    captured_requests: dict[str, Any] = {}
    estimated_state = CurrentStateEstimationResult(
        hawkes_excitation_state=fit_model_module.HawkesExcitationState(
            y_plus_plus=0.45,
            y_plus_minus=0.15,
            y_minus_plus=0.3,
            y_minus_minus=0.2,
        ),
        mean_reversion_boundary_state=MeanReversionBoundaryState(
            observed_log_price=0.05,
            anchor_value=0.05,
            pressure_state=0.1,
            deviation=0.0,
        ),
    )
    explicit_boundary_state = MeanReversionBoundaryState(
        observed_log_price=0.2,
        anchor_value=0.2,
        pressure_state=0.15,
        deviation=0.0,
    )
    continuation_data = CurrentStateEstimationRequest(
        observed_tick_data=pandas.DataFrame(
            {
                "timestamp_utc": [datetime(2026, 2, 2, 14, 30, tzinfo=UTC)],
                "trade_price": [100.0],
                "trade_sign": ["buy"],
            }
        ),
    )

    monkeypatch.setattr(
        fit_model_module,
        "estimate_current_state_for_model",
        lambda model_value, *, state_time_utc, request: (
            captured_estimation_calls.append((state_time_utc, request)) or estimated_state
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "provide_hourly_factor_contract",
        lambda **kwargs: _build_test_hourly_factor_contract(
            start_time_utc=cast(datetime, kwargs["range_start_utc"]),
            interval_end_times_utc=(cast(datetime, kwargs["range_end_utc"]),),
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_seeded_tick_event_simulation",
        lambda request, **kwargs: _capture_request(
            captured_requests,
            "seeded",
            request,
            object(),
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "_build_high_level_simulation_result",
        lambda **kwargs: JaniformSimulationResult(
            simulation_kind="seeded_tick",
            metadata={},
            provenance={},
            diagnostics={},
            raw_result=kwargs["simulation_result"],
        ),
    )

    model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=1,
        continuation_data=continuation_data,
        initial_mean_reversion_boundary_state=explicit_boundary_state,
    )

    assert captured_estimation_calls == [
        (datetime(2026, 2, 2, 15, tzinfo=UTC), continuation_data)
    ]
    assert captured_requests["seeded"].initial_excitation_state == estimated_state.hawkes_excitation_state
    assert captured_requests["seeded"].initial_mean_reversion_boundary_state == explicit_boundary_state


def test_high_level_model_simulate_uses_stationary_hawkes_state_when_no_continuation_data_is_supplied(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-simulate-stationary-default",
    )
    prepared_inputs = fit_model_module._prepare_model_simulation_inputs(
        model,
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=1,
        interval="1h",
        simulation_kind="seeded_tick",
        continuation_data=None,
        initial_excitation_state=None,
        initial_mean_reversion_boundary_state=None,
        debug_trace_enabled=False,
        sample_every_nth_transition=1,
        max_records_per_chunk=1_000,
        state_snapshot_capture=SeededTickEventStateSnapshotCapture.DISABLED,
    )
    runtime_factors = fit_model_module._prepare_model_runtime_factors(
        model,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=1,
        interval="1h",
    )

    assert prepared_inputs.seeded_request is not None
    assert (
        prepared_inputs.seeded_request.initial_excitation_state
        == runtime_factors.kernel_parameters.stationary_excitation_state()
    )


def test_high_level_model_coarse_simulation_skips_seeded_contract_preparation(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-coarse-only",
    )
    coarse_result = object()

    def _unexpected_seeded_runtime(request: Any, **kwargs: Any) -> NoReturn:
        del request
        del kwargs
        raise AssertionError("coarse simulation should not dispatch the seeded runtime")

    monkeypatch.setattr(
        fit_model_module,
        "provide_hourly_factor_contract",
        lambda **kwargs: _build_test_hourly_factor_contract(
            start_time_utc=kwargs["range_start_utc"],
            interval_end_times_utc=(
                kwargs["range_start_utc"] + timedelta(minutes=15),
                kwargs["range_start_utc"] + timedelta(minutes=30),
            ),
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_seeded_tick_event_simulation",
        _unexpected_seeded_runtime,
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_coarse_simulation",
        lambda request, **kwargs: coarse_result,
    )
    monkeypatch.setattr(
        fit_model_module,
        "_build_high_level_simulation_result",
        lambda **kwargs: JaniformSimulationResult(
            simulation_kind="coarse_interval",
            metadata={},
            provenance={},
            diagnostics={},
            raw_result=kwargs["simulation_result"],
        ),
    )

    observed = model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=2,
        interval="15min",
    )

    assert observed.simulation_kind == "coarse_interval"
    assert observed.raw_result is coarse_result


def test_high_level_model_simulation_materializes_mean_reversion_runtime_inputs(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-mean-reversion-runtime-inputs",
    )

    mean_reversion_payload = MeanReversionFittedParameters(
        kappa=0.25,
        anchor_model=MeanReversionAnchorModel.zero_gain_endogenous_core_pressure_level(),
    ).to_payload()

    model.parameters["return_components"]["fitted_parameters"][
        "mean_reversion_fitted_parameters"
    ] = mean_reversion_payload
    model.parameters["mean_reversion"]["fitted_parameters"] = mean_reversion_payload
    model.parameters["joint_refinement"]["fitted_parameters"]["return_components_fitted_parameters"][
        "mean_reversion_fitted_parameters"
    ] = mean_reversion_payload
    model.parameters["joint_refinement"]["baseline_return_components_fitted_parameters"][
        "mean_reversion_fitted_parameters"
    ] = mean_reversion_payload

    captured_requests: dict[str, Any] = {}
    coarse_result = object()
    seeded_result = object()

    def _build_runtime_hourly_factor_contract(**kwargs: Any) -> HourlyFactorContract:
        interval_duration_nanos = fit_model_module._parse_interval_duration_nanos(
            kwargs["interval"],
            field_name="interval",
        )
        interval_duration = timedelta(microseconds=interval_duration_nanos / 1_000)
        start_time_utc = kwargs["range_start_utc"]

        return _build_test_hourly_factor_contract(
            start_time_utc=start_time_utc,
            interval_end_times_utc=(
                start_time_utc + interval_duration,
                start_time_utc + (interval_duration * 2),
            ),
        )

    monkeypatch.setattr(
        fit_model_module,
        "provide_hourly_factor_contract",
        _build_runtime_hourly_factor_contract,
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_seeded_tick_event_simulation",
        lambda request, **kwargs: _capture_request(
            captured_requests,
            "seeded",
            request,
            seeded_result,
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_coarse_simulation",
        lambda request, **kwargs: _capture_request(
            captured_requests,
            "coarse",
            request,
            coarse_result,
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "_build_high_level_simulation_result",
        lambda **kwargs: JaniformSimulationResult(
            simulation_kind=(
                "seeded_tick"
                if kwargs["simulation_result"] is seeded_result
                else "coarse_interval"
            ),
            metadata={},
            provenance={},
            diagnostics={},
            raw_result=kwargs["simulation_result"],
        ),
    )

    model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
        horizon_interval_count=2,
    )
    model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
        horizon_interval_count=2,
        interval=timedelta(minutes=30),
    )

    seeded_request = captured_requests["seeded"]
    coarse_request = captured_requests["coarse"]
    seeded_end_anchor_values = tuple(
        float(anchor_value)
        for anchor_value in seeded_request.mean_reversion_runtime_interval_anchors.end_anchor_values
    )
    coarse_end_anchor_values = tuple(
        float(anchor_value)
        for anchor_value in coarse_request.model_inputs.risk_model_inputs.mean_reversion_runtime_interval_anchors.end_anchor_values
    )

    assert seeded_request.mean_reversion_fitted_parameters.to_payload() == mean_reversion_payload
    assert seeded_request.mean_reversion_runtime_interval_anchors.start_anchor_value == pytest.approx(0.0)
    assert len(seeded_end_anchor_values) == 2
    assert seeded_end_anchor_values == pytest.approx((0.0, 0.0))
    assert seeded_request.initial_mean_reversion_boundary_state.observed_log_price == pytest.approx(0.0)
    assert seeded_request.initial_mean_reversion_boundary_state.anchor_value == pytest.approx(0.0)
    assert seeded_request.initial_mean_reversion_boundary_state.pressure_state == pytest.approx(0.0)
    assert seeded_request.initial_mean_reversion_boundary_state.deviation == pytest.approx(0.0)

    coarse_model_inputs = coarse_request.model_inputs.risk_model_inputs
    assert coarse_model_inputs.mean_reversion_fitted_parameters.to_payload() == mean_reversion_payload
    assert coarse_model_inputs.mean_reversion_runtime_interval_anchors.start_anchor_value == pytest.approx(0.0)
    assert len(coarse_end_anchor_values) == 2
    assert coarse_end_anchor_values == pytest.approx((0.0, 0.0))
    assert coarse_model_inputs.initial_mean_reversion_boundary_state.observed_log_price == pytest.approx(0.0)
    assert coarse_model_inputs.initial_mean_reversion_boundary_state.anchor_value == pytest.approx(0.0)
    assert coarse_model_inputs.initial_mean_reversion_boundary_state.deviation == pytest.approx(0.0)


def test_high_level_model_materializes_endogenous_mean_reversion_runtime_inputs_via_native_recursion(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-endogenous-mean-reversion-runtime-inputs",
    )
    mean_reversion_payload = build_endogenous_mean_reversion_fitted_parameters_payload(kappa=0.25)

    update_model_mean_reversion_payload(model, mean_reversion_payload)

    captured_requests: dict[str, Any] = {}
    captured_native_requests: list[dict[str, Any]] = []

    coarse_result = object()
    seeded_result = object()

    def _build_runtime_hourly_factor_contract(**kwargs: Any) -> HourlyFactorContract:
        interval_duration_nanos = fit_model_module._parse_interval_duration_nanos(
            kwargs["interval"],
            field_name="interval",
        )
        interval_duration = timedelta(microseconds=interval_duration_nanos / 1_000)
        start_time_utc = kwargs["range_start_utc"]

        return _build_test_hourly_factor_contract(
            start_time_utc=start_time_utc,
            interval_end_times_utc=(
                start_time_utc + interval_duration,
                start_time_utc + (interval_duration * 2),
            ),
        )

    def _materialize_runtime_anchors(
        kernel_parameters: Any,
        nig_shared_plus_shift_configuration: Any,
        news_process_configuration: Any,
        initial_excitation_state: Any,
        mean_reversion_fitted_parameters: Any,
        interval_alignment_policy: str,
        start_anchor_value: float,
        s_h: Any,
        known_nu_h: Any,
        surprise_nu_h: Any,
        interval_deltas: Any,
    ) -> dict[str, Any]:
        captured_native_requests.append(
            {
                "kernel_parameters": kernel_parameters,
                "nig_shared_plus_shift_configuration": nig_shared_plus_shift_configuration,
                "news_process_configuration": news_process_configuration,
                "initial_excitation_state": initial_excitation_state,
                "mean_reversion_fitted_parameters": mean_reversion_fitted_parameters,
                "interval_alignment_policy": interval_alignment_policy,
                "start_anchor_value": start_anchor_value,
                "s_h": tuple(s_h),
                "known_nu_h": tuple(known_nu_h),
                "surprise_nu_h": tuple(surprise_nu_h),
                "interval_deltas": tuple(interval_deltas),
            }
        )

        return {
            "interval_alignment_policy": interval_alignment_policy,
            "start_anchor_value": start_anchor_value,
            "start_pressure_state": 0.0,
            "end_anchor_values": [0.1, 0.3],
            "end_pressure_states": [0.0, 0.0],
            "interval_deltas": list(interval_deltas),
        }

    monkeypatch.setattr(
        fit_model_module,
        "provide_hourly_factor_contract",
        _build_runtime_hourly_factor_contract,
    )
    monkeypatch.setattr(
        fit_model_module,
        "native_module",
        lambda: SimpleNamespace(
            materialize_runtime_structural_seasonality=(
                lambda *args, **kwargs: {
                    "interval_start_utc_nanos": [
                        datetime_to_utc_nanos(datetime(2026, 2, 2, 15, 30, tzinfo=UTC)),
                        datetime_to_utc_nanos(datetime(2026, 2, 2, 16, 30, tzinfo=UTC)),
                    ],
                    "seasonality_factors": [1.0, 1.0],
                }
            ),
            materialize_endogenous_mean_reversion_runtime_interval_anchors=(
                _materialize_runtime_anchors
            )
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_seeded_tick_event_simulation",
        lambda request, **kwargs: _capture_request(
            captured_requests,
            "seeded",
            request,
            seeded_result,
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_coarse_simulation",
        lambda request, **kwargs: _capture_request(
            captured_requests,
            "coarse",
            request,
            coarse_result,
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "_build_high_level_simulation_result",
        lambda **kwargs: JaniformSimulationResult(
            simulation_kind=(
                "seeded_tick"
                if kwargs["simulation_result"] is seeded_result
                else "coarse_interval"
            ),
            metadata={},
            provenance={},
            diagnostics={},
            raw_result=kwargs["simulation_result"],
        ),
    )

    model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
        horizon_interval_count=2,
    )
    model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
        horizon_interval_count=2,
        interval=timedelta(minutes=30),
    )

    seeded_request = captured_requests["seeded"]
    coarse_request = captured_requests["coarse"]

    assert len(captured_native_requests) == 2
    assert captured_native_requests[0]["start_anchor_value"] == pytest.approx(0.0)
    assert captured_native_requests[0]["interval_alignment_policy"] == (
        "explicit_runtime_interval_grid_alignment.v1"
    )
    assert seeded_request.mean_reversion_fitted_parameters.to_payload() == mean_reversion_payload
    assert seeded_request.mean_reversion_runtime_interval_anchors.start_anchor_value == pytest.approx(0.0)
    assert seeded_request.mean_reversion_runtime_interval_anchors.end_anchor_values == pytest.approx(
        (0.1, 0.3)
    )
    assert seeded_request.mean_reversion_runtime_interval_anchors.start_pressure_state == pytest.approx(
        0.0
    )
    assert seeded_request.mean_reversion_runtime_interval_anchors.end_pressure_states == pytest.approx(
        (0.0, 0.0)
    )
    assert seeded_request.initial_mean_reversion_boundary_state.observed_log_price == pytest.approx(0.0)
    assert seeded_request.initial_mean_reversion_boundary_state.anchor_value == pytest.approx(0.0)
    assert seeded_request.initial_mean_reversion_boundary_state.pressure_state == pytest.approx(0.0)
    assert seeded_request.initial_mean_reversion_boundary_state.deviation == pytest.approx(0.0)

    coarse_model_inputs = coarse_request.model_inputs.risk_model_inputs
    assert coarse_model_inputs.mean_reversion_runtime_interval_anchors.end_anchor_values == pytest.approx(
        (0.1, 0.3)
    )
    assert coarse_model_inputs.mean_reversion_runtime_interval_anchors.end_pressure_states == pytest.approx(
        (0.0, 0.0)
    )
    assert coarse_model_inputs.initial_mean_reversion_boundary_state.pressure_state == pytest.approx(
        0.0
    )
    assert coarse_model_inputs.initial_mean_reversion_boundary_state.deviation == pytest.approx(0.0)


def test_high_level_simulation_result_exposes_tabular_exports(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured_output_paths: list[Path] = []
    captured_export_context: list[tuple[Any, Any]] = []
    calendar_registry_sentinel = object()

    def capture_persist_coarse_simulation(
        *,
        simulation_result: object,
        output_path: str | Path,
        preferences: Any,
        sink: Any = None,
        calendar_registry: Any = None,
        exchange_code: Any = None,
    ) -> None:
        del simulation_result

        captured_output_paths.append(Path(output_path))
        captured_export_context.append((calendar_registry, exchange_code))

        if sink is not None:
            sink.write_records(
                [
                    {
                        preferences.timestamp_column: "2026-02-02T16:00:00Z",
                        "mid_price": 1.01,
                        "observed_log_price": 0.01,
                        "anchor_value": 0.005,
                        "pressure_state": 0.002,
                        "deviation_from_anchor": 0.005,
                    }
                ]
            )
            sink.close()
            return

        Path(output_path).write_text("parquet placeholder", encoding="utf-8")

    monkeypatch.setattr(
        fit_model_module,
        "persist_coarse_simulation",
        capture_persist_coarse_simulation,
    )

    result = JaniformSimulationResult(
        simulation_kind="coarse_interval",
        metadata={"schema_version": "simulation.coarse_interval.path_output_schema_metadata.v1"},
        provenance={"model_run_id": "model-1"},
        diagnostics={"simulated_interval_count": 1},
        raw_result=object(),
        _preferences=SimpleNamespace(
            timestamp_column="time",
            tick_price_input_policy="bid_ask_midpoint",
            bid_column="bid",
            ask_column="ask",
        ),
    )
    result._calendar_registry = calendar_registry_sentinel
    result._exchange_code = "FXSPOT"

    pandas_frame = result.to_pandas()
    numpy_array = result.to_numpy()
    parquet_path = tmp_path / "simulation.parquet"
    result.to_parquet(parquet_path)

    assert list(pandas_frame.columns) == [
        "time",
        "mid_price",
        "observed_log_price",
        "anchor_value",
        "pressure_state",
        "deviation_from_anchor",
    ]
    assert pandas_frame.iloc[0]["mid_price"] == pytest.approx(1.01)
    assert numpy_array.shape == (1, 6)
    assert parquet_path.read_text(encoding="utf-8") == "parquet placeholder"
    assert captured_output_paths[-1] == parquet_path
    assert captured_export_context == [
        (calendar_registry_sentinel, "FXSPOT"),
        (calendar_registry_sentinel, "FXSPOT"),
    ]


def test_build_high_level_coarse_simulation_result_preserves_session_context_for_exports(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    import janiform._workflow_outputs as workflow_outputs_module

    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-coarse-export-context",
    )
    coarse_result = SimpleNamespace(
        output_schema_metadata=SimpleNamespace(to_payload=lambda: {"schema_version": "test"}),
        provenance=SimpleNamespace(to_payload=lambda: {"source": "test"}),
        diagnostics=SimpleNamespace(to_payload=lambda: {"simulated_interval_count": 1}),
    )
    preferences = workflow_outputs_module.SimulationOutputPreferences(
        file_format="parquet",
        timestamp_column="time",
        timestamp_timezone="UTC",
        tick_price_input_policy="bid_ask_midpoint",
        price_column=None,
        trade_sign_column=None,
        bid_column="bid",
        ask_column="ask",
        reference_price=1.0,
    )

    result = fit_model_module._build_high_level_simulation_result(
        model=model,
        simulation_result=coarse_result,
        preferences=preferences,
    )

    assert result._exchange_code == model.session_resolution.exchange_code
    assert result._calendar_registry is not None
    assert (
        result._calendar_registry.resolve_market_state(
            model.session_resolution.exchange_code,
            datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
        )
        == MarketState.OPEN
    )


def test_persist_coarse_simulation_suppresses_closed_fx_weekend_rows() -> None:
    import janiform._workflow_outputs as workflow_outputs_module

    registry = fit_estimator_module.PresetRegistry()
    _, session_artifact = registry.resolve(
        category=PresetCategory.SESSION,
        preset_name="fx_spot_24_5",
    )
    calendar_registry = CalendarRegistry(
        fit_estimator_module._calendar_registry_json_from_session_configuration(
            session_artifact.configuration
        )
    )

    class _CapturingSink:
        def __init__(self) -> None:
            self.rows: list[dict[str, Any]] = []

        def write_records(self, records: list[dict[str, Any]]) -> None:
            self.rows.extend(records)

        def close(self) -> None:
            return None

    sink = _CapturingSink()
    preferences = workflow_outputs_module.SimulationOutputPreferences(
        file_format="parquet",
        timestamp_column="timestamp_utc",
        timestamp_timezone="UTC",
        tick_price_input_policy="bid_ask_midpoint",
        price_column=None,
        trade_sign_column=None,
        bid_column=None,
        ask_column=None,
        reference_price=1.0,
    )
    simulation_result = SimpleNamespace(
        records=(
            SimpleNamespace(
                interval_start_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 6, 18, 45, tzinfo=UTC)),
                interval_end_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 6, 19, 0, tzinfo=UTC)),
                observed_log_price_end=0.01,
                anchor_value_end=0.005,
                pressure_state_end=0.002,
                deviation_from_anchor_end=0.005,
            ),
            SimpleNamespace(
                interval_start_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 8, 20, 45, tzinfo=UTC)),
                interval_end_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 8, 21, 0, tzinfo=UTC)),
                observed_log_price_end=999.0,
                anchor_value_end=999.0,
                pressure_state_end=999.0,
                deviation_from_anchor_end=999.0,
            ),
            SimpleNamespace(
                interval_start_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 8, 21, 0, tzinfo=UTC)),
                interval_end_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 8, 21, 15, tzinfo=UTC)),
                observed_log_price_end=0.05,
                anchor_value_end=0.03,
                pressure_state_end=0.01,
                deviation_from_anchor_end=0.02,
            ),
        )
    )

    workflow_outputs_module.persist_coarse_simulation(
        simulation_result=simulation_result,
        output_path=Path("simulation.parquet"),
        preferences=preferences,
        sink=sink,
        calendar_registry=calendar_registry,
        exchange_code="FXSPOT",
    )

    assert [row["timestamp_utc"] for row in sink.rows] == [
        datetime(2026, 2, 6, 19, 0, tzinfo=UTC),
        datetime(2026, 2, 8, 21, 15, tzinfo=UTC),
    ]
    assert [row["mid_price"] for row in sink.rows] == pytest.approx(
        [
            math.exp(0.01),
            math.exp(0.05),
        ]
    )
    assert [row["anchor_value"] for row in sink.rows] == pytest.approx([0.005, 0.03])
    assert [row["pressure_state"] for row in sink.rows] == pytest.approx([0.002, 0.01])
    assert [row["deviation_from_anchor"] for row in sink.rows] == pytest.approx([0.005, 0.02])


def test_persist_seeded_tick_simulation_preserves_non_trade_fine_path_rows() -> None:
    import janiform._workflow_outputs as workflow_outputs_module

    class _CapturingSink:
        def __init__(self) -> None:
            self.rows: list[dict[str, Any]] = []

        def write_records(self, records: list[dict[str, Any]]) -> None:
            self.rows.extend(records)

        def close(self) -> None:
            return None

    sink = _CapturingSink()
    preferences = workflow_outputs_module.SimulationOutputPreferences(
        file_format="parquet",
        timestamp_column="timestamp_utc",
        timestamp_timezone="UTC",
        tick_price_input_policy="explicit_price_column",
        price_column="trade_price",
        trade_sign_column="trade_sign",
        bid_column=None,
        ask_column=None,
        reference_price=100.0,
    )
    simulation_result = SimpleNamespace(
        fine_path=(
            SimpleNamespace(
                timestamp_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 2, 15, 0, tzinfo=UTC)),
                observed_log_price=0.10,
                anchor_value=0.04,
                pressure_state=0.01,
                deviation_from_anchor=0.06,
                side=SeededTickEventSide.BUY,
            ),
            SimpleNamespace(
                timestamp_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 2, 15, 15, tzinfo=UTC)),
                observed_log_price=0.12,
                anchor_value=0.05,
                pressure_state=0.02,
                deviation_from_anchor=0.07,
                side=None,
            ),
        )
    )

    workflow_outputs_module.persist_seeded_tick_simulation(
        simulation_result=simulation_result,
        output_path=Path("simulation.parquet"),
        preferences=preferences,
        sink=sink,
    )

    assert [row["timestamp_utc"] for row in sink.rows] == [
        datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
        datetime(2026, 2, 2, 15, 15, tzinfo=UTC),
    ]
    assert [row["trade_price"] for row in sink.rows] == pytest.approx(
        [
            100.0 * math.exp(0.10),
            100.0 * math.exp(0.12),
        ]
    )
    assert [row["trade_sign"] for row in sink.rows] == [
        SeededTickEventSide.BUY.value,
        None,
    ]
    assert [row["anchor_value"] for row in sink.rows] == pytest.approx([0.04, 0.05])
    assert [row["pressure_state"] for row in sink.rows] == pytest.approx([0.01, 0.02])
    assert [row["deviation_from_anchor"] for row in sink.rows] == pytest.approx([0.06, 0.07])


def test_high_level_seeded_simulation_result_preserves_empty_tabular_schema(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def capture_empty_seeded_simulation(
        *,
        simulation_result: object,
        output_path: str | Path,
        preferences: Any,
        sink: Any = None,
    ) -> None:
        del simulation_result, output_path, preferences

        if sink is not None:
            sink.close()

    monkeypatch.setattr(
        fit_model_module,
        "persist_seeded_tick_simulation",
        capture_empty_seeded_simulation,
    )

    result = JaniformSimulationResult(
        simulation_kind="seeded_tick",
        metadata={},
        provenance={},
        diagnostics={},
        raw_result=object(),
        _preferences=SimpleNamespace(
            timestamp_column="timestamp_utc",
            tick_price_input_policy="explicit_price_column",
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
    )

    pandas_frame = result.to_pandas()
    numpy_array = result.to_numpy()

    assert list(pandas_frame.columns) == [
        "timestamp_utc",
        "trade_price",
        "trade_sign",
        "observed_log_price",
        "anchor_value",
        "pressure_state",
        "deviation_from_anchor",
    ]
    assert pandas_frame.empty
    assert numpy_array.shape == (0, 7)


def test_high_level_simulation_result_materializes_datetime_timestamps_in_memory(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured_output_paths: list[Path] = []

    def capture_seeded_simulation(
        *,
        simulation_result: object,
        output_path: str | Path,
        preferences: Any,
        sink: Any = None,
    ) -> None:
        del simulation_result, preferences

        captured_output_paths.append(Path(output_path))

        if sink is not None:
            sink.write_records(
                [
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 15, tzinfo=UTC),
                        "trade_price": 101.0,
                        "trade_sign": "buy",
                        "observed_log_price": 0.01,
                        "anchor_value": 0.0,
                        "pressure_state": 0.0,
                        "deviation_from_anchor": 0.01,
                    }
                ]
            )
            sink.close()

    monkeypatch.setattr(
        fit_model_module,
        "persist_seeded_tick_simulation",
        capture_seeded_simulation,
    )

    result = JaniformSimulationResult(
        simulation_kind="seeded_tick",
        metadata={},
        provenance={},
        diagnostics={},
        raw_result=object(),
        _preferences=SimpleNamespace(
            timestamp_column="timestamp_utc",
            tick_price_input_policy="explicit_price_column",
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
    )

    pandas_frame = result.to_pandas()

    assert pandas_frame.iloc[0]["timestamp_utc"] == datetime(2026, 2, 2, 15, tzinfo=UTC)
    assert captured_output_paths == [Path("simulation.parquet")]


def test_high_level_seeded_simulation_result_exports_full_fine_path_to_tabular_surfaces(
    tmp_path: Path,
) -> None:
    result = JaniformSimulationResult(
        simulation_kind="seeded_tick",
        metadata={},
        provenance={},
        diagnostics={},
        raw_result=SimpleNamespace(
            fine_path=(
                SimpleNamespace(
                    timestamp_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 2, 15, 0, tzinfo=UTC)),
                    observed_log_price=0.10,
                    anchor_value=0.04,
                    pressure_state=0.01,
                    deviation_from_anchor=0.06,
                    side=SeededTickEventSide.BUY,
                ),
                SimpleNamespace(
                    timestamp_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 2, 15, 15, tzinfo=UTC)),
                    observed_log_price=0.12,
                    anchor_value=0.05,
                    pressure_state=0.02,
                    deviation_from_anchor=0.07,
                    side=None,
                ),
            )
        ),
        _preferences=SimpleNamespace(
            timestamp_column="timestamp_utc",
            timestamp_timezone="UTC",
            tick_price_input_policy="explicit_price_column",
            price_column="trade_price",
            trade_sign_column="trade_sign",
            reference_price=100.0,
        ),
    )

    pandas_frame = result.to_pandas()
    numpy_array = result.to_numpy()
    parquet_path = tmp_path / "seeded-simulation.parquet"
    result.to_parquet(parquet_path)
    parquet_rows = parquet.read_table(parquet_path).to_pylist()

    assert list(pandas_frame["timestamp_utc"]) == [
        datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
        datetime(2026, 2, 2, 15, 15, tzinfo=UTC),
    ]
    assert list(pandas_frame["trade_price"]) == pytest.approx(
        [
            100.0 * math.exp(0.10),
            100.0 * math.exp(0.12),
        ]
    )
    assert pandas_frame.iloc[0]["trade_sign"] == "buy"
    assert pandas.isna(pandas_frame.iloc[1]["trade_sign"])
    assert pandas_frame.iloc[0]["pressure_state"] == pytest.approx(0.01)
    assert pandas_frame.iloc[1]["anchor_value"] == pytest.approx(0.05)
    assert numpy_array.shape == (2, 7)
    assert list(numpy_array[:, 2]) == ["buy", None]
    assert parquet_rows == [
        {
            "timestamp_utc": datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
            "trade_price": pytest.approx(100.0 * math.exp(0.10)),
            "trade_sign": "buy",
            "observed_log_price": pytest.approx(0.10),
            "anchor_value": pytest.approx(0.04),
            "pressure_state": pytest.approx(0.01),
            "deviation_from_anchor": pytest.approx(0.06),
        },
        {
            "timestamp_utc": datetime(2026, 2, 2, 15, 15, tzinfo=UTC),
            "trade_price": pytest.approx(100.0 * math.exp(0.12)),
            "trade_sign": None,
            "observed_log_price": pytest.approx(0.12),
            "anchor_value": pytest.approx(0.05),
            "pressure_state": pytest.approx(0.02),
            "deviation_from_anchor": pytest.approx(0.07),
        },
    ]


def test_high_level_model_price_option_dispatches_through_canonical_pricing_runner(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-pricing",
    )

    prepared_execution_input = object()
    captured_execution_inputs: list[Any] = []
    pricing_result = object()

    monkeypatch.setattr(
        fit_model_module,
        "_prepare_model_pricing_execution_input",
        lambda *args, **kwargs: prepared_execution_input,
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_vanilla_option_pricing",
        lambda execution_input: captured_execution_inputs.append(execution_input) or pricing_result,
    )

    observed_result = model.price_option(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        spot=100.0,
        strike=105.0,
        maturity_interval=4.0,
    )

    assert observed_result is pricing_result
    assert captured_execution_inputs == [prepared_execution_input]


def test_high_level_model_price_option_derives_boundary_state_from_spot_and_model_reference_price(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-pricing-derived-boundary",
    )
    mean_reversion_payload = build_endogenous_mean_reversion_fitted_parameters_payload(kappa=0.35)
    derived_boundary_state = MeanReversionBoundaryState(
        observed_log_price=0.25,
        anchor_value=0.25,
        pressure_state=0.1,
        deviation=0.0,
    )
    captured_execution_inputs: list[Any] = []
    captured_start_anchor_values: list[float] = []

    update_model_mean_reversion_payload(
        model,
        mean_reversion_payload,
    )
    reference_price = cast(
        float,
        cast(
            dict[str, Any],
            cast(dict[str, Any], model.metadata["simulation_output_preferences"]),
        )["reference_price"],
    )
    pricing_result = object()

    monkeypatch.setattr(
        fit_model_module,
        "_prepare_model_mean_reversion_runtime_inputs",
        lambda pricing_model, **kwargs: (
            captured_start_anchor_values.append(cast(float, kwargs["start_anchor_value"]))
            or (
                fit_model_module._mean_reversion_fitted_parameters(pricing_model),
                fit_model_module.MeanReversionRuntimeIntervalAnchors(
                    interval_alignment_policy="explicit_runtime_interval_grid_alignment.v1",
                    start_anchor_value=derived_boundary_state.anchor_value,
                    start_pressure_state=derived_boundary_state.pressure_state,
                    end_anchor_values=(derived_boundary_state.anchor_value,),
                    end_pressure_states=(derived_boundary_state.pressure_state,),
                    interval_deltas=(1.0,),
                ),
                derived_boundary_state,
            )
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_vanilla_option_pricing",
        lambda execution_input: captured_execution_inputs.append(execution_input) or pricing_result,
    )

    observed_result = model.price_option(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        spot=100.0,
        strike=105.0,
        maturity_interval=4.0,
    )

    assert observed_result is pricing_result
    assert len(captured_execution_inputs) == 1
    assert captured_start_anchor_values == [pytest.approx(math.log(100.0 / reference_price))]

    pricing_boundary_input = captured_execution_inputs[0].request.boundary_input
    assert pricing_boundary_input.mean_reversion_fitted_parameters.to_payload() == mean_reversion_payload
    assert pricing_boundary_input.observed_boundary_state == derived_boundary_state
    assert pricing_boundary_input.interval_delta == pytest.approx(1.0)


def test_high_level_model_estimate_current_state_dispatches_to_native_estimator(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-estimate-current-state",
    )
    captured_native_arguments: list[tuple[Any, ...]] = []
    request = CurrentStateEstimationRequest(
        observed_tick_data=pandas.DataFrame(
            {
                "timestamp_utc": [
                    datetime(2026, 2, 2, 13, tzinfo=UTC),
                    datetime(2026, 2, 2, 14, tzinfo=UTC),
                ],
                "trade_price": [100.0, 100.5],
                "trade_sign": ["buy", "sell"],
            }
        ),
        observed_price=100.0,
    )

    monkeypatch.setattr(
        state_estimation_module,
        "native_module",
        lambda: SimpleNamespace(
            estimate_current_state_from_observed_data=lambda *args: (
                captured_native_arguments.append(args)
                or {
                    "hawkes_excitation_state": {
                        "y_plus_plus": 0.2,
                        "y_plus_minus": 0.1,
                        "y_minus_plus": 0.1,
                        "y_minus_minus": 0.25,
                    },
                    "mean_reversion_boundary_state": {
                        "observed_log_price": 0.0,
                        "anchor_value": 0.0,
                        "pressure_state": 0.0,
                        "deviation": 0.0,
                    },
                }
            )
        ),
    )

    result = model.estimate_current_state(
        state_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        request=request,
    )

    assert result == CurrentStateEstimationResult(
        hawkes_excitation_state=fit_model_module.HawkesExcitationState(
            y_plus_plus=0.2,
            y_plus_minus=0.1,
            y_minus_plus=0.1,
            y_minus_minus=0.25,
        ),
        mean_reversion_boundary_state=MeanReversionBoundaryState(
            observed_log_price=0.0,
            anchor_value=0.0,
            pressure_state=0.0,
            deviation=0.0,
        ),
    )
    assert len(captured_native_arguments) == 1
    assert captured_native_arguments[0][2] == datetime_to_utc_nanos(
        datetime(2026, 2, 2, 15, tzinfo=UTC)
    )
    assert captured_native_arguments[0][3] == pytest.approx(
        fit_model_module._pricing_observed_log_price_from_spot(model, spot=100.0)
    )
    assert captured_native_arguments[0][4] == (
        datetime(2026, 2, 2, 13, tzinfo=UTC).isoformat(),
        datetime(2026, 2, 2, 14, tzinfo=UTC).isoformat(),
    )
    assert captured_native_arguments[0][5] == (100.0, 100.5)
    assert captured_native_arguments[0][6] == (
        "buy",
        "sell",
    )


def test_high_level_model_price_option_uses_estimated_current_state_when_observed_data_is_supplied(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-pricing-estimated-current-state",
    )
    estimated_state = CurrentStateEstimationResult(
        hawkes_excitation_state=fit_model_module.HawkesExcitationState(
            y_plus_plus=0.45,
            y_plus_minus=0.15,
            y_minus_plus=0.3,
            y_minus_minus=0.2,
        ),
        mean_reversion_boundary_state=MeanReversionBoundaryState(
            observed_log_price=0.05,
            anchor_value=0.05,
            pressure_state=0.0,
            deviation=0.0,
        ),
    )
    observed_data = CurrentStateEstimationRequest(
        observed_tick_data=pandas.DataFrame(
            {
                "timestamp_utc": [datetime(2026, 2, 2, 14, 30, tzinfo=UTC)],
                "trade_price": [100.0],
                "trade_sign": ["buy"],
            }
        ),
    )
    captured_execution_inputs: list[Any] = []
    captured_estimation_calls: list[tuple[datetime, CurrentStateEstimationRequest]] = []
    pricing_result = object()

    monkeypatch.setattr(
        fit_model_module,
        "estimate_current_state_for_model",
        lambda model_value, *, state_time_utc, request: (
            captured_estimation_calls.append((state_time_utc, request)) or estimated_state
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_vanilla_option_pricing",
        lambda execution_input: captured_execution_inputs.append(execution_input) or pricing_result,
    )

    observed_result = model.price_option(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        spot=100.0,
        strike=105.0,
        maturity_interval=4.0,
        observed_data=observed_data,
    )

    assert observed_result is pricing_result
    assert captured_estimation_calls == [
        (
            datetime(2026, 2, 2, 15, tzinfo=UTC),
            CurrentStateEstimationRequest(
                observed_tick_data=observed_data.observed_tick_data,
                tick_data_column_mapping=observed_data.tick_data_column_mapping,
                observed_price=100.0,
            ),
        )
    ]
    assert (
        captured_execution_inputs[0].request.boundary_input.hawkes_boundary_state
        == estimated_state.hawkes_excitation_state
    )
    assert (
        captured_execution_inputs[0].request.boundary_input.observed_boundary_state
        == estimated_state.mean_reversion_boundary_state
    )


def test_high_level_model_price_option_uses_stationary_hawkes_boundary_state_by_default(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-pricing-stationary-hawkes-boundary",
    )
    pricing_result = object()
    captured_execution_inputs: list[Any] = []

    monkeypatch.setattr(
        fit_model_module,
        "run_vanilla_option_pricing",
        lambda execution_input: captured_execution_inputs.append(execution_input) or pricing_result,
    )

    observed_result = model.price_option(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        spot=100.0,
        strike=105.0,
        maturity_interval=4.0,
    )

    runtime_factors = fit_model_module._prepare_model_runtime_factors(
        model,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=4,
        interval="1h",
    )

    assert observed_result is pricing_result
    assert len(captured_execution_inputs) == 1
    assert (
        captured_execution_inputs[0].request.boundary_input.hawkes_boundary_state
        == runtime_factors.kernel_parameters.stationary_excitation_state()
    )


def test_high_level_model_price_option_requires_reference_price_to_derive_pricing_state(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-pricing-missing-reference-price",
    )
    update_model_mean_reversion_payload(
        model,
        build_endogenous_mean_reversion_fitted_parameters_payload(kappa=0.35),
    )
    cast(dict[str, Any], model.metadata["simulation_output_preferences"]).pop("reference_price", None)

    with pytest.raises(CalibrationValidationError) as captured_error:
        model.price_option(
            valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
            spot=100.0,
            strike=105.0,
            maturity_interval=4.0,
        )

    assert (
        captured_error.value.failure.field_path
        == "metadata.simulation_output_preferences.reference_price"
    )
    assert (
        captured_error.value.failure.message
        == "must be present so JaniformModel can derive pricing state from spot"
    )


def test_high_level_model_price_option_end_to_end_supports_endogenous_anchor_mean_reversion(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-pricing-constant-anchor-end-to-end",
    )

    update_model_mean_reversion_payload(
        model,
        build_endogenous_mean_reversion_fitted_parameters_payload(kappa=0.35),
    )

    pricing_result = model.price_option(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        spot=100.0,
        strike=105.0,
        maturity_interval=1.0,
        boundary_state=fit_model_module.HawkesExcitationState(
            y_plus_plus=0.0,
            y_plus_minus=0.0,
            y_minus_plus=0.0,
            y_minus_minus=0.0,
        ),
        include_transform_diagnostics=True,
        include_integration_quality_diagnostics=True,
    )

    assert pricing_result.response.price >= 0.0
    assert pricing_result.run_diagnostics is not None
    assert pricing_result.run_diagnostics.anchored_recursion is not None


def test_high_level_model_price_options_dispatches_through_batch_pricing_runner(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-batch-pricing",
    )
    batch = JaniformVanillaOptionBatch(
        spot=100.0,
        strikes=(95.0, 105.0),
        maturity_intervals=(1.0, 2.0),
        payoff_types=(VanillaOptionPayoffType.CALL, VanillaOptionPayoffType.PUT),
        risk_free_rates=(0.01, 0.01),
        dividend_yields=(0.0, 0.0),
    )
    prepared_native_execution_input = {"prepared": "batch"}
    observed_result = JaniformVanillaOptionBatchPricingResult(
        spot=100.0,
        strikes=(95.0, 105.0),
        maturity_intervals=(1.0, 2.0),
        payoff_types=(VanillaOptionPayoffType.CALL, VanillaOptionPayoffType.PUT),
        risk_free_rates=(0.01, 0.01),
        dividend_yields=(0.0, 0.0),
        prices=(10.0, 11.0),
        implied_volatilities=(0.2, 0.25),
        implied_volatility_inversion_statuses=(
            VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
            VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
        ),
        forward_prices=(100.5, 101.0),
        discount_factors=(0.99, 0.98),
        boundary_state_norms=(0.0, 0.0),
    )

    monkeypatch.setattr(
        fit_model_module,
        "_prepare_model_batch_pricing_native_input",
        lambda *args, **kwargs: prepared_native_execution_input,
    )
    monkeypatch.setattr(
        fit_model_module,
        "_run_model_vanilla_option_batch_pricing",
        lambda native_execution_input: (
            observed_result
            if native_execution_input is prepared_native_execution_input
            else None
        ),
    )

    result = model.price_options(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        batch=batch,
    )

    assert result is observed_result


def test_high_level_model_price_option_surface_dispatches_through_surface_pricing_runner(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-surface-pricing",
    )
    surface = JaniformVanillaOptionSurface(
        spot=100.0,
        strikes=(95.0, 105.0),
        maturity_intervals=(1.0, 2.0),
        payoff_type=VanillaOptionPayoffType.CALL,
        risk_free_rate=0.01,
        dividend_yield=0.0,
    )
    prepared_native_execution_input = {"prepared": "surface"}
    observed_result = JaniformVanillaOptionSurfacePricingResult(
        spot=100.0,
        strikes=(95.0, 105.0),
        maturity_intervals=(1.0, 2.0),
        payoff_type=VanillaOptionPayoffType.CALL,
        risk_free_rate=0.01,
        dividend_yield=0.0,
        price_grid=((10.0, 8.0), (11.0, 9.0)),
        implied_volatility_grid=((0.2, 0.18), (0.24, 0.21)),
        implied_volatility_inversion_status_grid=(
            (
                VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
                VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
            ),
            (
                VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
                VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
            ),
        ),
        forward_price_grid=((100.5, 100.5), (101.0, 101.0)),
        discount_factor_grid=((0.99, 0.99), (0.98, 0.98)),
        boundary_state_norm_grid=((0.0, 0.0), (0.0, 0.0)),
    )

    monkeypatch.setattr(
        fit_model_module,
        "_prepare_model_surface_pricing_native_input",
        lambda *args, **kwargs: prepared_native_execution_input,
    )
    monkeypatch.setattr(
        fit_model_module,
        "_run_model_vanilla_option_surface_pricing",
        lambda native_execution_input: (
            observed_result
            if native_execution_input is prepared_native_execution_input
            else None
        ),
    )

    result = model.price_option_surface(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        surface=surface,
    )

    assert result is observed_result


def test_high_level_model_price_options_matches_single_option_engine_on_shared_nodes(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-batch-pricing-parity",
    )
    boundary_state = fit_model_module.HawkesExcitationState(
        y_plus_plus=0.0,
        y_plus_minus=0.0,
        y_minus_plus=0.0,
        y_minus_minus=0.0,
    )
    batch = JaniformVanillaOptionBatch(
        spot=100.0,
        strikes=(95.0, 100.0, 105.0),
        maturity_intervals=(1.0, 1.0, 1.0),
        payoff_types=(
            VanillaOptionPayoffType.CALL,
            VanillaOptionPayoffType.PUT,
            VanillaOptionPayoffType.CALL,
        ),
        risk_free_rates=(0.01, 0.01, 0.01),
        dividend_yields=(0.0, 0.0, 0.0),
    )

    batch_result = model.price_options(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        batch=batch,
        boundary_state=boundary_state,
        include_transform_diagnostics=True,
        include_integration_quality_diagnostics=True,
        include_calibration_hooks=True,
    )

    assert isinstance(batch_result, JaniformVanillaOptionBatchPricingResult)
    assert batch_result.node_count == batch.node_count

    for node_index in range(batch.node_count):
        single_result = model.price_option(
            valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
            spot=batch.spot,
            strike=batch.strikes[node_index],
            maturity_interval=batch.maturity_intervals[node_index],
            payoff_type=batch.payoff_types[node_index],
            risk_free_rate=batch.risk_free_rates[node_index],
            dividend_yield=batch.dividend_yields[node_index],
            boundary_state=boundary_state,
            include_transform_diagnostics=True,
            include_integration_quality_diagnostics=True,
            include_calibration_hooks=True,
        )

        assert batch_result.prices[node_index] == pytest.approx(single_result.response.price)
        assert batch_result.implied_volatilities[node_index] == pytest.approx(
            cast(float, single_result.response.implied_volatility)
        )
        assert (
            batch_result.implied_volatility_inversion_statuses[node_index]
            is single_result.response.implied_volatility_inversion_status
        )
        assert batch_result.forward_prices[node_index] == pytest.approx(
            single_result.response.diagnostics.forward_price
        )
        assert batch_result.discount_factors[node_index] == pytest.approx(
            single_result.response.diagnostics.discount_factor
        )
        assert batch_result.boundary_state_norms[node_index] == pytest.approx(
            single_result.response.diagnostics.boundary_state_norm
        )
        assert batch_result.run_diagnostics is not None
        assert single_result.run_diagnostics is not None
        assert (
            batch_result.run_diagnostics.transform[node_index]
            == single_result.run_diagnostics.transform
        )
        assert (
            batch_result.run_diagnostics.integration_quality[node_index]
            == single_result.run_diagnostics.integration_quality
        )
        assert batch_result.calibration_hooks is not None
        assert single_result.calibration_hooks is not None
        assert batch_result.calibration_hooks.option_prices[node_index] == pytest.approx(
            single_result.calibration_hooks.option_price
        )


def test_high_level_model_price_option_surface_matches_single_option_engine_on_shared_grid(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-surface-pricing-parity",
    )
    boundary_state = fit_model_module.HawkesExcitationState(
        y_plus_plus=0.0,
        y_plus_minus=0.0,
        y_minus_plus=0.0,
        y_minus_minus=0.0,
    )
    surface = JaniformVanillaOptionSurface(
        spot=100.0,
        strikes=(95.0, 105.0),
        maturity_intervals=(1.0,),
        payoff_type=VanillaOptionPayoffType.CALL,
        risk_free_rate=0.01,
        dividend_yield=0.0,
    )

    surface_result = model.price_option_surface(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        surface=surface,
        boundary_state=boundary_state,
        include_transform_diagnostics=True,
        include_integration_quality_diagnostics=True,
        include_calibration_hooks=True,
    )

    assert isinstance(surface_result, JaniformVanillaOptionSurfacePricingResult)
    assert surface_result.shape == (1, 2)

    for maturity_index, maturity_interval in enumerate(surface.maturity_intervals):
        for strike_index, strike in enumerate(surface.strikes):
            single_result = model.price_option(
                valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
                spot=surface.spot,
                strike=strike,
                maturity_interval=maturity_interval,
                payoff_type=surface.payoff_type,
                risk_free_rate=surface.risk_free_rate,
                dividend_yield=surface.dividend_yield,
                boundary_state=boundary_state,
                include_transform_diagnostics=True,
                include_integration_quality_diagnostics=True,
                include_calibration_hooks=True,
            )

            assert surface_result.price_grid[maturity_index][strike_index] == pytest.approx(
                single_result.response.price
            )
            assert surface_result.implied_volatility_grid[maturity_index][strike_index] == pytest.approx(
                cast(float, single_result.response.implied_volatility)
            )
            assert (
                surface_result.implied_volatility_inversion_status_grid[maturity_index][strike_index]
                is single_result.response.implied_volatility_inversion_status
            )
            assert surface_result.forward_price_grid[maturity_index][strike_index] == pytest.approx(
                single_result.response.diagnostics.forward_price
            )
            assert surface_result.discount_factor_grid[maturity_index][strike_index] == pytest.approx(
                single_result.response.diagnostics.discount_factor
            )
            assert (
                surface_result.boundary_state_norm_grid[maturity_index][strike_index]
                == pytest.approx(single_result.response.diagnostics.boundary_state_norm)
            )
            assert surface_result.run_diagnostics is not None
            assert single_result.run_diagnostics is not None
            assert (
                surface_result.run_diagnostics.transform_grid[maturity_index][strike_index]
                == single_result.run_diagnostics.transform
            )
            assert surface_result.calibration_hooks is not None
            assert single_result.calibration_hooks is not None
            assert (
                surface_result.calibration_hooks.option_price_grid[maturity_index][strike_index]
                == pytest.approx(single_result.calibration_hooks.option_price)
            )


def test_runtime_pricing_seasonality_source_stays_materialized_for_off_hour_windows(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-off-hour-surface-pricing",
    )
    valuation_time_utc = datetime(2026, 2, 2, 15, 34, tzinfo=UTC)
    runtime_factors = fit_model_module._prepare_model_runtime_factors(
        model,
        start_time_utc=valuation_time_utc,
        horizon_interval_count=2,
        interval="1h",
    )

    seasonality_source = fit_model_module._build_runtime_hourly_seasonality_source_from_model(
        model,
        calendar_registry=runtime_factors.calendar_registry,
        exchange_code=runtime_factors.exchange_code,
        range_start_utc=valuation_time_utc,
        range_end_utc=runtime_factors.range_end_utc,
        interval="1h",
    )

    assert isinstance(
        seasonality_source,
        fit_model_module.HourlySeasonalityProviderMaterializedSource,
    )
    assert seasonality_source.source_timestamps_utc == (
        datetime(2026, 2, 2, 15, 34, tzinfo=UTC),
        datetime(2026, 2, 2, 16, 34, tzinfo=UTC),
    )


def test_runtime_pricing_seasonality_source_uses_explicit_hourly_factors_for_hour_aligned_windows(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-hour-aligned-surface-pricing",
    )
    valuation_time_utc = datetime(2026, 2, 2, 15, tzinfo=UTC)
    runtime_factors = fit_model_module._prepare_model_runtime_factors(
        model,
        start_time_utc=valuation_time_utc,
        horizon_interval_count=2,
        interval="1h",
    )

    seasonality_source = fit_model_module._build_runtime_hourly_seasonality_source_from_model(
        model,
        calendar_registry=runtime_factors.calendar_registry,
        exchange_code=runtime_factors.exchange_code,
        range_start_utc=valuation_time_utc,
        range_end_utc=runtime_factors.range_end_utc,
        interval="1h",
    )

    assert isinstance(
        seasonality_source,
        fit_model_module.HourlySeasonalityProviderExplicitSource,
    )
    assert tuple(seasonality_source.hourly_factors) == (1.0, 1.0)


def test_high_level_fit_routes_through_prepared_staged_runner(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc",
                "2026-02-02T14:30:00Z",
                "2026-02-02T15:00:00Z",
                "2026-02-02T20:59:00Z",
                "2026-02-03T14:30:00Z",
                "2026-02-03T20:59:00Z",
            ]
        ),
        encoding="utf-8",
    )

    captured_prepared_handle: dict[str, Any] = {}

    def capturing_runner(*args, **kwargs):
        captured_prepared_handle["prepared_handle"] = args[0]
        return _successful_staged_runner_payload("high-level-fit-runner-path")

    monkeypatch.setattr(
        fit_estimator_module,
        "_run_prepared_staged_calibration_with_raw_payload",
        capturing_runner,
    )

    estimator = JaniformEstimator(
        workflow_scope=CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY,
        tick_data_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
        ),
        market_profile="us_equity_cash",
    )

    _, fit_report = estimator.fit(
        run_id="high-level-fit-runner-path",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
        report=True,
    )

    captured_input = captured_prepared_handle["prepared_handle"]
    assert hasattr(captured_input, "serialize_execution_input_json")
    assert _prepared_runner_run_id(captured_input) == fit_report.run_id
    assert fit_report.staged_request.run_id == fit_report.run_id
    assert "execution_input" not in fit_report.to_payload()


def test_high_level_fit_reuses_single_load_and_translation_pass_when_session_is_explicit(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
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

    normalize_call_count = 0
    translate_call_count = 0
    original_normalize_source = fit_estimator_module.normalize_market_data_source
    original_translate_request = (
        fit_estimator_module.translate_market_data_request_to_prepared_calibration_from_phases
    )

    def counting_normalize_source(*args, **kwargs):
        nonlocal normalize_call_count
        normalize_call_count += 1
        return original_normalize_source(*args, **kwargs)

    def counting_translate_request(*args, **kwargs):
        nonlocal translate_call_count
        translate_call_count += 1
        return original_translate_request(*args, **kwargs)

    monkeypatch.setattr(
        fit_estimator_module,
        "normalize_market_data_source",
        counting_normalize_source,
    )
    monkeypatch.setattr(
        fit_estimator_module,
        "translate_market_data_request_to_prepared_calibration_from_phases",
        counting_translate_request,
    )
    monkeypatch.setattr(
        fit_estimator_module,
        "_run_prepared_staged_calibration_with_raw_payload",
        lambda *args, **kwargs: _successful_staged_runner_payload("high-level-fit-single-pass"),
    )

    estimator = JaniformEstimator(
        workflow_scope=CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY,
        tick_data_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
        ),
        session="XNYS",
        known_news="none",
    )

    estimator.fit(
        run_id="high-level-fit-single-pass",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
    )

    assert normalize_call_count == 1
    assert translate_call_count == 1


def test_high_level_fit_named_built_in_session_does_not_require_materialization(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
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

    translate_call_count = 0
    original_translate_request = (
        fit_estimator_module.translate_market_data_request_to_prepared_calibration_from_phases
    )

    def counting_translate_request(*args, **kwargs):
        nonlocal translate_call_count
        translate_call_count += 1
        return original_translate_request(*args, **kwargs)

    monkeypatch.setattr(
        fit_estimator_module,
        "translate_market_data_request_to_prepared_calibration_from_phases",
        counting_translate_request,
    )
    monkeypatch.setattr(
        fit_estimator_module,
        "_run_prepared_staged_calibration_with_raw_payload",
        lambda *args, **kwargs: _successful_staged_runner_payload("high-level-fit-built-in-session"),
    )

    def fail_if_called(*args, **kwargs):
        raise AssertionError("TemporaryDirectory should not be used for built-in session presets")

    monkeypatch.setattr(fit_estimator_module, "TemporaryDirectory", fail_if_called)

    estimator = JaniformEstimator(
        workflow_scope=CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY,
        tick_data_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
        ),
        session="XNYS",
        known_news="none",
    )

    estimator.fit(
        run_id="high-level-fit-built-in-session",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
    )

    assert translate_call_count == 1


def test_high_level_fit_progress_splits_loading_and_translation_steps(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
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

    progress_details: list[str] = []

    class _CapturingProgress:
        def __enter__(self) -> Self:
            return self

        def __exit__(self, exc_type, exc_value, traceback) -> None:
            _ = exc_type
            _ = exc_value
            _ = traceback

            return None

        def update(
            self,
            *,
            detail: str | None = None,
            subdetail: str | None = None,
            completed_units: int | None = None,
            total_units: int | None = None,
            reset_progress_units: bool = False,
        ) -> None:
            _ = subdetail
            _ = completed_units
            _ = total_units
            _ = reset_progress_units
            if detail is not None:
                progress_details.append(detail)

    def capturing_progress(
        self: Any,
        label: str,
        *,
        total_steps: int | None = None,
    ) -> _CapturingProgress:
        assert label == "fit"
        assert total_steps == 8

        return _CapturingProgress()

    monkeypatch.setattr(
        fit_estimator_module.JaniformOutputController,
        "progress",
        capturing_progress,
    )
    monkeypatch.setattr(
        fit_estimator_module,
        "_run_prepared_staged_calibration_with_raw_payload",
        lambda *args, **kwargs: _successful_staged_runner_payload("high-level-fit-progress"),
    )

    estimator = JaniformEstimator(
        workflow_scope=CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY,
        tick_data_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
        ),
        session="XNYS",
        known_news="none",
    )

    estimator.fit(
        run_id="high-level-fit-progress",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
    )

    assert progress_details == [
        "inferring workflow inputs",
        "normalize source",
        "resolve session",
        "resolve presets",
        "build market-data request",
        "resolve session",
        "prepare stage inputs",
        "running calibration",
    ]


def test_high_level_fit_report_returns_success_details_with_model(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
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
                "2026-02-04T14:30:00Z,105.0,buy",
            ]
        ),
        encoding="utf-8",
    )

    estimator = JaniformEstimator(
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        tick_data_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        ),
        market_profile="us_equity_cash",
    )
    monkeypatch.setattr(
        fit_estimator_module,
        "_run_prepared_staged_calibration_with_raw_payload",
        lambda prepared_handle, **kwargs: _successful_full_model_staged_runner_payload(
            _prepared_runner_run_id(prepared_handle)
        ),
    )

    model, fit_report = estimator.fit(
        run_id="high-level-fit-failure",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
        report=True,
    )

    assert model is not None
    assert fit_report.success is True
    assert fit_report.staged_response.failure is None
    assert [stage_payload["stage_report_kind"] for stage_payload in fit_report.stage_report_payloads] == [
        "seasonality",
        "hawkes",
        "return_components",
        "mean_reversion",
        "joint_refinement",
    ]


def test_high_level_fit_rejects_missing_exchange_context_for_automatic_session_inference(
    tmp_path: Path,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc",
                "2026-02-02T14:30:00Z",
                "2026-02-02T15:00:00Z",
            ]
        ),
        encoding="utf-8",
    )

    estimator = JaniformEstimator(
        workflow_scope=CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY,
        tick_data_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
        ),
    )

    with pytest.raises(CalibrationValidationError):
        estimator.fit(
            run_id="high-level-fit-invalid",
            tick_data_path=tick_data_path,
            tick_data_format=CalibrationInputFileFormat.CSV,
        )


def _patch_successful_staged_runner(
    monkeypatch: pytest.MonkeyPatch,
    *,
    run_id: str,
) -> None:
    monkeypatch.setattr(
        fit_estimator_module,
        "_run_prepared_staged_calibration_with_raw_payload",
        lambda *args, **kwargs: _successful_staged_runner_payload(run_id),
    )


def _build_full_workflow_model(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    run_id: str,
) -> JaniformModel:
    tick_data_path = tmp_path / f"{run_id}.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:00:00Z,101.0,sell",
                "2026-02-02T20:59:00Z,102.0,buy",
                "2026-02-03T14:30:00Z,99.0,sell",
                "2026-02-03T20:59:00Z,98.0,buy",
                "2026-02-04T14:30:00Z,97.0,sell",
            ]
        ),
        encoding="utf-8",
    )

    monkeypatch.setattr(
        fit_estimator_module,
        "_run_prepared_staged_calibration_with_raw_payload",
        lambda *args, **kwargs: _successful_full_model_staged_runner_payload(run_id),
    )

    estimator = JaniformEstimator(
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        tick_data_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        ),
        market_profile="us_equity_cash",
    )

    return estimator.fit(
        run_id=run_id,
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
    )


def _build_test_hourly_factor_contract(
    *,
    start_time_utc: datetime,
    interval_end_times_utc: tuple[datetime, ...],
) -> HourlyFactorContract:
    interval_start_utc_nanos = [
        datetime_to_utc_nanos(start_time_utc),
        *[
            datetime_to_utc_nanos(timestamp_utc)
            for timestamp_utc in interval_end_times_utc[:-1]
        ],
    ]
    interval_end_utc_nanos = [
        datetime_to_utc_nanos(timestamp_utc)
        for timestamp_utc in interval_end_times_utc
    ]
    interval_duration_nanos = interval_end_utc_nanos[0] - interval_start_utc_nanos[0]

    return HourlyFactorContract(
        SimpleNamespace(
            schema_version="seasonality.hourly_factor.contract.v1",
            exchange_code="XNYS",
            exchange_timezone_name="America/New_York",
            request_start_utc_nanos=interval_start_utc_nanos[0],
            request_end_utc_nanos=interval_end_utc_nanos[-1],
            interval_duration_nanos=interval_duration_nanos,
            interval_indices=list(range(len(interval_end_times_utc))),
            interval_start_utc_nanos=interval_start_utc_nanos,
            interval_end_utc_nanos=interval_end_utc_nanos,
            interval_start_exchange_local=[
                f"start_{interval_index}"
                for interval_index in range(len(interval_end_times_utc))
            ],
            interval_end_exchange_local=[
                f"end_{interval_index}"
                for interval_index in range(len(interval_end_times_utc))
            ],
            s_h=[1.0 for _ in interval_end_times_utc],
            known_component_nu_h=[0.0 for _ in interval_end_times_utc],
            surprise_component_nu_h=[0.0 for _ in interval_end_times_utc],
            nu_h=[0.0 for _ in interval_end_times_utc],
            interval_count=len(interval_end_times_utc),
            first_interval_start_utc_nanos=interval_start_utc_nanos[0],
            last_interval_end_utc_nanos=interval_end_utc_nanos[-1],
            skipped_local_hour_interval_count=0,
            repeated_local_hour_interval_count=0,
            interval_boundary_policy="start_time_utc_anchor_plus_fixed_interval_delta_half_open_utc_boundaries.v1",
            interval_alignment_policy="explicit_runtime_interval_grid_alignment.v1",
            timezone_transition_policy="utc_monotonic_without_local_gap_backfill.v1",
            spring_forward_policy="skip_missing_local_hour_without_synthetic_bucket.v1",
            fall_back_policy="retain_both_utc_hours_with_distinct_utc_boundaries.v1",
            seasonality_materialization_method="test.seasonality",
            news_intensity_materialization_method="test.news_intensity",
        )
    )


def _capture_request(
    captured_requests: dict[str, Any],
    key: str,
    request: Any,
    result: Any,
) -> Any:
    captured_requests[key] = request
    return result


def _successful_staged_runner_payload(run_id: str) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
    raw_response = {
        "schema_version": "calibration.orchestration.staged_run.report.v1",
        "failure_triage_policy": "mode_aware_failure_kind_and_checkpoint_context.v1",
        "run_id": run_id,
        "execution_mode": "memory",
        "status": "success",
        "stage_reports": [
            {
                "stage_report_kind": "seasonality",
                "stage": "seasonality",
                "status": "success",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "normalized_stage_payload": {
                    "composition_schema_version": "seasonality.deterministic.composition.v1",
                    "exchange_code": "XNYS",
                    "intraday_minutes_per_bin": 480,
                    "intraday_factor_by_bin": [1.0] * 3,
                    "day_of_week_factor": [1.0] * 7,
                    "day_of_year_factor": [1.0] * 366,
                },
            }
        ],
        "emitted_checkpoints": [],
    }

    return validate_staged_calibration_response(raw_response), raw_response


def _successful_full_model_staged_runner_payload(run_id: str) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
    raw_response = {
        "schema_version": "calibration.orchestration.staged_run.report.v1",
        "failure_triage_policy": "mode_aware_failure_kind_and_checkpoint_context.v1",
        "run_id": run_id,
        "execution_mode": "memory",
        "status": "success",
        "stage_reports": [
            {
                "stage_report_kind": "seasonality",
                "stage": "seasonality",
                "status": "success",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "normalized_stage_payload": {
                    "composition_schema_version": "seasonality.deterministic.composition.v1",
                    "exchange_code": "XNYS",
                    "timestamp_utc_nanos": [
                        1770042600000000000,
                        1770046200000000000,
                        1770129000000000000,
                    ],
                    "normalized_combined_factor_by_event": [0.95, 1.05, 1.0],
                    "intraday_minutes_per_bin": 480,
                    "intraday_factor_by_bin": [1.0] * 3,
                    "day_of_week_factor": [1.0] * 7,
                    "day_of_year_factor": [1.0] * 366,
                },
            },
            {
                "stage_report_kind": "hawkes",
                "stage": "hawkes",
                "status": "success",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "fitted_stage_payload": {
                    "schema_version": "calibration.stage_output.hawkes.v1",
                    "selected_candidate_id": "candidate_stable_low_loss",
                    "fitted_parameters": {
                        "kernel_parameters": {
                            "lambda_bar": 0.8,
                            "beta": 1.6,
                            "alpha": {
                                "plus_plus": 0.2,
                                "plus_minus": 0.1,
                                "minus_plus": 0.1,
                                "minus_minus": 0.25,
                            },
                            "rho": {
                                "plus_plus": 0.08,
                                "plus_minus": 0.04,
                                "minus_plus": 0.05,
                                "minus_minus": 0.1,
                            },
                        },
                        "structured_alpha": {
                            "a_self": 0.35,
                            "a_cross": 0.06,
                            "delta_self": 0.07,
                            "delta_cross": 0.04,
                        },
                        "structured_rho": {
                            "r_self": 0.3,
                            "r_cross": 0.08,
                            "delta_self_r": 0.04,
                            "delta_cross_r": 0.05,
                        },
                        "self_dominance_policy": {
                            "minimum_self_to_cross_ratio": 1.0,
                        },
                        "stability_numeric_tolerance": 0.0,
                    },
                },
            },
            {
                "stage_report_kind": "return_components",
                "stage": "return_components",
                "status": "success",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "fitted_stage_payload": {
                    "schema_version": "calibration.stage_output.return_components.v1",
                    "selected_candidate_id": "return_candidate_balanced",
                    "fitted_parameters": {
                        "nig_shared_plus_shift_configuration": {
                            "alpha": 2.3,
                            "beta_0": -0.8,
                            "delta": 0.7,
                            "mu": 0.1,
                            "delta_beta": -0.2,
                        },
                        "news_process_configuration": {
                            "schema_version": "news.process.configuration.v1",
                            "arrivals": {
                                "known": {
                                    "scheduled_event_timestamps_utc_nanos": [],
                                },
                                "surprise": {
                                    "baseline_intensity": 0.02,
                                },
                            },
                            "shared_mark_law": {
                                "gamma_severity_marks": {
                                    "shape_k": 1.4,
                                    "scale_theta": 0.6,
                                },
                                "double_exponential_base_shock": {
                                    "probability_positive": 0.35,
                                    "eta_plus": 2.1,
                                    "eta_minus": 3.0,
                                },
                                "coupled_gap_marks": {
                                    "sigma_g": 0.8,
                                },
                            },
                            "known_component_mark_scales": {
                                "activity_coupling_scale": 1.0,
                                "return_jump_scale": 1.0,
                            },
                            "surprise_component_mark_scales": {
                                "activity_coupling_scale": 1.0,
                                "return_jump_scale": 1.0,
                            },
                        },
                        "surprise_deterministic_intensity_profile": {
                            "weekly_hour_utc_multipliers": [
                                2.0 if hour_index == 14 else 1.0 for hour_index in range(168)
                            ],
                            "profile_resolution_hours": 1,
                            "fit_policy": (
                                "selected_exogenous_surprise_episode_rate_per_utc_hour_of_week_"
                                "with_baseline_shrinkage.v1"
                            ),
                            "normalization_policy": (
                                "exposure_weighted_mean_one_over_source_window.v1"
                            ),
                        },
                        "mean_reversion_fitted_parameters": build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload(),
                    },
                },
            },
            {
                "stage_report_kind": "mean_reversion",
                "stage": "mean_reversion",
                "status": "success",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "fitted_stage_payload": {
                    "schema_version": "calibration.stage_output.mean_reversion.v1",
                    "fitted_parameters": build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload(),
                    "diagnostics": {
                        "selected_candidate_family": "random_walk_null",
                        "calibration_grid": {
                            "calibration_grid_kind": "synthetic_daily_close_grid.v1",
                            "calibration_grid_interval_hours": 24.0,
                            "source_observation_count": 4,
                            "retained_observation_count": 0,
                            "coverage_ratio": 0.0,
                        },
                        "anchor_parameter_estimates": {
                            "reference_interval_hours": 24.0,
                            "pressure_persistence_per_reference_interval": 0.0,
                            "pressure_level_gain": 0.0,
                        },
                        "anchor_origin_level": None,
                        "phi_reference": 1.0,
                        "kappa": 0.0,
                        "residual_sum_of_squares": 0.0,
                        "quasi_log_likelihood": None,
                        "information_criterion_bic": None,
                        "model_selection_policy": "gaussian_quasi_likelihood_bic_with_endogenous_core_pressure_level_candidate_family_profile.v1",
                        "deviation_sample_count": 0,
                        "regression_pair_count": 0,
                        "least_squares_rank": 0,
                        "evaluated_candidates": [
                            {
                                "candidate_family": "random_walk_null",
                                "anchor_model": {
                                    "reference_interval_hours": 24.0,
                                    "pressure_persistence_per_reference_interval": 0.0,
                                    "pressure_level_gain": 0.0,
                                },
                                "anchor_origin_level": None,
                                "phi_reference": 1.0,
                                "kappa": 0.0,
                                "residual_sum_of_squares": 0.0,
                                "quasi_log_likelihood": None,
                                "information_criterion_bic": None,
                                "parameter_count": 0,
                                "least_squares_rank": 0,
                                "deviation_sample_count": 0,
                                "regression_pair_count": 0,
                                "core_expected_increments": [],
                                "pressure_state_path": [],
                                "boundary_anchor_values": [],
                                "deviations": [],
                                "residuals": [],
                                "stage_decision_reason": "endogenous_anchor_runtime_projection_required.v1",
                            }
                        ],
                        "stage_decision_reason": "endogenous_anchor_runtime_projection_required.v1",
                    },
                    "return_components_input_transport": "in_memory_output",
                    "return_components_selected_candidate_id": "return_candidate_balanced",
                },
            },
            {
                "stage_report_kind": "joint_refinement",
                "stage": "joint_refinement",
                "status": "success",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "fitted_stage_payload": {
                    "schema_version": "calibration.stage_output.joint_refinement.v1",
                    "selected_candidate_id": "joint_refinement_baseline",
                    "fitted_parameters": {
                        "hawkes_fitted_parameters": {
                            "kernel_parameters": {
                                "lambda_bar": 0.8,
                                "beta": 1.6,
                                "alpha": {
                                    "plus_plus": 0.2,
                                    "plus_minus": 0.1,
                                    "minus_plus": 0.1,
                                    "minus_minus": 0.25,
                                },
                                "rho": {
                                    "plus_plus": 0.08,
                                    "plus_minus": 0.04,
                                    "minus_plus": 0.05,
                                    "minus_minus": 0.1,
                                },
                            },
                            "structured_alpha": {
                                "a_self": 0.35,
                                "a_cross": 0.06,
                                "delta_self": 0.07,
                                "delta_cross": 0.04,
                            },
                            "structured_rho": {
                                "r_self": 0.3,
                                "r_cross": 0.08,
                                "delta_self_r": 0.04,
                                "delta_cross_r": 0.05,
                            },
                            "self_dominance_policy": {
                                "minimum_self_to_cross_ratio": 1.0,
                            },
                            "stability_numeric_tolerance": 0.0,
                        },
                        "return_components_fitted_parameters": {
                            "nig_shared_plus_shift_configuration": {
                                "alpha": 2.3,
                                "beta_0": -0.8,
                                "delta": 0.7,
                                "mu": 0.1,
                                "delta_beta": -0.2,
                            },
                            "news_process_configuration": {
                                "schema_version": "news.process.configuration.v1",
                                "arrivals": {
                                    "known": {
                                        "scheduled_event_timestamps_utc_nanos": [],
                                    },
                                    "surprise": {
                                        "baseline_intensity": 0.02,
                                    },
                                },
                                "shared_mark_law": {
                                    "gamma_severity_marks": {
                                        "shape_k": 1.4,
                                        "scale_theta": 0.6,
                                    },
                                    "double_exponential_base_shock": {
                                        "probability_positive": 0.35,
                                        "eta_plus": 2.1,
                                        "eta_minus": 3.0,
                                    },
                                    "coupled_gap_marks": {
                                        "sigma_g": 0.8,
                                    },
                                },
                                "known_component_mark_scales": {
                                    "activity_coupling_scale": 1.0,
                                    "return_jump_scale": 1.0,
                                },
                                "surprise_component_mark_scales": {
                                    "activity_coupling_scale": 1.0,
                                    "return_jump_scale": 1.0,
                                },
                            },
                            "surprise_deterministic_intensity_profile": {
                                "weekly_hour_utc_multipliers": [
                                    2.0 if hour_index == 14 else 1.0 for hour_index in range(168)
                                ],
                                "profile_resolution_hours": 1,
                                "fit_policy": (
                                    "selected_exogenous_surprise_episode_rate_per_utc_hour_of_week_"
                                    "with_baseline_shrinkage.v1"
                                ),
                                "normalization_policy": (
                                    "exposure_weighted_mean_one_over_source_window.v1"
                                ),
                            },
                            "mean_reversion_fitted_parameters": build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload(),
                        },
                    },
                    "baseline_hawkes_fitted_parameters": {
                        "kernel_parameters": {
                            "lambda_bar": 0.8,
                            "beta": 1.6,
                            "alpha": {
                                "plus_plus": 0.2,
                                "plus_minus": 0.1,
                                "minus_plus": 0.1,
                                "minus_minus": 0.25,
                            },
                            "rho": {
                                "plus_plus": 0.08,
                                "plus_minus": 0.04,
                                "minus_plus": 0.05,
                                "minus_minus": 0.1,
                            },
                        },
                        "structured_alpha": {
                            "a_self": 0.35,
                            "a_cross": 0.06,
                            "delta_self": 0.07,
                            "delta_cross": 0.04,
                        },
                        "structured_rho": {
                            "r_self": 0.3,
                            "r_cross": 0.08,
                            "delta_self_r": 0.04,
                            "delta_cross_r": 0.05,
                        },
                        "self_dominance_policy": {
                            "minimum_self_to_cross_ratio": 1.0,
                        },
                        "stability_numeric_tolerance": 0.0,
                    },
                    "baseline_return_components_fitted_parameters": {
                        "nig_shared_plus_shift_configuration": {
                            "alpha": 2.3,
                            "beta_0": -0.8,
                            "delta": 0.7,
                            "mu": 0.1,
                            "delta_beta": -0.2,
                        },
                        "news_process_configuration": {
                            "schema_version": "news.process.configuration.v1",
                            "arrivals": {
                                "known": {
                                    "scheduled_event_timestamps_utc_nanos": [],
                                },
                                "surprise": {
                                    "baseline_intensity": 0.02,
                                },
                            },
                            "shared_mark_law": {
                                "gamma_severity_marks": {
                                    "shape_k": 1.4,
                                    "scale_theta": 0.6,
                                },
                                "double_exponential_base_shock": {
                                    "probability_positive": 0.35,
                                    "eta_plus": 2.1,
                                    "eta_minus": 3.0,
                                },
                                "coupled_gap_marks": {
                                    "sigma_g": 0.8,
                                },
                            },
                            "known_component_mark_scales": {
                                "activity_coupling_scale": 1.0,
                                "return_jump_scale": 1.0,
                            },
                            "surprise_component_mark_scales": {
                                "activity_coupling_scale": 1.0,
                                "return_jump_scale": 1.0,
                            },
                        },
                        "surprise_deterministic_intensity_profile": {
                            "weekly_hour_utc_multipliers": [
                                2.0 if hour_index == 14 else 1.0 for hour_index in range(168)
                            ],
                            "profile_resolution_hours": 1,
                            "fit_policy": (
                                "selected_exogenous_surprise_episode_rate_per_utc_hour_of_week_"
                                "with_baseline_shrinkage.v1"
                            ),
                            "normalization_policy": (
                                "exposure_weighted_mean_one_over_source_window.v1"
                            ),
                        },
                        "mean_reversion_fitted_parameters": build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload(),
                    },
                    "objective_breakdown": {
                        "aggregated_likelihood_fit": 1.0,
                        "aggregated_return_shape_fit": 1.0,
                        "aggregated_return_scale_fit": 1.0,
                        "business_time_return_scale_fit": 1.0,
                        "intraday_return_level_fit": 1.0,
                        "activity_shape_fit": 1.0,
                        "activity_level_fit": 1.0,
                        "rv_activity_coupling_fit": 1.0,
                        "price_span_sanity_fit": 1.0,
                        "weighted_objective": 1.0,
                        "hard_constraint_penalty": 0.0,
                    },
                    "optimizer_diagnostics": {
                        "local_objective": 1.0,
                        "final_objective": 1.0,
                        "local_evaluation_count": 0,
                        "final_simulation_seeds": [17],
                        "stop_reason": "baseline_seed",
                    },
                },
            },
        ],
        "emitted_checkpoints": [],
    }

    return validate_staged_calibration_response(raw_response), raw_response


def _prepared_runner_run_id(prepared_handle: Any) -> str:
    prepared_payload = json.loads(prepared_handle.serialize_execution_input_json())
    return str(prepared_payload["request"]["run_id"])
