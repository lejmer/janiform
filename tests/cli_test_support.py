import json
from contextlib import redirect_stderr, redirect_stdout
from datetime import UTC, datetime, timedelta
from io import StringIO
from pathlib import Path
from typing import Any, Optional

import orjson
import pyarrow as pa
from pyarrow import parquet

import janiform.cli.command_tree as cli_command_tree_module
from examples._shared import (
    build_coarse_simulation_request as build_example_coarse_simulation_request,
    build_pricing_interval_factors,
    build_pricing_optional_output_selection,
    build_pricing_request,
)
from janiform.calendar import CalendarRegistry
from janiform.hourly_factors import (
    HourlyNewsIntensityProviderExplicitSource,
    HourlySeasonalityProviderExplicitSource,
)
from janiform.pricing import (
    GammaSeverityMarkConfiguration,
    NigSharedPlusShiftConfiguration,
    VanillaOptionPricingExecutionInput,
)
from janiform.simulation import (
    CoarseSimulationRunRequest,
    CoupledGapMarkConfiguration,
    DoubleExponentialBaseShockConfiguration,
    HawkesExcitationState,
    HawkesKernelLoadings,
    HawkesKernelParameters,
    KnownNewsArrivalConfiguration,
    NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration,
    NewsProcessConfiguration,
    SeededTickSimulationRequest,
    SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
    load_hourly_factor_contract_for_simulation,
)


def build_pricing_execution_input() -> VanillaOptionPricingExecutionInput:
    return VanillaOptionPricingExecutionInput(
        request=build_pricing_request(),
        interval_factors=build_pricing_interval_factors(),
        optional_output_selection=build_pricing_optional_output_selection(),
    )


def build_simulation_request(calendar_registry_json: str) -> SeededTickSimulationRequest:
    calendar_registry = CalendarRegistry(calendar_registry_json)
    start_time_utc = datetime(2026, 2, 2, 14, tzinfo=UTC)

    hourly_factor_contract = load_hourly_factor_contract_for_simulation(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=start_time_utc,
        range_end_utc=start_time_utc + timedelta(hours=2),
        seasonality_source=HourlySeasonalityProviderExplicitSource(hourly_factors=(1.0, 0.9)),
        news_intensity_source=HourlyNewsIntensityProviderExplicitSource(hourly_factors=(0.2, 0.1)),
    )

    return SeededTickSimulationRequest.from_runtime_inputs(
        seed=17,
        start_time_utc=start_time_utc,
        horizon_interval_count=2,
        kernel_parameters=HawkesKernelParameters(
            lambda_bar=0.8,
            beta=1.6,
            alpha=HawkesKernelLoadings(
                plus_plus=0.2,
                plus_minus=0.1,
                minus_plus=0.1,
                minus_minus=0.25,
            ),
            rho=HawkesKernelLoadings(
                plus_plus=0.08,
                plus_minus=0.04,
                minus_plus=0.05,
                minus_minus=0.1,
            ),
        ),
        initial_excitation_state=HawkesExcitationState(
            y_plus_plus=0.0,
            y_plus_minus=0.0,
            y_minus_plus=0.0,
            y_minus_minus=0.0,
        ),
        nig_shared_plus_shift_configuration=NigSharedPlusShiftConfiguration(
            alpha=2.3,
            beta_0=-0.8,
            delta=0.7,
            mu=0.1,
            delta_beta=-0.2,
        ),
        news_process_configuration=NewsProcessConfiguration(
            schema_version="news.process.configuration.v1",
            arrivals=NewsArrivalConfiguration(
                known=KnownNewsArrivalConfiguration(
                    scheduled_event_timestamps_utc_nanos=(),
                ),
                surprise=SurpriseNewsArrivalConfiguration(
                    baseline_intensity=0.0,
                ),
            ),
            shared_mark_law=SharedNewsMarkLawConfiguration(
                gamma_severity_marks=GammaSeverityMarkConfiguration(
                    shape_k=1.5,
                    scale_theta=0.9,
                ),
                double_exponential_base_shock=DoubleExponentialBaseShockConfiguration(
                    probability_positive=0.4,
                    eta_plus=1.3,
                    eta_minus=1.1,
                ),
                coupled_gap_marks=CoupledGapMarkConfiguration(sigma_g=0.75),
            ),
            known_component_mark_scales=NewsComponentMarkScaleConfiguration(
                activity_coupling_scale=0.6,
                return_jump_scale=1.2,
            ),
            surprise_component_mark_scales=NewsComponentMarkScaleConfiguration(
                activity_coupling_scale=0.6,
                return_jump_scale=1.2,
            ),
        ),
        hourly_factor_contract=hourly_factor_contract,
    )


def build_coarse_simulation_request(calendar_registry_json: str) -> CoarseSimulationRunRequest:
    return build_example_coarse_simulation_request(CalendarRegistry(calendar_registry_json))


def invoke_cli(
    arguments: list[str],
    *,
    environment: Optional[dict[str, str]] = None,
) -> tuple[int, str, str]:
    stdout_buffer = StringIO()
    stderr_buffer = StringIO()

    try:
        with redirect_stdout(stdout_buffer), redirect_stderr(stderr_buffer):
            exit_code = cli_command_tree_module._invoke_application(
                arguments,
                environment={} if environment is None else environment,
            )
    except SystemExit as e:
        exit_code = int(e.code)

    return exit_code, stdout_buffer.getvalue(), stderr_buffer.getvalue()


def render_help(arguments: list[str]) -> str:
    exit_code, stdout_text, stderr_text = invoke_cli([*arguments, "--help"])
    if exit_code != 0:
        raise AssertionError(f"expected help to succeed, got {exit_code}: {stderr_text}")
    return stdout_text


def normalize_help_output(help_output: str) -> str:
    return " ".join(help_output.split())


def write_json_path(path: Path, payload: dict[str, Any]) -> Path:
    path.write_text(json.dumps(payload, sort_keys=True), encoding="utf-8")
    return path


def write_market_data_input_files(
    tmp_path: Path,
    *,
    calendar_registry_json: str,
    include_known_news: bool = False,
    include_option_surface: bool = False,
    ambiguous_tick_timestamp: bool = False,
    tick_price_column_name: str = "trade_price",
    tick_trade_sign_column_name: str = "trade_sign",
    tick_data_format: str = "csv",
) -> dict[str, Path]:
    data_directory = tmp_path / "data"
    config_directory = tmp_path / "config"

    data_directory.mkdir(parents=True, exist_ok=True)
    config_directory.mkdir(parents=True, exist_ok=True)

    tick_data_path = data_directory / f"ticks.{tick_data_format}"
    if tick_data_format == "csv":
        if ambiguous_tick_timestamp:
            tick_data_path.write_text(
                "\n".join(
                    [
                        "timestamp_utc",
                        "2026-11-01T01:30:00",
                    ]
                ),
                encoding="utf-8",
            )
        else:
            tick_data_path.write_text(
                "\n".join(
                    [
                        f"timestamp_utc,{tick_price_column_name},{tick_trade_sign_column_name}",
                        "2026-02-02T14:30:00Z,100.0,buy",
                        "2026-02-02T15:00:00Z,101.0,sell",
                        "2026-02-02T20:59:00Z,102.0,buy",
                        "2026-02-04T14:30:00Z,95.0,sell",
                    ]
                ),
                encoding="utf-8",
            )
    else:
        parquet.write_table(
            pa.table(
                {
                    "timestamp_utc": [
                        datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                        datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
                        datetime(2026, 2, 2, 20, 59, tzinfo=UTC),
                        datetime(2026, 2, 4, 14, 30, tzinfo=UTC),
                    ],
                    tick_price_column_name: [100.0, 101.0, 102.0, 95.0],
                    tick_trade_sign_column_name: ["buy", "sell", "buy", "sell"],
                }
            ),
            tick_data_path,
        )

    session_definition_path = config_directory / "sessions.json"
    session_definition_path.write_text(calendar_registry_json, encoding="utf-8")

    file_paths: dict[str, Path] = {
        "tick_data": tick_data_path,
        "session_definition": session_definition_path,
    }

    if include_known_news:
        known_news_path = config_directory / "known-news.csv"
        known_news_path.write_text(
            "\n".join(
                [
                    "event_timestamp_utc,event_code",
                    "2026-02-02T15:05:00Z,open_macro",
                    "2026-02-02T22:00:00Z,closed_macro",
                ]
            ),
            encoding="utf-8",
        )
        file_paths["known_news"] = known_news_path

    if include_option_surface:
        option_surface_path = data_directory / "options.parquet"
        parquet.write_table(
            pa.table(
                {
                    "quote_timestamp_utc": [datetime(2026, 2, 2, 20, 0, tzinfo=UTC)],
                    "expiry_utc": [datetime(2026, 3, 20, 20, 0, tzinfo=UTC)],
                    "strike": [100.0],
                    "option_type": ["C"],
                    "bid": [4.0],
                    "ask": [5.0],
                }
            ),
            option_surface_path,
        )
        file_paths["option_surface"] = option_surface_path

    return file_paths


__all__ = [
    "build_coarse_simulation_request",
    "build_pricing_execution_input",
    "build_simulation_request",
    "invoke_cli",
    "normalize_help_output",
    "render_help",
    "write_json_path",
    "write_market_data_input_files",
]
