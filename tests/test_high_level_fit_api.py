import json
import math
from collections.abc import Mapping
from datetime import UTC, datetime, timedelta
from pathlib import Path
from types import SimpleNamespace
from typing import Any, NoReturn, Self

import pyarrow as pa
import pandas
import pytest
from pyarrow import parquet

import janiform.calibration.fit.estimator as fit_estimator_module
import janiform.calibration.fit.model as fit_model_module
import janiform.calibration.market_data.ingestion as market_data_ingestion_module
from janiform.calibration import (
    CalibrationValidationError,
    MeanReversionAnchorModel,
    MeanReversionFittedParameters,
)
from janiform._timestamps import datetime_to_utc_nanos
from janiform.calibration.fit import (
    JaniformEstimator,
    JaniformFitReport,
    JaniformModel,
    JaniformSimulationResult,
)
from janiform.calibration.market_data import (
    CalibrationDuplicateRecordPolicy,
    CalibrationInputFileFormat,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
)
from janiform.calibration.staged import StagedCalibrationResponse, validate_staged_calibration_response
from janiform.calendar import CalendarRegistry, MarketState
from janiform.hourly_factors import HourlyFactorContract
from janiform.simulation import SeededTickEventSide
from tests.calibration_test_support import build_disabled_mean_reversion_fitted_parameters_payload


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
        "_run_staged_calibration_with_raw_payload",
        lambda execution_input, **kwargs: _successful_full_model_staged_runner_payload(
            execution_input.request.run_id
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
        "translate_market_data_request_to_staged_execution_input",
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
        "_run_staged_calibration_with_raw_payload",
        lambda execution_input, **kwargs: _successful_full_model_staged_runner_payload(
            execution_input.request.run_id
        ),
    )

    estimator = JaniformEstimator(market_profile="us_equity_cash")
    model = estimator.fit(tick_data_path=tick_data_path)
    runtime_factors = fit_model_module._prepare_model_runtime_factors(
        model,
        start_time_utc=datetime(2026, 2, 2, 14, 0, tzinfo=UTC),
        horizon_interval_count=2,
        interval="1h",
    )

    assert runtime_factors.surprise_component_nu_h[0] == pytest.approx(0.04)
    assert runtime_factors.surprise_component_nu_h[1] == pytest.approx(0.02)


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
        "_run_staged_calibration_with_raw_payload",
        lambda execution_input, **kwargs: _successful_full_model_staged_runner_payload(
            execution_input.request.run_id
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
    _, session_artifact = registry.resolve_session_preset("fx_spot_24_5")

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
    assert (
        summary["preset_provenance"]["known_news"]["selected_preset"]["preset_name"]
        == "none"
    )
    assert summary["known_news_activation"] == {
        "selected_preset": "none",
        "event_count": 0,
        "active": False,
    }
    assert summary["session_resolution"]["resolution_kind"] == "explicit_override"
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
        == build_disabled_mean_reversion_fitted_parameters_payload()
    )
    assert "timestamp_utc_nanos" not in persisted_payload["parameters"]["seasonality"]
    assert "normalized_combined_factor_by_event" not in persisted_payload["parameters"]["seasonality"]
    assert first_path.read_text(encoding="utf-8").startswith("{\n  ")
    assert first_path.read_text(encoding="utf-8") == second_path.read_text(encoding="utf-8")


def test_high_level_model_save_and_load_roundtrip_preserves_deterministic_mean_reversion_block(
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
        anchor_model=MeanReversionAnchorModel.deterministic_interval_grid(
            source_interval_start_utc_nanos=(10, 20),
            source_interval_end_utc_nanos=(20, 30),
            anchor_values=(4.2, 4.1),
            source_alignment_policy="piecewise_constant_utc_interval_grid.v1",
        ),
    ).to_payload()

    model.parameters["return_components"]["fitted_parameters"][
        "mean_reversion_fitted_parameters"
    ] = mean_reversion_payload
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

    class _FakeTimestampEventPreprocessingResult:
        exchange_local_minute_of_day = [540, 600]
        exchange_local_day_of_week = bytes([1, 1])
        exchange_local_day_of_year = [33, 33]

    def _capture_preprocess_timestamp_events(
        event_records: list[dict[str, datetime]],
        *,
        exchange_code: str,
        calendar_registry: CalendarRegistry,
        timestamp_field_name: str = "timestamp",
        session_filter: fit_model_module.SessionFilter = fit_model_module.SessionFilter.OPEN_ONLY,
        sparse_hour_event_threshold: int = 1,
    ) -> _FakeTimestampEventPreprocessingResult:
        assert exchange_code == loaded_model.session_resolution.exchange_code
        assert calendar_registry is calendar_registry_instance
        assert timestamp_field_name == "timestamp"
        assert session_filter == fit_model_module.SessionFilter.ALL
        assert sparse_hour_event_threshold == 1
        assert event_records == [
            {"timestamp": datetime(2026, 2, 2, 14, tzinfo=UTC)},
            {"timestamp": datetime(2026, 2, 2, 15, tzinfo=UTC)},
        ]

        return _FakeTimestampEventPreprocessingResult()

    calendar_registry_instance = calendar_registry

    monkeypatch.setattr(
        fit_model_module,
        "preprocess_timestamp_events",
        _capture_preprocess_timestamp_events,
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

    class _FakeTimestampEventPreprocessingResult:
        exchange_local_minute_of_day = [540, 600]
        exchange_local_day_of_week = bytes([1, 1])
        exchange_local_day_of_year = [33, 33]

    monkeypatch.setattr(
        fit_model_module,
        "preprocess_timestamp_events",
        lambda *args, **kwargs: _FakeTimestampEventPreprocessingResult(),
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

    runtime_factors = fit_model_module._prepare_model_runtime_factors(
        model,
        start_time_utc=datetime(2026, 2, 2, 14, 0, tzinfo=UTC),
        horizon_interval_count=4,
        interval="15min",
    )

    assert runtime_factors.seasonality_hourly_factors == pytest.approx(
        (0.0, 0.0, 1.0, 1.0)
    )


def test_runtime_coarse_news_intensity_components_scale_known_news_to_per_hour_units() -> None:
    known_news_timestamp_utc = datetime(2026, 2, 2, 14, 5, tzinfo=UTC)
    news_materialization = SimpleNamespace(
        interval_start_utc_nanos=(
            datetime_to_utc_nanos(datetime(2026, 2, 2, 14, 0, tzinfo=UTC)),
            datetime_to_utc_nanos(datetime(2026, 2, 2, 14, 15, tzinfo=UTC)),
        ),
        interval_end_utc_nanos=(
            datetime_to_utc_nanos(datetime(2026, 2, 2, 14, 15, tzinfo=UTC)),
            datetime_to_utc_nanos(datetime(2026, 2, 2, 14, 30, tzinfo=UTC)),
        ),
        surprise_component_nu_h=(0.2, 0.3),
    )

    known_component_nu_h, surprise_component_nu_h = (
        fit_model_module._normalize_runtime_coarse_news_intensity_components(
            news_materialization=news_materialization,
            known_news_event_timestamps_utc=(known_news_timestamp_utc,),
            interval_delta=0.25,
        )
    )

    assert known_component_nu_h == pytest.approx((4.0, 0.0))
    assert surprise_component_nu_h == pytest.approx((0.2, 0.3))


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
        lambda request: captured_calls.append(("coarse", request)) or coarse_result,
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_seeded_tick_event_simulation",
        lambda request: captured_calls.append(("ticks", request)) or tick_result,
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

    def _unexpected_seeded_contract_loader(**kwargs) -> NoReturn:
        del kwargs
        raise AssertionError("coarse simulation should not prepare seeded hourly factor inputs")

    def _unexpected_seeded_runtime(request: Any) -> NoReturn:
        del request
        raise AssertionError("coarse simulation should not dispatch the seeded runtime")

    monkeypatch.setattr(
        fit_model_module,
        "load_hourly_factor_contract_for_simulation",
        _unexpected_seeded_contract_loader,
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_seeded_tick_event_simulation",
        _unexpected_seeded_runtime,
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_coarse_simulation",
        lambda request: coarse_result,
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
        anchor_model=MeanReversionAnchorModel.deterministic_interval_grid(
            source_interval_start_utc_nanos=(
                datetime_to_utc_nanos(datetime(2026, 2, 2, 15, 0, tzinfo=UTC)),
                datetime_to_utc_nanos(datetime(2026, 2, 2, 16, 0, tzinfo=UTC)),
                datetime_to_utc_nanos(datetime(2026, 2, 2, 17, 0, tzinfo=UTC)),
            ),
            source_interval_end_utc_nanos=(
                datetime_to_utc_nanos(datetime(2026, 2, 2, 16, 0, tzinfo=UTC)),
                datetime_to_utc_nanos(datetime(2026, 2, 2, 17, 0, tzinfo=UTC)),
                datetime_to_utc_nanos(datetime(2026, 2, 2, 18, 0, tzinfo=UTC)),
            ),
            anchor_values=(0.25, 0.5, 0.75),
            source_alignment_policy="piecewise_constant_utc_interval_grid.v1",
        ),
    ).to_payload()

    model.parameters["return_components"]["fitted_parameters"][
        "mean_reversion_fitted_parameters"
    ] = mean_reversion_payload
    model.parameters["joint_refinement"]["fitted_parameters"]["return_components_fitted_parameters"][
        "mean_reversion_fitted_parameters"
    ] = mean_reversion_payload
    model.parameters["joint_refinement"]["baseline_return_components_fitted_parameters"][
        "mean_reversion_fitted_parameters"
    ] = mean_reversion_payload

    captured_requests: dict[str, Any] = {}
    coarse_result = object()
    seeded_result = object()

    monkeypatch.setattr(
        fit_model_module,
        "load_hourly_factor_contract_for_simulation",
        lambda **kwargs: _build_test_hourly_factor_contract(
            start_time_utc=kwargs["range_start_utc"],
            interval_end_times_utc=(
                datetime(2026, 2, 2, 16, 30, tzinfo=UTC),
                datetime(2026, 2, 2, 17, 30, tzinfo=UTC),
            ),
        ),
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_seeded_tick_event_simulation",
        lambda request: _capture_request(captured_requests, "seeded", request, seeded_result),
    )
    monkeypatch.setattr(
        fit_model_module,
        "run_coarse_simulation",
        lambda request: _capture_request(captured_requests, "coarse", request, coarse_result),
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

    assert seeded_request.mean_reversion_fitted_parameters.to_payload() == mean_reversion_payload
    assert seeded_request.mean_reversion_runtime_interval_anchors.start_anchor_value == pytest.approx(0.25)
    assert seeded_request.mean_reversion_runtime_interval_anchors.end_anchor_values == pytest.approx(
        (0.5, 0.75)
    )
    assert seeded_request.initial_mean_reversion_boundary_state.observed_log_price == pytest.approx(0.0)
    assert seeded_request.initial_mean_reversion_boundary_state.anchor_value == pytest.approx(0.25)
    assert seeded_request.initial_mean_reversion_boundary_state.deviation == pytest.approx(-0.25)

    coarse_model_inputs = coarse_request.model_inputs.risk_model_inputs
    assert coarse_model_inputs.mean_reversion_fitted_parameters.to_payload() == mean_reversion_payload
    assert coarse_model_inputs.mean_reversion_runtime_interval_anchors.start_anchor_value == pytest.approx(0.25)
    assert coarse_model_inputs.mean_reversion_runtime_interval_anchors.end_anchor_values == pytest.approx(
        (0.5, 0.5)
    )
    assert coarse_model_inputs.initial_mean_reversion_boundary_state.observed_log_price == pytest.approx(0.0)
    assert coarse_model_inputs.initial_mean_reversion_boundary_state.anchor_value == pytest.approx(0.25)
    assert coarse_model_inputs.initial_mean_reversion_boundary_state.deviation == pytest.approx(-0.25)


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
    ]
    assert pandas_frame.iloc[0]["mid_price"] == pytest.approx(1.01)
    assert numpy_array.shape == (1, 2)
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
    _, session_artifact = registry.resolve_session_preset("fx_spot_24_5")
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
            ),
            SimpleNamespace(
                interval_start_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 8, 20, 45, tzinfo=UTC)),
                interval_end_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 8, 21, 0, tzinfo=UTC)),
                observed_log_price_end=999.0,
            ),
            SimpleNamespace(
                interval_start_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 8, 21, 0, tzinfo=UTC)),
                interval_end_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 8, 21, 15, tzinfo=UTC)),
                observed_log_price_end=0.05,
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
                side=SeededTickEventSide.BUY,
            ),
            SimpleNamespace(
                timestamp_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 2, 15, 15, tzinfo=UTC)),
                observed_log_price=0.12,
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

    assert list(pandas_frame.columns) == ["timestamp_utc", "trade_price", "trade_sign"]
    assert pandas_frame.empty
    assert numpy_array.shape == (0, 3)


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
                    side=SeededTickEventSide.BUY,
                ),
                SimpleNamespace(
                    timestamp_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 2, 15, 15, tzinfo=UTC)),
                    observed_log_price=0.12,
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
    assert numpy_array.shape == (2, 3)
    assert list(numpy_array[:, 2]) == ["buy", None]
    assert parquet_rows == [
        {
            "timestamp_utc": datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
            "trade_price": pytest.approx(100.0 * math.exp(0.10)),
            "trade_sign": "buy",
        },
        {
            "timestamp_utc": datetime(2026, 2, 2, 15, 15, tzinfo=UTC),
            "trade_price": pytest.approx(100.0 * math.exp(0.12)),
            "trade_sign": None,
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

    captured_execution_input: dict[str, Any] = {}

    def capturing_runner(*args, **kwargs):
        captured_execution_input["execution_input"] = args[0]
        return _successful_staged_runner_payload("high-level-fit-runner-path")

    monkeypatch.setattr(
        fit_estimator_module,
        "_run_staged_calibration_with_raw_payload",
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

    captured_input = captured_execution_input["execution_input"]
    assert captured_input == fit_report.execution_input


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

    translate_call_count = 0
    session_resolution_load_count = 0
    original_translate_request = (
        fit_estimator_module.translate_market_data_request_to_staged_execution_input
    )
    original_session_resolution = (
        fit_estimator_module.resolve_market_data_session_configuration_from_source_file
    )

    def counting_translate_request(*args, **kwargs):
        nonlocal translate_call_count
        translate_call_count += 1
        return original_translate_request(*args, **kwargs)

    def counting_session_resolution(*args, **kwargs):
        nonlocal session_resolution_load_count
        session_resolution_load_count += 1
        return original_session_resolution(*args, **kwargs)

    monkeypatch.setattr(
        fit_estimator_module,
        "translate_market_data_request_to_staged_execution_input",
        counting_translate_request,
    )
    monkeypatch.setattr(
        fit_estimator_module,
        "resolve_market_data_session_configuration_from_source_file",
        counting_session_resolution,
    )
    monkeypatch.setattr(
        fit_estimator_module,
        "_run_staged_calibration_with_raw_payload",
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

    assert translate_call_count == 1
    assert session_resolution_load_count == 0


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

        def update(self, *, detail: str | None = None) -> None:
            if detail is not None:
                progress_details.append(detail)

    def capturing_progress(
        self: Any,
        label: str,
        *,
        total_steps: int | None = None,
    ) -> _CapturingProgress:
        assert label == "fit"
        assert total_steps == 7
        return _CapturingProgress()

    monkeypatch.setattr(
        fit_estimator_module.JaniformOutputController,
        "progress",
        capturing_progress,
    )
    monkeypatch.setattr(
        fit_estimator_module,
        "_run_staged_calibration_with_raw_payload",
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
        "resolving session configuration",
        "resolving preset configuration",
        "building market-data request",
        "loading market data",
        "translating market data",
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
        "_run_staged_calibration_with_raw_payload",
        lambda execution_input, **kwargs: _successful_full_model_staged_runner_payload(
            execution_input.request.run_id
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
        "_run_staged_calibration_with_raw_payload",
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
        "_run_staged_calibration_with_raw_payload",
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
            interval_start_exchange_local=[],
            interval_end_exchange_local=[],
            s_h=[1.0 for _ in interval_end_times_utc],
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
                        "mean_reversion_fitted_parameters": build_disabled_mean_reversion_fitted_parameters_payload(),
                    },
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
                            "mean_reversion_fitted_parameters": build_disabled_mean_reversion_fitted_parameters_payload(),
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
                        "mean_reversion_fitted_parameters": build_disabled_mean_reversion_fitted_parameters_payload(),
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
