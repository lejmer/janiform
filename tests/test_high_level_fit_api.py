import json
import math
from datetime import UTC, datetime
from pathlib import Path
from types import SimpleNamespace
from typing import Any, NoReturn, Self

import pytest

import janiform.calibration.fit.estimator as fit_estimator_module
import janiform.calibration.fit.model as fit_model_module
from janiform.calibration import CalibrationValidationError
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
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
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


def test_janiform_estimator_supports_simple_zero_config_tick_fit_without_tempdir(
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

    def _unexpected_tempdir(*args, **kwargs) -> NoReturn:
        raise AssertionError("temporary directory should not be created for the common memory path")

    monkeypatch.setattr(fit_estimator_module, "TemporaryDirectory", _unexpected_tempdir)

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
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
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
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
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
    assert summary["session_resolution"]["resolution_kind"] == "automatic_inference"
    assert summary["fitted_parameter_summary"]["selected_candidate_ids"] == {
        "hawkes": "candidate_stable_low_loss",
        "return_components": "return_candidate_balanced",
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
    assert first_path.read_text(encoding="utf-8").startswith("{\n  ")
    assert first_path.read_text(encoding="utf-8") == second_path.read_text(encoding="utf-8")


def test_loaded_high_level_model_runtime_seasonality_uses_native_timestamp_preprocessing(
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
    captured_call: dict[str, Any] = {}

    class _FakeTimestampEventPreprocessingResult:
        exchange_local_minute_of_day = [570, 630]
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
        captured_call["event_records"] = event_records
        captured_call["exchange_code"] = exchange_code
        captured_call["calendar_registry"] = calendar_registry
        captured_call["timestamp_field_name"] = timestamp_field_name
        captured_call["session_filter"] = session_filter
        captured_call["sparse_hour_event_threshold"] = sparse_hour_event_threshold
        return _FakeTimestampEventPreprocessingResult()

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

    assert captured_call["exchange_code"] == loaded_model.session_resolution.exchange_code
    assert captured_call["calendar_registry"] is calendar_registry
    assert captured_call["timestamp_field_name"] == "timestamp"
    assert captured_call["session_filter"] == fit_model_module.SessionFilter.ALL
    assert captured_call["sparse_hour_event_threshold"] == 1
    assert captured_call["event_records"] == [
        {"timestamp": datetime(2026, 2, 2, 14, tzinfo=UTC)},
        {"timestamp": datetime(2026, 2, 2, 15, tzinfo=UTC)},
    ]
    assert seasonality_source.hourly_factors == (1.0, 1.0)


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
                return_components=SimpleNamespace(total_return=0.01),
            ),
            SimpleNamespace(
                interval_start_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 8, 20, 45, tzinfo=UTC)),
                interval_end_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 8, 21, 0, tzinfo=UTC)),
                return_components=SimpleNamespace(total_return=0.02),
            ),
            SimpleNamespace(
                interval_start_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 8, 21, 0, tzinfo=UTC)),
                interval_end_utc_nanos=datetime_to_utc_nanos(datetime(2026, 2, 8, 21, 15, tzinfo=UTC)),
                return_components=SimpleNamespace(total_return=0.03),
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
            math.exp(0.06),
        ]
    )


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
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
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

    load_call_count = 0
    translate_call_count = 0
    extra_python_load_count = 0
    original_load_translation_inputs = fit_estimator_module._load_market_data_translation_inputs
    original_translate_loaded_inputs = (
        fit_estimator_module._translate_loaded_market_data_request_to_staged_execution_input
    )
    original_load_market_data = fit_estimator_module.load_market_data_calibration_inputs

    def counting_load_translation_inputs(*args, **kwargs):
        nonlocal load_call_count
        load_call_count += 1
        return original_load_translation_inputs(*args, **kwargs)

    def counting_translate_loaded_inputs(*args, **kwargs):
        nonlocal translate_call_count
        translate_call_count += 1
        return original_translate_loaded_inputs(*args, **kwargs)

    def unexpected_extra_python_load(*args, **kwargs):
        nonlocal extra_python_load_count
        extra_python_load_count += 1
        return original_load_market_data(*args, **kwargs)

    monkeypatch.setattr(
        fit_estimator_module,
        "_load_market_data_translation_inputs",
        counting_load_translation_inputs,
    )
    monkeypatch.setattr(
        fit_estimator_module,
        "_translate_loaded_market_data_request_to_staged_execution_input",
        counting_translate_loaded_inputs,
    )
    monkeypatch.setattr(
        fit_estimator_module,
        "load_market_data_calibration_inputs",
        unexpected_extra_python_load,
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
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
        ),
        session="XNYS",
        known_news="none",
    )

    estimator.fit(
        run_id="high-level-fit-single-pass",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
    )

    assert load_call_count == 1
    assert translate_call_count == 1
    assert extra_python_load_count == 0


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
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
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


def test_high_level_fit_report_returns_failure_details_without_model(
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
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        ),
        market_profile="us_equity_cash",
    )

    model, fit_report = estimator.fit(
        run_id="high-level-fit-failure",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
        report=True,
    )

    assert model is None
    assert fit_report.success is False
    assert fit_report.staged_response.failure is not None
    assert fit_report.staged_response.failure.stage is not None
    assert fit_report.stage_report_payloads[-1]["stage_report_kind"] == "hawkes"


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
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
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
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        ),
        market_profile="us_equity_cash",
    )

    return estimator.fit(
        run_id=run_id,
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
    )


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
                    },
                },
            },
        ],
        "emitted_checkpoints": [],
    }

    return validate_staged_calibration_response(raw_response), raw_response
