import json
from datetime import UTC, datetime
from pathlib import Path

import pytest

import janiform.calibration.fit.estimator as fit_estimator_module
import janiform.calibration.fit.model as fit_model_module
from janiform.calibration import CalibrationValidationError
from janiform.calibration.fit import JaniformEstimator, JaniformFitReport, JaniformModel
from janiform.calibration.market_data import (
    CalibrationDuplicateRecordPolicy,
    CalibrationInputFileFormat,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
)
from janiform.calibration.staged import validate_staged_calibration_response


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
    assert summary["session_resolution"]["resolution_kind"] == "explicit_override"
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

    assert loaded_model.parameters == model.parameters
    assert loaded_model.metadata == model.metadata
    assert loaded_model.summary() == model.summary()
    assert first_path.read_text(encoding="utf-8") == second_path.read_text(encoding="utf-8")


def test_high_level_model_simulate_dispatches_by_output_mode(
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
    captured_calls: list[tuple[str, object]] = []
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

    observed_coarse = model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=2,
        output="coarse",
    )
    observed_ticks = model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=2,
        output="ticks",
    )
    observed_both = model.simulate(
        seed=17,
        start_time_utc=datetime(2026, 2, 2, 15, tzinfo=UTC),
        horizon_interval_count=2,
        output="both",
    )

    assert observed_coarse is coarse_result
    assert observed_ticks is tick_result
    assert observed_both == (coarse_result, tick_result)
    assert captured_calls == [
        ("coarse", prepared_inputs.coarse_request),
        ("ticks", prepared_inputs.seeded_request),
        ("coarse", prepared_inputs.coarse_request),
        ("ticks", prepared_inputs.seeded_request),
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
    captured_execution_inputs: list[object] = []
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

    captured_execution_input: dict[str, object] = {}

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
            trade_price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
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
            trade_price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
        ),
        market_profile="us_equity_cash",
    )

    return estimator.fit(
        run_id=run_id,
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
    )


def _successful_staged_runner_payload(run_id: str) -> tuple[object, dict[str, object]]:
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


def _successful_full_model_staged_runner_payload(run_id: str) -> tuple[object, dict[str, object]]:
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
