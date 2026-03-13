import json
from datetime import UTC, datetime
from pathlib import Path
from typing import cast

import orjson
import pytest

import janiform.cli as cli_module
import janiform.cli.command_tree as cli_command_tree_module
from janiform.hourly_factors import (
    HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
    HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
    HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
)
from tests.cli_test_support import invoke_cli, normalize_help_output, render_help, write_json_path


def test_hourly_factors_cli_help_documents_materialization_and_contract_inputs() -> None:
    namespace_help = normalize_help_output(render_help(["hourly-factors"]))
    contract_help = normalize_help_output(render_help(["hourly-factors", "build-contract"]))

    assert "materialize-seasonality" in namespace_help
    assert "materialize-news-intensity" in namespace_help
    assert "build-contract" in namespace_help
    assert "--config-json PATH" in contract_help
    assert "--calendar-registry-json PATH" in contract_help
    assert "--range-start-utc RANGE_START_UTC" in contract_help
    assert "--range-end-utc RANGE_END_UTC" in contract_help
    assert "--seasonality-explicit-source-json PATH" in contract_help
    assert "--seasonality-materialized-source-json PATH" in contract_help
    assert "--news-intensity-explicit-source-json PATH" in contract_help
    assert "--news-intensity-materialized-source-json PATH" in contract_help


def test_hourly_factor_materialization_cli_parser_defaults_to_json_output() -> None:
    parser = cli_command_tree_module._build_parser()
    parsed_arguments = parser.parse_args(
        [
            "hourly-factors",
            "materialize-seasonality",
            "--calendar-registry-json",
            "calendar-registry.json",
            "--exchange-code",
            "XNYS",
            "--range-start-utc",
            "2026-02-02T13:00:00Z",
            "--range-end-utc",
            "2026-02-02T18:00:00Z",
            "--materialization-input-json",
            "seasonality-input.json",
        ]
    )

    assert parsed_arguments.output == "json"


def test_hourly_factor_build_contract_cli_parser_accepts_explicit_source_arguments() -> None:
    parser = cli_command_tree_module._build_parser()
    parsed_arguments = parser.parse_args(
        [
            "hourly-factors",
            "build-contract",
            "--calendar-registry-json",
            "calendar-registry.json",
            "--exchange-code",
            "XNYS",
            "--range-start-utc",
            "2026-02-02T14:00:00Z",
            "--range-end-utc",
            "2026-02-02T17:00:00Z",
            "--seasonality-explicit-source-json",
            "seasonality-explicit.json",
            "--news-intensity-explicit-source-json",
            "news-explicit.json",
        ]
    )

    assert Path(parsed_arguments.seasonality_explicit_source_json) == Path("seasonality-explicit.json")
    assert Path(parsed_arguments.news_intensity_explicit_source_json) == Path("news-explicit.json")


def test_hourly_factor_build_contract_cli_parser_accepts_materialized_source_arguments() -> None:
    parser = cli_command_tree_module._build_parser()
    parsed_arguments = parser.parse_args(
        [
            "hourly-factors",
            "build-contract",
            "--calendar-registry-json",
            "calendar-registry.json",
            "--exchange-code",
            "XNYS",
            "--range-start-utc",
            "2026-02-02T14:00:00Z",
            "--range-end-utc",
            "2026-02-02T17:00:00Z",
            "--seasonality-materialized-source-json",
            "seasonality-materialized.json",
            "--news-intensity-materialized-source-json",
            "news-materialized.json",
        ]
    )

    assert Path(parsed_arguments.seasonality_materialized_source_json) == Path("seasonality-materialized.json")
    assert Path(parsed_arguments.news_intensity_materialized_source_json) == Path("news-materialized.json")


def test_hourly_factor_build_contract_cli_parser_requires_both_source_groups() -> None:
    parser = cli_command_tree_module._build_parser()

    with pytest.raises(SystemExit) as captured_exit:
        parser.parse_args(
            [
                "hourly-factors",
                "build-contract",
                "--calendar-registry-json",
                "calendar-registry.json",
                "--exchange-code",
                "XNYS",
                "--range-start-utc",
                "2026-02-02T14:00:00Z",
                "--range-end-utc",
                "2026-02-02T17:00:00Z",
                "--seasonality-explicit-source-json",
                "seasonality-explicit.json",
            ]
        )

    assert captured_exit.value.code == 2


def test_hourly_factor_materialize_seasonality_cli_generates_canonical_report(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    calendar_registry_path = write_json_path(
        tmp_path / "calendar_registry.json",
        cast(dict[str, object], orjson.loads(calendar_registry_json)),
    )
    materialization_input_path = write_json_path(
        tmp_path / "seasonality_materialization_input.json",
        {
            "source_timestamps_utc": [
                "2026-02-02T14:05:00Z",
                "2026-02-02T14:20:00Z",
                "2026-02-02T15:10:00Z",
                "2026-02-02T17:15:00Z",
            ],
            "source_seasonality_factors": [1.2, 1.0, 0.8, 1.4],
            "interpolation_method": "previous_observation_carry_forward.v1",
            "aggregation_method": "mean_of_observations.v1",
            "sparse_fallback_policy": "previous_materialized_then_global_mean.v1",
            "sparse_hour_minimum_observation_count": 2,
        },
    )

    exit_code, stdout_text, stderr_text = invoke_cli(
        [
            "hourly-factors",
            "materialize-seasonality",
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--exchange-code",
            "XNYS",
            "--range-start-utc",
            "2026-02-02T13:00:00Z",
            "--range-end-utc",
            "2026-02-02T18:00:00Z",
            "--materialization-input-json",
            str(materialization_input_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert stderr_text == ""

    payload = orjson.loads(stdout_text)
    assert payload["schema_version"] == HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION
    assert payload["s_h"] == [1.1, 1.1, 0.8, 0.8, 1.4]
    assert payload["interval_count"] == 5
    assert payload["sparse_interval_count"] == 4
    assert payload["fallback_interval_count"] == 1
    assert stdout_text == f"{json.dumps(payload, sort_keys=True)}\n"


def test_hourly_factor_materialize_news_intensity_cli_generates_canonical_report(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    calendar_registry_path = write_json_path(
        tmp_path / "calendar_registry.json",
        cast(dict[str, object], orjson.loads(calendar_registry_json)),
    )
    materialization_input_path = write_json_path(
        tmp_path / "news_materialization_input.json",
        {
            "known_scheduled_event_timestamps_utc": [
                "2026-02-02T14:05:00Z",
                "2026-02-02T15:10:00Z",
            ],
            "surprise_baseline_intensity": 0.1,
            "surprise_deterministic_time_multiplier_by_interval_index": {
                "0": 1.0,
                "1": 2.0,
                "2": 1.0,
            },
        },
    )

    exit_code, stdout_text, stderr_text = invoke_cli(
        [
            "hourly-factors",
            "materialize-news-intensity",
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--exchange-code",
            "XNYS",
            "--range-start-utc",
            "2026-02-02T14:00:00Z",
            "--range-end-utc",
            "2026-02-02T17:00:00Z",
            "--materialization-input-json",
            str(materialization_input_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert stderr_text == ""

    payload = orjson.loads(stdout_text)
    assert payload["schema_version"] == HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION
    assert payload["known_component_nu_h"] == [1.0, 1.0, 0.0]
    assert payload["surprise_component_nu_h"] == [0.1, 0.2, 0.1]
    assert payload["nu_h"] == [1.1, 1.2, 0.1]
    assert payload["known_scheduled_arrival_count"] == 2
    assert stdout_text == f"{json.dumps(payload, sort_keys=True)}\n"


def test_hourly_factor_build_contract_cli_supports_materialized_source_artifacts(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    calendar_registry_path = write_json_path(
        tmp_path / "calendar_registry.json",
        cast(dict[str, object], orjson.loads(calendar_registry_json)),
    )
    seasonality_materialization_path = write_json_path(
        tmp_path / "seasonality_materialization.json",
        {
            "schema_version": HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
            "exchange_code": "XNYS",
            "exchange_timezone_name": "America/New_York",
            "request_start_utc_nanos": _utc_nanos(2026, 2, 2, 14, 0, 0),
            "request_end_utc_nanos": _utc_nanos(2026, 2, 2, 17, 0, 0),
            "interval_indices": [0, 1, 2],
            "interval_start_utc_nanos": [],
            "interval_end_utc_nanos": [],
            "interval_start_exchange_local": [],
            "interval_end_exchange_local": [],
            "s_h": [1.0, 0.9, 1.1],
            "source_point_count": 3,
            "interval_count": 3,
            "sparse_interval_count": 0,
            "interpolated_interval_count": 0,
            "fallback_interval_count": 0,
            "skipped_local_hour_interval_count": 0,
            "repeated_local_hour_interval_count": 0,
            "interpolation_method": "previous_observation_carry_forward.v1",
            "aggregation_method": "mean_of_observations.v1",
            "sparse_fallback_policy": "global_mean.v1",
            "sparse_hour_minimum_observation_count": 1,
            "interval_boundary_policy": "start_inclusive_end_exclusive.v1",
            "interval_alignment_policy": "utc_hour_aligned.v1",
            "timezone_transition_policy": "utc_hour_bucket_projected_to_exchange_local.v1",
            "spring_forward_policy": "skip_nonexistent_local_hour_labels.v1",
            "fall_back_policy": "preserve_repeated_local_hour_labels_with_numeric_utc_offset.v1",
        },
    )
    news_materialization_path = write_json_path(
        tmp_path / "news_materialization.json",
        {
            "schema_version": HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
            "exchange_code": "XNYS",
            "exchange_timezone_name": "America/New_York",
            "request_start_utc_nanos": _utc_nanos(2026, 2, 2, 14, 0, 0),
            "request_end_utc_nanos": _utc_nanos(2026, 2, 2, 17, 0, 0),
            "interval_indices": [0, 1, 2],
            "interval_start_utc_nanos": [],
            "interval_end_utc_nanos": [],
            "interval_start_exchange_local": [],
            "interval_end_exchange_local": [],
            "known_component_nu_h": [1.0, 1.0, 0.0],
            "surprise_component_nu_h": [0.1, 0.2, 0.1],
            "nu_h": [1.1, 1.2, 0.1],
            "interval_count": 3,
            "known_component_non_zero_interval_count": 2,
            "surprise_component_non_zero_interval_count": 3,
            "combined_component_non_zero_interval_count": 3,
            "known_scheduled_arrival_count": 2,
            "surprise_multiplier_override_interval_count": 3,
            "skipped_local_hour_interval_count": 0,
            "repeated_local_hour_interval_count": 0,
            "materialization_method": "known_calendar_anchor_plus_surprise_background_controls.v1",
            "component_aggregation_policy": "known_plus_surprise_component_sum.v1",
            "arrival_intensity_policy": "known_arrivals_counted_per_hour_plus_surprise_baseline.v1",
            "known_news_calendar_anchor_policy": "scheduled_events_increment_hourly_count.v1",
            "surprise_news_intensity_control_policy": "interval_multiplier_applied_to_baseline.v1",
            "known_news_calendar_anchor_count": 2,
            "surprise_baseline_intensity": 0.1,
            "interval_boundary_policy": "start_inclusive_end_exclusive.v1",
            "interval_alignment_policy": "utc_hour_aligned.v1",
            "timezone_transition_policy": "utc_hour_bucket_projected_to_exchange_local.v1",
            "spring_forward_policy": "skip_nonexistent_local_hour_labels.v1",
            "fall_back_policy": "preserve_repeated_local_hour_labels_with_numeric_utc_offset.v1",
        },
    )

    exit_code, stdout_text, stderr_text = invoke_cli(
        [
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
            "--seasonality-materialized-source-json",
            str(seasonality_materialization_path),
            "--news-intensity-materialized-source-json",
            str(news_materialization_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert stderr_text == ""

    payload = orjson.loads(stdout_text)
    assert payload["schema_version"] == HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION
    assert payload["s_h"] == [1.0, 0.9, 1.1]
    assert payload["nu_h"] == [1.1, 1.2, 0.1]
    assert payload["seasonality_materialization_method"] == HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION
    assert payload["news_intensity_materialization_method"] == HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION


def test_hourly_factor_materialize_seasonality_cli_rejects_invalid_range(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    calendar_registry_path = write_json_path(
        tmp_path / "calendar_registry.json",
        cast(dict[str, object], orjson.loads(calendar_registry_json)),
    )
    materialization_input_path = write_json_path(
        tmp_path / "seasonality_materialization_input.json",
        {
            "source_timestamps_utc": ["2026-02-02T14:05:00Z"],
            "source_seasonality_factors": [1.0],
        },
    )

    exit_code, stdout_text, stderr_text = invoke_cli(
        [
            "hourly-factors",
            "materialize-seasonality",
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--exchange-code",
            "XNYS",
            "--range-start-utc",
            "2026-02-02T14:00:00Z",
            "--range-end-utc",
            "2026-02-02T14:00:00Z",
            "--materialization-input-json",
            str(materialization_input_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert "[cli.validation.hourly_factors.materialize_seasonality.invalid_input]" in stderr_text
    assert "range_start_utc must be earlier than range_end_utc" in stderr_text


def test_hourly_factor_materialize_seasonality_cli_rejects_malformed_timestamp_input(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    calendar_registry_path = write_json_path(
        tmp_path / "calendar_registry.json",
        cast(dict[str, object], orjson.loads(calendar_registry_json)),
    )
    materialization_input_path = write_json_path(
        tmp_path / "seasonality_materialization_input.json",
        {
            "source_timestamps_utc": ["not-a-timestamp"],
            "source_seasonality_factors": [1.0],
        },
    )

    exit_code, stdout_text, stderr_text = invoke_cli(
        [
            "hourly-factors",
            "materialize-seasonality",
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--exchange-code",
            "XNYS",
            "--range-start-utc",
            "2026-02-02T14:00:00Z",
            "--range-end-utc",
            "2026-02-02T15:00:00Z",
            "--materialization-input-json",
            str(materialization_input_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert "[cli.validation.hourly_factors.materialize_seasonality.invalid_input]" in stderr_text
    assert 'field_path="materialization_input_document"' in stderr_text
    assert "contains invalid timestamp" in stderr_text


def _utc_nanos(year: int, month: int, day: int, hour: int, minute: int, second: int) -> int:
    return int(datetime(year, month, day, hour, minute, second, tzinfo=UTC).timestamp() * 1_000_000_000)
