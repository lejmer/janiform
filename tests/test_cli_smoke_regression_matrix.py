import json
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pytest

import janiform.cli as cli_module
from examples._shared import (
    build_stylized_fact_artifact_export_config,
    build_stylized_fact_horizon_comparison_request,
    build_stylized_fact_metric_catalog,
)
from tests.calibration_test_support import build_valid_execution_input_model
from tests.cli_test_support import (
    build_coarse_simulation_request,
    build_simulation_request,
    invoke_cli,
    write_json_path,
    write_market_data_input_files,
)
from tests.test_cli_high_level_commands import _build_full_workflow_model


@dataclass(frozen=True, slots=True)
class CliSmokeInvocation:
    command_arguments: list[str]
    validate_stdout: Callable[[str], None]


@dataclass(frozen=True, slots=True)
class CliSmokeScenario:
    scenario_id: str
    build_invocation: Callable[[str, Path], CliSmokeInvocation]


@dataclass(frozen=True, slots=True)
class CliRegressionScenario:
    scenario_id: str
    build_arguments: Callable[[str, Path], list[str]]
    expected_exit_code: int
    expected_stdout: str
    expected_stderr_substrings: tuple[str, ...]


def _smoke_scenarios() -> tuple[CliSmokeScenario, ...]:
    return (
        CliSmokeScenario(
            scenario_id="simulation_coarse_interval_json_nominal",
            build_invocation=_build_coarse_simulation_smoke_invocation,
        ),
        CliSmokeScenario(
            scenario_id="simulation_seeded_ticks_json_nominal",
            build_invocation=_build_simulation_smoke_invocation,
        ),
        CliSmokeScenario(
            scenario_id="seasonality_preprocess_timestamps_json_nominal",
            build_invocation=_build_seasonality_preprocess_smoke_invocation,
        ),
        CliSmokeScenario(
            scenario_id="seasonality_diagnostics_json_nominal",
            build_invocation=_build_seasonality_diagnostics_smoke_invocation,
        ),
        CliSmokeScenario(
            scenario_id="hourly_factors_build_contract_explicit_json_nominal",
            build_invocation=_build_hourly_factors_build_contract_smoke_invocation,
        ),
        CliSmokeScenario(
            scenario_id="price_json_nominal",
            build_invocation=_build_price_smoke_invocation,
        ),
        CliSmokeScenario(
            scenario_id="stylized_facts_horizon_comparison_disk_export_json_nominal",
            build_invocation=_build_stylized_facts_smoke_invocation,
        ),
        CliSmokeScenario(
            scenario_id="calibration_market_data_csv_known_news_default_disk",
            build_invocation=_build_market_data_smoke_invocation,
        ),
        CliSmokeScenario(
            scenario_id="calibration_market_data_parquet_option_surface_default_disk",
            build_invocation=_build_market_data_parquet_option_surface_smoke_invocation,
        ),
        CliSmokeScenario(
            scenario_id="calibration_staged_default_disk_override",
            build_invocation=_build_staged_disk_smoke_invocation,
        ),
        CliSmokeScenario(
            scenario_id="calibration_staged_explicit_memory_override",
            build_invocation=_build_staged_memory_smoke_invocation,
        ),
    )


def _regression_scenarios() -> tuple[CliRegressionScenario, ...]:
    return (
        CliRegressionScenario(
            scenario_id="simulation_missing_request_file_reports_request_json_path",
            build_arguments=_build_simulation_missing_request_arguments,
            expected_exit_code=int(cli_module.CliExitCode.VALIDATION_ERROR),
            expected_stdout="",
            expected_stderr_substrings=(
                "[cli.validation.simulate.seeded_ticks.invalid_input]",
                'field_path="request_json"',
                '"path": ',
            ),
        ),
        CliRegressionScenario(
            scenario_id="price_missing_strike_reports_required_option",
            build_arguments=_build_price_missing_strike_arguments,
            expected_exit_code=2,
            expected_stdout="",
            expected_stderr_substrings=(
                "Usage: janiform price [OPTIONS]",
                "Missing option '--strike'",
            ),
        ),
        CliRegressionScenario(
            scenario_id="calibration_market_data_known_news_without_timestamp_mapping_is_rejected",
            build_arguments=_build_market_data_inconsistent_known_news_arguments,
            expected_exit_code=int(cli_module.CliExitCode.VALIDATION_ERROR),
            expected_stdout="",
            expected_stderr_substrings=(
                "[calibration.validation.contract]",
                'field_path="column_mapping.known_news"',
                "csv or parquet",
            ),
        ),
        CliRegressionScenario(
            scenario_id="calibration_staged_disk_checkpoint_is_incompatible_with_memory_mode",
            build_arguments=_build_staged_memory_incompatible_checkpoint_arguments,
            expected_exit_code=int(cli_module.CliExitCode.VALIDATION_ERROR),
            expected_stdout="",
            expected_stderr_substrings=(
                "[cli.validation.fit.staged.invalid_input]",
                'field_path="disk_checkpoint_directory"',
                'message="--disk-checkpoint-directory requires --execution-mode disk"',
            ),
        ),
    )


def _build_simulation_smoke_invocation(calendar_registry_json: str, tmp_path: Path) -> CliSmokeInvocation:
    request = build_simulation_request(calendar_registry_json)
    request_path = write_json_path(tmp_path / "simulation_request.json", request.to_payload())
    calendar_registry_path = write_json_path(
        tmp_path / "calendar_registry.json",
        json.loads(calendar_registry_json),
    )

    return CliSmokeInvocation(
        command_arguments=[
            "simulate",
            "seeded-ticks",
            "--request-json",
            str(request_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
        ],
        validate_stdout=_validate_simulation_smoke_stdout,
    )


def _build_coarse_simulation_smoke_invocation(
    calendar_registry_json: str,
    tmp_path: Path,
) -> CliSmokeInvocation:
    request = build_coarse_simulation_request(calendar_registry_json)
    request_path = write_json_path(tmp_path / "coarse_simulation_request.json", request.to_payload())

    return CliSmokeInvocation(
        command_arguments=[
            "simulate",
            "coarse-interval",
            "--request-json",
            str(request_path),
        ],
        validate_stdout=_validate_coarse_simulation_smoke_stdout,
    )


def _build_price_smoke_invocation(_: str, tmp_path: Path) -> CliSmokeInvocation:
    model_path = _write_price_model(tmp_path, run_id="smoke-price-model")
    output_path = tmp_path / "pricing.json"

    return CliSmokeInvocation(
        command_arguments=[
            "price",
            "--model-path",
            str(model_path),
            "--valuation-time-utc",
            "2026-02-02T15:00:00Z",
            "--spot",
            "101.0",
            "--strike",
            "100.0",
            "--maturity-interval",
            "1.0",
            "--output-path",
            str(output_path),
            "--output",
            "json",
        ],
        validate_stdout=_validate_price_smoke_stdout,
    )


def _validate_coarse_simulation_smoke_stdout(stdout_text: str) -> None:
    payload = _parse_json_command_output(stdout_text)
    assert payload["schema_version"] == "simulation.coarse_interval.path.v1"
    assert payload["simulated_interval_count"] == 3
    assert len(payload["records"]) == 3


def _build_hourly_factors_build_contract_smoke_invocation(
    calendar_registry_json: str,
    tmp_path: Path,
) -> CliSmokeInvocation:
    calendar_registry_path = write_json_path(
        tmp_path / "calendar_registry.json",
        json.loads(calendar_registry_json),
    )
    seasonality_source_path = write_json_path(
        tmp_path / "hourly_factor_seasonality_source.json",
        {
            "hourly_factors": [1.0, 0.9, 1.1],
            "materialization_method": "shared_precomputed_hourly_s_h.v1",
        },
    )
    news_source_path = write_json_path(
        tmp_path / "hourly_factor_news_source.json",
        {
            "hourly_factors": [0.2, 0.0, 0.3],
            "materialization_method": "shared_precomputed_hourly_nu_h.v1",
        },
    )

    return CliSmokeInvocation(
        command_arguments=[
            "hourly-factors",
            "build-contract",
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--exchange-code",
            "XNYS",
            "--range-start-utc",
            "2026-02-02T14:00:00Z",
            "--range-end-utc",
            "2026-02-02T17:00:00Z",
            "--seasonality-explicit-source-json",
            str(seasonality_source_path),
            "--news-intensity-explicit-source-json",
            str(news_source_path),
        ],
        validate_stdout=_validate_hourly_factors_build_contract_smoke_stdout,
    )


def _build_seasonality_preprocess_smoke_invocation(
    calendar_registry_json: str,
    tmp_path: Path,
) -> CliSmokeInvocation:
    event_records_path = write_json_path(
        tmp_path / "timestamp_event_records.json",
        {
            "event_records": [
                {"timestamp": "2026-02-02T14:30:00Z"},
                {"timestamp": "2026-02-02T16:30:00Z"},
                {"timestamp": "2026-02-02T16:31:00Z"},
            ]
        },
    )
    calendar_registry_path = write_json_path(
        tmp_path / "calendar_registry.json",
        json.loads(calendar_registry_json),
    )

    def validate_stdout(stdout_text: str) -> None:
        payload = _parse_json_command_output(stdout_text)
        assert payload["schema_version"] == "seasonality.timestamp_event.preprocessed.v1"
        assert payload["exchange_code"] == "XNYS"
        assert payload["accepted_record_count"] == 3
        assert payload["missing_hour_bucket_count"] == 1
        assert payload["sparse_hour_bucket_count"] == 1

    return CliSmokeInvocation(
        command_arguments=[
            "seasonality",
            "preprocess-timestamps",
            "--event-records-json",
            str(event_records_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--exchange-code",
            "XNYS",
            "--session-filter",
            "all",
            "--sparse-hour-event-threshold",
            "2",
        ],
        validate_stdout=validate_stdout,
    )


def _build_seasonality_diagnostics_smoke_invocation(
    calendar_registry_json: str,
    tmp_path: Path,
) -> CliSmokeInvocation:
    event_records_path = write_json_path(
        tmp_path / "seasonality_diagnostics_event_records.json",
        {
            "event_records": [
                {"timestamp": "2026-02-02T14:30:00Z"},
                {"timestamp": "2026-02-02T15:30:00Z"},
                {"timestamp": "2026-02-03T14:45:00Z"},
                {"timestamp": "2026-02-04T14:30:00Z"},
                {"timestamp": "2026-02-05T15:15:00Z"},
                {"timestamp": "2026-02-06T14:30:00Z"},
            ]
        },
    )
    calendar_registry_path = write_json_path(
        tmp_path / "seasonality_diagnostics_calendar_registry.json",
        json.loads(calendar_registry_json),
    )

    def validate_stdout(stdout_text: str) -> None:
        payload = _parse_json_command_output(stdout_text)
        assert payload["schema_version"] == "seasonality.diagnostics.report.v1"
        assert payload["dataset_id"] == "seasonality-smoke"
        assert payload["intraday_component_summary"]["component_name"] == "intraday"
        assert payload["fit_quality_metrics"]["combined_factor_mean"] > 0.0

    return CliSmokeInvocation(
        command_arguments=[
            "seasonality",
            "diagnostics",
            "--dataset-id",
            "seasonality-smoke",
            "--event-records-json",
            str(event_records_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--exchange-code",
            "XNYS",
            "--session-filter",
            "all",
            "--intraday-minutes-per-bin",
            "30",
        ],
        validate_stdout=validate_stdout,
    )


def _build_stylized_facts_smoke_invocation(_: str, tmp_path: Path) -> CliSmokeInvocation:
    metric_catalog_path = write_json_path(
        tmp_path / "stylized_fact_metric_catalog.json",
        build_stylized_fact_metric_catalog(),
    )
    comparison_request_path = write_json_path(
        tmp_path / "stylized_fact_comparison_request.json",
        build_stylized_fact_horizon_comparison_request(),
    )
    export_directory = tmp_path / "stylized-facts-smoke-export"
    artifact_export_path = write_json_path(
        tmp_path / "stylized_fact_artifact_export.json",
        build_stylized_fact_artifact_export_config(
            export_directory=export_directory,
            file_stem="smoke_report",
        ),
    )

    def validate_stdout(stdout_text: str) -> None:
        payload = _parse_json_command_output(stdout_text)
        assert payload["schema_version"] == "validation.stylized_fact.horizon_comparison.report_bundle.v1"
        assert payload["comparison_report"]["metric_catalog_id"] == "stylized_fact_metric_catalog"
        assert payload["artifact_export"]["file_stem"] == "smoke_report"
        assert len(payload["artifact_export"]["exported_files"]) == 3

    return CliSmokeInvocation(
        command_arguments=[
            "stylized-facts",
            "horizon-comparison",
            "--metric-catalog-json",
            str(metric_catalog_path),
            "--comparison-request-json",
            str(comparison_request_path),
            "--artifact-export-json",
            str(artifact_export_path),
            "--execution-mode",
            "disk",
        ],
        validate_stdout=validate_stdout,
    )


def _build_market_data_smoke_invocation(calendar_registry_json: str, tmp_path: Path) -> CliSmokeInvocation:
    market_data_paths = write_market_data_input_files(
        tmp_path,
        calendar_registry_json=calendar_registry_json,
        include_known_news=True,
    )
    checkpoint_directory = tmp_path / "market-data-smoke-checkpoints"

    def validate_stdout(stdout_text: str) -> None:
        payload = _parse_json_command_output(stdout_text)
        assert payload["schema_version"] == cli_module.CLI_MARKET_DATA_SUMMARY_SCHEMA_VERSION
        assert payload["command"] == cli_module.CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT
        assert payload["input_files"]["known_news"]["file_format"] == "csv"
        assert payload["column_mapping"]["known_news"]["event_timestamp_column"] == "event_timestamp_utc"
        assert payload["output_emission_policy"]["execution_mode"] == "disk"
        assert payload["output_artifact_locations"]["disk_checkpoint_directory"] == str(checkpoint_directory)
        assert payload["diagnostics_summary"]["tick_record_count"] == 4
        assert payload["provenance_summary"]["tick_data_summary"]["file_format"] == "csv"

    return CliSmokeInvocation(
        command_arguments=[
            "fit",
            "plan",
            "--run-id",
            "cli-smoke-market-data",
            "--workflow-scope",
            "seasonality_only",
            "--tick-data-path",
            str(market_data_paths["tick_data"]),
            "--tick-data-format",
            "csv",
            "--tick-data-provenance-label",
            "smoke-ticks",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--exchange-code",
            "XNYS",
            "--session-definition-path",
            str(market_data_paths["session_definition"]),
            "--session-definition-format",
            "json",
            "--session-definition-provenance-label",
            "smoke-session-calendar",
            "--session-timezone",
            "America/New_York",
            "--known-news-path",
            str(market_data_paths["known_news"]),
            "--known-news-format",
            "csv",
            "--known-news-provenance-label",
            "smoke-known-news",
            "--known-news-timezone",
            "UTC",
            "--known-news-timestamp-column",
            "event_timestamp_utc",
            "--known-news-event-id-column",
            "event_code",
            "--timestamp-timezone",
            "UTC",
            "--disk-checkpoint-directory",
            str(checkpoint_directory),
        ],
        validate_stdout=validate_stdout,
    )


def _build_market_data_parquet_option_surface_smoke_invocation(
    calendar_registry_json: str,
    tmp_path: Path,
) -> CliSmokeInvocation:
    market_data_paths = write_market_data_input_files(
        tmp_path,
        calendar_registry_json=calendar_registry_json,
        include_known_news=True,
        include_option_surface=True,
        tick_data_format="parquet",
    )
    checkpoint_directory = tmp_path / "market-data-parquet-smoke-checkpoints"

    def validate_stdout(stdout_text: str) -> None:
        payload = _parse_json_command_output(stdout_text)
        assert payload["schema_version"] == cli_module.CLI_MARKET_DATA_SUMMARY_SCHEMA_VERSION
        assert payload["workflow_scope"] == "full_with_option_surface"
        assert payload["input_files"]["tick_data"]["file_format"] == "parquet"
        assert payload["input_files"]["option_surface"]["file_format"] == "parquet"
        assert payload["diagnostics_summary"]["option_surface_record_count"] == 1
        assert payload["provenance_summary"]["option_surface_summary"]["file_format"] == "parquet"
        assert payload["output_artifact_locations"]["disk_checkpoint_directory"] == str(checkpoint_directory)

    return CliSmokeInvocation(
        command_arguments=[
            "fit",
            "plan",
            "--run-id",
            "cli-smoke-market-data-parquet",
            "--workflow-scope",
            "full_with_option_surface",
            "--tick-data-path",
            str(market_data_paths["tick_data"]),
            "--tick-data-format",
            "parquet",
            "--tick-data-provenance-label",
            "smoke-parquet-ticks",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--tick-price-input-policy",
            "trade_price_column",
            "--tick-trade-price-column",
            "trade_price",
            "--tick-trade-sign-column",
            "trade_sign",
            "--exchange-code",
            "XNYS",
            "--session-definition-path",
            str(market_data_paths["session_definition"]),
            "--session-definition-format",
            "json",
            "--session-definition-provenance-label",
            "smoke-session-calendar",
            "--session-timezone",
            "America/New_York",
            "--known-news-path",
            str(market_data_paths["known_news"]),
            "--known-news-format",
            "csv",
            "--known-news-provenance-label",
            "smoke-known-news",
            "--known-news-timezone",
            "UTC",
            "--known-news-timestamp-column",
            "event_timestamp_utc",
            "--known-news-event-id-column",
            "event_code",
            "--option-surface-path",
            str(market_data_paths["option_surface"]),
            "--option-surface-format",
            "parquet",
            "--option-surface-provenance-label",
            "smoke-option-surface",
            "--option-quote-input-policy",
            "bid_ask_midpoint",
            "--option-quote-timestamp-column",
            "quote_timestamp_utc",
            "--option-expiry-column",
            "expiry_utc",
            "--option-strike-column",
            "strike",
            "--option-type-column",
            "option_type",
            "--option-bid-column",
            "bid",
            "--option-ask-column",
            "ask",
            "--timestamp-timezone",
            "UTC",
            "--disk-checkpoint-directory",
            str(checkpoint_directory),
        ],
        validate_stdout=validate_stdout,
    )


def _build_staged_disk_smoke_invocation(calendar_registry_json: str, tmp_path: Path) -> CliSmokeInvocation:
    execution_input_path = write_json_path(
        tmp_path / "staged_execution_input.json",
        build_valid_execution_input_model().to_payload(),
    )
    calendar_registry_path = write_json_path(
        tmp_path / "calendar_registry.json",
        json.loads(calendar_registry_json),
    )
    checkpoint_directory = tmp_path / "staged-disk-smoke-checkpoints"

    def validate_stdout(stdout_text: str) -> None:
        payload = _parse_json_command_output(stdout_text)
        assert payload["schema_version"] == "calibration.orchestration.staged_run.report.v1"
        assert payload["execution_mode"] == "disk"
        assert isinstance(payload["emitted_checkpoints"], list)

    return CliSmokeInvocation(
        command_arguments=[
            "fit",
            "staged",
            "--execution-input-json",
            str(execution_input_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--disk-checkpoint-directory",
            str(checkpoint_directory),
        ],
        validate_stdout=validate_stdout,
    )


def _build_staged_memory_smoke_invocation(calendar_registry_json: str, tmp_path: Path) -> CliSmokeInvocation:
    execution_input_path = write_json_path(
        tmp_path / "staged_memory_execution_input.json",
        build_valid_execution_input_model().to_payload(),
    )
    calendar_registry_path = write_json_path(
        tmp_path / "calendar_registry.json",
        json.loads(calendar_registry_json),
    )

    def validate_stdout(stdout_text: str) -> None:
        payload = _parse_json_command_output(stdout_text)
        assert payload["schema_version"] == "calibration.orchestration.staged_run.report.v1"
        assert payload["execution_mode"] == "memory"
        assert isinstance(payload["stage_reports"], list)

    return CliSmokeInvocation(
        command_arguments=[
            "fit",
            "staged",
            "--execution-input-json",
            str(execution_input_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--execution-mode",
            "memory",
        ],
        validate_stdout=validate_stdout,
    )


def _build_simulation_missing_request_arguments(calendar_registry_json: str, tmp_path: Path) -> list[str]:
    calendar_registry_path = write_json_path(
        tmp_path / "calendar_registry.json",
        json.loads(calendar_registry_json),
    )

    return [
        "simulate",
        "seeded-ticks",
        "--request-json",
        str(tmp_path / "missing_request.json"),
        "--calendar-registry-json",
        str(calendar_registry_path),
    ]


def _build_price_missing_strike_arguments(_: str, tmp_path: Path) -> list[str]:
    model_path = _write_price_model(tmp_path, run_id="missing-strike-price-model")
    return [
        "price",
        "--model-path",
        str(model_path),
        "--valuation-time-utc",
        "2026-02-02T15:00:00Z",
        "--spot",
        "101.0",
        "--maturity-interval",
        "1.0",
    ]


def _build_market_data_inconsistent_known_news_arguments(
    calendar_registry_json: str,
    tmp_path: Path,
) -> list[str]:
    market_data_paths = write_market_data_input_files(
        tmp_path,
        calendar_registry_json=calendar_registry_json,
        include_known_news=True,
    )

    return [
        "fit",
        "plan",
        "--run-id",
        "cli-regression-known-news-mapping",
        "--workflow-scope",
        "seasonality_only",
        "--tick-data-path",
        str(market_data_paths["tick_data"]),
        "--tick-data-format",
        "csv",
        "--tick-data-provenance-label",
        "regression-ticks",
        "--tick-timestamp-column",
        "timestamp_utc",
        "--exchange-code",
        "XNYS",
        "--session-definition-path",
        str(market_data_paths["session_definition"]),
        "--session-definition-format",
        "json",
        "--session-definition-provenance-label",
        "regression-session-calendar",
        "--session-timezone",
        "America/New_York",
        "--known-news-path",
        str(market_data_paths["known_news"]),
        "--known-news-format",
        "csv",
        "--known-news-provenance-label",
        "regression-known-news",
        "--known-news-timezone",
        "UTC",
        "--timestamp-timezone",
        "UTC",
        "--execution-mode",
        "memory",
    ]


def _build_staged_memory_incompatible_checkpoint_arguments(
    calendar_registry_json: str,
    tmp_path: Path,
) -> list[str]:
    execution_input_path = write_json_path(
        tmp_path / "staged_invalid_memory_execution_input.json",
        build_valid_execution_input_model().to_payload(),
    )
    calendar_registry_path = write_json_path(
        tmp_path / "calendar_registry.json",
        json.loads(calendar_registry_json),
    )

    return [
        "fit",
        "staged",
        "--execution-input-json",
        str(execution_input_path),
        "--calendar-registry-json",
        str(calendar_registry_path),
        "--execution-mode",
        "memory",
        "--disk-checkpoint-directory",
        str(tmp_path / "unexpected-checkpoints"),
    ]


def _validate_simulation_smoke_stdout(stdout_text: str) -> None:
    payload = _parse_json_command_output(stdout_text)
    assert payload["schema_version"] == "simulation.seeded_tick_event.path.v1"
    assert payload["metadata"]["path_record_schema_version"] == "simulation.seeded_tick_event.record.v1"
    assert isinstance(payload["fine_path"], list)
    assert isinstance(payload["event_tape"], list)


def _validate_price_smoke_stdout(stdout_text: str) -> None:
    payload = _parse_json_command_output(stdout_text)
    assert payload["schema_version"] == cli_module.CLI_PRICE_RESULT_SCHEMA_VERSION
    assert payload["response"]["price"] >= 0.0
    assert payload["response"]["diagnostics"]["forward_price"] > 0.0


def _write_price_model(tmp_path: Path, *, run_id: str) -> Path:
    monkeypatch = pytest.MonkeyPatch()
    try:
        monkeypatch.chdir(tmp_path)
        model = _build_full_workflow_model(
            tmp_path,
            monkeypatch,
            run_id=run_id,
        )
    finally:
        monkeypatch.undo()

    model_path = tmp_path / f"{run_id}.json"
    model.save(model_path)
    return model_path


def _validate_hourly_factors_build_contract_smoke_stdout(stdout_text: str) -> None:
    payload = _parse_json_command_output(stdout_text)
    assert payload["schema_version"] == "seasonality.hourly_factor.contract.v1"
    assert payload["s_h"] == [1.0, 0.9, 1.1]
    assert payload["nu_h"] == [0.2, 0.0, 0.3]


def _parse_json_command_output(stdout_text: str) -> dict[str, Any]:
    payload: Any = json.loads(stdout_text)
    assert isinstance(payload, dict)
    assert stdout_text == f"{json.dumps(payload, sort_keys=True)}\n"
    return payload


def _scenario_id(scenario: CliSmokeScenario | CliRegressionScenario) -> str:
    return scenario.scenario_id


def _scenario_failure_message(
    *,
    scenario_id: str,
    command_arguments: list[str],
    detail: str,
) -> str:
    return (
        f"scenario_id={scenario_id} "
        f"command=janiform "
        f"args={json.dumps(command_arguments)} "
        f"{detail}"
    )


CLI_SMOKE_SCENARIOS: tuple[CliSmokeScenario, ...] = _smoke_scenarios()
CLI_REGRESSION_SCENARIOS: tuple[CliRegressionScenario, ...] = _regression_scenarios()


@pytest.mark.parametrize("scenario", CLI_SMOKE_SCENARIOS, ids=_scenario_id)
def test_cli_smoke_matrix_covers_primary_workflow_paths(
    scenario: CliSmokeScenario,
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    invocation = scenario.build_invocation(calendar_registry_json, tmp_path)
    exit_code, stdout_text, stderr_text = invoke_cli(invocation.command_arguments)

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), _scenario_failure_message(
        scenario_id=scenario.scenario_id,
        command_arguments=invocation.command_arguments,
        detail=f"expected success exit code, observed exit_code={exit_code}, stderr={stderr_text!r}",
    )
    assert stderr_text == "", _scenario_failure_message(
        scenario_id=scenario.scenario_id,
        command_arguments=invocation.command_arguments,
        detail=f"expected empty stderr, observed stderr={stderr_text!r}",
    )

    invocation.validate_stdout(stdout_text)


@pytest.mark.parametrize("scenario", CLI_REGRESSION_SCENARIOS, ids=_scenario_id)
def test_cli_regression_matrix_preserves_exit_codes_and_key_argument_paths(
    scenario: CliRegressionScenario,
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    command_arguments = scenario.build_arguments(calendar_registry_json, tmp_path)
    exit_code, stdout_text, stderr_text = invoke_cli(command_arguments)

    assert exit_code == scenario.expected_exit_code, _scenario_failure_message(
        scenario_id=scenario.scenario_id,
        command_arguments=command_arguments,
        detail=f"expected exit_code={scenario.expected_exit_code}, observed exit_code={exit_code}",
    )
    assert stdout_text == scenario.expected_stdout, _scenario_failure_message(
        scenario_id=scenario.scenario_id,
        command_arguments=command_arguments,
        detail=f"expected stdout={scenario.expected_stdout!r}, observed stdout={stdout_text!r}",
    )

    for expected_stderr_substring in scenario.expected_stderr_substrings:
        assert expected_stderr_substring in stderr_text, _scenario_failure_message(
            scenario_id=scenario.scenario_id,
            command_arguments=command_arguments,
            detail=(
                "missing stderr substring "
                f"{expected_stderr_substring!r} in stderr={stderr_text!r}"
            ),
        )


def test_cli_smoke_matrix_remains_bounded_for_merge_time_execution() -> None:
    assert len(CLI_SMOKE_SCENARIOS) == 11
    assert len(CLI_REGRESSION_SCENARIOS) == 4
