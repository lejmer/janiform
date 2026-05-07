import json
from copy import deepcopy
from pathlib import Path
from typing import Any, Callable, Optional, cast

import pytest

import janiform.calibration.fit._execution as fit_execution_module
from janiform._workflow_outputs import generate_workflow_run_id
from janiform.calibration.fit import JaniformEstimator
from janiform.model import JaniformModel
from janiform.calibration.fit._preparation.contracts import FitExecutionConfiguration
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
    *,
    progress_event: Optional[Any] = None,
) -> None:
    monkeypatch.setattr(
        fit_execution_module,
        "_run_fit_with_native_progress",
        _fit_workflow_runner_from_staged_runner(staged_runner, progress_event=progress_event),
    )


def _fit_workflow_runner_from_staged_runner(
    staged_runner: StagedRunner,
    *,
    progress_event: Optional[Any] = None,
) -> FitWorkflowRunner:
    def run_fit_workflow(
        configuration: FitExecutionConfiguration,
        *,
        output_dir: Optional[str | Path],
        run_id: Optional[str],
        resume_settings: Any,
        tick_data_path: str | Path,
        tick_data_format: Optional[CalibrationInputFileFormat],
        tick_data_provenance_label: str,
        option_surface_path: Optional[str | Path],
        option_surface_format: CalibrationInputFileFormat,
        option_surface_provenance_label: str,
        option_surface_valuation_time_utc: Any,
        carry: Any,
        progress_callback: Any,
    ) -> NativeFitWorkflowOutput:
        _ = resume_settings
        _ = option_surface_path
        _ = option_surface_format
        _ = option_surface_provenance_label
        _ = option_surface_valuation_time_utc
        _ = carry

        if progress_event is not None and progress_callback is not None:
            progress_callback(progress_event)

        resolved_run_id = run_id or generate_workflow_run_id(Path(tick_data_path).stem)
        prepared_handle = _PreparedExecutionHandle(
            run_id=resolved_run_id,
            configuration=configuration,
        )
        staged_response, raw_response_payload = staged_runner(
            prepared_handle,
            configuration=configuration,
        )
        model_payload = (
            None
            if staged_response.status != CalibrationStageExecutionStatus.SUCCESS
            else _successful_full_model_payload(
                run_id=resolved_run_id,
                configuration=configuration,
            )
        )
        fit_report_payload = _fit_report_payload(
            run_id=resolved_run_id,
            success=staged_response.status == CalibrationStageExecutionStatus.SUCCESS,
            raw_response_payload=raw_response_payload,
            tick_data_path=tick_data_path,
            tick_data_format=tick_data_format,
            tick_data_provenance_label=tick_data_provenance_label,
            configuration=configuration,
        )

        _ = output_dir

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
    def __init__(self, *, run_id: str, configuration: FitExecutionConfiguration) -> None:
        self._run_id = run_id
        self._configuration = configuration

    def serialize_execution_input_json(self) -> str:
        payload: dict[str, Any] = {"request": {"run_id": self._run_id}}
        mean_reversion = self._configuration.preparation.mean_reversion
        if mean_reversion is not None:
            payload["mean_reversion_stage_input"] = {
                "calibration_grid_kind": "synthetic_daily_close_grid.v1",
                "calibration_grid_interval_hours": mean_reversion.calibration_grid_interval_hours,
                "source_observation_count": 0,
                "coverage_ratio": 0.0,
                "observations": [],
            }

        return json.dumps(payload)


def _fit_report_payload(
    *,
    run_id: str,
    success: bool,
    raw_response_payload: dict[str, Any],
    tick_data_path: str | Path,
    tick_data_format: Optional[CalibrationInputFileFormat],
    tick_data_provenance_label: str,
    configuration: FitExecutionConfiguration,
) -> dict[str, Any]:
    selected_stages = tuple(
        cast(str, stage_report["stage"])
        for stage_report in cast(list[dict[str, Any]], raw_response_payload["stage_reports"])
    )
    workflow_scope = _workflow_scope_value(configuration)

    return {
        "schema_version": "janiform.fit_report.v1",
        "run_id": run_id,
        "workflow_scope": workflow_scope,
        "success": success,
        "summary": {
            "run_id": run_id,
            "workflow_scope": workflow_scope,
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
            configuration=configuration,
        ),
        "prepared_data_reference": "memory://prepared/ticks",
        "prepared_payload_path": None,
        "staged_execution_input_path": None,
        "staged_request": _staged_request_payload(
            run_id=run_id,
            selected_stages=selected_stages,
            configuration=configuration,
        ),
        "staged_response": raw_response_payload,
    }


def _successful_full_model_payload(
    *,
    run_id: str,
    configuration: FitExecutionConfiguration,
) -> dict[str, Any]:
    return {
        "schema_version": "calibration.high_level.model.v1",
        "run_id": run_id,
        "workflow_scope": _workflow_scope_value(configuration),
        "parameters": _successful_full_model_parameters_payload(),
        "metadata": _successful_full_model_metadata_fixture(
            run_id=run_id,
            configuration=configuration,
        ),
        "resolved_configuration": _resolved_configuration_payload(configuration),
        "session_resolution": _session_resolution_payload(configuration),
    }


def _preparation_payload(
    *,
    run_id: str,
    selected_stages: tuple[str, ...],
    tick_data_path: str | Path,
    tick_data_format: Optional[CalibrationInputFileFormat],
    tick_data_provenance_label: str,
    configuration: FitExecutionConfiguration,
) -> dict[str, Any]:
    tick_data_format_label = "csv" if tick_data_format is None else tick_data_format.value

    return {
        "resolved_configuration": _resolved_configuration_payload(configuration),
        "session_resolution": _session_resolution_payload(configuration),
        "diagnostics": _preparation_diagnostics_payload(
            selected_stages=selected_stages,
            configuration=configuration,
        ),
        "provenance": _preparation_provenance_payload(
            selected_stages=selected_stages,
            tick_data_path=tick_data_path,
            tick_data_format=str(tick_data_format_label),
            tick_data_provenance_label=tick_data_provenance_label,
            configuration=configuration,
        ),
        "input_inference": _input_inference_payload(
            run_id=run_id,
            configuration=configuration,
        ),
        "reference_price": 100.0,
    }


def _resolved_configuration_payload(configuration: FitExecutionConfiguration) -> dict[str, Any]:
    market_profile_name = _market_profile_name(configuration)
    session_preset_name = _session_preset_name(configuration)
    session_configuration = _session_configuration_payload(session_preset_name)

    selected_session = {
        "category": "session",
        "registry_scope": "built_in",
        "preset_name": session_preset_name,
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
                "profile_name": market_profile_name,
                "session_reference": {
                    "usage_mode": "named",
                    "preset_name": session_preset_name,
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
                "preset_name": market_profile_name,
            },
            "applied_override_fields": [],
        },
        "session": {
            "configuration": session_configuration,
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
            "recommended_reference": (
                {
                    "usage_mode": "named",
                    "category": "known_news",
                    "registry_scope": "built_in",
                    "preset_name": "us_macro_core",
                }
                if market_profile_name == "us_equity_cash"
                else None
            ),
            "recommendation_policy": (
                "suggest_only" if market_profile_name == "us_equity_cash" else None
            ),
            "recommendation_applied": False,
        },
        "diagnostics": [],
    }


def _session_configuration_payload(session_preset_name: str = "XNYS") -> dict[str, Any]:
    exchange_code = _exchange_code_for_session(session_preset_name)
    exchange_name = (
        "FX spot synthetic 24/5"
        if session_preset_name == "fx_spot_24_5"
        else "New York Stock Exchange"
    )
    timezone_name = "UTC" if session_preset_name == "fx_spot_24_5" else "America/New_York"

    return {
        "exchange_code": exchange_code,
        "exchange_name": exchange_name,
        "timezone_name": timezone_name,
        "regular_session_windows": [
            {
                "weekday": weekday,
                "label": "regular",
                "open_time": "00:00" if session_preset_name == "fx_spot_24_5" else "09:30",
                "close_time": "24:00" if session_preset_name == "fx_spot_24_5" else "16:00",
            }
            for weekday in ("mon", "tue", "wed", "thu", "fri")
        ],
        "recurring_closed_windows": [],
    }


def _session_resolution_payload(
    configuration: Optional[FitExecutionConfiguration] = None,
) -> dict[str, Any]:
    session_preset_name = "XNYS" if configuration is None else _session_preset_name(configuration)
    session_configuration = _session_configuration_payload(session_preset_name)

    return {
        "schema_version": "calibration.market_data.session_resolution.v1",
        "resolution_kind": "automatic_inference",
        "provenance_kind": "bundled_preset",
        "selected_policy_id": session_preset_name,
        "manual_override_applied": False,
        "exchange_code": session_configuration["exchange_code"],
        "exchange_name": session_configuration["exchange_name"],
        "resolved_timezone_name": session_configuration["timezone_name"],
        "timezone_source": "session",
        "open_session_duration_hours_per_week": 32.5,
        "confidence_score": 1.0,
        "confidence_label": "high",
        "regular_session_windows": session_configuration["regular_session_windows"],
        "recurring_closed_windows": [],
        "reasons": [],
        "ambiguity_warnings": [],
        "metadata_discrepancies": [],
        "resolved_session_definition_json": json.dumps(
            _calendar_registry_payload(session_configuration)
        ),
    }


def _market_profile_name(configuration: FitExecutionConfiguration) -> str:
    market_profile_reference = configuration.preparation.market_profile_reference
    if market_profile_reference is None or market_profile_reference.preset_name is None:
        return "us_equity_cash"

    return market_profile_reference.preset_name


def _workflow_scope_value(configuration: FitExecutionConfiguration) -> str:
    workflow_scope = configuration.preparation.workflow_scope_override
    if workflow_scope is None:
        return "full_without_option_surface"

    return str(workflow_scope.value)


def _session_preset_name(configuration: FitExecutionConfiguration) -> str:
    session_reference = configuration.preparation.effective_session_reference
    if session_reference is not None and session_reference.preset_name is not None:
        return session_reference.preset_name

    return "fx_spot_24_5" if _market_profile_name(configuration) == "eurusd_spot" else "XNYS"


def _exchange_code_for_session(session_preset_name: str) -> str:
    return "FXSPOT" if session_preset_name == "fx_spot_24_5" else "XNYS"


def _calendar_registry_payload(session_configuration: dict[str, Any]) -> dict[str, Any]:
    return {
        "schema_version": "calendar.registry.v1",
        "artifact_date": "2026-01-31",
        "exchanges": [
            {
                "schema_version": "calendar.v1",
                "exchange_code": session_configuration["exchange_code"],
                "exchange_name": session_configuration["exchange_name"],
                "timezone_name": session_configuration["timezone_name"],
                "rule_metadata": {
                    "holiday_calendar_code": "TEST",
                    "early_close_rule_set": "NONE",
                },
                "regular_sessions": [
                    {
                        "weekday": weekday,
                        "windows": [
                            {
                                "label": window["label"],
                                "open_time": window["open_time"],
                                "close_time": window["close_time"],
                            }
                        ],
                    }
                    for weekday, window in zip(
                        ("Monday", "Tuesday", "Wednesday", "Thursday", "Friday"),
                        session_configuration["regular_session_windows"],
                        strict=True,
                    )
                ],
                "holiday_dates": [],
                "early_close_sessions": [],
            }
        ],
    }


def _preparation_diagnostics_payload(
    *,
    selected_stages: tuple[str, ...],
    configuration: FitExecutionConfiguration,
) -> dict[str, Any]:
    return {
        "workflow_scope": _workflow_scope_value(configuration),
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
    configuration: FitExecutionConfiguration,
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
        "workflow_scope": _workflow_scope_value(configuration),
        "selected_stages": list(selected_stages),
        "session_definition_path": None,
        "derived_trade_events_data_reference": "memory://prepared/ticks",
        "derived_option_surface_data_reference": None,
        "derivation_policies": [],
        "deterministic_defaults_applied": [],
    }


def _staged_request_payload(
    *,
    run_id: str,
    selected_stages: tuple[str, ...],
    configuration: FitExecutionConfiguration,
) -> dict[str, Any]:
    return {
        "run_id": run_id,
        "data_inputs": {
            "exchange_code": _exchange_code_for_session(_session_preset_name(configuration)),
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


def _input_inference_payload(
    *,
    run_id: str,
    configuration: FitExecutionConfiguration,
) -> dict[str, Any]:
    return {
        "run_id": run_id,
        "workflow_scope": _workflow_scope_value(configuration),
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
        "timestamp_timezone_source": "explicit",
        "normalization_policy": {
            "timestamp_timezone": "UTC",
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


def _successful_full_model_metadata_fixture(
    *,
    run_id: str,
    configuration: FitExecutionConfiguration,
) -> dict[str, Any]:
    workflow_scope = _workflow_scope_value(configuration)
    market_profile_name = _market_profile_name(configuration)
    session_preset_name = _session_preset_name(configuration)
    exchange_code = _exchange_code_for_session(session_preset_name)

    return {
        "run_id": run_id,
        "workflow_scope": workflow_scope,
        "calibration_scope": "underlying_only",
        "execution_mode": "memory",
        "selected_stages": [
            "seasonality",
            "hawkes",
            "return_components",
            "mean_reversion",
            "joint_refinement",
        ],
        "market_profile": market_profile_name,
        "session_preset": session_preset_name,
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
                    "preset_name": market_profile_name,
                },
                "applied_override_fields": [],
            },
            "session": {
                "source_kind": "built_in",
                "selected_preset": {
                    "category": "session",
                    "registry_scope": "built_in",
                    "preset_name": session_preset_name,
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
            "selected_policy_id": session_preset_name,
            "exchange_code": exchange_code,
            "exchange_name": (
                "FX spot synthetic 24/5"
                if session_preset_name == "fx_spot_24_5"
                else "New York Stock Exchange"
            ),
            "resolved_timezone_name": (
                "UTC" if session_preset_name == "fx_spot_24_5" else "America/New_York"
            ),
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
        "input_inference": _input_inference_payload(
            run_id=run_id,
            configuration=configuration,
        ),
        "diagnostics": [],
        "fit_report_summary": {
            "run_id": run_id,
            "workflow_scope": workflow_scope,
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
