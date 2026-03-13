import json
from pathlib import Path

import orjson

import janiform.cli as cli_module
from examples.cli_documentation_setup import write_cli_documentation_inputs
from tests.cli_test_support import invoke_cli, normalize_help_output, render_help

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
CLI_COMMAND_REFERENCE_PATH = (
    REPOSITORY_ROOT / "docs" / "development" / "guides" / "cli_command_reference_and_examples.md"
)


def test_cli_command_reference_doc_contains_required_sections_and_commands() -> None:
    reference_document = _cli_command_reference_text()

    required_sections: list[str] = [
        "## Shared Setup",
        "## Shared Conventions",
        "## Presets",
        "## High-Level Fit",
        "## Fit Plan",
        "## Fit Staged",
        "## High-Level Simulation",
        "## Explicit Simulation",
        "## Seasonality",
        "## Hourly Factors",
        "## Pricing",
        "## Stylized Facts",
        "## Developer Regression",
    ]
    for required_section in required_sections:
        assert required_section in reference_document

    required_commands: list[str] = [
        "janiform presets list",
        "janiform presets show",
        "janiform presets export",
        "janiform fit",
        "janiform fit plan",
        "janiform fit staged",
        "janiform simulate",
        "janiform simulate coarse-interval",
        "janiform simulate seeded-ticks",
        "janiform seasonality diagnostics",
        "janiform seasonality preprocess-timestamps",
        "janiform hourly-factors materialize-seasonality",
        "janiform hourly-factors materialize-news-intensity",
        "janiform hourly-factors build-contract",
        "janiform pricing vanilla-option",
        "janiform stylized-facts horizon-comparison",
        "janiform dev regression deterministic",
    ]
    for required_command in required_commands:
        assert required_command in reference_document


def test_cli_command_reference_doc_documents_execution_modes_and_market_data_inputs() -> None:
    reference_document = _cli_command_reference_text()
    normalized_document = " ".join(reference_document.split())

    assert "--execution-mode {disk,memory}" in reference_document
    assert "The CLI default is `disk` for `fit plan` and `fit staged`." in reference_document
    assert "The stylized-fact report command defaults to `memory`." in reference_document
    assert "--disk-checkpoint-directory" in reference_document
    assert "--artifact-export-json" in reference_document
    assert "--known-news-path" in reference_document
    assert "--option-surface-path" in reference_document
    assert "explicit fit workflow boundary" in normalized_document
    assert "tick-data-first" not in normalized_document
    assert "market-data-first" in normalized_document


def test_cli_command_reference_doc_stays_aligned_with_cli_help_output() -> None:
    reference_document = _cli_command_reference_text()
    coarse_simulation_help = normalize_help_output(render_help(["simulate", "coarse-interval"]))
    simulation_help = normalize_help_output(render_help(["simulate", "seeded-ticks"]))
    seasonality_diagnostics_help = normalize_help_output(render_help(["seasonality", "diagnostics"]))
    seasonality_help = normalize_help_output(render_help(["seasonality", "preprocess-timestamps"]))
    presets_list_help = normalize_help_output(render_help(["presets", "list"]))
    presets_show_help = normalize_help_output(render_help(["presets", "show"]))
    fit_help = normalize_help_output(render_help(["fit"]))
    simulate_help = normalize_help_output(render_help(["simulate"]))
    fit_plan_help = normalize_help_output(render_help(["fit", "plan"]))
    fit_staged_help = normalize_help_output(render_help(["fit", "staged"]))
    hourly_factors_seasonality_help = normalize_help_output(
        render_help(["hourly-factors", "materialize-seasonality"])
    )
    hourly_factors_contract_help = normalize_help_output(
        render_help(["hourly-factors", "build-contract"])
    )
    pricing_help = normalize_help_output(render_help(["pricing", "vanilla-option"]))
    stylized_facts_help = normalize_help_output(render_help(["stylized-facts", "horizon-comparison"]))
    deterministic_help = normalize_help_output(render_help(["dev", "regression", "deterministic"]))

    shared_expectations: tuple[str, ...] = (
        "--output {human,json}",
        "--execution-mode {disk,memory}",
        "--workflow-scope {seasonality_only,full_without_option_surface,full_with_option_surface}",
        "--execution-input-json PATH",
        "--request-json PATH",
        "--dataset-id DATASET_ID",
        "--event-records-json PATH",
        "--calendar-registry-json PATH",
        "--range-start-utc RANGE_START_UTC",
        "--range-end-utc RANGE_END_UTC",
        "--metric-catalog-json PATH",
        "--comparison-request-json PATH",
        "--market MARKET",
        "--session-preset SESSION_PRESET",
        "--known-news KNOWN_NEWS",
        "--session-json PATH",
        "--known-news-json PATH",
        "--model-output-path PATH",
        "--model-path PATH",
        "--simulation-output {coarse,ticks,both}",
    )
    for expected_text in shared_expectations:
        assert expected_text in reference_document

    assert "--category {session,known-news,market-profile}" in presets_list_help
    assert "{session,known-news,market-profile}" in presets_show_help
    assert "--market MARKET" in fit_help
    assert "--session-preset SESSION_PRESET" in fit_help
    assert "--known-news KNOWN_NEWS" in fit_help
    assert "--model-output-path PATH" in fit_help
    assert "--model-path PATH" in simulate_help
    assert "--simulation-output {coarse,ticks,both}" in simulate_help
    assert "--request-json PATH" in coarse_simulation_help
    assert "--request-json PATH" in simulation_help
    assert "--workflow-scope {seasonality_only,full_without_option_surface,full_with_option_surface}" in fit_plan_help
    assert "--execution-mode {disk,memory}" in fit_plan_help
    assert "--execution-mode {disk,memory}" in fit_staged_help
    assert "--dataset-id DATASET_ID" in seasonality_diagnostics_help
    assert "--intraday-smoothing-kernel {gaussian,uniform}" in seasonality_diagnostics_help
    assert "--calendar-registry-json PATH" in simulation_help
    assert "--event-records-json PATH" in seasonality_help
    assert "--session-filter {all,open_only,closed_only}" in seasonality_help
    assert "--materialization-input-json PATH" in hourly_factors_seasonality_help
    assert "--seasonality-explicit-source-json PATH" in hourly_factors_contract_help
    assert "--news-intensity-materialized-source-json PATH" in hourly_factors_contract_help
    assert "--execution-input-json PATH" in pricing_help
    assert "--metric-catalog-json PATH" in stylized_facts_help
    assert "--comparison-request-json PATH" in stylized_facts_help
    assert "--artifact-export-json PATH" in stylized_facts_help
    assert "--calendar-registry-json PATH" in deterministic_help
    assert "--calendar-cases-json PATH" in deterministic_help
    assert "--seasonality-cases-json PATH" in deterministic_help


def test_cli_command_reference_doc_examples_smoke_for_primary_workflows(tmp_path: Path) -> None:
    documented_paths = write_cli_documentation_inputs(tmp_path / "cli-docs")

    coarse_simulation_exit_code, coarse_simulation_stdout, coarse_simulation_stderr = invoke_cli(
        [
            "simulate",
            "coarse-interval",
            "--request-json",
            documented_paths["coarse_simulation_request_json"],
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

    seasonality_exit_code, seasonality_stdout, seasonality_stderr = invoke_cli(
        [
            "seasonality",
            "diagnostics",
            "--dataset-id",
            "cli-docs-dataset",
            "--event-records-json",
            documented_paths["timestamp_event_records_json"],
            "--calendar-registry-json",
            documented_paths["calendar_registry_json"],
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
    assert seasonality_payload["dataset_id"] == "cli-docs-dataset"

    preprocess_exit_code, preprocess_stdout, preprocess_stderr = invoke_cli(
        [
            "seasonality",
            "preprocess-timestamps",
            "--event-records-json",
            documented_paths["timestamp_event_records_json"],
            "--calendar-registry-json",
            documented_paths["calendar_registry_json"],
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
            documented_paths["calendar_registry_json"],
            "--exchange-code",
            "XNYS",
            "--range-start-utc",
            "2026-02-02T14:00:00Z",
            "--range-end-utc",
            "2026-02-02T17:00:00Z",
            "--materialization-input-json",
            documented_paths["hourly_factor_seasonality_materialization_input_json"],
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
            documented_paths["calendar_registry_json"],
            "--exchange-code",
            "XNYS",
            "--range-start-utc",
            "2026-02-02T14:00:00Z",
            "--range-end-utc",
            "2026-02-02T17:00:00Z",
            "--materialization-input-json",
            documented_paths["hourly_factor_news_materialization_input_json"],
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
            documented_paths["calendar_registry_json"],
            "--exchange-code",
            "XNYS",
            "--range-start-utc",
            "2026-02-02T14:00:00Z",
            "--range-end-utc",
            "2026-02-02T17:00:00Z",
            "--seasonality-materialized-source-json",
            documented_paths["hourly_factor_materialized_seasonality_json"],
            "--news-intensity-materialized-source-json",
            documented_paths["hourly_factor_materialized_news_intensity_json"],
            "--output",
            "json",
        ]
    )
    assert hourly_factor_contract_exit_code == int(cli_module.CliExitCode.SUCCESS), hourly_factor_contract_stderr
    hourly_factor_contract_payload = orjson.loads(hourly_factor_contract_stdout)
    assert hourly_factor_contract_payload["schema_version"] == "seasonality.hourly_factor.contract.v1"

    pricing_exit_code, pricing_stdout, pricing_stderr = invoke_cli(
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

    stylized_facts_memory_exit_code, stylized_facts_memory_stdout, stylized_facts_memory_stderr = invoke_cli(
        [
            "stylized-facts",
            "horizon-comparison",
            "--metric-catalog-json",
            documented_paths["stylized_fact_metric_catalog_json"],
            "--comparison-request-json",
            documented_paths["stylized_fact_comparison_request_json"],
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
            documented_paths["stylized_fact_metric_catalog_json"],
            "--comparison-request-json",
            documented_paths["stylized_fact_comparison_request_json"],
            "--artifact-export-json",
            documented_paths["stylized_fact_artifact_export_json"],
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
            documented_paths["tick_data_csv"],
            "--tick-data-format",
            "csv",
            "--tick-data-provenance-label",
            "primary-tick-feed",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--exchange-code",
            "XNYS",
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

    market_data_memory_exit_code, market_data_memory_stdout, market_data_memory_stderr = invoke_cli(
        [
            "fit",
            "plan",
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
            "--tick-trade-sign-column",
            "trade_sign",
            "--exchange-code",
            "XNYS",
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

    exit_code, stdout_text, stderr_text = invoke_cli(
        [
            "fit",
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


def _cli_command_reference_text() -> str:
    return CLI_COMMAND_REFERENCE_PATH.read_text(encoding="utf-8")
