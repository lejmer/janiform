import argparse
import json
from pathlib import Path

from janiform.calibration import CalibrationExecutionMode
from janiform.pricing import VanillaOptionPricingExecutionInput
from janiform.simulation import SeededTickSimulationRequest, load_hourly_factor_contract_for_simulation

from ._shared import (
    CALENDAR_REGISTRY_JSON,
    SIMULATION_RANGE_END_UTC,
    SIMULATION_RANGE_START_UTC,
    build_calibration_execution_input,
    build_calendar_registry,
    build_hawkes_kernel_parameters,
    build_initial_excitation_state,
    build_pricing_interval_factors,
    build_pricing_optional_output_selection,
    build_pricing_request,
    build_simulation_news_source,
    build_simulation_seasonality_source,
    write_market_data_fixture_files,
)


def write_cli_documentation_inputs(output_directory: Path) -> dict[str, str]:
    output_directory.mkdir(parents=True, exist_ok=True)
    market_data_paths = write_market_data_fixture_files(output_directory)

    calendar_registry_path = output_directory / "calendar_registry.json"
    calendar_registry_path.write_text(
        json.dumps(json.loads(CALENDAR_REGISTRY_JSON), indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    calendar_registry = build_calendar_registry()
    hourly_factor_contract = load_hourly_factor_contract_for_simulation(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=SIMULATION_RANGE_START_UTC,
        range_end_utc=SIMULATION_RANGE_END_UTC,
        seasonality_source=build_simulation_seasonality_source(),
        news_intensity_source=build_simulation_news_source(),
    )
    simulation_request = SeededTickSimulationRequest.from_runtime_inputs(
        seed=17,
        start_time_utc=SIMULATION_RANGE_START_UTC,
        horizon_interval_count=hourly_factor_contract.interval_count,
        kernel_parameters=build_hawkes_kernel_parameters(),
        initial_excitation_state=build_initial_excitation_state(),
        hourly_factor_contract=hourly_factor_contract,
    )
    simulation_request_path = output_directory / "simulation_request.json"
    simulation_request_path.write_text(
        json.dumps(simulation_request.to_payload(), indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    pricing_execution_input = VanillaOptionPricingExecutionInput(
        request=build_pricing_request(),
        interval_factors=build_pricing_interval_factors(),
        optional_output_selection=build_pricing_optional_output_selection(),
    )
    pricing_execution_input_path = output_directory / "pricing_execution_input.json"
    pricing_execution_input_path.write_text(
        json.dumps(pricing_execution_input.to_payload(), indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    calibration_execution_input = build_calibration_execution_input(
        execution_mode=CalibrationExecutionMode.MEMORY,
    )
    calibration_execution_input_path = output_directory / "calibration_execution_input.json"
    calibration_execution_input_path.write_text(
        json.dumps(calibration_execution_input.to_payload(), indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    output_paths: dict[str, str] = {
        "calendar_registry_json": str(calendar_registry_path),
        "calibration_execution_input_json": str(calibration_execution_input_path),
        "known_news_csv": str(market_data_paths["known_news"]),
        "option_surface_parquet": str(market_data_paths["option_surface"]),
        "pricing_execution_input_json": str(pricing_execution_input_path),
        "session_definition_json": str(market_data_paths["session_definition"]),
        "simulation_request_json": str(simulation_request_path),
        "tick_data_csv": str(market_data_paths["tick_csv"]),
        "tick_data_parquet": str(market_data_paths["tick_parquet"]),
    }
    return output_paths


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Write repo-local input files used by the CLI command-reference examples.",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        required=True,
        help="Directory where the example input files should be written.",
    )
    parsed_arguments = parser.parse_args()
    output_paths = write_cli_documentation_inputs(parsed_arguments.output_dir)

    print(json.dumps(output_paths, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
