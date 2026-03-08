import json
from pathlib import Path

import orjson

import janiform.cli as cli_module
from examples.cli_documentation_setup import write_cli_documentation_inputs
from tests.test_cli_workflow_contracts import _invoke_cli, _normalize_help_output, _render_help

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
CLI_COMMAND_REFERENCE_PATH = (
    REPOSITORY_ROOT / "docs" / "development" / "guides" / "cli_command_reference_and_examples.md"
)


def test_cli_command_reference_doc_contains_required_sections_and_commands() -> None:
    reference_document = _cli_command_reference_text()

    required_sections: list[str] = [
        "## Shared Setup",
        "## Shared Conventions",
        "## Simulation",
        "## Pricing",
        "## Market-Data Calibration",
        "## Advanced Staged Calibration",
        "## Developer Regression",
    ]
    for required_section in required_sections:
        assert required_section in reference_document

    required_commands: list[str] = [
        "janiform simulation seeded-ticks",
        "janiform pricing vanilla-option",
        "janiform calibration market-data",
        "janiform calibration staged",
        "janiform dev regression deterministic",
    ]
    for required_command in required_commands:
        assert required_command in reference_document


def test_cli_command_reference_doc_documents_execution_modes_and_market_data_inputs() -> None:
    reference_document = _cli_command_reference_text()
    normalized_document = " ".join(reference_document.split())

    assert "--execution-mode {disk,memory}" in reference_document
    assert "The CLI default is `disk` for calibration commands." in reference_document
    assert "--disk-checkpoint-directory" in reference_document
    assert "--known-news-path" in reference_document
    assert "--option-surface-path" in reference_document
    assert "advanced or internal calibration path" in normalized_document
    assert "tick-data-first" not in normalized_document
    assert "market-data-first" in normalized_document


def test_cli_command_reference_doc_stays_aligned_with_cli_help_output() -> None:
    reference_document = _cli_command_reference_text()
    simulation_help = _normalize_help_output(_render_help(["simulation", "seeded-ticks"]))
    pricing_help = _normalize_help_output(_render_help(["pricing", "vanilla-option"]))
    market_data_help = _normalize_help_output(_render_help(["calibration", "market-data"]))
    staged_help = _normalize_help_output(_render_help(["calibration", "staged"]))
    deterministic_help = _normalize_help_output(_render_help(["dev", "regression", "deterministic"]))

    shared_expectations: tuple[str, ...] = (
        "--output {human,json}",
        "--execution-mode {disk,memory}",
        "--workflow-scope {seasonality_only,full_without_option_surface,full_with_option_surface}",
        "--execution-input-json PATH",
        "--request-json PATH",
        "--calendar-registry-json PATH",
    )
    for expected_text in shared_expectations:
        assert expected_text in reference_document

    assert "--request-json PATH" in simulation_help
    assert "--calendar-registry-json PATH" in simulation_help
    assert "--execution-input-json PATH" in pricing_help
    assert "--workflow-scope {seasonality_only,full_without_option_surface,full_with_option_surface}" in market_data_help
    assert "--execution-mode {disk,memory}" in market_data_help
    assert "--execution-mode {disk,memory}" in staged_help
    assert "--calendar-registry-json PATH" in deterministic_help
    assert "--calendar-cases-json PATH" in deterministic_help
    assert "--seasonality-cases-json PATH" in deterministic_help


def test_cli_command_reference_doc_examples_smoke_for_primary_workflows(tmp_path: Path) -> None:
    documented_paths = write_cli_documentation_inputs(tmp_path / "cli-docs")

    simulation_exit_code, simulation_stdout, simulation_stderr = _invoke_cli(
        [
            "simulation",
            "seeded-ticks",
            "--request-json",
            documented_paths["simulation_request_json"],
            "--calendar-registry-json",
            documented_paths["calendar_registry_json"],
            "--output",
            "json",
        ]
    )
    assert simulation_exit_code == int(cli_module.CliExitCode.SUCCESS), simulation_stderr
    simulation_payload = orjson.loads(simulation_stdout)
    assert simulation_payload["schema_version"] == "simulation.seeded_tick_event.path.v1"

    pricing_exit_code, pricing_stdout, pricing_stderr = _invoke_cli(
        [
            "pricing",
            "vanilla-option",
            "--execution-input-json",
            documented_paths["pricing_execution_input_json"],
            "--output",
            "json",
        ]
    )
    assert pricing_exit_code == int(cli_module.CliExitCode.SUCCESS), pricing_stderr
    pricing_payload = orjson.loads(pricing_stdout)
    assert pricing_payload["schema_version"] == "pricing.vanilla_option.run_result.v1"

    market_data_default_exit_code, market_data_default_stdout, market_data_default_stderr = _invoke_cli(
        [
            "calibration",
            "market-data",
            "--run-id",
            "cli-market-data-default-disk",
            "--workflow-scope",
            "seasonality_only",
            "--tick-data-path",
            documented_paths["tick_data_csv"],
            "--tick-data-format",
            "csv",
            "--tick-data-provenance-label",
            "primary-tick-feed",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--exchange-code",
            "XNYS",
            "--session-definition-path",
            documented_paths["session_definition_json"],
            "--session-definition-format",
            "json",
            "--session-definition-provenance-label",
            "session-calendar",
            "--session-timezone",
            "America/New_York",
            "--known-news-path",
            documented_paths["known_news_csv"],
            "--known-news-format",
            "csv",
            "--known-news-provenance-label",
            "known-news-calendar",
            "--known-news-timezone",
            "UTC",
            "--known-news-timestamp-column",
            "event_timestamp_utc",
            "--known-news-event-id-column",
            "event_code",
            "--timestamp-timezone",
            "UTC",
            "--disk-checkpoint-directory",
            str(tmp_path / "cli-docs" / "checkpoints" / "market-data-default-disk"),
            "--output",
            "json",
        ]
    )
    assert market_data_default_exit_code == int(cli_module.CliExitCode.SUCCESS), market_data_default_stderr
    market_data_default_payload = orjson.loads(market_data_default_stdout)
    assert market_data_default_payload["command"] == cli_module.CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT
    assert market_data_default_payload["output_emission_policy"]["execution_mode"] == "disk"

    market_data_memory_exit_code, market_data_memory_stdout, market_data_memory_stderr = _invoke_cli(
        [
            "calibration",
            "market-data",
            "--run-id",
            "cli-market-data-memory-option-surface",
            "--workflow-scope",
            "full_with_option_surface",
            "--tick-data-path",
            documented_paths["tick_data_parquet"],
            "--tick-data-format",
            "parquet",
            "--tick-data-provenance-label",
            "primary-tick-feed",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--tick-price-input-policy",
            "trade_price_column",
            "--tick-trade-price-column",
            "trade_price",
            "--exchange-code",
            "XNYS",
            "--session-definition-path",
            documented_paths["session_definition_json"],
            "--session-definition-format",
            "json",
            "--session-definition-provenance-label",
            "session-calendar",
            "--session-timezone",
            "America/New_York",
            "--known-news-path",
            documented_paths["known_news_csv"],
            "--known-news-format",
            "csv",
            "--known-news-provenance-label",
            "known-news-calendar",
            "--known-news-timezone",
            "UTC",
            "--known-news-timestamp-column",
            "event_timestamp_utc",
            "--known-news-event-id-column",
            "event_code",
            "--option-surface-path",
            documented_paths["option_surface_parquet"],
            "--option-surface-format",
            "parquet",
            "--option-surface-provenance-label",
            "options-close",
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
            "--execution-mode",
            "memory",
            "--output",
            "json",
        ]
    )
    assert market_data_memory_exit_code == int(cli_module.CliExitCode.SUCCESS), market_data_memory_stderr
    market_data_memory_payload = orjson.loads(market_data_memory_stdout)
    assert market_data_memory_payload["workflow_scope"] == "full_with_option_surface"
    assert market_data_memory_payload["output_emission_policy"]["execution_mode"] == "memory"
    assert market_data_memory_payload["input_files"]["option_surface"]["file_format"] == "parquet"


def test_cli_command_reference_doc_examples_smoke_for_advanced_staged_workflow(tmp_path: Path) -> None:
    documented_paths = write_cli_documentation_inputs(tmp_path / "cli-docs")

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "staged",
            "--execution-input-json",
            documented_paths["calibration_execution_input_json"],
            "--calendar-registry-json",
            documented_paths["calendar_registry_json"],
            "--execution-mode",
            "memory",
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    payload = json.loads(stdout_text)
    assert payload["execution_mode"] == "memory"
    assert payload["status"] == "success"


def test_cli_command_reference_doc_examples_smoke_for_developer_regression_workflow() -> None:
    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "dev",
            "regression",
            "deterministic",
            "--calendar-registry-json",
            str(REPOSITORY_ROOT / "tests" / "fixtures" / "calendar" / "calendar_service_regression_matrix.json"),
            "--calendar-cases-json",
            str(REPOSITORY_ROOT / "tests" / "fixtures" / "calendar" / "calendar_service_regression_matrix.json"),
            "--seasonality-cases-json",
            str(REPOSITORY_ROOT / "tests" / "fixtures" / "hourly_factors" / "hourly_factor_regression_matrix.json"),
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    payload = json.loads(stdout_text)
    assert payload["schema_version"] == "validation.deterministic_regression_harness.report.v1"
    assert payload["summary"]["failed_case_count"] == 0


def _cli_command_reference_text() -> str:
    return CLI_COMMAND_REFERENCE_PATH.read_text(encoding="utf-8")
