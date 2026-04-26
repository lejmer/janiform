from copy import deepcopy
from pathlib import Path
from typing import Any, Callable, Optional, cast

import pytest

import janiform.calibration.fit._execution as fit_execution_module
from janiform._workflow_outputs import (
    build_run_manifest,
    resolve_fit_artifact_paths,
    write_json_document,
)
from janiform.calibration.fit import JaniformEstimator, JaniformModel
from janiform.calibration.fit._runtime.workflow import NativeFitWorkflowOutput
from janiform.calibration.market_data import (
    CalibrationInputFileFormat,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
)
from janiform.calibration.staged import StagedCalibrationResponse
from janiform.calibration.staged.contracts import CalibrationStageExecutionStatus
from tests.support.high_level_workflows import successful_full_model_staged_runner_payload

StagedRunner = Callable[..., tuple[StagedCalibrationResponse, dict[str, Any]]]
FitWorkflowRunner = Callable[..., NativeFitWorkflowOutput]


def build_full_workflow_model(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    run_id: str,
) -> JaniformModel:
    tick_data_path = _write_tick_data(
        tmp_path / f"{run_id}.csv",
        rows=(
            "2026-02-02T14:30:00Z,100.0,buy",
            "2026-02-02T15:00:00Z,101.0,sell",
            "2026-02-02T20:59:00Z,102.0,buy",
            "2026-02-03T14:30:00Z,99.0,sell",
            "2026-02-03T20:59:00Z,98.0,buy",
            "2026-02-04T14:30:00Z,97.0,sell",
        ),
    )

    patch_fit_workflow_runner(
        monkeypatch,
        lambda *args, **kwargs: successful_full_model_staged_runner_payload(run_id),
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


def patch_fit_workflow_runner(
    monkeypatch: pytest.MonkeyPatch,
    staged_runner: StagedRunner,
) -> None:
    monkeypatch.setattr(
        fit_execution_module,
        "run_prepared_fit_workflow",
        _fit_workflow_runner_from_staged_runner(staged_runner),
    )


def _fit_workflow_runner_from_staged_runner(
    staged_runner: StagedRunner,
) -> FitWorkflowRunner:
    def run_prepared_fit_workflow(
        configuration: Any,
        *,
        prepared_workflow: Any,
        output_dir: Optional[str | Path],
        resume_settings: Any,
        progress_mode: Any,
    ) -> NativeFitWorkflowOutput:
        _ = resume_settings
        _ = progress_mode

        staged_response, raw_response_payload = staged_runner(
            prepared_workflow.prepared.prepared_handle,
            configuration=configuration,
            prepared_workflow=prepared_workflow,
        )
        model_payload = (
            None
            if staged_response.status != CalibrationStageExecutionStatus.SUCCESS
            else _successful_full_model_payload(run_id=prepared_workflow.run_id)
        )
        fit_report_payload = _fit_report_placeholder(
            run_id=prepared_workflow.run_id,
            success=staged_response.status == CalibrationStageExecutionStatus.SUCCESS,
        )

        if output_dir is not None:
            _write_fit_workflow_artifacts(
                output_dir=output_dir,
                fit_report_payload=fit_report_payload,
                model_payload=model_payload,
            )

        return NativeFitWorkflowOutput(
            staged_response=staged_response,
            raw_staged_response_payload=raw_response_payload,
            fit_report_payload=fit_report_payload,
            model_payload=model_payload,
            event_log_path="",
            latest_state_snapshot_path="",
            execution_duration_seconds=0.0,
            temporary_directory=None,
        )

    return run_prepared_fit_workflow


def _fit_report_placeholder(
    *,
    run_id: str,
    success: bool,
) -> dict[str, Any]:
    return {
        "schema_version": "janiform.fit_report.v1",
        "run_id": run_id,
        "success": success,
    }


def _successful_full_model_payload(*, run_id: str) -> dict[str, Any]:
    return {
        "schema_version": "calibration.high_level.model.v1",
        "run_id": run_id,
        "workflow_scope": "full_without_option_surface",
        "parameters": _successful_full_model_parameters_payload(),
        "metadata": _successful_full_model_metadata_fixture(run_id=run_id),
    }


def _successful_full_model_parameters_payload() -> dict[str, Any]:
    _, raw_response = successful_full_model_staged_runner_payload("fixture-run")
    stage_reports = cast(list[dict[str, Any]], raw_response["stage_reports"])
    seasonality_stage_payload = cast(dict[str, Any], stage_reports[0]["normalized_stage_payload"])

    return {
        "seasonality": {
            "schema_version": seasonality_stage_payload["composition_schema_version"],
            "exchange_code": seasonality_stage_payload["exchange_code"],
            "intraday_minutes_per_bin": seasonality_stage_payload.get("intraday_minutes_per_bin"),
            "intraday_factor_by_bin": tuple(seasonality_stage_payload["intraday_factor_by_bin"]),
            "day_of_week_factor": tuple(seasonality_stage_payload["day_of_week_factor"]),
            "day_of_year_factor": tuple(seasonality_stage_payload["day_of_year_factor"]),
        },
        "hawkes": deepcopy(stage_reports[1]["fitted_stage_payload"]),
        "return_components": deepcopy(stage_reports[2]["fitted_stage_payload"]),
        "mean_reversion": deepcopy(stage_reports[3]["fitted_stage_payload"]),
        "joint_refinement": deepcopy(stage_reports[4]["fitted_stage_payload"]),
    }


def _successful_full_model_metadata_fixture(*, run_id: str) -> dict[str, Any]:
    return {
        "run_id": run_id,
        "workflow_scope": "full_without_option_surface",
        "calibration_scope": "underlying_only",
        "execution_mode": "memory",
        "selected_stages": [
            "seasonality",
            "hawkes",
            "return_components",
            "mean_reversion",
            "joint_refinement",
        ],
        "market_profile": "us_equity_cash",
        "session_preset": "XNYS",
        "known_news_preset": "none",
        "session_source_kind": "built_in",
        "known_news_source_kind": "built_in",
        "fitted_parameter_summary": {
            "stage_keys": [
                "hawkes",
                "joint_refinement",
                "mean_reversion",
                "return_components",
                "seasonality",
            ],
            "selected_candidate_ids": {
                "hawkes": "candidate_stable_low_loss",
                "return_components": "return_candidate_balanced",
                "joint_refinement": "joint_refinement_baseline",
            },
        },
        "preset_provenance": {
            "market_profile": {
                "source_kind": "built_in",
                "selected_preset": {
                    "category": "market_profile",
                    "registry_scope": "bundled",
                    "preset_name": "us_equity_cash",
                },
                "applied_override_fields": [],
            },
            "session": {
                "source_kind": "built_in",
                "selected_preset": {
                    "category": "session",
                    "registry_scope": "bundled",
                    "preset_name": "XNYS",
                },
                "applied_override_fields": [],
                "inferred_from_data": False,
            },
            "known_news": {
                "source_kind": "built_in",
                "selected_preset": {
                    "category": "known_news",
                    "registry_scope": "bundled",
                    "preset_name": "none",
                },
                "applied_override_fields": [],
                "recommended_preset": None,
                "recommendation_policy": None,
                "recommendation_applied": False,
            },
        },
        "session_resolution": {
            "resolution_kind": "automatic_inference",
            "provenance_kind": "bundled_preset",
            "selected_policy_id": "XNYS",
            "exchange_code": "XNYS",
            "exchange_name": "New York Stock Exchange",
            "resolved_timezone_name": "America/New_York",
            "confidence_score": 1.0,
            "confidence_label": "high",
            "manual_override_applied": False,
        },
        "known_news_activation": {
            "selected_preset": "none",
            "event_count": 0,
            "active": False,
        },
        "simulation_output_preferences": {
            "reference_price": 100.0,
        },
        "diagnostic_artifacts": {
            "artifact_bundle_available": False,
            "artifact_families": [],
        },
        "input_inference": {
            "run_id": run_id,
            "workflow_scope": "full_without_option_surface",
            "tick_data_format": "csv",
            "timestamp_column": "timestamp_utc",
            "timestamp_timezone": "UTC",
            "timestamp_timezone_source": "explicit",
            "tick_price_input_policy": "explicit_price_column",
            "tick_price_columns": {
                "price_column": "trade_price",
                "trade_sign_column": "trade_sign",
                "bid_column": None,
                "ask_column": None,
            },
            "available_columns": [
                "timestamp_utc",
                "trade_price",
                "trade_sign",
            ],
            "candidate_columns": {
                "timestamp": ["timestamp_utc"],
                "price": ["trade_price"],
                "trade_sign": ["trade_sign"],
            },
            "default_calibration_hint_ids": [],
            "market_profile": "us_equity_cash",
            "known_news": {
                "selected_preset": "none",
                "recommended_preset": None,
                "recommendation_applied": False,
            },
            "session_source_kind": "built_in",
        },
        "diagnostics": [],
        "fit_report_summary": {
            "run_id": run_id,
            "workflow_scope": "full_without_option_surface",
            "success": True,
            "selected_stages": [
                "seasonality",
                "hawkes",
                "return_components",
                "mean_reversion",
                "joint_refinement",
            ],
            "stage_statuses": [
                {"stage": "seasonality", "status": "success"},
                {"stage": "hawkes", "status": "success"},
                {"stage": "return_components", "status": "success"},
                {"stage": "mean_reversion", "status": "success"},
                {"stage": "joint_refinement", "status": "success"},
            ],
            "diagnostic_count": 0,
        },
    }


def _write_fit_workflow_artifacts(
    *,
    output_dir: str | Path,
    fit_report_payload: dict[str, Any],
    model_payload: Optional[dict[str, Any]],
) -> None:
    paths = resolve_fit_artifact_paths(
        run_id=str(fit_report_payload["run_id"]),
        output_dir=output_dir,
    )
    paths.run_directory.mkdir(parents=True, exist_ok=True)
    write_json_document(paths.report_path, fit_report_payload)

    additional_artifacts = None
    if model_payload is not None:
        write_json_document(paths.primary_output_path, model_payload)
        additional_artifacts = {"model_path": str(paths.primary_output_path)}

    write_json_document(
        paths.manifest_path,
        build_run_manifest(
            workflow="fit",
            run_id=paths.run_id,
            primary_output_path=paths.primary_output_path,
            report_path=paths.report_path,
            run_directory=paths.run_directory,
            additional_artifacts=additional_artifacts,
        ),
    )


def _write_tick_data(path: Path, *, rows: tuple[str, ...]) -> Path:
    path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                *rows,
            ]
        ),
        encoding="utf-8",
    )

    return path
