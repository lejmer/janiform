from __future__ import annotations

from datetime import UTC, datetime
from pathlib import Path
from typing import Any, cast

import pytest

import janiform._native_api as native_api_module
import janiform.calibration.fit._runtime.option_pricing.support as pricing_support_module
import janiform.calibration.fit._runtime.payloads as runtime_payloads_module
import janiform.forecast.result as forecast_result_module
from janiform.calibration.fit.state import CurrentStateEstimationResult
from janiform.calibration.market_data import CalibrationTickDataColumnMapping
from janiform.forecast import ForecastTableFormat, JaniformForecast
from janiform.long_variance import LongVarianceState
from janiform.market_pressure import MarketPressureState
from janiform.mean_reversion import MeanReversionBoundaryState
from janiform.model import JaniformModel
from janiform.simulation import HawkesExcitationState
from tests.support.calibration import (
    build_neutral_long_variance_parameters_payload,
    build_no_anchor_impact_market_pressure_parameters_payload,
)


def test_model_forecast_builds_native_request_with_defaults_and_cpu_count(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    native = _ForecastNativeStub()
    monkeypatch.setattr(native_api_module, "native_module", lambda: native)

    model = _forecast_model()
    progress_events: list[dict[str, Any]] = []
    start_date_utc = datetime(2026, 2, 2, 15, tzinfo=UTC)

    forecast = model.forecast(
        start_date_utc=start_date_utc,
        interval="15min",
        horizon_interval_count=96,
        cpu_count=0,
        progress_callback=progress_events.append,
    )

    request = native.forecast_requests[0]

    assert request["start_date_utc"] == start_date_utc.isoformat()
    assert request["initial_price"] is None
    assert request["state"] is None
    assert request["interval_duration_nanos"] == 900_000_000_000
    assert request["horizon_interval_count"] == 96
    assert request["end_date_utc"] is None
    assert request["measure"] == "physical"
    assert request["table_format"] == "parquet"
    assert request["distribution_node_count"] == 513
    assert request["cpu_count"] == 0
    assert native.forecast_progress_modes == ["none"]
    assert forecast.expected_path.column("expected_price") == [100.0, 101.0]


def test_model_forecast_accepts_current_state_without_initial_price(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    native = _ForecastNativeStub()
    monkeypatch.setattr(native_api_module, "native_module", lambda: native)

    model = _forecast_model()
    state = _current_state()

    model.forecast(
        state=state,
        horizon_interval_count=1,
        progress_mode="none",
    )

    request = native.forecast_requests[0]

    assert request["initial_price"] is None
    assert request["state"] == state.to_payload()


def test_model_forecast_rejects_initial_price_with_current_state(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    native = _ForecastNativeStub()
    monkeypatch.setattr(native_api_module, "native_module", lambda: native)

    with pytest.raises(ValueError, match="initial_price must be omitted"):
        _forecast_model().forecast(
            initial_price=100.0,
            state=_current_state(),
            horizon_interval_count=1,
            progress_mode="none",
        )

    assert native.forecast_requests == []


def test_model_state_tracker_updates_native_state_and_forecasts_from_snapshot(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    native = _ForecastNativeStub()
    monkeypatch.setattr(native_api_module, "native_module", lambda: native)
    monkeypatch.setattr(
        pricing_support_module,
        "model_reference_price",
        lambda model: 100.0,
    )
    monkeypatch.setattr(
        runtime_payloads_module,
        "hawkes_kernel_parameters_for_model",
        lambda model: _Payload({"lambda_bar": 1.0, "alpha": {}, "beta": {}, "rho": {}}),
    )
    monkeypatch.setattr(
        runtime_payloads_module,
        "long_variance_parameters_for_model",
        lambda model: _Payload(build_neutral_long_variance_parameters_payload()),
    )
    monkeypatch.setattr(
        runtime_payloads_module,
        "mean_reversion_fitted_parameters_for_model",
        lambda model: _Payload({"kappa": 0.0}),
    )
    monkeypatch.setattr(
        runtime_payloads_module,
        "market_pressure_parameters_for_model",
        lambda model: _Payload(build_no_anchor_impact_market_pressure_parameters_payload()),
    )

    model = _forecast_model()
    state_time_utc = datetime(2026, 2, 2, 15, tzinfo=UTC)
    tracker = model.state_tracker(
        tick_data_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp",
            price_column="price",
            trade_sign_column="side",
        )
    )

    snapshot = tracker.update(
        {
            "timestamp": [state_time_utc],
            "price": [101.0],
            "side": ["buy"],
        }
    )
    forecast = tracker.forecast(horizon_interval_count=1, progress_mode="none")

    tracker_request = native.tracker_requests[0]
    forecast_request = native.forecast_requests[0]

    assert tracker_request["reference_price"] == 100.0
    assert tracker_request["previous_snapshot"] is None
    assert tracker_request["observed_price"] is None
    assert tracker_request["observation_trade_prices"] == (101.0,)
    assert tracker_request["observation_trade_sides"] == ("buy",)
    assert snapshot == _current_state_snapshot()
    assert forecast_request["start_date_utc"] == snapshot.state_time_utc.isoformat()
    assert forecast_request["state"] == snapshot.state.to_payload()
    assert forecast.expected_path.column("expected_price") == [100.0, 101.0]


def test_forecast_save_and_load_round_trip_through_native_bundle_helpers(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    native = _ForecastNativeStub()
    monkeypatch.setattr(forecast_result_module, "native_module", lambda: native)

    forecast = JaniformForecast.from_native_payload(_forecast_payload())
    saved_path = forecast.save(tmp_path / "forecast", table_format=ForecastTableFormat.JSON)
    loaded = JaniformForecast.load(tmp_path / "loaded_forecast")

    assert saved_path == tmp_path / "forecast_20260618T120000Z"
    assert native.save_table_formats == ["json"]
    assert native.saved_payloads[0]["expected_path"][1]["expected_price"] == 101.0
    assert loaded.bundle_path == tmp_path / "loaded_forecast"
    assert loaded.expected_path._path == tmp_path / "loaded_forecast" / "expected_path.parquet"
    assert loaded.terminal_distribution.quantiles.column("probability") == [0.05, 0.5, 0.95]


class _ForecastNativeStub:
    def __init__(self) -> None:
        self.forecast_requests: list[dict[str, Any]] = []
        self.forecast_progress_modes: list[str] = []
        self.tracker_requests: list[dict[str, Any]] = []
        self.saved_payloads: list[dict[str, Any]] = []
        self.save_table_formats: list[str] = []

    def run_forecast_from_model_artifact(
        self,
        request: dict[str, Any],
        event_callback: Any,
        *,
        progress_mode: str,
    ) -> dict[str, Any]:
        self.forecast_requests.append(request)
        self.forecast_progress_modes.append(progress_mode)

        if event_callback is not None:
            event_callback({"phase": "resolving_horizon", "completed_units": 1, "total_units": 1})

        return _forecast_payload()

    def update_current_state_tracker(
        self,
        kernel_parameters: dict[str, Any],
        long_variance_parameters: dict[str, Any],
        mean_reversion_fitted_parameters: dict[str, Any],
        market_pressure_parameters: dict[str, Any],
        reference_price: float,
        previous_snapshot: Any,
        state_time_utc_nanos: int,
        observed_price: Any,
        observation_timestamps_utc: tuple[str, ...],
        observation_trade_prices: tuple[float, ...],
        observation_trade_sides: tuple[str, ...],
    ) -> dict[str, Any]:
        self.tracker_requests.append(
            {
                "kernel_parameters": kernel_parameters,
                "long_variance_parameters": long_variance_parameters,
                "mean_reversion_fitted_parameters": mean_reversion_fitted_parameters,
                "market_pressure_parameters": market_pressure_parameters,
                "reference_price": reference_price,
                "previous_snapshot": previous_snapshot,
                "state_time_utc_nanos": state_time_utc_nanos,
                "observed_price": observed_price,
                "observation_timestamps_utc": observation_timestamps_utc,
                "observation_trade_prices": observation_trade_prices,
                "observation_trade_sides": observation_trade_sides,
            }
        )

        return _current_state_snapshot().to_payload()

    def save_forecast_bundle_from_payload(
        self,
        forecast_payload: dict[str, Any],
        path: str,
        table_format: str,
    ) -> dict[str, str]:
        self.saved_payloads.append(forecast_payload)
        self.save_table_formats.append(table_format)

        return {"path": str(Path(path).with_name("forecast_20260618T120000Z"))}

    def load_forecast_bundle_manifest_py(self, path: str) -> dict[str, Any]:
        loaded_manifest = dict(_forecast_payload()["manifest"])
        loaded_manifest["expected_path_table"] = {"file_name": "expected_path.parquet"}
        loaded_manifest["terminal_distribution_table"] = {
            "file_name": "terminal_distribution.parquet"
        }

        return loaded_manifest


def _forecast_model() -> JaniformModel:
    return JaniformModel(
        run_id="fit_1",
        workflow_scope=cast(Any, "full_without_option_surface"),
        parameters={},
        metadata={},
        resolved_configuration=cast(Any, {}),
        session_resolution=cast(Any, {}),
        _model_payload={"schema_version": "janiform.model_artifact.v1", "run_id": "fit_1"},
    )


def _current_state() -> CurrentStateEstimationResult:
    return CurrentStateEstimationResult(
        hawkes_excitation_state=HawkesExcitationState(
            y_plus_plus=1.0,
            y_plus_minus=2.0,
            y_minus_plus=3.0,
            y_minus_minus=4.0,
        ),
        long_variance_state=LongVarianceState(level=1.25),
        mean_reversion_boundary_state=MeanReversionBoundaryState(
            observed_log_price=0.02,
            anchor_value=0.015,
            deviation=0.005,
        ),
        market_pressure_state=MarketPressureState(state_weights=[0.9, 0.1]),
    )


def _current_state_snapshot() -> Any:
    from janiform.calibration.fit.state import CurrentStateTrackerSnapshot

    return CurrentStateTrackerSnapshot(
        state_time_utc_nanos=1_770_044_400_000_000_000,
        observed_price=101.0,
        state=_current_state(),
    )


class _Payload:
    def __init__(self, payload: dict[str, Any]) -> None:
        self._payload = payload

    def to_payload(self) -> dict[str, Any]:
        return self._payload


def _forecast_payload() -> dict[str, Any]:
    return {
        "manifest": {
            "schema_version": "janiform.forecast.v1",
            "run_id": "forecast_1",
            "quantiles": [
                {"probability": 0.05, "terminal_price": 95.0},
                {"probability": 0.5, "terminal_price": 100.0},
                {"probability": 0.95, "terminal_price": 105.0},
            ],
            "expected_path_table": {"file_name": "expected_path.parquet"},
            "terminal_distribution_table": {"file_name": "terminal_distribution.parquet"},
        },
        "expected_path": [
            {"interval_index": 0, "expected_price": 100.0},
            {"interval_index": 1, "expected_price": 101.0},
        ],
        "terminal_distribution": [
            {"node_index": 0, "terminal_price": 95.0},
            {"node_index": 1, "terminal_price": 100.0},
            {"node_index": 2, "terminal_price": 105.0},
        ],
        "bundle_path": None,
    }
