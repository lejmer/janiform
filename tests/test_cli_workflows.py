import json
from pathlib import Path

import orjson
import pytest

import janiform.cli as cli_module
from tests.support.cli import invoke_cli, normalize_help_output, render_help
from tests.support.cli_documentation import write_cli_documentation_inputs
from tests.support.high_level_workflows import build_full_workflow_model

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]


def test_cli_help_surfaces_primary_commands_and_options() -> None:
    root_help = normalize_help_output(render_help([]))
    fit_help = normalize_help_output(render_help(["fit"]))
    simulate_help = normalize_help_output(render_help(["simulate"]))
    price_help = normalize_help_output(render_help(["price"]))
    presets_help = normalize_help_output(render_help(["presets"]))
    stylized_facts_help = normalize_help_output(render_help(["stylized-facts", "horizon-comparison"]))

    assert "Primary workflows are the fit, simulate, and price commands." in root_help
    assert "fit" in root_help
    assert "simulate" in root_help
    assert "price" in root_help
    assert "presets" in root_help

    assert "--market MARKET" in fit_help
    assert "--workflow-scope {seasonality_only,full_without_option_surface,full_with_option_surface}" in fit_help
    assert "--output-dir DIRECTORY" in fit_help
    assert "--model-path PATH" in simulate_help
    assert "--interval INTERVAL" in simulate_help
    assert "--valuation-time-utc VALUATION_TIME_UTC" in price_help
    assert "list" in presets_help
    assert "show" in presets_help
    assert "export" in presets_help
    assert "--metric-catalog-json PATH" in stylized_facts_help
    assert "--comparison-request-json PATH" in stylized_facts_help


def test_cli_primary_workflows_smoke(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.chdir(tmp_path)

    workflow_paths = write_cli_documentation_inputs(tmp_path / "cli-workflows")

    coarse_simulation_exit_code, coarse_simulation_stdout, coarse_simulation_stderr = invoke_cli(
        [
            "simulate",
            "coarse-interval",
            "--request-json",
            workflow_paths["coarse_simulation_request_json"],
            "--output",
            "json",
        ]
    )
    assert coarse_simulation_exit_code == int(cli_module.CliExitCode.SUCCESS), coarse_simulation_stderr
    coarse_simulation_payload = orjson.loads(coarse_simulation_stdout)
    assert coarse_simulation_payload["schema_version"] == "simulation.coarse_interval.path.v1"

    simulation_exit_code, simulation_stdout, simulation_stderr = invoke_cli(
        [
            "simulate",
            "seeded-ticks",
            "--request-json",
            workflow_paths["simulation_request_json"],
            "--calendar-registry-json",
            workflow_paths["calendar_registry_json"],
            "--output",
            "json",
        ]
    )
    assert simulation_exit_code == int(cli_module.CliExitCode.SUCCESS), simulation_stderr
    simulation_payload = orjson.loads(simulation_stdout)
    assert simulation_payload["schema_version"] == "simulation.seeded_tick_event.path.v1"

    seasonality_exit_code, seasonality_stdout, seasonality_stderr = invoke_cli(
        [
            "seasonality",
            "diagnostics",
            "--dataset-id",
            "cli-workflows-dataset",
            "--event-records-json",
            workflow_paths["timestamp_event_records_json"],
            "--calendar-registry-json",
            workflow_paths["calendar_registry_json"],
            "--exchange-code",
            "XNYS",
            "--session-filter",
            "all",
            "--output",
            "json",
        ]
    )
    assert seasonality_exit_code == int(cli_module.CliExitCode.SUCCESS), seasonality_stderr
    seasonality_payload = orjson.loads(seasonality_stdout)
    assert seasonality_payload["schema_version"] == "seasonality.diagnostics.report.v1"
    assert seasonality_payload["dataset_id"] == "cli-workflows-dataset"

    preprocess_exit_code, preprocess_stdout, preprocess_stderr = invoke_cli(
        [
            "seasonality",
            "preprocess-timestamps",
            "--event-records-json",
            workflow_paths["timestamp_event_records_json"],
            "--calendar-registry-json",
            workflow_paths["calendar_registry_json"],
            "--exchange-code",
            "XNYS",
            "--session-filter",
            "all",
            "--output",
            "json",
        ]
    )
    assert preprocess_exit_code == int(cli_module.CliExitCode.SUCCESS), preprocess_stderr
    preprocess_payload = orjson.loads(preprocess_stdout)
    assert preprocess_payload["schema_version"] == "seasonality.timestamp_event.preprocessed.v1"
    assert preprocess_payload["accepted_record_count"] == 3

    hourly_factor_seasonality_exit_code, hourly_factor_seasonality_stdout, hourly_factor_seasonality_stderr = invoke_cli(
        [
            "hourly-factors",
            "materialize-seasonality",
            "--calendar-registry-json",
            workflow_paths["calendar_registry_json"],
            "--exchange-code",
            "XNYS",
            "--range-start-utc",
            "2026-02-02T14:00:00Z",
            "--range-end-utc",
            "2026-02-02T17:00:00Z",
            "--materialization-input-json",
            workflow_paths["hourly_factor_seasonality_materialization_input_json"],
            "--output",
            "json",
        ]
    )
    assert hourly_factor_seasonality_exit_code == int(cli_module.CliExitCode.SUCCESS), hourly_factor_seasonality_stderr
    hourly_factor_seasonality_payload = orjson.loads(hourly_factor_seasonality_stdout)
    assert hourly_factor_seasonality_payload["schema_version"] == "seasonality.hourly_s_h.materialization.v1"

    hourly_factor_news_exit_code, hourly_factor_news_stdout, hourly_factor_news_stderr = invoke_cli(
        [
            "hourly-factors",
            "materialize-news-intensity",
            "--calendar-registry-json",
            workflow_paths["calendar_registry_json"],
            "--exchange-code",
            "XNYS",
            "--range-start-utc",
            "2026-02-02T14:00:00Z",
            "--range-end-utc",
            "2026-02-02T17:00:00Z",
            "--materialization-input-json",
            workflow_paths["hourly_factor_news_materialization_input_json"],
            "--output",
            "json",
        ]
    )
    assert hourly_factor_news_exit_code == int(cli_module.CliExitCode.SUCCESS), hourly_factor_news_stderr
    hourly_factor_news_payload = orjson.loads(hourly_factor_news_stdout)
    assert hourly_factor_news_payload["schema_version"] == "seasonality.hourly_nu_h.materialization.v1"

    hourly_factor_contract_exit_code, hourly_factor_contract_stdout, hourly_factor_contract_stderr = invoke_cli(
        [
            "hourly-factors",
            "build-contract",
            "--calendar-registry-json",
            workflow_paths["calendar_registry_json"],
            "--exchange-code",
            "XNYS",
            "--range-start-utc",
            "2026-02-02T14:00:00Z",
            "--range-end-utc",
            "2026-02-02T17:00:00Z",
            "--seasonality-materialized-source-json",
            workflow_paths["hourly_factor_materialized_seasonality_json"],
            "--news-intensity-materialized-source-json",
            workflow_paths["hourly_factor_materialized_news_intensity_json"],
            "--output",
            "json",
        ]
    )
    assert hourly_factor_contract_exit_code == int(cli_module.CliExitCode.SUCCESS), hourly_factor_contract_stderr
    hourly_factor_contract_payload = orjson.loads(hourly_factor_contract_stdout)
    assert hourly_factor_contract_payload["schema_version"] == "seasonality.hourly_factor.contract.v1"

    nested_monkeypatch = pytest.MonkeyPatch()
    try:
        model = build_full_workflow_model(
            tmp_path,
            nested_monkeypatch,
            run_id="cli-workflows-model",
        )
        model_path = tmp_path / "cli-workflows-model.json"
        model.save(model_path)
    finally:
        nested_monkeypatch.undo()

    pricing_exit_code, pricing_stdout, pricing_stderr = invoke_cli(
        [
            "price",
            "--model-path",
            str(model_path),
            "--valuation-time-utc",
            "2026-02-02T15:00:00Z",
            "--spot",
            "100.0",
            "--strike",
            "105.0",
            "--maturity-interval",
            "1.0",
            "--output",
            "json",
        ]
    )
    assert pricing_exit_code == int(cli_module.CliExitCode.SUCCESS), pricing_stderr
    pricing_payload = orjson.loads(pricing_stdout)
    assert pricing_payload["schema_version"] == cli_module.CLI_PRICE_RESULT_SCHEMA_VERSION

    stylized_facts_memory_exit_code, stylized_facts_memory_stdout, stylized_facts_memory_stderr = invoke_cli(
        [
            "stylized-facts",
            "horizon-comparison",
            "--metric-catalog-json",
            workflow_paths["stylized_fact_metric_catalog_json"],
            "--comparison-request-json",
            workflow_paths["stylized_fact_comparison_request_json"],
            "--execution-mode",
            "memory",
            "--output",
            "json",
        ]
    )
    assert stylized_facts_memory_exit_code == int(cli_module.CliExitCode.SUCCESS), stylized_facts_memory_stderr
    stylized_facts_memory_payload = orjson.loads(stylized_facts_memory_stdout)
    assert stylized_facts_memory_payload["schema_version"] == "validation.stylized_fact.horizon_comparison.report_bundle.v1"
    assert stylized_facts_memory_payload["artifact_export"] is None

    stylized_facts_disk_exit_code, stylized_facts_disk_stdout, stylized_facts_disk_stderr = invoke_cli(
        [
            "stylized-facts",
            "horizon-comparison",
            "--metric-catalog-json",
            workflow_paths["stylized_fact_metric_catalog_json"],
            "--comparison-request-json",
            workflow_paths["stylized_fact_comparison_request_json"],
            "--artifact-export-json",
            workflow_paths["stylized_fact_artifact_export_json"],
            "--execution-mode",
            "disk",
            "--output",
            "json",
        ]
    )
    assert stylized_facts_disk_exit_code == int(cli_module.CliExitCode.SUCCESS), stylized_facts_disk_stderr
    stylized_facts_disk_payload = orjson.loads(stylized_facts_disk_stdout)
    assert stylized_facts_disk_payload["artifact_export"]["file_stem"] == "stylized_fact_horizon_report"
    assert len(stylized_facts_disk_payload["artifact_export"]["exported_files"]) == 3

    market_data_default_exit_code, market_data_default_stdout, market_data_default_stderr = invoke_cli(
        [
            "fit",
            "plan",
            "--run-id",
            "cli-market-data-default-disk",
            "--workflow-scope",
            "seasonality_only",
            "--tick-data-path",
            workflow_paths["tick_data_csv"],
            "--tick-data-format",
            "csv",
            "--tick-data-provenance-label",
            "primary-tick-feed",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--exchange-code",
            "XNYS",
            "--known-news-path",
            workflow_paths["known_news_csv"],
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
            str(tmp_path / "cli-workflows" / "checkpoints" / "market-data-default-disk"),
            "--output",
            "json",
        ]
    )
    assert market_data_default_exit_code == int(cli_module.CliExitCode.SUCCESS), market_data_default_stderr
    market_data_default_payload = orjson.loads(market_data_default_stdout)
    assert market_data_default_payload["command"] == cli_module.CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT
    assert market_data_default_payload["output_emission_policy"]["execution_mode"] == "disk"

    market_data_memory_exit_code, market_data_memory_stdout, market_data_memory_stderr = invoke_cli(
        [
            "fit",
            "plan",
            "--run-id",
            "cli-market-data-memory-option-surface",
            "--workflow-scope",
            "full_with_option_surface",
            "--tick-data-path",
            workflow_paths["tick_data_parquet"],
            "--tick-data-format",
            "parquet",
            "--tick-data-provenance-label",
            "primary-tick-feed",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--tick-price-input-policy",
            "explicit_price_column",
            "--tick-price-column",
            "trade_price",
            "--tick-trade-sign-column",
            "trade_sign",
            "--exchange-code",
            "XNYS",
            "--known-news-path",
            workflow_paths["known_news_csv"],
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
            workflow_paths["option_surface_parquet"],
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


def test_cli_staged_fit_workflow_smoke(tmp_path: Path) -> None:
    workflow_paths = write_cli_documentation_inputs(tmp_path / "cli-workflows")

    exit_code, stdout_text, stderr_text = invoke_cli(
        [
            "fit",
            "staged",
            "--execution-input-json",
            workflow_paths["calibration_execution_input_json"],
            "--calendar-registry-json",
            workflow_paths["calendar_registry_json"],
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


def test_cli_developer_regression_workflow_smoke() -> None:
    exit_code, stdout_text, stderr_text = invoke_cli(
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
