from __future__ import annotations

import json
import math
from copy import deepcopy
from datetime import UTC, date, datetime, timedelta
from pathlib import Path
from types import SimpleNamespace
from typing import Any, NoReturn, Optional, cast

import pyarrow as pa
import pandas
import pytest
from pyarrow import parquet

import janiform._native_api as native_api_module
import janiform.calibration.fit.estimator as fit_estimator_module
import janiform.calibration.fit._preparation.configuration as fit_preparation_configuration_module
import janiform.calibration.fit._runtime.payloads as fit_runtime_payloads_module
import janiform.calibration.fit._runtime.option_pricing.native as fit_pricing_native_module
import janiform.calibration.fit._runtime.option_pricing.support as fit_pricing_support_module
import janiform.calibration.fit._runtime.option_pricing.workflow as fit_pricing_workflow_module
import janiform.calibration.fit._runtime.seasonality as fit_seasonality_runtime_module
import janiform.calibration.fit._runtime.simulation.workflow as fit_simulation_workflow_module
import janiform.calibration.fit.state._estimation as state_estimation_module
import janiform.calibration.market_data.ingestion as market_data_ingestion_module
import janiform.model as model_module
import janiform.pricing.fitted.market_snapshot._workflow.pricing as fit_market_snapshot_pricing_workflow_module
import janiform.simulation.result as simulation_result_module
from janiform._timestamps import datetime_to_utc_nanos
from janiform._workflow_outputs import SimulationArtifactSink
from janiform._workflow_progress import WorkflowProgressMode
from janiform.calibration import CalibrationValidationError
from janiform.calibration.fit import JaniformEstimator, JaniformFitReport, JaniformFitResult
from janiform.model import JaniformModel
from janiform.pricing.fitted import (
    JaniformObservedOptionSurfacePricingResult,
    JaniformVanillaOptionGrid,
    JaniformVanillaOptionGridBatch,
    JaniformVanillaOptionGridBatchPricingResult,
    JaniformVanillaOptionGridPricingResult,
    JaniformVanillaOptionMaturitySummary,
    JaniformVanillaOptionSurface,
    JaniformVanillaOptionSurfacePricingResult,
)
from janiform.simulation import JaniformSimulationResult
from janiform.calibration.fit.state import (
    CurrentStateEstimationRequest,
    CurrentStateEstimationResult,
)
from janiform.calibration.market_data import (
    CalibrationInputFileFormat,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataOutputPreferences,
    CalibrationMarketDataPreprocessingSettings,
    CalibrationMarketDataReturnComponentsPreprocessingSettings,
    CalibrationMarketDataSeasonalityPreprocessingSettings,
    CalibrationMarketDataWorkflowScope,
    CalibrationMeanReversionConfiguration,
    CalibrationOptionQuoteInputPolicy,
    CalibrationOptionSurfaceColumnMapping,
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
)
from janiform.calibration.market_data.contracts import CalibrationExecutionMode
from janiform.calibration.staged import (
    CalibrationOptimizerSettings,
    CalibrationResumeSettings,
    CalibrationResumeSource,
    CalibrationStage,
    CalibrationStageSolverSettings,
    StagedCalibrationResponse,
    validate_staged_calibration_response,
)
from janiform.calendar import CalendarRegistry, MarketState
from janiform.errors import JaniformExecutionError
from janiform.hourly_factors import (
    HourlyFactorContract,
    HourlySeasonalityProviderExplicitSource,
    HourlySeasonalityProviderMaterializedSource,
)
from janiform.mean_reversion import (
    MeanReversionAnchorModel,
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
)
from janiform.pricing import (
    AutoActivityCompressedPricingIntervalSchedulePolicy,
    AutoExactSparsePricingIntervalSchedulePolicy,
    VanillaOptionPricingDiagnostics,
    run_vanilla_option_pricing,
)
from janiform.pricing import (
    VanillaOptionImpliedVolatilityInversionStatus,
    VanillaOptionPayoffType,
    VanillaOptionPricingRunResult,
    VanillaOptionPricingStatus,
)
from janiform.seasonality import IntradaySmoothingKernel, SessionFilter
from janiform.simulation import SeededTickEventSide
from janiform.simulation.shared import HawkesExcitationState
from tests.support.calibration import (
    build_endogenous_mean_reversion_fitted_parameters_payload,
    build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload,
    update_model_mean_reversion_payload,
)
from tests.support.fit.workflows import patch_fit_workflow_runner
from tests.support.pricing import (
    build_anchored_runtime_execution_input_model,
    build_runtime_interval_schedule,
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
    assert model.run_id == "high-level-fit-success"
    assert model.workflow_scope == CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY


def test_janiform_estimator_fit_forwards_progress_events_to_callback(
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
            ]
        ),
        encoding="utf-8",
    )

    progress_event = {
        "event_class": "progress",
        "payload": {
            "stage": {
                "stage_id": "fit_return_distribution",
                "label": "Fit return distribution",
            },
            "phase": {
                "phase_id": "estimate_student_t",
                "label": "Estimate Student t",
            },
            "measures": [
                {
                    "measure_kind": "bounded_count",
                    "completed_units": 1,
                    "total_units": 3,
                },
            ],
        },
    }
    observed_events: list[dict[str, Any]] = []

    patch_fit_workflow_runner(
        monkeypatch,
        lambda prepared_handle, **kwargs: _successful_full_model_staged_runner_payload(
            _prepared_runner_run_id(prepared_handle)
        ),
        progress_event=progress_event,
    )

    estimator = JaniformEstimator(market_profile="us_equity_cash")

    estimator.fit(
        tick_data_path=tick_data_path,
        progress_callback=observed_events.append,
    )

    assert observed_events == [progress_event]


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

    patch_fit_workflow_runner(
        monkeypatch,
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
    assert model.workflow_scope == CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE
    assert model.resolved_configuration.session.selected_preset is not None
    assert model.resolved_configuration.session.selected_preset.preset_name == "XNYS"


def test_prepare_fit_workflow_uses_column_only_parquet_schema_discovery(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.parquet"
    parquet.write_table(
        pa.table(
            {
                "time": [
                    datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                    datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
                ],
                "close": [1.0850, 1.0852],
            }
        ),
        tick_data_path,
    )

    def _unexpected_read_table(*args, **kwargs) -> NoReturn:
        del args, kwargs
        raise AssertionError("fit input inference should not materialize a parquet table")

    monkeypatch.setattr(market_data_ingestion_module.parquet, "read_table", _unexpected_read_table)

    estimator = JaniformEstimator(market_profile="eurusd_spot")
    prepared_workflow = estimator._prepare_fit_workflow(
        run_id="high-level-parquet-schema-only",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.PARQUET,
        tick_data_provenance_label="tick_data",
        option_surface_path=None,
        option_surface_format=CalibrationInputFileFormat.PARQUET,
        option_surface_provenance_label="option_surface",
    )

    if prepared_workflow.temporary_directory is not None:
        prepared_workflow.temporary_directory.cleanup()


def test_prepare_fit_workflow_infers_market_profile_from_synthetic_tick_path(
    tmp_path: Path,
) -> None:
    tick_data_path = tmp_path / "Synthetic_EURUSD_Data_1M_ticks.parquet"
    parquet.write_table(
        pa.table(
            {
                "time": [
                    datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                    datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
                ],
                "close": [1.0850, 1.0852],
            }
        ),
        tick_data_path,
    )

    estimator = JaniformEstimator()
    prepared_workflow = estimator._prepare_fit_workflow(
        run_id="high-level-inferred-eurusd-profile",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.PARQUET,
        tick_data_provenance_label="tick_data",
        option_surface_path=None,
        option_surface_format=CalibrationInputFileFormat.PARQUET,
        option_surface_provenance_label="option_surface",
    )

    if prepared_workflow.temporary_directory is not None:
        prepared_workflow.temporary_directory.cleanup()

    market_profile = prepared_workflow.resolved_configuration.market_profile
    session = prepared_workflow.resolved_configuration.session

    assert market_profile is not None
    assert market_profile.configuration.profile_name == "eurusd_spot"
    assert session.selected_preset is not None
    assert session.selected_preset.preset_name == "fx_spot_24_5"


def test_prepare_fit_workflow_passes_explicit_mean_reversion_request_to_preparation(
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

    estimator = JaniformEstimator(
        market_profile="us_equity_cash",
        mean_reversion=CalibrationMeanReversionConfiguration(
            calibration_grid_interval_hours=36.0,
        ),
    )
    prepared_workflow = estimator._prepare_fit_workflow(
        run_id="high-level-mean-reversion-request",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
        tick_data_provenance_label="tick_data",
        option_surface_path=None,
        option_surface_format=CalibrationInputFileFormat.PARQUET,
        option_surface_provenance_label="option_surface",
    )

    request = prepared_workflow.prepared.request
    if prepared_workflow.temporary_directory is not None:
        prepared_workflow.temporary_directory.cleanup()

    assert request.mean_reversion is not None
    assert request.mean_reversion.calibration_grid_interval_hours == pytest.approx(36.0)


def test_prepare_fit_workflow_passes_optimizer_settings_by_stage_to_preparation(
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
    prepared_workflow = estimator._prepare_fit_workflow(
        run_id="high-level-optimizer-settings-request",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
        tick_data_provenance_label="tick_data",
        option_surface_path=None,
        option_surface_format=CalibrationInputFileFormat.PARQUET,
        option_surface_provenance_label="option_surface",
    )

    request = prepared_workflow.prepared.request
    if prepared_workflow.temporary_directory is not None:
        prepared_workflow.temporary_directory.cleanup()

    assert request.optimizer_settings_by_stage == optimizer_settings_by_stage


def test_prepare_fit_workflow_passes_preprocessing_settings_to_preparation(
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
    prepared_workflow = estimator._prepare_fit_workflow(
        run_id="high-level-preprocessing-settings-request",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
        tick_data_provenance_label="tick_data",
        option_surface_path=None,
        option_surface_format=CalibrationInputFileFormat.PARQUET,
        option_surface_provenance_label="option_surface",
    )

    request = prepared_workflow.prepared.request
    if prepared_workflow.temporary_directory is not None:
        prepared_workflow.temporary_directory.cleanup()

    assert request.preprocessing_settings == preprocessing_settings


def test_prepare_fit_workflow_passes_option_surface_inputs_to_session_inference(
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
    option_surface_path = tmp_path / "options.parquet"
    parquet.write_table(
        pa.table(
            {
                "quote_timestamp": [datetime(2026, 2, 3, 15, 0, tzinfo=UTC)],
                "expiry_timestamp": [datetime(2026, 3, 20, 20, 0, tzinfo=UTC)],
                "strike": [100.0],
                "option_type": ["C"],
                "bid_price": [4.0],
                "ask_price": [5.0],
            }
        ),
        option_surface_path,
    )

    estimator = JaniformEstimator(
        tick_data_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
            option_quote_input_policy=CalibrationOptionQuoteInputPolicy.BID_ASK_MIDPOINT,
        ),
        option_surface_column_mapping=CalibrationOptionSurfaceColumnMapping(
            quote_timestamp_column="quote_timestamp",
            expiry_timestamp_column="expiry_timestamp",
            strike_column="strike",
            option_type_column="option_type",
            bid_column="bid_price",
            ask_column="ask_price",
        ),
        market_profile="us_equity_cash",
    )
    prepared_workflow = estimator._prepare_fit_workflow(
        run_id="high-level-option-session-inference",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
        tick_data_provenance_label="tick_data",
        option_surface_path=option_surface_path,
        option_surface_format=CalibrationInputFileFormat.PARQUET,
        option_surface_provenance_label="option_surface",
        option_surface_valuation_time_utc=datetime(2026, 2, 3, 15, 0, tzinfo=UTC),
    )

    request = prepared_workflow.prepared.request
    if prepared_workflow.temporary_directory is not None:
        prepared_workflow.temporary_directory.cleanup()

    assert request.workflow_scope == CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE
    assert request.option_surface_source is not None
    assert request.option_surface_source.path == str(option_surface_path)
    assert request.column_mapping.option_surface is not None
    assert (
        request.normalization_policy.option_quote_input_policy
        == CalibrationOptionQuoteInputPolicy.BID_ASK_MIDPOINT
    )


def test_prepare_fit_workflow_allows_inferred_option_quote_input_policy(
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
    option_surface_path = tmp_path / "options.parquet"
    parquet.write_table(
        pa.table(
            {
                "quote_timestamp_utc": [datetime(2026, 2, 3, 15, 0, tzinfo=UTC)],
                "expiry_utc": [datetime(2026, 3, 20, 20, 0, tzinfo=UTC)],
                "strike": [100.0],
                "option_type": ["C"],
                "bid": [4.0],
                "ask": [5.0],
            }
        ),
        option_surface_path,
    )

    estimator = JaniformEstimator(
        market_profile="us_equity_cash",
    )
    prepared_workflow = estimator._prepare_fit_workflow(
        run_id="high-level-inferred-option-policy-request",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
        tick_data_provenance_label="tick_data",
        option_surface_path=option_surface_path,
        option_surface_format=CalibrationInputFileFormat.PARQUET,
        option_surface_provenance_label="option_surface",
    )

    request = prepared_workflow.prepared.request
    if prepared_workflow.temporary_directory is not None:
        prepared_workflow.temporary_directory.cleanup()

    assert request.workflow_scope == CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE
    assert request.option_surface_source is not None
    assert request.normalization_policy.option_quote_input_policy is None


def test_price_option_snapshot_returns_tidy_snapshot_and_surface_df(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-observed-option-snapshot",
    )

    tick_data_path = tmp_path / "observed_ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "trade_time,trade_price,trade_side",
                "2026-02-03T14:30:00Z,100.0,buy",
                "2026-02-03T14:45:00Z,101.0,sell",
                "2026-02-03T15:00:00Z,102.0,buy",
                "2026-02-03T15:15:00Z,103.0,sell",
            ]
        ),
        encoding="utf-8",
    )

    option_surface_path = tmp_path / "observed_surface.csv"
    option_surface_path.write_text(
        "\n".join(
            [
                "quote_time,expiry_utc,strike_price,right,best_bid,best_ask",
                "2026-02-03T15:00:00Z,2026-03-20T20:00:00Z,95.0,C,8.0,8.5",
                "2026-02-03T15:00:00Z,2026-03-20T20:00:00Z,105.0,C,3.0,3.5",
                "2026-02-03T14:59:00Z,2026-04-17T20:00:00Z,95.0,C,9.0,9.5",
                "2026-02-03T14:59:00Z,2026-04-17T20:00:00Z,105.0,C,4.0,4.5",
                "2026-02-03T16:00:00Z,2026-03-20T20:00:00Z,95.0,C,8.2,8.7",
            ]
        ),
        encoding="utf-8",
    )

    captured_arguments: dict[str, Any] = {}

    def _capture_price_option_grid(
        model: JaniformModel,
        *,
        valuation_time_utc: datetime,
        batch: JaniformVanillaOptionGridBatch,
        observed_data: Optional[CurrentStateEstimationRequest] = None,
        boundary_state: Any = None,
        include_transform_diagnostics: bool = False,
        include_integration_quality_diagnostics: bool = False,
        cpu_count: Optional[int] = None,
        silent: bool = False,
        progress_mode: Any = model_module.WorkflowProgressMode.AUTO,
        progress: Any = None,
    ) -> JaniformVanillaOptionGridBatchPricingResult:
        _ = model
        _ = boundary_state
        _ = include_transform_diagnostics
        _ = include_integration_quality_diagnostics
        _ = progress
        captured_arguments["valuation_time_utc"] = valuation_time_utc
        captured_arguments["batch"] = batch
        captured_arguments["observed_data"] = observed_data
        captured_arguments["cpu_count"] = cpu_count
        captured_arguments["silent"] = silent
        captured_arguments["progress_mode"] = progress_mode

        grids = []
        for grid_index, grid in enumerate(batch.grids):
            grids.append(
                JaniformVanillaOptionGridPricingResult(
                    payoff_type=grid.payoff_type,
                    risk_free_rate=grid.risk_free_rate,
                    dividend_yield=grid.dividend_yield,
                    strikes=grid.strikes,
                    maturity_intervals=(0.1 + 0.1 * grid_index,),
                    maturity_terminal_interval_indices=(grid_index,),
                    price_grid=(tuple(1.0 + strike / 100.0 for strike in grid.strikes),),
                    implied_volatility_grid=(
                        tuple(0.2 + 0.01 * strike_index + 0.05 * grid_index for strike_index, _strike in enumerate(grid.strikes)),
                    ),
                    implied_volatility_inversion_status_grid=(
                        tuple(
                            VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED
                            for _ in grid.strikes
                        ),
                    ),
                    pricing_status_grid=(
                        tuple(VanillaOptionPricingStatus.SUCCEEDED for _ in grid.strikes),
                    ),
                    pricing_failure_stage_grid=(tuple(None for _ in grid.strikes),),
                    pricing_failure_reason_grid=(tuple(None for _ in grid.strikes),),
                    forward_price_grid=(tuple(100.0 for _ in grid.strikes),),
                    discount_factor_grid=(tuple(1.0 for _ in grid.strikes),),
                    boundary_state_norm_grid=(tuple(0.3 for _ in grid.strikes),),
                    maturity_summaries=(None,),
                )
            )

        return JaniformVanillaOptionGridBatchPricingResult(
            spot=batch.spot,
            grids=tuple(grids),
            runtime_interval_schedule=build_runtime_interval_schedule((0.1, 0.1)),
        )

    monkeypatch.setattr(
        fit_market_snapshot_pricing_workflow_module,
        "price_option_grid_for_model",
        _capture_price_option_grid,
    )

    result = model.price_option_snapshot(
        tick_data=tick_data_path,
        option_surface=option_surface_path,
        valuation_time_utc=datetime(2026, 2, 3, 15, 0, tzinfo=UTC),
        option_type="call",
        silent=True,
        progress_mode=WorkflowProgressMode.NONE,
    )

    observed_data = captured_arguments["observed_data"]
    assert observed_data is not None
    assert list(observed_data.observed_tick_data["trade_time"]) == [
        "2026-02-03T14:30:00Z",
        "2026-02-03T14:45:00Z",
        "2026-02-03T15:00:00Z",
    ]
    assert observed_data.observed_price == pytest.approx(102.0)

    batch = captured_arguments["batch"]
    assert batch.spot == pytest.approx(102.0)
    assert tuple(grid.strikes for grid in batch.grids) == (
        (95.0, 105.0),
        (95.0, 105.0),
    )
    assert captured_arguments["silent"] is True
    assert captured_arguments["progress_mode"] is WorkflowProgressMode.NONE

    frame = result.to_pandas()
    assert list(frame.columns) == [
        "quote_timestamp_utc",
        "expiry_timestamp_utc",
        "strike",
        "option_type",
        "risk_free_rate",
        "dividend_yield",
        "maturity_interval",
        "observed_bid_price",
        "observed_ask_price",
        "observed_mid_price",
        "model_price",
        "model_implied_volatility",
        "implied_volatility_inversion_status",
        "pricing_status",
        "pricing_failure_stage",
        "pricing_failure_reason",
        "forward_price",
        "log_forward_moneyness",
        "discount_factor",
        "boundary_state_norm",
    ]
    assert len(frame) == 4
    assert set(frame["observed_mid_price"]) == {3.25, 4.25, 8.25, 9.25}
    assert set(frame["quote_timestamp_utc"]) == {
        datetime(2026, 2, 3, 14, 59, tzinfo=UTC),
        datetime(2026, 2, 3, 15, 0, tzinfo=UTC),
    }
    assert sorted(set(frame["log_forward_moneyness"])) == pytest.approx(
        sorted(
            [
                math.log(95.0 / 100.0),
                math.log(105.0 / 100.0),
            ]
        )
    )
    assert result.nodes[0].log_forward_moneyness is not None

    surface_df = result.to_surface_df()
    assert tuple(surface_df.columns) == (95.0, 105.0)
    assert surface_df.shape == (2, 2)
    assert surface_df.iloc[0, 0] == pytest.approx(0.2)
    assert surface_df.iloc[1, 1] == pytest.approx(0.26)


def test_price_option_surface_returns_single_expiry_evolution_surface(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-observed-option-surface",
    )

    tick_data_path = tmp_path / "observed_ticks_surface.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "trade_time,trade_price,trade_side",
                "2026-02-03T14:30:00Z,100.0,buy",
                "2026-02-03T14:55:00Z,101.0,sell",
                "2026-02-03T15:00:00Z,102.0,buy",
            ]
        ),
        encoding="utf-8",
    )

    option_surface_path = tmp_path / "observed_surface_single_expiry.csv"
    option_surface_path.write_text(
        "\n".join(
            [
                "quote_time,expiry_utc,strike_price,right,best_bid,best_ask",
                "2026-02-03T14:55:00Z,2026-03-20T20:00:00Z,95.0,C,8.0,8.5",
                "2026-02-03T14:55:00Z,2026-03-20T20:00:00Z,105.0,C,3.0,3.5",
                "2026-02-03T15:00:00Z,2026-03-20T20:00:00Z,95.0,C,8.2,8.7",
                "2026-02-03T15:00:00Z,2026-03-20T20:00:00Z,105.0,C,3.2,3.7",
            ]
        ),
        encoding="utf-8",
    )

    observed_valuation_times: list[datetime] = []

    def _capture_price_option_grid(
        model: JaniformModel,
        *,
        valuation_time_utc: datetime,
        batch: JaniformVanillaOptionGridBatch,
        observed_data: Optional[CurrentStateEstimationRequest] = None,
        boundary_state: Any = None,
        include_transform_diagnostics: bool = False,
        include_integration_quality_diagnostics: bool = False,
        cpu_count: Optional[int] = None,
        silent: bool = False,
        progress_mode: Any = model_module.WorkflowProgressMode.AUTO,
        progress: Any = None,
    ) -> JaniformVanillaOptionGridBatchPricingResult:
        _ = model
        _ = observed_data
        _ = boundary_state
        _ = include_transform_diagnostics
        _ = include_integration_quality_diagnostics
        _ = cpu_count
        _ = silent
        _ = progress_mode
        _ = progress
        observed_valuation_times.append(valuation_time_utc)

        forward_price = 100.0 if valuation_time_utc.minute == 55 else 102.0
        volatility_level_shift = 0.0 if valuation_time_utc.minute == 55 else 0.05
        grids = tuple(
            JaniformVanillaOptionGridPricingResult(
                payoff_type=grid.payoff_type,
                risk_free_rate=grid.risk_free_rate,
                dividend_yield=grid.dividend_yield,
                strikes=grid.strikes,
                maturity_intervals=(0.1,),
                maturity_terminal_interval_indices=(0,),
                price_grid=(tuple(1.0 + strike / 100.0 for strike in grid.strikes),),
                implied_volatility_grid=(
                    tuple(
                        0.2 + 0.01 * strike_index + volatility_level_shift
                        for strike_index, _strike in enumerate(grid.strikes)
                    ),
                ),
                implied_volatility_inversion_status_grid=(
                    tuple(
                        VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED
                        for _ in grid.strikes
                    ),
                ),
                pricing_status_grid=(
                    tuple(VanillaOptionPricingStatus.SUCCEEDED for _ in grid.strikes),
                ),
                pricing_failure_stage_grid=(tuple(None for _ in grid.strikes),),
                pricing_failure_reason_grid=(tuple(None for _ in grid.strikes),),
                forward_price_grid=(tuple(forward_price for _ in grid.strikes),),
                discount_factor_grid=(tuple(1.0 for _ in grid.strikes),),
                boundary_state_norm_grid=(tuple(0.3 for _ in grid.strikes),),
                maturity_summaries=(None,),
            )
            for grid in batch.grids
        )

        return JaniformVanillaOptionGridBatchPricingResult(
            spot=batch.spot,
            grids=grids,
            runtime_interval_schedule=build_runtime_interval_schedule((0.1, 0.1)),
        )

    monkeypatch.setattr(
        fit_market_snapshot_pricing_workflow_module,
        "price_option_grid_for_model",
        _capture_price_option_grid,
    )

    result = model.price_option_surface(
        tick_data=tick_data_path,
        option_surface=option_surface_path,
        valuation_time_utc=datetime(2026, 2, 3, 15, 0, tzinfo=UTC),
        option_type="call",
        valuation_count=2,
        cpu_count=1,
    )

    assert isinstance(result, JaniformObservedOptionSurfacePricingResult)
    assert observed_valuation_times == [
        datetime(2026, 2, 3, 14, 55, tzinfo=UTC),
        datetime(2026, 2, 3, 15, 0, tzinfo=UTC),
    ]

    frame = result.to_pandas()
    assert tuple(sorted(frame["valuation_time_utc"].unique())) == (
        datetime(2026, 2, 3, 14, 55, tzinfo=UTC),
        datetime(2026, 2, 3, 15, 0, tzinfo=UTC),
    )
    assert frame["expiry_timestamp_utc"].nunique(dropna=False) == 1

    surface_df = result.to_surface_df()
    assert surface_df.shape[0] == 2
    assert surface_df.shape[1] >= 1
    assert surface_df.iloc[0, 0] != surface_df.iloc[1, 0]


def test_vanilla_option_pricing_diagnostics_exposes_log_forward_moneyness() -> None:
    diagnostics = VanillaOptionPricingDiagnostics(
        forward_price=100.0,
        discount_factor=0.99,
        maturity_interval=1.0,
        boundary_state_norm=0.0,
    )

    assert diagnostics.log_forward_moneyness(strike=95.0) == pytest.approx(math.log(95.0 / 100.0))


def test_grid_pricing_result_exposes_log_forward_moneyness_grid() -> None:
    result = JaniformVanillaOptionGridPricingResult(
        payoff_type=VanillaOptionPayoffType.CALL,
        risk_free_rate=0.01,
        dividend_yield=0.0,
        strikes=(95.0, 105.0),
        maturity_intervals=(1.0,),
        maturity_terminal_interval_indices=(0,),
        price_grid=((1.0, 2.0),),
        implied_volatility_grid=((0.2, 0.21),),
        implied_volatility_inversion_status_grid=((
            VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
            VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
        ),),
        pricing_status_grid=((
            VanillaOptionPricingStatus.SUCCEEDED,
            VanillaOptionPricingStatus.SUCCEEDED,
        ),),
        pricing_failure_stage_grid=((None, None),),
        pricing_failure_reason_grid=((None, None),),
        forward_price_grid=((100.0, 100.0),),
        discount_factor_grid=((0.99, 0.99),),
        boundary_state_norm_grid=((0.0, 0.0),),
        maturity_summaries=(None,),
    )

    assert result.log_forward_moneyness_grid == ((
        pytest.approx(math.log(95.0 / 100.0)),
        pytest.approx(math.log(105.0 / 100.0)),
    ),)


def test_surface_pricing_result_exposes_log_forward_moneyness_grid() -> None:
    result = JaniformVanillaOptionSurfacePricingResult(
        spot=100.0,
        strikes=(95.0, 105.0),
        maturity_intervals=(1.0,),
        payoff_type=VanillaOptionPayoffType.CALL,
        risk_free_rate=0.01,
        dividend_yield=0.0,
        price_grid=((1.0, 2.0),),
        implied_volatility_grid=((0.2, 0.21),),
        implied_volatility_inversion_status_grid=((
            VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
            VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
        ),),
        pricing_status_grid=((
            VanillaOptionPricingStatus.SUCCEEDED,
            VanillaOptionPricingStatus.SUCCEEDED,
        ),),
        pricing_failure_stage_grid=((None, None),),
        pricing_failure_reason_grid=((None, None),),
        forward_price_grid=((100.0, 100.0),),
        discount_factor_grid=((0.99, 0.99),),
        boundary_state_norm_grid=((0.0, 0.0),),
        maturity_summaries=(None,),
        runtime_interval_schedule=build_runtime_interval_schedule((1.0,)),
        maturity_terminal_interval_indices=(0,),
    )

    assert result.log_forward_moneyness_grid == ((
        pytest.approx(math.log(95.0 / 100.0)),
        pytest.approx(math.log(105.0 / 100.0)),
    ),)


def test_high_level_model_simulate_sends_model_payload_to_native_workflow(
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

    patch_fit_workflow_runner(
        monkeypatch,
        lambda prepared_handle, **kwargs: _successful_full_model_staged_runner_payload(
            _prepared_runner_run_id(prepared_handle)
        ),
    )

    estimator = JaniformEstimator(market_profile="us_equity_cash")
    model = estimator.fit(tick_data_path=tick_data_path)

    captured_requests = _capture_native_model_artifact_simulation_requests(monkeypatch)

    model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 14, 0, tzinfo=UTC),
        horizon_interval_count=2,
        interval="1h",
        coarse=True,
    )

    assert captured_requests[0]["simulation_kind"] == "coarse_interval"
    assert captured_requests[0]["model_payload"]["parameters"] == model.parameters
    assert captured_requests[0]["interval_duration_nanos"] == 3_600_000_000_000


def test_high_level_model_simulate_defaults_match_cli_model_simulation(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-simulate-defaults",
    )
    captured_requests = _capture_native_model_artifact_simulation_requests(monkeypatch)

    before_simulation = datetime.now(UTC)
    model.simulate(horizon_interval_count=2)
    after_simulation = datetime.now(UTC)

    start_time_utc = datetime.fromisoformat(captured_requests[0]["start_time_utc"])

    assert captured_requests[0]["seed"] == 0
    assert before_simulation <= start_time_utc <= after_simulation


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

    patch_fit_workflow_runner(
        monkeypatch,
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
    _, session_artifact = registry.resolve_session(
        preset_name="fx_spot_24_5",
    )

    calendar_registry = CalendarRegistry(
        fit_preparation_configuration_module.calendar_registry_json_from_session_configuration(
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

    fit_result = estimator.fit_with_report(
        run_id="high-level-fit-with-report",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
    )

    model = fit_result.model
    fit_report = fit_result.report

    assert isinstance(fit_result, JaniformFitResult)
    assert isinstance(model, JaniformModel)
    assert isinstance(fit_report, JaniformFitReport)
    assert fit_result.succeeded is True
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
    assert model.run_id == "high-level-fit-no-retention"
    assert model.session_resolution.exchange_code == "XNYS"
    assert model.resolved_configuration.session.selected_preset is not None
    assert model.resolved_configuration.session.selected_preset.preset_name == "XNYS"


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

    assert set(summary["fitted_parameters"]) == {"seasonality", "joint_refinement"}
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
    assert list(summary["fitted_parameter_summary"]["selected_candidate_ids"]) == [
        "hawkes",
        "return_components",
        "joint_refinement",
    ]


def test_high_level_model_load_preserves_rust_model_artifact_metadata(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-persistence",
    )
    model_path = tmp_path / "model.json"

    _write_rust_model_artifact(model, model_path)
    loaded_model = JaniformModel.load(model_path)
    persisted_payload = json.loads(model_path.read_text(encoding="utf-8"))

    assert loaded_model.parameters == model.parameters
    assert loaded_model.metadata == model.metadata
    assert loaded_model.summary() == model.summary()
    assert set(persisted_payload) == {
        "schema_version",
        "run_id",
        "workflow_scope",
        "parameters",
        "metadata",
        "resolved_configuration",
        "session_resolution",
    }
    assert set(persisted_payload["parameters"]) == {"seasonality", "joint_refinement"}
    assert (
        persisted_payload["parameters"]["joint_refinement"]["fitted_parameters"][
            "return_components_fitted_parameters"
        ]["mean_reversion_fitted_parameters"]
        == build_zero_kappa_endogenous_mean_reversion_fitted_parameters_payload()
    )
    assert set(persisted_payload["parameters"]["seasonality"]) == {
        "schema_version",
        "exchange_code",
        "intraday_minutes_per_bin",
        "intraday_factor_by_bin",
        "day_of_week_factor",
        "day_of_year_factor",
    }
    assert model_path.read_text(encoding="utf-8").startswith("{\n  ")


def test_high_level_model_save_dispatches_model_payload_to_native_artifact_writer(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-native-save",
    )
    captured_requests: list[dict[str, Any]] = []

    def _save_model_artifact(request: dict[str, Any]) -> dict[str, str]:
        captured_requests.append(request)

        return {"path": str(tmp_path / "model.jf")}

    monkeypatch.setattr(
        native_api_module,
        "native_module",
        lambda: SimpleNamespace(save_model_artifact=_save_model_artifact),
    )

    saved_path = model.save()
    explicit_saved_path = model.save(tmp_path / "explicit-model.jf")

    assert saved_path == tmp_path / "model.jf"
    assert explicit_saved_path == tmp_path / "model.jf"
    assert captured_requests[0] == {
        "model_payload": model._native_model_payload(),
        "path": None,
    }
    assert captured_requests[1] == {
        "model_payload": model._native_model_payload(),
        "path": str(tmp_path / "explicit-model.jf"),
    }


def test_high_level_model_load_recovers_cli_runtime_metadata(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-cli-model-load",
    )
    artifact_directory = tmp_path / "run" / "artifacts"
    artifact_directory.mkdir(parents=True)
    model_path = artifact_directory / "model.jf"
    report_path = artifact_directory / "report.json"

    _write_rust_model_artifact(model, model_path)
    model_payload = json.loads(model_path.read_text(encoding="utf-8"))
    expected_input_inference = deepcopy(model_payload["metadata"]["input_inference"])
    model_payload["resolved_configuration"] = None
    model_payload["metadata"]["input_inference"] = None
    model_path.write_text(json.dumps(model_payload), encoding="utf-8")
    report_path.write_text(
        json.dumps(
            {
                "preparation": {
                    "request": {
                        "run_id": expected_input_inference["run_id"],
                        "workflow_scope": expected_input_inference["workflow_scope"],
                        "tick_data_source": expected_input_inference["tick_data_source"],
                        "column_mapping": {
                            "tick_data": expected_input_inference["tick_data_column_mapping"],
                        },
                        "normalization_policy": expected_input_inference["normalization_policy"],
                    },
                },
            }
        ),
        encoding="utf-8",
    )

    loaded_model = JaniformModel.load(model_path)

    assert loaded_model.session_resolution == model.session_resolution
    assert loaded_model.resolved_configuration.session.inferred_from_data is True
    assert loaded_model.resolved_configuration.session.inference_resolution == model.session_resolution
    assert loaded_model.resolved_configuration.known_news.selected_preset is not None
    assert loaded_model.resolved_configuration.known_news.selected_preset.preset_name == "none"
    assert loaded_model.metadata["input_inference"] == expected_input_inference


def test_high_level_fit_records_diagnostic_artifact_families_from_staged_response_dicts(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "diagnostic-artifact-fit.csv"
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

    def successful_payload_with_diagnostics(
        run_id: str,
    ) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
        _, raw_response = _successful_full_model_staged_runner_payload(run_id)
        raw_response["diagnostic_artifact_references"] = [
            {
                "artifact_family": "candidate_frontier",
                "relative_path": "diagnostics/candidate_frontier.json",
            },
            {
                "artifact_family": "coarse_tick_consistency",
                "relative_path": "diagnostics/coarse_tick_consistency.json",
            },
        ]

        return validate_staged_calibration_response(raw_response), raw_response

    patch_fit_workflow_runner(
        monkeypatch,
        lambda prepared_handle, **kwargs: successful_payload_with_diagnostics(
            _prepared_runner_run_id(prepared_handle)
        ),
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

    fit_result = estimator.fit_with_report(
        run_id="diagnostic-artifact-fit",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
    )

    assert fit_result.model is not None
    assert fit_result.report.staged_response.diagnostic_artifact_references == (
        {
            "artifact_family": "candidate_frontier",
            "relative_path": "diagnostics/candidate_frontier.json",
        },
        {
            "artifact_family": "coarse_tick_consistency",
            "relative_path": "diagnostics/coarse_tick_consistency.json",
        },
    )


def test_high_level_model_load_preserves_endogenous_mean_reversion_block(
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

    update_model_mean_reversion_payload(model, mean_reversion_payload)

    persisted_model_path = tmp_path / "model-mean-reversion.json"

    _write_rust_model_artifact(model, persisted_model_path)
    loaded_model = JaniformModel.load(persisted_model_path)

    assert (
        loaded_model.parameters["joint_refinement"]["fitted_parameters"][
            "return_components_fitted_parameters"
        ]["mean_reversion_fitted_parameters"]
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

    _write_rust_model_artifact(model, persisted_model_path)
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
        fit_seasonality_runtime_module,
        "native_module",
        lambda: SimpleNamespace(
            materialize_runtime_structural_seasonality=(
                _materialize_runtime_structural_seasonality
            )
        ),
    )

    seasonality_source = fit_seasonality_runtime_module.build_runtime_hourly_seasonality_source_from_model(
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

    _write_rust_model_artifact(model, persisted_model_path)
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
        fit_seasonality_runtime_module,
        "native_module",
        lambda: SimpleNamespace(
            materialize_runtime_structural_seasonality=(
                _materialize_runtime_structural_seasonality
            )
        ),
    )

    seasonality_source = fit_seasonality_runtime_module.build_runtime_hourly_seasonality_source_from_model(
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

    _write_rust_model_artifact(model, persisted_model_path)
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
        fit_seasonality_runtime_module.build_runtime_hourly_seasonality_source_from_model(
            loaded_model,
            calendar_registry=registry,
            exchange_code=loaded_model.session_resolution.exchange_code,
            range_start_utc=datetime(2026, 2, 2, 14, tzinfo=UTC),
            range_end_utc=datetime(2026, 2, 2, 16, tzinfo=UTC),
            interval="1h",
        )


def test_high_level_model_simulate_sends_coarse_interval_request_to_native_workflow(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-closed-seasonality-gating",
    )

    captured_requests = _capture_native_model_artifact_simulation_requests(monkeypatch)

    model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 14, 0, tzinfo=UTC),
        horizon_interval_count=4,
        interval="15min",
        coarse=True,
    )

    assert captured_requests[0]["simulation_kind"] == "coarse_interval"
    assert captured_requests[0]["horizon_interval_count"] == 4
    assert captured_requests[0]["interval_duration_nanos"] == 900_000_000_000


def test_high_level_model_simulate_dispatches_by_explicit_coarse_flag(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-simulate",
    )

    captured_requests: list[tuple[str, int]] = []
    coarse_result = object()
    tick_result = object()

    class _FakeNativeModule:
        def run_simulation_from_model_artifact(
            self,
            request_payload: dict[str, Any],
            progress_callback: Any,
            progress_mode: str = "none",
        ) -> dict[str, Any]:
            del progress_callback, progress_mode
            captured_requests.append(
                (
                    str(request_payload["simulation_kind"]),
                    int(request_payload["interval_duration_nanos"]),
                )
            )

            return {}

    monkeypatch.setattr(
        fit_simulation_workflow_module,
        "native_module",
        lambda: _FakeNativeModule(),
    )
    monkeypatch.setattr(
        fit_simulation_workflow_module,
        "_adapt_native_simulation_output",
        lambda raw_output, **kwargs: (
            tick_result
            if kwargs["simulation_kind"].value == "seeded_tick"
            else coarse_result
        ),
    )
    monkeypatch.setattr(
        fit_simulation_workflow_module,
        "build_simulation_result",
        lambda model_value, **kwargs: JaniformSimulationResult(
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
        coarse=True,
    )
    observed_seeded_with_custom_interval = model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=2,
        interval="15min",
    )

    assert isinstance(observed_seeded, JaniformSimulationResult)
    assert isinstance(observed_coarse, JaniformSimulationResult)
    assert isinstance(observed_seeded_with_custom_interval, JaniformSimulationResult)
    assert observed_seeded.raw_result is tick_result
    assert observed_coarse.raw_result is coarse_result
    assert observed_seeded_with_custom_interval.raw_result is tick_result
    assert observed_seeded.simulation_kind == "seeded_tick"
    assert observed_coarse.simulation_kind == "coarse_interval"
    assert observed_seeded_with_custom_interval.simulation_kind == "seeded_tick"
    assert captured_requests == [
        ("seeded_tick", 3_600_000_000_000),
        ("coarse_interval", 900_000_000_000),
        ("seeded_tick", 900_000_000_000),
    ]


def test_high_level_model_simulate_forwards_progress_events_to_callback(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-progress",
    )

    simulated_result = object()
    observed_callback_events: list[tuple[str, int, int]] = []

    class _FakeNativeModule:
        def run_simulation_from_model_artifact(
            self,
            request_payload: dict[str, Any],
            progress_callback: Any,
            progress_mode: str = "none",
        ) -> dict[str, Any]:
            del request_payload, progress_mode
            assert progress_callback is not None

            progress_callback(
                {
                    "phase": "seeded_tick_intervals",
                    "completed_units": 2,
                    "total_units": 5,
                }
            )

            return {}

    monkeypatch.setattr(
        fit_simulation_workflow_module,
        "native_module",
        lambda: _FakeNativeModule(),
    )
    monkeypatch.setattr(
        fit_simulation_workflow_module,
        "_adapt_native_simulation_output",
        lambda raw_output, **kwargs: simulated_result,
    )
    monkeypatch.setattr(
        fit_simulation_workflow_module,
        "build_simulation_result",
        lambda model_value, **kwargs: JaniformSimulationResult(
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
            (
                cast(str, event["phase"]),
                cast(int, event["completed_units"]),
                cast(int, event["total_units"]),
            )
        ),
    )

    assert result.raw_result is simulated_result
    assert observed_callback_events == [("seeded_tick_intervals", 2, 5)]


def test_run_model_vanilla_option_surface_pricing_uses_terminal_progress_native_call(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    observed_native_inputs: list[dict[str, Any]] = []
    observed_progress_modes: list[str] = []
    expected_result = JaniformVanillaOptionSurfacePricingResult(
        spot=100.0,
        strikes=(95.0,),
        maturity_intervals=(1.0,),
        payoff_type=VanillaOptionPayoffType.CALL,
        risk_free_rate=0.01,
        dividend_yield=0.0,
        price_grid=((5.0,),),
        implied_volatility_grid=((0.2,),),
        implied_volatility_inversion_status_grid=(
            (VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,),
        ),
        pricing_status_grid=((VanillaOptionPricingStatus.SUCCEEDED,),),
        pricing_failure_stage_grid=((None,),),
        pricing_failure_reason_grid=((None,),),
        forward_price_grid=((100.5,),),
        discount_factor_grid=((0.99,),),
        boundary_state_norm_grid=((0.0,),),
        maturity_summaries=(None,),
        runtime_interval_schedule=build_runtime_interval_schedule((1.0,)),
        maturity_terminal_interval_indices=(0,),
    )

    class _FakeNativeModule:
        def run_vanilla_option_surface_pricing_with_terminal_progress(
            self,
            execution_input: dict[str, Any],
            progress_mode: str,
        ) -> dict[str, Any]:
            observed_native_inputs.append(execution_input)
            observed_progress_modes.append(progress_mode)

            payload = expected_result.to_payload()
            return {
                "response": payload,
                "maturity_summaries": payload["maturity_summaries"],
                "runtime_interval_schedule": payload["runtime_interval_schedule"],
                "maturity_terminal_interval_indices": payload[
                    "maturity_terminal_interval_indices"
                ],
            }

    monkeypatch.setattr(
        fit_pricing_native_module,
        "native_module",
        lambda: _FakeNativeModule(),
    )
    monkeypatch.setattr(
        fit_pricing_native_module,
        "native_exception_matches",
        lambda native, error, name: False,
    )

    result = fit_pricing_native_module.run_model_vanilla_option_surface_pricing(
        native_execution_input={"prepared": "surface"},
        progress_mode="human",
    )

    assert observed_native_inputs == [{"prepared": "surface"}]
    assert observed_progress_modes == ["human"]
    assert result == expected_result


def test_price_option_grid_for_model_does_not_render_python_pricing_progress(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-grid-batch-pre-native-progress",
    )
    observed_result = object()
    captured_progress_modes: list[str] = []

    def capture_pricing_call(
        native_execution_input: dict[str, Any],
        *,
        progress_mode: str,
    ) -> object:
        del native_execution_input
        captured_progress_modes.append(progress_mode)

        return observed_result

    monkeypatch.setattr(
        fit_pricing_workflow_module,
        "run_model_vanilla_option_grid_batch_pricing",
        capture_pricing_call,
    )

    batch = JaniformVanillaOptionGridBatch(
        spot=100.0,
        grids=(
            JaniformVanillaOptionGrid(
                payoff_type=VanillaOptionPayoffType.CALL,
                risk_free_rate=0.01,
                dividend_yield=0.0,
                strikes=(95.0,),
                tenors=("1h",),
            ),
        ),
    )

    result = fit_pricing_workflow_module.price_option_grid_for_model(
        model,
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        batch=batch,
    )

    assert result is observed_result
    assert captured_progress_modes == ["human"]


def test_high_level_model_simulate_sends_continuation_data_to_native_workflow(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-simulate-estimated-current-state",
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
    captured_requests = _capture_native_model_artifact_simulation_requests(monkeypatch)

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
        coarse=True,
        continuation_data=continuation_data,
    )

    assert [request["simulation_kind"] for request in captured_requests] == [
        "seeded_tick",
        "coarse_interval",
    ]
    for request in captured_requests:
        assert request["continuation_data"] == {
            "observation_timestamp_utc_nanos": (
                datetime_to_utc_nanos(datetime(2026, 2, 2, 14, 30, tzinfo=UTC)),
            ),
            "observation_trade_prices": (100.0,),
            "observation_trade_sides": ("buy",),
            "observed_price": None,
        }
        assert request["initial_excitation_state"] is None
        assert request["initial_mean_reversion_boundary_state"] is None


def test_high_level_model_simulate_sends_hawkes_override_to_native_workflow(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-simulate-hawkes-override-with-continuation",
    )
    explicit_initial_excitation_state = HawkesExcitationState(
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
    captured_requests = _capture_native_model_artifact_simulation_requests(monkeypatch)

    model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=1,
        continuation_data=continuation_data,
        initial_excitation_state=explicit_initial_excitation_state,
    )

    assert captured_requests[0]["continuation_data"] is not None
    assert captured_requests[0]["initial_excitation_state"] == explicit_initial_excitation_state.to_payload()
    assert captured_requests[0]["initial_mean_reversion_boundary_state"] is None


def test_high_level_model_simulate_sends_mean_reversion_boundary_override_to_native_workflow(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-simulate-boundary-override",
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
    captured_requests = _capture_native_model_artifact_simulation_requests(monkeypatch)

    model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=1,
        continuation_data=continuation_data,
        initial_mean_reversion_boundary_state=explicit_boundary_state,
    )

    assert captured_requests[0]["continuation_data"] is not None
    assert captured_requests[0]["initial_excitation_state"] is None
    assert (
        captured_requests[0]["initial_mean_reversion_boundary_state"]
        == explicit_boundary_state.to_payload()
    )


def test_high_level_model_simulate_leaves_state_initialization_to_native_workflow_by_default(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-simulate-stationary-default",
    )
    captured_requests = _capture_native_model_artifact_simulation_requests(monkeypatch)

    model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=1,
        interval="1h",
    )

    assert captured_requests[0]["simulation_kind"] == "seeded_tick"
    assert captured_requests[0]["continuation_data"] is None
    assert captured_requests[0]["initial_excitation_state"] is None
    assert captured_requests[0]["initial_mean_reversion_boundary_state"] is None


def test_high_level_model_coarse_simulation_selects_native_coarse_workflow(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-coarse-only",
    )
    captured_requests = _capture_native_model_artifact_simulation_requests(monkeypatch)

    observed = model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=2,
        interval="15min",
        coarse=True,
    )

    assert observed.simulation_kind == "coarse_interval"
    assert captured_requests[0]["simulation_kind"] == "coarse_interval"


def test_high_level_model_artifact_simulation_sends_mean_reversion_payload_to_native_workflow(
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

    update_model_mean_reversion_payload(model, mean_reversion_payload)

    captured_requests = _capture_native_model_artifact_simulation_requests(monkeypatch)

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
        coarse=True,
    )

    assert [request["simulation_kind"] for request in captured_requests] == [
        "seeded_tick",
        "coarse_interval",
    ]
    for request in captured_requests:
        assert (
            _mean_reversion_fitted_parameters_payload_from_model_payload(request["model_payload"])
            == mean_reversion_payload
        )


def test_high_level_model_sends_endogenous_mean_reversion_payload_to_native_workflow(
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

    captured_requests = _capture_native_model_artifact_simulation_requests(monkeypatch)

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
        coarse=True,
    )

    assert [request["simulation_kind"] for request in captured_requests] == [
        "seeded_tick",
        "coarse_interval",
    ]
    for request in captured_requests:
        assert (
            _mean_reversion_fitted_parameters_payload_from_model_payload(request["model_payload"])
            == mean_reversion_payload
        )


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
        sink: Optional[SimulationArtifactSink] = None,
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
        simulation_result_module,
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

    result = fit_simulation_workflow_module.build_simulation_result(
        model,
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
    _, session_artifact = registry.resolve_session(
        preset_name="fx_spot_24_5",
    )
    calendar_registry = CalendarRegistry(
        fit_preparation_configuration_module.calendar_registry_json_from_session_configuration(
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
        sink: Optional[SimulationArtifactSink] = None,
    ) -> None:
        del simulation_result, output_path, preferences

        if sink is not None:
            sink.close()

    monkeypatch.setattr(
        simulation_result_module,
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
        sink: Optional[SimulationArtifactSink] = None,
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
        simulation_result_module,
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

    captured_requests, pricing_result = _capture_native_single_option_pricing_requests(monkeypatch)

    observed_result = model.price_option(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        spot=100.0,
        strike=105.0,
        expiry_timestamp_utc=datetime(2026, 2, 2, 19, tzinfo=UTC),
    )

    assert observed_result == pricing_result
    assert captured_requests[0]["model_payload"]["run_id"] == "high-level-model-pricing"
    assert captured_requests[0]["valuation_time_utc"] == "2026-02-02T15:00:00+00:00"
    assert captured_requests[0]["spot"] == 100.0
    assert captured_requests[0]["strike"] == 105.0
    assert captured_requests[0]["expiry_timestamp_utc"] == "2026-02-02T19:00:00+00:00"
    assert captured_requests[0]["progress_mode"] == "human"


def test_high_level_model_price_option_forwards_terminal_progress_suppression_to_native(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-pricing-native-progress-suppression",
    )

    captured_requests, _pricing_result = _capture_native_single_option_pricing_requests(monkeypatch)

    model.price_option(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        spot=100.0,
        strike=105.0,
        tenor="4h",
        silent=True,
    )
    model.price_option(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        spot=100.0,
        strike=105.0,
        tenor="4h",
        progress_mode=WorkflowProgressMode.NONE,
    )
    model.price_option(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        spot=100.0,
        strike=105.0,
        tenor="4h",
        progress_mode=WorkflowProgressMode.JSONL,
    )

    assert [request["progress_mode"] for request in captured_requests] == [
        "none",
        "none",
        "none",
    ]


def test_high_level_model_price_option_delegates_default_boundary_state_to_native_workflow(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-pricing-derived-boundary",
    )
    update_model_mean_reversion_payload(
        model,
        build_endogenous_mean_reversion_fitted_parameters_payload(kappa=0.35),
    )
    captured_requests, pricing_result = _capture_native_single_option_pricing_requests(monkeypatch)

    observed_result = model.price_option(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        spot=100.0,
        strike=105.0,
        tenor="4h",
    )

    assert observed_result == pricing_result
    assert captured_requests[0]["spot"] == 100.0
    assert captured_requests[0]["continuation_data"] is None
    assert captured_requests[0]["initial_excitation_state"] is None


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
        hawkes_excitation_state=HawkesExcitationState(
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
        fit_pricing_support_module.price_observed_log_price_from_spot(model, spot=100.0)
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
    observed_data = CurrentStateEstimationRequest(
        observed_tick_data=pandas.DataFrame(
            {
                "timestamp_utc": [datetime(2026, 2, 2, 14, 30, tzinfo=UTC)],
                "trade_price": [100.0],
                "trade_sign": ["buy"],
            }
        ),
    )
    captured_requests, pricing_result = _capture_native_single_option_pricing_requests(monkeypatch)

    observed_result = model.price_option(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        spot=100.0,
        strike=105.0,
        expiry_date=date(2026, 2, 3),
        observed_data=observed_data,
    )

    assert observed_result == pricing_result
    assert captured_requests[0]["continuation_data"] == {
        "observation_timestamp_utc_nanos": (
            datetime_to_utc_nanos(datetime(2026, 2, 2, 14, 30, tzinfo=UTC)),
        ),
        "observation_trade_prices": (100.0,),
        "observation_trade_sides": ("buy",),
        "observed_price": 100.0,
    }


def test_high_level_model_price_option_uses_stationary_hawkes_boundary_state_by_default(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-pricing-stationary-hawkes-boundary",
    )
    captured_requests, pricing_result = _capture_native_single_option_pricing_requests(monkeypatch)

    observed_result = model.price_option(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        spot=100.0,
        strike=105.0,
        tenor="4h",
    )

    assert observed_result == pricing_result
    assert captured_requests[0]["continuation_data"] is None
    assert captured_requests[0]["initial_excitation_state"] is None


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
    model_payload = model._model_payload
    assert isinstance(model_payload, dict)
    payload_metadata = model_payload["metadata"]
    assert isinstance(payload_metadata, dict)
    cast(dict[str, Any], payload_metadata["simulation_output_preferences"]).pop(
        "reference_price",
        None,
    )

    with pytest.raises(CalibrationValidationError) as captured_error:
        model.price_option(
            valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
            spot=100.0,
            strike=105.0,
            expiry_timestamp_utc=datetime(2026, 2, 2, 19, tzinfo=UTC),
        )

    assert (
        captured_error.value.failure.field_path
        == "metadata.simulation_output_preferences.reference_price"
    )
    assert (
        captured_error.value.failure.message
        == "must be present so runtime state can be derived from spot"
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
        tenor="1h",
        boundary_state=HawkesExcitationState(
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
    assert pricing_result.run_diagnostics.spectral is not None


def test_high_level_model_price_option_resolves_expiry_timestamp_date_and_tenor_inputs(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-pricing-maturity-resolution",
    )
    captured_requests, _pricing_result = _capture_native_single_option_pricing_requests(monkeypatch)

    model.price_option(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        spot=100.0,
        strike=105.0,
        expiry_timestamp_utc=datetime(2026, 2, 2, 19, tzinfo=UTC),
    )
    model.price_option(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        spot=100.0,
        strike=105.0,
        expiry_date=date(2026, 2, 3),
    )
    model.price_option(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        spot=100.0,
        strike=105.0,
        tenor="2d",
    )

    assert [request["expiry_timestamp_utc"] for request in captured_requests] == [
        "2026-02-02T19:00:00+00:00",
        None,
        None,
    ]
    assert [request["expiry_date"] for request in captured_requests] == [
        None,
        "2026-02-03",
        None,
    ]
    assert [request["tenor"] for request in captured_requests] == [None, None, "2d"]


def test_high_level_grid_batch_pricing_resolves_mixed_maturity_axes_to_native_request(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-grid-batch-maturity-resolution",
    )
    batch = JaniformVanillaOptionGridBatch(
        spot=100.0,
        grids=(
            JaniformVanillaOptionGrid(
                payoff_type=VanillaOptionPayoffType.CALL,
                risk_free_rate=0.01,
                dividend_yield=0.0,
                strikes=(95.0, 105.0),
                expiry_timestamps_utc=(datetime(2026, 2, 2, 16, tzinfo=UTC), None),
                expiry_dates=(None, None),
                tenors=(None, "2h"),
            ),
        ),
    )

    native_execution_input = fit_pricing_workflow_module.prepare_model_grid_batch_pricing_native_input(
        model,
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        batch=batch,
        observed_data=None,
        boundary_state=None,
        include_transform_diagnostics=False,
        include_integration_quality_diagnostics=False,
        pricing_interval_schedule_policy=None,
        cpu_count=None,
    )

    assert native_execution_input["request"]["grids"][0]["maturity_intervals"] == [1.0, 2.0]


def test_high_level_surface_pricing_resolves_mixed_maturity_axes_to_native_request(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-surface-maturity-resolution",
    )

    surface = JaniformVanillaOptionSurface(
        spot=100.0,
        strikes=(95.0, 105.0),
        expiry_timestamps_utc=(datetime(2026, 2, 2, 16, tzinfo=UTC), None),
        expiry_dates=(None, date(2026, 2, 3)),
        tenors=(None, None),
        payoff_type=VanillaOptionPayoffType.CALL,
        risk_free_rate=0.01,
        dividend_yield=0.0,
    )

    native_execution_input = fit_pricing_workflow_module.prepare_model_surface_pricing_native_input(
        model,
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        surface=surface,
        observed_data=None,
        boundary_state=None,
        include_transform_diagnostics=False,
        include_integration_quality_diagnostics=False,
        pricing_interval_schedule_policy=None,
        cpu_count=None,
    )

    assert native_execution_input["request"]["market_inputs"]["maturity_intervals"] == [1.0, 24.0]


def test_high_level_grid_batch_pricing_defaults_to_activity_compressed_runtime_schedule(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-grid-batch-activity-compressed-default",
    )
    batch = JaniformVanillaOptionGridBatch(
        spot=100.0,
        grids=(
            JaniformVanillaOptionGrid(
                payoff_type=VanillaOptionPayoffType.CALL,
                risk_free_rate=0.01,
                dividend_yield=0.0,
                strikes=(95.0,),
                tenors=("16h",),
            ),
        ),
    )

    native_execution_input = fit_pricing_workflow_module.prepare_model_grid_batch_pricing_native_input(
        model,
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        batch=batch,
        observed_data=None,
        boundary_state=None,
        include_transform_diagnostics=False,
        include_integration_quality_diagnostics=False,
        pricing_interval_schedule_policy=None,
        cpu_count=None,
    )

    assert (
        native_execution_input["runtime_interval_schedule"]["metadata"]["policy_id"]
        == "auto_activity_compressed.v1"
    )


def test_high_level_model_price_option_grid_dispatches_through_grid_batch_pricing_runner(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-batch-pricing",
    )

    batch = JaniformVanillaOptionGridBatch(
        spot=100.0,
        grids=(
            JaniformVanillaOptionGrid(
                payoff_type=VanillaOptionPayoffType.CALL,
                risk_free_rate=0.01,
                dividend_yield=0.0,
                strikes=(95.0,),
                tenors=("1h",),
            ),
            JaniformVanillaOptionGrid(
                payoff_type=VanillaOptionPayoffType.PUT,
                risk_free_rate=0.01,
                dividend_yield=0.0,
                strikes=(105.0,),
                tenors=("2h",),
            ),
        ),
    )

    prepared_native_execution_input = {"prepared": "batch"}

    observed_result = JaniformVanillaOptionGridBatchPricingResult(
        spot=100.0,
        grids=(
            JaniformVanillaOptionGridPricingResult(
                payoff_type=VanillaOptionPayoffType.CALL,
                risk_free_rate=0.01,
                dividend_yield=0.0,
                strikes=(95.0,),
                maturity_intervals=(1.0,),
                maturity_terminal_interval_indices=(0,),
                price_grid=((10.0,),),
                implied_volatility_grid=((0.2,),),
                implied_volatility_inversion_status_grid=((
                    VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
                ),),
                pricing_status_grid=((VanillaOptionPricingStatus.SUCCEEDED,),),
                pricing_failure_stage_grid=((None,),),
                pricing_failure_reason_grid=((None,),),
                forward_price_grid=((100.5,),),
                discount_factor_grid=((0.99,),),
                boundary_state_norm_grid=((0.0,),),
                maturity_summaries=(
                    JaniformVanillaOptionMaturitySummary(
                        maturity_interval=1.0,
                        terminal_anchor_value=0.0,
                        deviation_domain_lower_bound=-1.0,
                        deviation_domain_upper_bound=1.0,
                        collocation_node_count=3,
                        collocation_nodes=(-1.0, 0.0, 1.0),
                        interval_summaries=(),
                        representative_transform=None,
                        integration_quality=None,
                    ),
                ),
            ),
            JaniformVanillaOptionGridPricingResult(
                payoff_type=VanillaOptionPayoffType.PUT,
                risk_free_rate=0.01,
                dividend_yield=0.0,
                strikes=(105.0,),
                maturity_intervals=(2.0,),
                maturity_terminal_interval_indices=(1,),
                price_grid=((11.0,),),
                implied_volatility_grid=((0.25,),),
                implied_volatility_inversion_status_grid=(
                    (
                        VanillaOptionImpliedVolatilityInversionStatus.SUCCEEDED,
                    ),
                ),
                pricing_status_grid=((VanillaOptionPricingStatus.SUCCEEDED,),),
                pricing_failure_stage_grid=((None,),),
                pricing_failure_reason_grid=((None,),),
                forward_price_grid=((101.0,),),
                discount_factor_grid=((0.98,),),
                boundary_state_norm_grid=((0.0,),),
                maturity_summaries=(
                    JaniformVanillaOptionMaturitySummary(
                        maturity_interval=2.0,
                        terminal_anchor_value=0.0,
                        deviation_domain_lower_bound=-1.0,
                        deviation_domain_upper_bound=1.0,
                        collocation_node_count=3,
                        collocation_nodes=(-1.0, 0.0, 1.0),
                        interval_summaries=(),
                        representative_transform=None,
                        integration_quality=None,
                    ),
                ),
            ),
        ),
        runtime_interval_schedule=build_runtime_interval_schedule((1.0, 1.0)),
    )

    monkeypatch.setattr(
        fit_pricing_workflow_module,
        "prepare_model_grid_batch_pricing_native_input",
        lambda model_value, *args, **kwargs: prepared_native_execution_input,
    )
    monkeypatch.setattr(
        fit_pricing_workflow_module,
        "run_model_vanilla_option_grid_batch_pricing",
        lambda native_execution_input, *, progress_mode: (
            observed_result
            if native_execution_input is prepared_native_execution_input
            and progress_mode == "human"
            else None
        ),
    )

    result = model.price_option_grid(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        batch=batch,
    )

    assert result is observed_result


def test_high_level_model_price_option_grid_surface_dispatches_through_surface_pricing_runner(
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
        tenors=("1h", "2h"),
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
        pricing_status_grid=(
            (
                VanillaOptionPricingStatus.SUCCEEDED,
                VanillaOptionPricingStatus.SUCCEEDED,
            ),
            (
                VanillaOptionPricingStatus.SUCCEEDED,
                VanillaOptionPricingStatus.SUCCEEDED,
            ),
        ),
        pricing_failure_stage_grid=((None, None), (None, None)),
        pricing_failure_reason_grid=((None, None), (None, None)),
        forward_price_grid=((100.5, 100.5), (101.0, 101.0)),
        discount_factor_grid=((0.99, 0.99), (0.98, 0.98)),
        boundary_state_norm_grid=((0.0, 0.0), (0.0, 0.0)),
        maturity_summaries=(
            JaniformVanillaOptionMaturitySummary(
                maturity_interval=1.0,
                terminal_anchor_value=0.0,
                deviation_domain_lower_bound=-1.0,
                deviation_domain_upper_bound=1.0,
                collocation_node_count=3,
                collocation_nodes=(-1.0, 0.0, 1.0),
                interval_summaries=(),
                representative_transform=None,
                integration_quality=None,
            ),
            JaniformVanillaOptionMaturitySummary(
                maturity_interval=2.0,
                terminal_anchor_value=0.0,
                deviation_domain_lower_bound=-1.0,
                deviation_domain_upper_bound=1.0,
                collocation_node_count=3,
                collocation_nodes=(-1.0, 0.0, 1.0),
                interval_summaries=(),
                representative_transform=None,
                integration_quality=None,
            ),
        ),
        runtime_interval_schedule=build_runtime_interval_schedule((1.0, 1.0)),
        maturity_terminal_interval_indices=(0, 1),
    )

    monkeypatch.setattr(
        fit_pricing_workflow_module,
        "prepare_model_surface_pricing_native_input",
        lambda model_value, *args, **kwargs: prepared_native_execution_input,
    )
    monkeypatch.setattr(
        fit_pricing_workflow_module,
        "run_model_vanilla_option_surface_pricing",
        lambda native_execution_input, *, progress_mode: (
            observed_result
            if native_execution_input is prepared_native_execution_input
            and progress_mode == "human"
            else None
        ),
    )

    result = model.price_option_grid_surface(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        surface=surface,
    )

    assert result is observed_result


def test_high_level_model_reserved_cross_instrument_entrypoints_raise_not_implemented(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-reserved-cross-instrument-entrypoints",
    )

    with pytest.raises(NotImplementedError, match="price_future"):
        model.price_future(contract="reserved")

    with pytest.raises(NotImplementedError, match="price_swap"):
        model.price_swap(contract="reserved")

    with pytest.raises(NotImplementedError, match="price_instruments"):
        model.price_instruments(contracts=("reserved",))


def test_high_level_model_price_option_grid_matches_single_option_engine_on_shared_groups(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-batch-pricing-parity",
    )

    boundary_state = HawkesExcitationState(
        y_plus_plus=0.0,
        y_plus_minus=0.0,
        y_minus_plus=0.0,
        y_minus_minus=0.0,
    )

    batch = JaniformVanillaOptionGridBatch(
        spot=100.0,
        grids=(
            JaniformVanillaOptionGrid(
                payoff_type=VanillaOptionPayoffType.CALL,
                risk_free_rate=0.01,
                dividend_yield=0.0,
                strikes=(95.0, 105.0),
                tenors=("1h",),
            ),
            JaniformVanillaOptionGrid(
                payoff_type=VanillaOptionPayoffType.PUT,
                risk_free_rate=0.01,
                dividend_yield=0.0,
                strikes=(100.0,),
                tenors=("1h",),
            ),
        ),
    )

    batch_result = model.price_option_grid(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        batch=batch,
        boundary_state=boundary_state,
        include_transform_diagnostics=True,
        include_integration_quality_diagnostics=True,
    )

    assert isinstance(batch_result, JaniformVanillaOptionGridBatchPricingResult)

    flattened_specs = [
        (0, 0, 95.0, VanillaOptionPayoffType.CALL),
        (0, 1, 105.0, VanillaOptionPayoffType.CALL),
        (1, 0, 100.0, VanillaOptionPayoffType.PUT),
    ]

    representative_single_results: dict[int, VanillaOptionPricingRunResult] = {}

    for group_index, strike_index, strike, payoff_type in flattened_specs:
        single_result = model.price_option(
            valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
            spot=batch.spot,
            strike=strike,
            tenor="1h",
            payoff_type=payoff_type,
            risk_free_rate=0.01,
            dividend_yield=0.0,
            boundary_state=boundary_state,
            include_transform_diagnostics=True,
            include_integration_quality_diagnostics=True,
            include_calibration_hooks=True,
        )

        observed_group = batch_result.grids[group_index]
        representative_single_results.setdefault(group_index, single_result)

        assert observed_group.price_grid[0][strike_index] == pytest.approx(single_result.response.price)
        assert observed_group.implied_volatility_grid[0][strike_index] == pytest.approx(
            cast(float, single_result.response.implied_volatility)
        )
        assert (
            observed_group.implied_volatility_inversion_status_grid[0][strike_index]
            is single_result.response.implied_volatility_inversion_status
        )
        assert observed_group.forward_price_grid[0][strike_index] == pytest.approx(
            single_result.response.diagnostics.forward_price
        )
        assert observed_group.discount_factor_grid[0][strike_index] == pytest.approx(
            single_result.response.diagnostics.discount_factor
        )
        assert observed_group.boundary_state_norm_grid[0][strike_index] == pytest.approx(
            single_result.response.diagnostics.boundary_state_norm
        )
        assert observed_group.maturity_summaries[0].representative_transform is not None
        assert single_result.run_diagnostics is not None
        assert observed_group.maturity_summaries[0].representative_transform == single_result.run_diagnostics.transform

    for group_index, representative_single_result in representative_single_results.items():
        observed_summary = batch_result.grids[group_index].maturity_summaries[0]

        assert representative_single_result.run_diagnostics is not None
        assert observed_summary.integration_quality == representative_single_result.run_diagnostics.integration_quality


def test_high_level_model_price_option_grid_surface_matches_single_option_engine_on_shared_grid(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="high-level-model-surface-pricing-parity",
    )

    boundary_state = HawkesExcitationState(
        y_plus_plus=0.0,
        y_plus_minus=0.0,
        y_minus_plus=0.0,
        y_minus_minus=0.0,
    )

    surface = JaniformVanillaOptionSurface(
        spot=100.0,
        strikes=(95.0, 105.0),
        tenors=("1h",),
        payoff_type=VanillaOptionPayoffType.CALL,
        risk_free_rate=0.01,
        dividend_yield=0.0,
    )

    surface_result = model.price_option_grid_surface(
        valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        surface=surface,
        boundary_state=boundary_state,
        include_transform_diagnostics=True,
        include_integration_quality_diagnostics=True,
    )

    assert isinstance(surface_result, JaniformVanillaOptionSurfacePricingResult)
    assert surface_result.shape == (1, 2)

    representative_single_results: dict[int, VanillaOptionPricingRunResult] = {}

    for maturity_index, maturity_interval in enumerate(surface_result.maturity_intervals):
        for strike_index, strike in enumerate(surface.strikes):
            single_result = model.price_option(
                valuation_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
                spot=surface.spot,
                strike=strike,
                tenor=f"{int(maturity_interval)}h",
                payoff_type=surface.payoff_type,
                risk_free_rate=surface.risk_free_rate,
                dividend_yield=surface.dividend_yield,
                boundary_state=boundary_state,
                include_transform_diagnostics=True,
                include_integration_quality_diagnostics=True,
                include_calibration_hooks=True,
            )

            representative_single_results.setdefault(maturity_index, single_result)

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
            assert surface_result.maturity_summaries[maturity_index].representative_transform is not None
            assert single_result.run_diagnostics is not None
            assert (
                surface_result.maturity_summaries[maturity_index].representative_transform
                == single_result.run_diagnostics.transform
            )

    for maturity_index, representative_single_result in representative_single_results.items():
        assert representative_single_result.run_diagnostics is not None
        assert (
            surface_result.maturity_summaries[maturity_index].integration_quality
            == representative_single_result.run_diagnostics.integration_quality
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

    runtime_factors = fit_runtime_payloads_module.prepare_model_runtime_factors_for_model(
        model,
        start_time_utc=valuation_time_utc,
        horizon_interval_count=2,
        interval="1h",
    )

    seasonality_source = fit_seasonality_runtime_module.build_runtime_hourly_seasonality_source_from_model(
        model,
        calendar_registry=runtime_factors.calendar_registry,
        exchange_code=runtime_factors.exchange_code,
        range_start_utc=valuation_time_utc,
        range_end_utc=runtime_factors.range_end_utc,
        interval="1h",
    )

    assert isinstance(
        seasonality_source,
        HourlySeasonalityProviderMaterializedSource,
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

    runtime_factors = fit_runtime_payloads_module.prepare_model_runtime_factors_for_model(
        model,
        start_time_utc=valuation_time_utc,
        horizon_interval_count=2,
        interval="1h",
    )

    seasonality_source = fit_seasonality_runtime_module.build_runtime_hourly_seasonality_source_from_model(
        model,
        calendar_registry=runtime_factors.calendar_registry,
        exchange_code=runtime_factors.exchange_code,
        range_start_utc=valuation_time_utc,
        range_end_utc=runtime_factors.range_end_utc,
        interval="1h",
    )

    assert isinstance(
        seasonality_source,
        HourlySeasonalityProviderExplicitSource,
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

    patch_fit_workflow_runner(
        monkeypatch,
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

    fit_result = estimator.fit_with_report(
        run_id="high-level-fit-runner-path",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
    )

    fit_report = fit_result.report

    captured_input = captured_prepared_handle["prepared_handle"]
    assert hasattr(captured_input, "serialize_execution_input_json")
    assert _prepared_runner_run_id(captured_input) == fit_report.run_id
    assert fit_report.staged_request.run_id == fit_report.run_id
    assert set(fit_report.to_payload()) == {
        "schema_version",
        "run_id",
        "workflow_scope",
        "success",
        "summary",
        "resolved_configuration",
        "session_resolution",
        "preparation_diagnostics",
        "preparation_provenance",
        "prepared_data_reference",
        "prepared_payload_path",
        "staged_execution_input_path",
        "staged_response",
    }


def test_high_level_fit_forwards_persisted_resume_and_option_surface_to_native_workflow(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:00:00Z,100.5,sell",
                "2026-02-02T20:59:00Z,101.0,buy",
            ]
        ),
        encoding="utf-8",
    )
    option_surface_path = tmp_path / "option_surface.parquet"
    option_surface_path.write_bytes(b"native workflow is mocked")

    run_id = "high-level-fit-option-resume"
    output_directory = tmp_path / ".janiform" / "runs"
    valuation_time_utc = datetime(2026, 4, 1, 23, 59, tzinfo=UTC)
    resume_settings = CalibrationResumeSettings(
        source=CalibrationResumeSource.PERSISTED_CHECKPOINT_REFERENCE,
        resume_from_stage=CalibrationStage.JOINT_REFINEMENT,
        checkpoint_reference=f"checkpoint://{run_id}/mean_reversion",
    )
    captured_native_inputs: dict[str, Any] = {}

    def capturing_runner(
        *args: Any,
        **kwargs: Any,
    ) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
        _ = args
        captured_native_inputs.update(kwargs)

        return _successful_full_model_staged_runner_payload(run_id)

    patch_fit_workflow_runner(
        monkeypatch,
        capturing_runner,
    )

    estimator = JaniformEstimator(
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE,
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
        output_preferences=CalibrationMarketDataOutputPreferences(
            execution_mode=CalibrationExecutionMode.DISK,
            disk_checkpoint_directory=str(output_directory / run_id),
        ),
        market_profile="us_equity_cash",
    )

    fit_result = estimator.fit_with_report(
        run_id=run_id,
        resume_settings=resume_settings,
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
        option_surface_path=option_surface_path,
        option_surface_format=CalibrationInputFileFormat.PARQUET,
        option_surface_valuation_time_utc=valuation_time_utc,
        output_dir=output_directory,
    )

    assert fit_result.succeeded is True
    assert captured_native_inputs["resume_settings"] == resume_settings
    assert captured_native_inputs["output_dir"] == output_directory
    assert captured_native_inputs["option_surface_path"] == option_surface_path
    assert captured_native_inputs["option_surface_format"] == CalibrationInputFileFormat.PARQUET
    assert captured_native_inputs["option_surface_valuation_time_utc"] == valuation_time_utc


def test_high_level_fit_reuses_single_native_preparation_pass_when_session_is_explicit(
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

    captured_dynamics_search_restart_candidate_budgets: list[Optional[int]] = []

    def capture_execution_configuration(
        *args,
        **kwargs,
    ) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
        configuration = kwargs["configuration"]
        captured_dynamics_search_restart_candidate_budgets.append(
            configuration.preparation.dynamics_search_restart_candidate_budget
        )

        return _successful_staged_runner_payload("high-level-fit-single-pass")

    patch_fit_workflow_runner(
        monkeypatch,
        capture_execution_configuration,
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
        dynamics_search_restart_candidate_budget=12,
        session="XNYS",
        known_news="none",
    )

    estimator.fit(
        run_id="high-level-fit-single-pass",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
    )

    assert captured_dynamics_search_restart_candidate_budgets == [12]


def test_high_level_estimator_rejects_zero_dynamics_search_restart_candidate_budget() -> None:
    with pytest.raises(CalibrationValidationError) as captured_error:
        JaniformEstimator(dynamics_search_restart_candidate_budget=0)

    assert (
        captured_error.value.failure.field_path
        == "dynamics_search_restart_candidate_budget"
    )
    assert "strictly positive" in captured_error.value.failure.message


def test_high_level_estimator_forwards_pricing_interval_schedule_policy(
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

    captured_policies: list[str] = []

    def capture_execution_configuration(
        *args,
        **kwargs,
    ) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
        configuration = kwargs["configuration"]
        captured_policies.append(configuration.preparation.pricing_interval_schedule_policy.policy)

        return _successful_staged_runner_payload("high-level-fit-policy-forward")

    patch_fit_workflow_runner(
        monkeypatch,
        capture_execution_configuration,
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
        pricing_interval_schedule_policy=AutoExactSparsePricingIntervalSchedulePolicy(),
        session="XNYS",
        known_news="none",
    )

    estimator.fit(
        run_id="high-level-fit-policy-forward",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
        pricing_interval_schedule_policy=AutoActivityCompressedPricingIntervalSchedulePolicy(),
    )

    assert captured_policies == ["auto_activity_compressed"]


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

    execution_configuration_count = 0

    def capture_execution_configuration(
        *args,
        **kwargs,
    ) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
        nonlocal execution_configuration_count
        execution_configuration_count += 1

        return _successful_staged_runner_payload("high-level-fit-built-in-session")

    patch_fit_workflow_runner(
        monkeypatch,
        capture_execution_configuration,
    )

    def fail_if_called(*args, **kwargs) -> NoReturn:
        del args, kwargs

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

    assert execution_configuration_count == 1


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

    patch_fit_workflow_runner(
        monkeypatch,
        lambda prepared_handle, **kwargs: _successful_full_model_staged_runner_payload(
            _prepared_runner_run_id(prepared_handle)
        ),
    )

    fit_result = estimator.fit_with_report(
        run_id="high-level-fit-failure",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
    )

    model = fit_result.model
    fit_report = fit_result.report

    assert model is not None
    assert fit_result.succeeded is True
    assert fit_report.success is True
    assert fit_report.staged_response.failure is None
    assert [report.stage_report_kind.value for report in fit_report.staged_response.stage_reports] == [
        "seasonality",
        "hawkes",
        "return_components",
        "mean_reversion",
        "joint_refinement",
    ]


def test_high_level_fit_with_report_returns_failed_result_without_raising(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:00:00Z,100.5,sell",
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

    patch_fit_workflow_runner(
        monkeypatch,
        lambda prepared_handle, **kwargs: _failed_full_model_staged_runner_payload(
            _prepared_runner_run_id(prepared_handle)
        ),
    )

    fit_result = estimator.fit_with_report(
        run_id="high-level-fit-report-failure",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
    )

    assert isinstance(fit_result, JaniformFitResult)
    assert fit_result.model is None
    assert fit_result.succeeded is False
    assert fit_result.report.success is False


def test_high_level_fit_resume_recovers_committed_disk_run_after_duplicate_manifest_failure(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:00:00Z,100.5,sell",
                "2026-02-02T20:59:00Z,101.0,buy",
            ]
        ),
        encoding="utf-8",
    )

    checkpoint_directory = tmp_path / "checkpoints"
    run_id = "high-level-fit-duplicate-manifest-recovery"
    _, successful_raw_response = _successful_full_model_staged_runner_payload(run_id)
    _write_stage_parameter_artifacts_for_stage_reports(
        checkpoint_directory=checkpoint_directory,
        run_id=run_id,
        raw_stage_reports=cast(list[dict[str, Any]], successful_raw_response["stage_reports"]),
    )
    _write_diagnostic_artifacts_for_recovered_disk_run(
        checkpoint_directory=checkpoint_directory,
        run_id=run_id,
    )
    _write_artifact_manifest_for_recovered_disk_run(
        checkpoint_directory=checkpoint_directory,
        run_id=run_id,
        selected_stage_kinds=(
            CalibrationStage.SEASONALITY,
            CalibrationStage.HAWKES,
            CalibrationStage.RETURN_COMPONENTS,
            CalibrationStage.MEAN_REVERSION,
            CalibrationStage.JOINT_REFINEMENT,
        ),
    )

    patch_fit_workflow_runner(
        monkeypatch,
        lambda prepared_handle, **kwargs: _duplicate_manifest_failure_staged_runner_payload(
            _prepared_runner_run_id(prepared_handle)
        ),
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
        output_preferences=CalibrationMarketDataOutputPreferences(
            execution_mode=CalibrationExecutionMode.DISK,
            disk_checkpoint_directory=str(checkpoint_directory),
            emit_input_provenance_report=True,
        ),
        market_profile="us_equity_cash",
    )

    fit_result = estimator.fit_with_report(
        run_id=run_id,
        resume_settings=CalibrationResumeSettings(
            source=CalibrationResumeSource.PERSISTED_CHECKPOINT_REFERENCE,
            checkpoint_reference=f"checkpoint://{run_id}/last-success",
        ),
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
        output_dir=tmp_path,
    )

    assert fit_result.succeeded is False
    assert fit_result.model is None
    assert fit_result.report.success is False
    assert fit_result.report.staged_response.status.value == "failure"
    assert fit_result.report.staged_response.execution_mode.value == "disk"


def test_high_level_fit_result_require_model_raises_for_failed_fit(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:00:00Z,100.5,sell",
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

    patch_fit_workflow_runner(
        monkeypatch,
        lambda prepared_handle, **kwargs: _failed_full_model_staged_runner_payload(
            _prepared_runner_run_id(prepared_handle)
        ),
    )

    fit_result = estimator.fit_with_report(
        run_id="high-level-fit-require-model-failure",
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
    )

    with pytest.raises(JaniformExecutionError, match="Calibration failed at return_components"):
        fit_result.require_model()


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
    patch_fit_workflow_runner(
        monkeypatch,
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

    patch_fit_workflow_runner(
        monkeypatch,
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


def _write_rust_model_artifact(model: JaniformModel, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(model._native_model_payload(), indent=2, sort_keys=True),
        encoding="utf-8",
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


def _capture_native_model_artifact_simulation_requests(
    monkeypatch: pytest.MonkeyPatch,
) -> list[dict[str, Any]]:
    captured_requests: list[dict[str, Any]] = []

    class _FakeNativeModule:
        def run_simulation_from_model_artifact(
            self,
            request_payload: dict[str, Any],
            progress_callback: Any,
            progress_mode: str = "none",
        ) -> dict[str, Any]:
            del progress_callback, progress_mode
            captured_requests.append(request_payload)

            return {}

    monkeypatch.setattr(
        fit_simulation_workflow_module,
        "native_module",
        lambda: _FakeNativeModule(),
    )
    monkeypatch.setattr(
        fit_simulation_workflow_module,
        "_adapt_native_simulation_output",
        lambda raw_output, **kwargs: kwargs["simulation_kind"],
    )
    monkeypatch.setattr(
        fit_simulation_workflow_module,
        "build_simulation_result",
        lambda model_value, **kwargs: JaniformSimulationResult(
            simulation_kind=kwargs["simulation_result"],
            metadata={},
            provenance={},
            diagnostics={},
            raw_result=kwargs["simulation_result"],
        ),
    )

    return captured_requests


def _capture_native_single_option_pricing_requests(
    monkeypatch: pytest.MonkeyPatch,
) -> tuple[list[dict[str, Any]], VanillaOptionPricingRunResult]:
    captured_requests: list[dict[str, Any]] = []
    pricing_result = run_vanilla_option_pricing(
        build_anchored_runtime_execution_input_model()
    )

    class _FakeNativeModule:
        def run_fitted_model_single_option_pricing_workflow(
            self,
            request_payload: dict[str, Any],
        ) -> dict[str, Any]:
            captured_requests.append(request_payload)

            return pricing_result.to_payload()

    monkeypatch.setattr(
        fit_pricing_workflow_module,
        "native_module",
        lambda: _FakeNativeModule(),
    )

    return captured_requests, pricing_result


def _mean_reversion_fitted_parameters_payload_from_model_payload(
    model_payload: dict[str, Any],
) -> dict[str, Any]:
    raw_joint_refinement = model_payload["parameters"]["joint_refinement"]
    raw_fitted_parameters = raw_joint_refinement["fitted_parameters"]
    raw_return_components = raw_fitted_parameters["return_components_fitted_parameters"]

    return raw_return_components["mean_reversion_fitted_parameters"]


def _write_stage_parameter_artifacts_for_stage_reports(
    *,
    checkpoint_directory: Path,
    run_id: str,
    raw_stage_reports: list[dict[str, Any]],
) -> None:
    artifact_directory = checkpoint_directory / "artifacts" / "calibration"
    artifact_directory.mkdir(parents=True, exist_ok=True)

    for raw_stage_report in raw_stage_reports:
        stage_kind = CalibrationStage(cast(str, raw_stage_report["stage_report_kind"]))
        raw_stage_payload = (
            raw_stage_report["normalized_stage_payload"]
            if stage_kind == CalibrationStage.SEASONALITY
            else raw_stage_report["fitted_stage_payload"]
        )
        stage_payload = deepcopy(cast(dict[str, Any], raw_stage_payload))
        stage_payload.pop("schema_version", None)
        artifact_payload = {
            "schema_version": "calibration.artifact_registry.stage_parameter.v1",
            "artifact_reference": f"artifact://{stage_kind.value}/{run_id}",
            "run_id": run_id,
            "execution_mode": "disk",
            "stage": stage_kind.value,
            "stage_output_schema_version": (
                "calibration.orchestration.stage_output.seasonality.v1"
                if stage_kind == CalibrationStage.SEASONALITY
                else cast(str, raw_stage_report["fitted_stage_payload"]["schema_version"])
            ),
            "stage_parameters": {
                "payload_kind": stage_kind.value,
                "payload": stage_payload,
            },
        }
        artifact_path = (
            checkpoint_directory
            / "artifacts"
            / "calibration"
            / f"{stage_kind.value}.stage_parameter.artifact.json"
        )
        artifact_path.write_text(json.dumps(artifact_payload), encoding="utf-8")


def _write_artifact_manifest_for_recovered_disk_run(
    *,
    checkpoint_directory: Path,
    run_id: str,
    selected_stage_kinds: tuple[CalibrationStage, ...],
) -> None:
    artifact_directory = checkpoint_directory / "artifacts" / "calibration"
    artifact_directory.mkdir(parents=True, exist_ok=True)

    artifact_references: list[dict[str, Any]] = [
        {
            "artifact_id": "dataset_snapshot",
            "artifact_kind": "dataset_snapshot",
            "artifact_reference": f"artifact://dataset_snapshot/{run_id}",
        }
    ]
    artifact_references.extend(
        {
            "artifact_id": f"{stage_kind.value}_stage_output",
            "artifact_kind": "stage_output",
            "artifact_reference": f"artifact://{stage_kind.value}/{run_id}",
            "stage": stage_kind.value,
        }
        for stage_kind in selected_stage_kinds
    )
    artifact_references.extend(
        {
            "artifact_id": f"{stage_kind.value}_checkpoint",
            "artifact_kind": "stage_checkpoint",
            "artifact_reference": f"checkpoint://{run_id}/{stage_kind.value}",
            "stage": stage_kind.value,
        }
        for stage_kind in selected_stage_kinds
    )
    artifact_references.extend(
        {
            "artifact_id": f"diagnostics_{artifact_family}",
            "artifact_kind": "diagnostics",
            "artifact_reference": f"artifact://diagnostics/{run_id}/{artifact_family}",
        }
        for artifact_family in (
            "observed_targets",
            "candidate_frontier",
            "coarse_tick_consistency",
            "selection_rationale",
            "simulation_summary",
            "search_surface_manifest",
            "runtime_profile",
        )
    )

    artifact_manifest_payload = {
        "schema_version": "calibration.artifact_registry.manifest.v1",
        "run_id": run_id,
        "execution_mode": "disk",
        "artifact_references": artifact_references,
        "index_metadata": {
            "recovery_policy": "test.committed_disk_run_recovery.v1",
        },
    }
    (artifact_directory / "artifact_manifest.json").write_text(
        json.dumps(artifact_manifest_payload),
        encoding="utf-8",
    )


def _write_diagnostic_artifacts_for_recovered_disk_run(
    *,
    checkpoint_directory: Path,
    run_id: str,
) -> None:
    diagnostics_directory = checkpoint_directory / "artifacts" / "calibration" / "diagnostics"
    diagnostics_directory.mkdir(parents=True, exist_ok=True)

    diagnostic_payloads = {
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
                "selected_candidate_id": "joint_refinement_baseline",
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
            "cpu_count": 1,
            "entries": [],
        },
    }

    for artifact_family, payload in diagnostic_payloads.items():
        (diagnostics_directory / f"{artifact_family}.json").write_text(
            json.dumps(payload),
            encoding="utf-8",
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


def _duplicate_manifest_failure_staged_runner_payload(
    run_id: str,
) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
    failure = {
        "failure_kind": "artifact_manifest_persist_failed",
        "stage": "joint_refinement",
        "error_message": (
            "committed artifact set already exists for run "
            f"`{run_id}` in `/tmp/{run_id}/artifacts/calibration`"
        ),
        "run_id": run_id,
    }
    raw_response = {
        "schema_version": "calibration.orchestration.staged_run.report.v1",
        "failure_triage_policy": "mode_aware_failure_kind_and_checkpoint_context.v1",
        "run_id": run_id,
        "execution_mode": "disk",
        "status": "failure",
        "failure": failure,
        "stage_reports": [
            {
                "stage_report_kind": "joint_refinement",
                "stage": "joint_refinement",
                "status": "failure",
                "run_id": run_id,
                "execution_mode": "disk",
                "handoff_policy": "summary_only_resume_stage_report.v1",
                "failure": failure,
            }
        ],
        "emitted_checkpoints": [],
        "resumed_from_checkpoint_reference": f"checkpoint://{run_id}/last-success",
        "resumed_from_last_successful_stage": "joint_refinement",
        "started_from_stage": "joint_refinement",
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
                                "activity_coupling_scale": 0.6,
                                "return_jump_scale": 1.0,
                            },
                            "surprise_component_mark_scales": {
                                "activity_coupling_scale": 0.6,
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
                                    "activity_coupling_scale": 0.6,
                                    "return_jump_scale": 1.0,
                                },
                                "surprise_component_mark_scales": {
                                    "activity_coupling_scale": 0.6,
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
                                "activity_coupling_scale": 0.6,
                                "return_jump_scale": 1.0,
                            },
                            "surprise_component_mark_scales": {
                                "activity_coupling_scale": 0.6,
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


def _failed_full_model_staged_runner_payload(run_id: str) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
    failure = {
        "failure_kind": "invalid_candidate_generation_input",
        "stage": "return_components",
        "error_message": (
            "open boundary for exchange `XNYS` at 1774224000000000000 must transition from "
            "closed to open, found previous=Open and boundary=Open"
        ),
        "run_id": run_id,
    }
    raw_response = {
        "schema_version": "calibration.orchestration.staged_run.report.v1",
        "failure_triage_policy": "mode_aware_failure_kind_and_checkpoint_context.v1",
        "run_id": run_id,
        "execution_mode": "memory",
        "status": "failure",
        "failure": failure,
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
                    },
                },
            },
            {
                "stage_report_kind": "return_components",
                "stage": "return_components",
                "status": "failure",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "failure": failure,
            },
        ],
        "emitted_checkpoints": [],
    }

    return validate_staged_calibration_response(raw_response), raw_response


def _prepared_runner_run_id(prepared_handle: Any) -> str:
    prepared_payload = json.loads(prepared_handle.serialize_execution_input_json())
    return str(prepared_payload["request"]["run_id"])
