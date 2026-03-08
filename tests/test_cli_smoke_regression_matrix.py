import json
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pytest

import janiform.cli as cli_module
from tests.calibration_test_support import build_valid_execution_input_model
from tests.test_cli_workflow_contracts import (
    _build_pricing_execution_input,
    _build_simulation_request,
    _invoke_cli,
    _write_json_path,
    _write_market_data_input_files,
)


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
            scenario_id="simulation_seeded_ticks_json_nominal",
            build_invocation=_build_simulation_smoke_invocation,
        ),
        CliSmokeScenario(
            scenario_id="pricing_vanilla_option_json_nominal",
            build_invocation=_build_pricing_smoke_invocation,
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
                "[cli.validation.simulation.seeded_ticks.invalid_input]",
                'field_path="request_json"',
                '"path": ',
            ),
        ),
        CliRegressionScenario(
            scenario_id="pricing_missing_strike_reports_market_inputs_field_path",
            build_arguments=_build_pricing_missing_strike_arguments,
            expected_exit_code=int(cli_module.CliExitCode.VALIDATION_ERROR),
            expected_stdout="",
            expected_stderr_substrings=(
                "[pricing.validation.contract]",
                'field_path="market_inputs.strike"',
                'message="is required"',
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
                "[cli.validation.calibration.staged.invalid_input]",
                'field_path="disk_checkpoint_directory"',
                'message="--disk-checkpoint-directory requires --execution-mode disk"',
            ),
        ),
    )


def _build_simulation_smoke_invocation(calendar_registry_json: str, tmp_path: Path) -> CliSmokeInvocation:
    request = _build_simulation_request(calendar_registry_json)
    request_path = _write_json_path(tmp_path / "simulation_request.json", request.to_payload())
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar_registry.json",
        json.loads(calendar_registry_json),
    )

    return CliSmokeInvocation(
        command_arguments=[
            "simulation",
            "seeded-ticks",
            "--request-json",
            str(request_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
        ],
        validate_stdout=_validate_simulation_smoke_stdout,
    )


def _build_pricing_smoke_invocation(_: str, tmp_path: Path) -> CliSmokeInvocation:
    execution_input = _build_pricing_execution_input()
    execution_input_path = _write_json_path(
        tmp_path / "pricing_execution_input.json",
        execution_input.to_payload(),
    )

    return CliSmokeInvocation(
        command_arguments=[
            "pricing",
            "vanilla-option",
            "--execution-input-json",
            str(execution_input_path),
        ],
        validate_stdout=_validate_pricing_smoke_stdout,
    )


def _build_market_data_smoke_invocation(calendar_registry_json: str, tmp_path: Path) -> CliSmokeInvocation:
    market_data_paths = _write_market_data_input_files(
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
            "calibration",
            "market-data",
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
    market_data_paths = _write_market_data_input_files(
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
            "calibration",
            "market-data",
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
    execution_input_path = _write_json_path(
        tmp_path / "staged_execution_input.json",
        build_valid_execution_input_model().to_payload(),
    )
    calendar_registry_path = _write_json_path(
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
            "calibration",
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
    execution_input_path = _write_json_path(
        tmp_path / "staged_memory_execution_input.json",
        build_valid_execution_input_model().to_payload(),
    )
    calendar_registry_path = _write_json_path(
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
            "calibration",
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
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar_registry.json",
        json.loads(calendar_registry_json),
    )

    return [
        "simulation",
        "seeded-ticks",
        "--request-json",
        str(tmp_path / "missing_request.json"),
        "--calendar-registry-json",
        str(calendar_registry_path),
    ]


def _build_pricing_missing_strike_arguments(_: str, tmp_path: Path) -> list[str]:
    execution_input_payload = _build_pricing_execution_input().to_payload()
    del execution_input_payload["request"]["market_inputs"]["strike"]

    execution_input_path = _write_json_path(
        tmp_path / "pricing_missing_strike.json",
        execution_input_payload,
    )

    return [
        "pricing",
        "vanilla-option",
        "--execution-input-json",
        str(execution_input_path),
    ]


def _build_market_data_inconsistent_known_news_arguments(
    calendar_registry_json: str,
    tmp_path: Path,
) -> list[str]:
    market_data_paths = _write_market_data_input_files(
        tmp_path,
        calendar_registry_json=calendar_registry_json,
        include_known_news=True,
    )

    return [
        "calibration",
        "market-data",
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
    execution_input_path = _write_json_path(
        tmp_path / "staged_invalid_memory_execution_input.json",
        build_valid_execution_input_model().to_payload(),
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar_registry.json",
        json.loads(calendar_registry_json),
    )

    return [
        "calibration",
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
    assert payload["metadata"]["event_record_schema_version"] == "simulation.seeded_tick_event.record.v1"
    assert isinstance(payload["event_tape"], list)


def _validate_pricing_smoke_stdout(stdout_text: str) -> None:
    payload = _parse_json_command_output(stdout_text)
    assert payload["schema_version"] == "pricing.vanilla_option.run_result.v1"
    assert payload["response"]["price"] >= 0.0
    assert payload["response"]["diagnostics"]["forward_price"] > 0.0


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
    exit_code, stdout_text, stderr_text = _invoke_cli(invocation.command_arguments)

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
    exit_code, stdout_text, stderr_text = _invoke_cli(command_arguments)

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
    assert len(CLI_SMOKE_SCENARIOS) == 6
    assert len(CLI_REGRESSION_SCENARIOS) == 4
