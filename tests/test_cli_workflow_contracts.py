import json
from contextlib import redirect_stderr, redirect_stdout
from datetime import UTC, datetime, timedelta
from io import StringIO
from pathlib import Path

import pytest

import janiform.calibration as calibration_module
import janiform.cli as cli_module
import janiform.pricing as pricing_module
import janiform.simulation as simulation_module
from janiform.calendar import CalendarRegistry
from janiform.calibration import (
    CalibrationExecutionMode,
    StagedCalibrationExecutionInput,
    StagedCalibrationResponse,
)
from janiform.hourly_factors import (
    HourlyNewsIntensityProviderExplicitSource,
    HourlySeasonalityProviderExplicitSource,
)
from janiform.pricing import VanillaOptionPricingExecutionInput
from janiform.simulation import (
    HawkesExcitationState,
    HawkesKernelLoadings,
    HawkesKernelParameters,
    SeededTickSimulationRequest,
    load_hourly_factor_contract_for_simulation,
)
from tests.calibration_test_support import build_valid_execution_input_model


def test_simulation_cli_help_documents_required_contract_inputs() -> None:
    help_output = _render_help(["simulation", "seeded-ticks"])

    assert "--request-json" in help_output
    assert "--calendar-registry-json" in help_output


def test_pricing_cli_help_documents_execution_input_contract() -> None:
    help_output = _render_help(["pricing", "vanilla-option"])

    assert "--execution-input-json" in help_output


def test_calibration_cli_help_documents_mode_and_runtime_arguments() -> None:
    help_output = _render_help(["calibration", "staged"])

    assert "--execution-input-json" in help_output
    assert "--calendar-registry-json" in help_output
    assert "--execution-mode {disk,memory}" in help_output
    assert "--disk-checkpoint-directory" in help_output


def test_simulation_cli_maps_request_json_to_runtime_request(
    calendar_registry_json: str,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    request = _build_simulation_request(calendar_registry_json)
    captured_request: dict[str, SeededTickSimulationRequest] = {}

    def capturing_runner(request_model: SeededTickSimulationRequest) -> simulation_module.SeededTickSimulationResponse:
        captured_request["request"] = request_model
        return simulation_module.run_seeded_tick_event_simulation(request_model)

    monkeypatch.setattr(cli_module, "run_seeded_tick_event_simulation", capturing_runner)

    request_path = _write_json_path(tmp_path / "simulation_request.json", request.to_payload())
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar_registry.json",
        json.loads(calendar_registry_json),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "simulation",
            "seeded-ticks",
            "--request-json",
            str(request_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
        ]
    )

    assert exit_code == 0, stderr_text
    assert captured_request["request"].hourly_factor_summary == request.hourly_factor_summary
    assert captured_request["request"].hourly_factor_contract.interval_count == request.horizon_interval_count

    command_output = json.loads(stdout_text)
    assert command_output["schema_version"] == "simulation.seeded_tick_event.path.v1"
    assert "event_tape" in command_output


def test_pricing_cli_maps_execution_input_json_to_runtime_contract(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    execution_input = _build_pricing_execution_input()
    captured_execution_input: dict[str, VanillaOptionPricingExecutionInput] = {}

    def capturing_runner(
        execution_input_model: VanillaOptionPricingExecutionInput,
    ) -> pricing_module.VanillaOptionPricingRunResult:
        captured_execution_input["execution_input"] = execution_input_model
        return pricing_module.run_vanilla_option_pricing(execution_input_model)

    monkeypatch.setattr(cli_module, "run_vanilla_option_pricing", capturing_runner)

    execution_input_path = _write_json_path(
        tmp_path / "pricing_execution_input.json",
        execution_input.to_payload(),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "pricing",
            "vanilla-option",
            "--execution-input-json",
            str(execution_input_path),
        ]
    )

    assert exit_code == 0, stderr_text
    assert captured_execution_input["execution_input"].request.payoff.payoff_type.value == "call"
    assert captured_execution_input["execution_input"].interval_factors.nu_h_known == pytest.approx(0.01)
    assert captured_execution_input["execution_input"].interval_factors.nu_h_surprise == pytest.approx(0.02)

    command_output = json.loads(stdout_text)
    assert command_output["schema_version"] == "pricing.vanilla_option.run_result.v1"
    assert command_output["response"]["price"] >= 0.0


def test_calibration_cli_defaults_execution_mode_to_disk(
    calendar_registry_json: str,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    execution_input = build_valid_execution_input_model()
    captured_execution_input: dict[str, StagedCalibrationExecutionInput] = {}

    def capturing_runner(
        execution_input_model: StagedCalibrationExecutionInput,
        *,
        calendar_registry: CalendarRegistry,
    ) -> StagedCalibrationResponse:
        del calendar_registry
        captured_execution_input["execution_input"] = execution_input_model
        return calibration_module.run_staged_calibration(
            execution_input_model,
            calendar_registry=CalendarRegistry(calendar_registry_json),
        )

    monkeypatch.setattr(cli_module, "run_staged_calibration", capturing_runner)

    execution_input_path = _write_json_path(
        tmp_path / "calibration_execution_input.json",
        execution_input.to_payload(),
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar_registry.json",
        json.loads(calendar_registry_json),
    )
    checkpoint_directory = tmp_path / "disk-checkpoints"

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "staged",
            "--execution-input-json",
            str(execution_input_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--disk-checkpoint-directory",
            str(checkpoint_directory),
        ]
    )

    assert exit_code == 0, stderr_text

    normalized_execution_input = captured_execution_input["execution_input"]
    assert normalized_execution_input.request.execution_mode == CalibrationExecutionMode.DISK
    assert normalized_execution_input.runtime_resources is not None
    assert normalized_execution_input.runtime_resources.disk_checkpoint_directory == str(checkpoint_directory)

    command_output = json.loads(stdout_text)
    assert command_output["execution_mode"] == "disk"


def test_calibration_cli_honors_explicit_memory_override(
    calendar_registry_json: str,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    execution_input = build_valid_execution_input_model()
    captured_execution_input: dict[str, StagedCalibrationExecutionInput] = {}

    def capturing_runner(
        execution_input_model: StagedCalibrationExecutionInput,
        *,
        calendar_registry: CalendarRegistry,
    ) -> StagedCalibrationResponse:
        del calendar_registry
        captured_execution_input["execution_input"] = execution_input_model
        return calibration_module.run_staged_calibration(
            execution_input_model,
            calendar_registry=CalendarRegistry(calendar_registry_json),
        )

    monkeypatch.setattr(cli_module, "run_staged_calibration", capturing_runner)

    execution_input_path = _write_json_path(
        tmp_path / "calibration_execution_input.json",
        execution_input.to_payload(),
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar_registry.json",
        json.loads(calendar_registry_json),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "staged",
            "--execution-input-json",
            str(execution_input_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--execution-mode",
            "memory",
        ]
    )

    assert exit_code == 0, stderr_text

    normalized_execution_input = captured_execution_input["execution_input"]
    assert normalized_execution_input.request.execution_mode == CalibrationExecutionMode.MEMORY
    assert normalized_execution_input.runtime_resources is None

    command_output = json.loads(stdout_text)
    assert command_output["execution_mode"] == "memory"


def test_calibration_cli_requires_disk_checkpoint_directory_when_defaulting_to_disk(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    execution_input_path = _write_json_path(
        tmp_path / "calibration_execution_input.json",
        build_valid_execution_input_model().to_payload(),
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar_registry.json",
        json.loads(calendar_registry_json),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "staged",
            "--execution-input-json",
            str(execution_input_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
        ]
    )

    assert exit_code == 1
    assert stdout_text == ""

    assert 'field_path="runtime_resources.disk_checkpoint_directory"' in stderr_text
    assert 'message="is required when execution_mode=disk"' in stderr_text


def test_calibration_cli_rejects_disk_checkpoint_directory_in_memory_mode(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    execution_input_path = _write_json_path(
        tmp_path / "calibration_execution_input.json",
        build_valid_execution_input_model().to_payload(),
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar_registry.json",
        json.loads(calendar_registry_json),
    )
    checkpoint_directory = tmp_path / "unexpected-disk-checkpoints"

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "staged",
            "--execution-input-json",
            str(execution_input_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--execution-mode",
            "memory",
            "--disk-checkpoint-directory",
            str(checkpoint_directory),
        ]
    )

    assert exit_code == 1
    assert stdout_text == ""

    assert 'field_path="disk_checkpoint_directory"' in stderr_text
    assert 'message="--disk-checkpoint-directory requires --execution-mode disk"' in stderr_text


def test_calibration_cli_rejects_malformed_close_open_boundary_observation(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    execution_input_payload = build_valid_execution_input_model().to_payload()
    del execution_input_payload["return_components_candidate_generation_input"][
        "observed_close_open_boundary_observations"
    ][0]["open_price"]

    execution_input_path = _write_json_path(
        tmp_path / "calibration_execution_input.json",
        execution_input_payload,
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar_registry.json",
        json.loads(calendar_registry_json),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "staged",
            "--execution-input-json",
            str(execution_input_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--execution-mode",
            "memory",
        ]
    )

    assert exit_code == 1
    assert stdout_text == ""
    assert (
        'field_path="return_components_candidate_generation_input.observed_close_open_boundary_observations[0].open_price"'
        in stderr_text
    )
    assert 'message="is required"' in stderr_text


def _build_pricing_execution_input() -> VanillaOptionPricingExecutionInput:
    from examples._shared import (
        build_pricing_interval_factors,
        build_pricing_optional_output_selection,
        build_pricing_request,
    )

    return VanillaOptionPricingExecutionInput(
        request=build_pricing_request(),
        interval_factors=build_pricing_interval_factors(),
        optional_output_selection=build_pricing_optional_output_selection(),
    )


def _build_simulation_request(calendar_registry_json: str) -> SeededTickSimulationRequest:
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
        hourly_factor_contract=hourly_factor_contract,
    )


def _invoke_cli(arguments: list[str]) -> tuple[int, str, str]:
    parser = cli_module._build_parser()
    parsed_arguments = parser.parse_args(arguments)
    stdout_buffer = StringIO()
    stderr_buffer = StringIO()

    with redirect_stdout(stdout_buffer), redirect_stderr(stderr_buffer):
        exit_code = parsed_arguments.handler(parsed_arguments)

    return exit_code, stdout_buffer.getvalue(), stderr_buffer.getvalue()


def _render_help(arguments: list[str]) -> str:
    parser = cli_module._build_parser()
    stdout_buffer = StringIO()

    with pytest.raises(SystemExit) as captured_exit:
        with redirect_stdout(stdout_buffer):
            parser.parse_args([*arguments, "--help"])

    assert captured_exit.value.code == 0
    return stdout_buffer.getvalue()


def _write_json_path(path: Path, payload: dict[str, object]) -> Path:
    path.write_text(json.dumps(payload, sort_keys=True), encoding="utf-8")
    return path
