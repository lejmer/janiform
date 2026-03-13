import json
from pathlib import Path

import orjson
import pytest

import janiform.cli as cli_module
from examples._shared import (
    build_stylized_fact_artifact_export_config,
    build_stylized_fact_horizon_comparison_request,
    build_stylized_fact_metric_catalog,
)
from tests.cli_test_support import invoke_cli, normalize_help_output, parse_cli_command, render_help, write_json_path


def test_stylized_facts_cli_help_documents_report_inputs_and_export_config() -> None:
    help_output = normalize_help_output(render_help(["stylized-facts", "horizon-comparison"]))

    assert "--config-json PATH" in help_output
    assert "--output {human,json}" in help_output
    assert "--metric-catalog-json PATH" in help_output
    assert "--comparison-request-json PATH" in help_output
    assert "--artifact-export-json PATH" in help_output
    assert "--execution-mode {disk,memory}" in help_output
    assert "Default: memory." in help_output
    assert "artifact export is requested" in help_output


def test_stylized_facts_cli_parser_defaults_to_memory_mode_and_json_output() -> None:
    parsed_arguments = parse_cli_command(
        [
            "stylized-facts",
            "horizon-comparison",
            "--metric-catalog-json",
            "metric-catalog.json",
            "--comparison-request-json",
            "comparison-request.json",
        ]
    )

    assert parsed_arguments.params["output"] == "json"
    assert parsed_arguments.params["execution_mode"] == "memory"
    assert parsed_arguments.params["artifact_export_json"] is None


def test_stylized_facts_cli_parser_accepts_disk_mode_and_artifact_export_argument() -> None:
    parsed_arguments = parse_cli_command(
        [
            "stylized-facts",
            "horizon-comparison",
            "--metric-catalog-json",
            "metric-catalog.json",
            "--comparison-request-json",
            "comparison-request.json",
            "--execution-mode",
            "disk",
            "--artifact-export-json",
            "artifact-export.json",
        ]
    )

    assert parsed_arguments.params["execution_mode"] == "disk"
    assert Path(str(parsed_arguments.params["artifact_export_json"])) == Path("artifact-export.json")


def test_stylized_facts_cli_parser_rejects_invalid_execution_mode() -> None:
    exit_code, _, _ = invoke_cli(
        [
            "stylized-facts",
            "horizon-comparison",
            "--metric-catalog-json",
            "metric-catalog.json",
            "--comparison-request-json",
            "comparison-request.json",
            "--execution-mode",
            "invalid",
        ]
    )

    assert exit_code == 2


def test_stylized_facts_cli_generates_report_bundle_in_memory_mode(tmp_path: Path) -> None:
    metric_catalog_path = write_json_path(
        tmp_path / "stylized_fact_metric_catalog.json",
        build_stylized_fact_metric_catalog(),
    )
    comparison_request_path = write_json_path(
        tmp_path / "stylized_fact_comparison_request.json",
        build_stylized_fact_horizon_comparison_request(),
    )

    exit_code, stdout_text, stderr_text = invoke_cli(
        [
            "stylized-facts",
            "horizon-comparison",
            "--metric-catalog-json",
            str(metric_catalog_path),
            "--comparison-request-json",
            str(comparison_request_path),
            "--execution-mode",
            "memory",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert stderr_text == ""

    payload = orjson.loads(stdout_text)
    assert payload["schema_version"] == "validation.stylized_fact.horizon_comparison.report_bundle.v1"
    assert payload["artifact_export"] is None
    assert payload["comparison_report"]["metric_catalog_id"] == "stylized_fact_metric_catalog"
    assert payload["comparison_report"]["summary"]["total_run_count"] == 1
    assert len(payload["table_summary"]["rows"]) == 2


def test_stylized_facts_cli_generates_human_summary_and_disk_artifacts(tmp_path: Path) -> None:
    metric_catalog_path = write_json_path(
        tmp_path / "stylized_fact_metric_catalog.json",
        build_stylized_fact_metric_catalog(),
    )
    comparison_request_path = write_json_path(
        tmp_path / "stylized_fact_comparison_request.json",
        build_stylized_fact_horizon_comparison_request(),
    )
    export_directory = tmp_path / "stylized-facts-export"
    artifact_export_path = write_json_path(
        tmp_path / "stylized_fact_artifact_export.json",
        build_stylized_fact_artifact_export_config(
            export_directory=export_directory,
            file_stem="cli_disk_report",
        ),
    )

    exit_code, stdout_text, stderr_text = invoke_cli(
        [
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
            "--output",
            "human",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert stderr_text == ""
    assert "stylized-facts horizon-comparison completed:" in stdout_text
    assert "execution_mode=disk" in stdout_text
    assert "artifact_export:" in stdout_text

    expected_exported_files = (
        export_directory / "cli_disk_report.report_bundle.json",
        export_directory / "cli_disk_report.table_summary.json",
        export_directory / "cli_disk_report.plot_payload.json",
    )
    for exported_file in expected_exported_files:
        assert exported_file.exists()


def test_stylized_facts_cli_rejects_invalid_metric_catalog_payload(tmp_path: Path) -> None:
    invalid_metric_catalog = build_stylized_fact_metric_catalog()
    invalid_metric_catalog["schema_version"] = "validation.stylized_fact.metric_catalog.v2"
    metric_catalog_path = write_json_path(
        tmp_path / "invalid_stylized_fact_metric_catalog.json",
        invalid_metric_catalog,
    )
    comparison_request_path = write_json_path(
        tmp_path / "stylized_fact_comparison_request.json",
        build_stylized_fact_horizon_comparison_request(),
    )

    exit_code, stdout_text, stderr_text = invoke_cli(
        [
            "stylized-facts",
            "horizon-comparison",
            "--metric-catalog-json",
            str(metric_catalog_path),
            "--comparison-request-json",
            str(comparison_request_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert "[cli.validation.stylized_facts.horizon_comparison.invalid_input]" in stderr_text
    assert 'field_path="command_input"' in stderr_text
    assert "stylized fact report generation failed" in stderr_text
    assert "schema version" in stderr_text


def test_stylized_facts_cli_rejects_invalid_artifact_export_configuration(tmp_path: Path) -> None:
    metric_catalog_path = write_json_path(
        tmp_path / "stylized_fact_metric_catalog.json",
        build_stylized_fact_metric_catalog(),
    )
    comparison_request_path = write_json_path(
        tmp_path / "stylized_fact_comparison_request.json",
        build_stylized_fact_horizon_comparison_request(),
    )
    artifact_export_path = write_json_path(
        tmp_path / "invalid_stylized_fact_artifact_export.json",
        {"export_directory": str(tmp_path / "stylized-facts-export")},
    )

    exit_code, stdout_text, stderr_text = invoke_cli(
        [
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
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert "[cli.validation.stylized_facts.horizon_comparison.invalid_input]" in stderr_text
    assert 'field_path="command_input"' in stderr_text
    assert "artifact_export.file_stem is required" in stderr_text


def test_stylized_facts_cli_command_supports_command_config_defaults(tmp_path: Path) -> None:
    metric_catalog_path = write_json_path(
        tmp_path / "stylized_fact_metric_catalog.json",
        build_stylized_fact_metric_catalog(),
    )
    comparison_request_path = write_json_path(
        tmp_path / "stylized_fact_comparison_request.json",
        build_stylized_fact_horizon_comparison_request(),
    )
    config_path = write_json_path(
        tmp_path / "stylized_fact_command_config.json",
        {
            "schema_version": cli_module.CLI_COMMAND_CONFIG_SCHEMA_VERSION,
            "command": "stylized-facts.horizon-comparison",
            "arguments": {
                "metric-catalog-json": str(metric_catalog_path),
                "comparison-request-json": str(comparison_request_path),
                "execution-mode": "memory",
            },
        },
    )

    exit_code, stdout_text, stderr_text = invoke_cli(
        [
            "stylized-facts",
            "horizon-comparison",
            "--config-json",
            str(config_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text

    payload = json.loads(stdout_text)
    assert payload["comparison_report"]["metric_catalog_id"] == "stylized_fact_metric_catalog"
