import json
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
        "_run_fit_with_output_controller",
        _fit_workflow_runner_from_staged_runner(staged_runner),
    )


def _fit_workflow_runner_from_staged_runner(
    staged_runner: StagedRunner,
) -> FitWorkflowRunner:
    def run_fit_workflow(
        configuration: Any,
        *,
        output_controller: Any,
        output_dir: Optional[str | Path],
        run_id: Optional[str],
        resume_settings: Any,
        tick_data_path: str | Path,
        tick_data_format: Optional[Any],
        tick_data_provenance_label: str,
        option_surface_path: Optional[str | Path],
        option_surface_format: Any,
        option_surface_provenance_label: str,
        option_surface_valuation_time_utc: Any,
        carry: Any,
    ) -> NativeFitWorkflowOutput:
        _ = output_controller
        _ = resume_settings
        _ = option_surface_path
        _ = option_surface_format
        _ = option_surface_provenance_label
        _ = option_surface_valuation_time_utc
        _ = carry

        resolved_run_id = run_id or "fit-workflow-test"
        prepared_handle = _PreparedExecutionHandle(run_id=resolved_run_id)
        staged_response, raw_response_payload = staged_runner(
            prepared_handle,
            configuration=configuration,
        )
        model_payload = (
            None
            if staged_response.status != CalibrationStageExecutionStatus.SUCCESS
            else _successful_full_model_payload(run_id=resolved_run_id)
        )
        fit_report_payload = _fit_report_payload(
            run_id=resolved_run_id,
            success=staged_response.status == CalibrationStageExecutionStatus.SUCCESS,
            raw_response_payload=raw_response_payload,
            tick_data_path=tick_data_path,
            tick_data_format=tick_data_format,
            tick_data_provenance_label=tick_data_provenance_label,
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

    return run_fit_workflow


class _PreparedExecutionHandle:
    def __init__(self, *, run_id: str) -> None:
        self._run_id = run_id

    def serialize_execution_input_json(self) -> str:
        return json.dumps({"request": {"run_id": self._run_id}})


def _fit_report_payload(
    *,
    run_id: str,
    success: bool,
    raw_response_payload: dict[str, Any],
    tick_data_path: str | Path,
    tick_data_format: Optional[Any],
    tick_data_provenance_label: str,
) -> dict[str, Any]:
    selected_stages = tuple(
        cast(str, stage_report["stage"])
        for stage_report in cast(list[dict[str, Any]], raw_response_payload["stage_reports"])
    )

    return {
        "schema_version": "janiform.fit_report.v1",
        "run_id": run_id,
        "workflow_scope": "full_without_option_surface",
        "success": success,
        "summary": {
            "run_id": run_id,
            "workflow_scope": "full_without_option_surface",
            "success": success,
            "selected_stages": list(selected_stages),
            "stage_statuses": [
                {
                    "stage": stage_report["stage"],
                    "status": stage_report["status"],
                }
                for stage_report in cast(list[dict[str, Any]], raw_response_payload["stage_reports"])
            ],
            "diagnostic_count": len(raw_response_payload.get("diagnostic_artifact_references", [])),
        },
        "preparation": _preparation_payload(
            run_id=run_id,
            selected_stages=selected_stages,
            tick_data_path=tick_data_path,
            tick_data_format=tick_data_format,
            tick_data_provenance_label=tick_data_provenance_label,
        ),
        "prepared_data_reference": "memory://prepared/ticks",
        "prepared_payload_path": None,
        "staged_execution_input_path": None,
        "staged_request": _staged_request_payload(run_id=run_id, selected_stages=selected_stages),
        "staged_response": raw_response_payload,
    }


def _successful_full_model_payload(*, run_id: str) -> dict[str, Any]:
    return {
        "schema_version": "calibration.high_level.model.v1",
        "run_id": run_id,
        "workflow_scope": "full_without_option_surface",
        "parameters": _successful_full_model_parameters_payload(),
        "metadata": _successful_full_model_metadata_fixture(run_id=run_id),
    }


def _preparation_payload(
    *,
    run_id: str,
    selected_stages: tuple[str, ...],
    tick_data_path: str | Path,
    tick_data_format: Optional[Any],
    tick_data_provenance_label: str,
) -> dict[str, Any]:
    tick_data_format_label = (
        "csv" if tick_data_format is None else getattr(tick_data_format, "value", tick_data_format)
    )

    return {
        "resolved_configuration": _resolved_configuration_payload(),
        "session_resolution": _session_resolution_payload(),
        "diagnostics": _preparation_diagnostics_payload(selected_stages=selected_stages),
        "provenance": _preparation_provenance_payload(
            selected_stages=selected_stages,
            tick_data_path=tick_data_path,
            tick_data_format=str(tick_data_format_label),
            tick_data_provenance_label=tick_data_provenance_label,
        ),
        "input_inference": _input_inference_payload(run_id=run_id),
        "reference_price": 100.0,
    }


def _resolved_configuration_payload() -> dict[str, Any]:
    selected_session = {
        "category": "session",
        "registry_scope": "built_in",
        "preset_name": "XNYS",
    }
    selected_known_news = {
        "category": "known_news",
        "registry_scope": "built_in",
        "preset_name": "none",
    }

    return {
        "schema_version": "calibration.preset_resolution.resolved_configuration.v1",
        "market_profile": {
            "configuration": {
                "profile_name": "us_equity_cash",
                "session_reference": {
                    "usage_mode": "named",
                    "preset_name": "XNYS",
                    "registry_scope": "built_in",
                },
                "known_news_reference": {
                    "usage_mode": "named",
                    "preset_name": "none",
                    "registry_scope": "built_in",
                },
                "known_news_policy": "suggest_only",
                "timestamp_timezone_name": "UTC",
                "column_alias_hints": [],
                "default_calibration_hint_ids": [],
            },
            "source_kind": "built_in",
            "selected_preset": {
                "category": "market_profile",
                "registry_scope": "built_in",
                "preset_name": "us_equity_cash",
            },
            "applied_override_fields": [],
        },
        "session": {
            "configuration": _session_configuration_payload(),
            "source_kind": "built_in",
            "selected_preset": selected_session,
            "inferred_from_data": False,
            "applied_override_fields": [],
        },
        "known_news": {
            "configuration": {
                "events": [],
                "calendar_timezone_name": "America/New_York",
            },
            "source_kind": "built_in",
            "selected_preset": selected_known_news,
            "applied_override_fields": [],
            "recommended_reference": None,
            "recommendation_policy": None,
            "recommendation_applied": False,
        },
        "diagnostics": [],
    }


def _session_configuration_payload() -> dict[str, Any]:
    return {
        "exchange_code": "XNYS",
        "exchange_name": "New York Stock Exchange",
        "timezone_name": "America/New_York",
        "regular_session_windows": [
            {
                "weekday": weekday,
                "label": "regular",
                "open_time": "09:30",
                "close_time": "16:00",
            }
            for weekday in ("mon", "tue", "wed", "thu", "fri")
        ],
        "recurring_closed_windows": [],
    }


def _session_resolution_payload() -> dict[str, Any]:
    return {
        "schema_version": "calibration.market_data.session_resolution.v1",
        "resolution_kind": "automatic_inference",
        "provenance_kind": "bundled_preset",
        "selected_policy_id": "XNYS",
        "manual_override_applied": False,
        "exchange_code": "XNYS",
        "exchange_name": "New York Stock Exchange",
        "resolved_timezone_name": "America/New_York",
        "timezone_source": "session",
        "open_session_duration_hours_per_week": 32.5,
        "confidence_score": 1.0,
        "confidence_label": "high",
        "regular_session_windows": _session_configuration_payload()["regular_session_windows"],
        "recurring_closed_windows": [],
        "reasons": [],
        "ambiguity_warnings": [],
        "metadata_discrepancies": [],
        "resolved_session_definition_json": json.dumps(_session_configuration_payload()),
    }


def _preparation_diagnostics_payload(*, selected_stages: tuple[str, ...]) -> dict[str, Any]:
    return {
        "workflow_scope": "full_without_option_surface",
        "selected_stages": list(selected_stages),
        "executed_preprocessing_steps": [],
        "tick_record_count": 3,
        "option_surface_record_count": 0,
        "known_news_event_count": 0,
        "known_news_events_outside_source_window_count": 0,
        "session_open_tick_count": 3,
        "session_closed_tick_count": 0,
        "known_news_open_event_count": 0,
        "known_news_closed_event_count": 0,
        "seasonality_overlay_count": 0,
        "hawkes_candidate_count": 1,
        "known_news_return_sample_count": 0,
        "intraday_jump_candidate_count": 0,
        "close_open_boundary_observation_count": 0,
        "material_close_open_gap_count": 0,
    }


def _preparation_provenance_payload(
    *,
    selected_stages: tuple[str, ...],
    tick_data_path: str | Path,
    tick_data_format: str,
    tick_data_provenance_label: str,
) -> dict[str, Any]:
    return {
        "tick_data_summary": {
            "path": str(tick_data_path),
            "file_format": tick_data_format,
            "provenance_label": tick_data_provenance_label,
            "source_row_count": 3,
            "retained_row_count": 3,
            "dropped_duplicate_row_count": 0,
            "sort_records_by_timestamp": True,
            "applied_timestamp_timezone": "UTC",
            "canonical_value_field": "trade_price",
            "value_mapping_policy": "explicit_price_column",
        },
        "option_surface_summary": None,
        "known_news_summary": None,
        "normalization_policy": {
            "timestamp_timezone": "UTC",
            "sort_records_by_timestamp": True,
            "duplicate_record_policy": None,
            "tick_price_input_policy": "explicit_price_column",
            "option_quote_input_policy": None,
        },
        "workflow_scope": "full_without_option_surface",
        "selected_stages": list(selected_stages),
        "session_definition_path": None,
        "derived_trade_events_data_reference": "memory://prepared/ticks",
        "derived_option_surface_data_reference": None,
        "derivation_policies": [],
        "deterministic_defaults_applied": [],
    }


def _staged_request_payload(*, run_id: str, selected_stages: tuple[str, ...]) -> dict[str, Any]:
    return {
        "run_id": run_id,
        "data_inputs": {
            "exchange_code": "XNYS",
            "trade_events_data_reference": "memory://prepared/ticks",
            "option_surface_data_reference": None,
        },
        "execution_mode": "memory",
        "selected_stages": list(selected_stages),
        "seed_settings": {
            "global_seed": 17,
            "stage_seed_overrides": [],
        },
        "optimizer_settings_by_stage": [
            {
                "stage": stage,
                "optimizer_settings": {
                    "solver_name": "test",
                    "max_iterations": 1,
                    "cpu_count": 1,
                    "absolute_tolerance": 1.0,
                    "relative_tolerance": 1.0,
                    "constraint_penalty_weight": 0.0,
                },
            }
            for stage in selected_stages
        ],
        "stage_handoffs": [],
        "resume_settings": None,
    }


def _input_inference_payload(*, run_id: str) -> dict[str, Any]:
    return {
        "run_id": run_id,
        "workflow_scope": "full_without_option_surface",
        "tick_data_source": {
            "file_format": "csv",
        },
        "tick_data_column_mapping": {
            "timestamp_column": "timestamp_utc",
            "price_column": "trade_price",
            "trade_sign_column": "trade_sign",
            "bid_column": None,
            "ask_column": None,
        },
        "normalization_policy": {
            "timestamp_timezone": "UTC",
            "timestamp_timezone_source": "explicit",
            "tick_price_input_policy": "explicit_price_column",
        },
    }


def _successful_full_model_parameters_payload() -> dict[str, Any]:
    _, raw_response = successful_full_model_staged_runner_payload("fixture-run")
    stage_reports = cast(list[dict[str, Any]], raw_response["stage_reports"])
    seasonality_stage_payload = cast(dict[str, Any], stage_reports[0]["normalized_stage_payload"])
    joint_refinement_payload = deepcopy(stage_reports[4]["fitted_stage_payload"])
    joint_refinement_payload.pop("baseline_hawkes_fitted_parameters", None)
    joint_refinement_payload.pop("baseline_return_components_fitted_parameters", None)

    return {
        "seasonality": {
            "schema_version": seasonality_stage_payload["composition_schema_version"],
            "exchange_code": seasonality_stage_payload["exchange_code"],
            "intraday_minutes_per_bin": seasonality_stage_payload.get("intraday_minutes_per_bin"),
            "intraday_factor_by_bin": tuple(seasonality_stage_payload["intraday_factor_by_bin"]),
            "day_of_week_factor": tuple(seasonality_stage_payload["day_of_week_factor"]),
            "day_of_year_factor": tuple(seasonality_stage_payload["day_of_year_factor"]),
        },
        "joint_refinement": joint_refinement_payload,
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
                "joint_refinement",
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
                    "registry_scope": "built_in",
                    "preset_name": "us_equity_cash",
                },
                "applied_override_fields": [],
            },
            "session": {
                "source_kind": "built_in",
                "selected_preset": {
                    "category": "session",
                    "registry_scope": "built_in",
                    "preset_name": "XNYS",
                },
                "applied_override_fields": [],
                "inferred_from_data": False,
            },
            "known_news": {
                "source_kind": "built_in",
                "selected_preset": {
                    "category": "known_news",
                    "registry_scope": "built_in",
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
        "input_inference": _input_inference_payload(run_id=run_id),
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
