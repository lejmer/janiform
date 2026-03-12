import json
from pathlib import Path

import pytest

import janiform.calibration._fit_api as high_level_module
from janiform.calibration import (
    CalibrationDuplicateRecordPolicy,
    CalibrationInputFileFormat,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
    CalibrationValidationError,
    JaniformEstimator,
    JaniformFitReport,
    JaniformModel,
    validate_staged_calibration_response,
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
        high_level_module,
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
        high_level_module,
        "_run_staged_calibration_with_raw_payload",
        lambda *args, **kwargs: _successful_staged_runner_payload(run_id),
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
