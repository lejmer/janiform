from pathlib import Path
from typing import cast

from ..context import CliCommandContext
from .._constants import (
    CLI_SIMULATION_COARSE_INTERVAL_ENTRYPOINT,
    CLI_SIMULATION_COARSE_INTERVAL_INTERNAL_ERROR_ID,
    CLI_SIMULATION_COARSE_INTERVAL_VALIDATION_ERROR_ID,
    CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
    CLI_SIMULATION_SEEDED_TICKS_INTERNAL_ERROR_ID,
    CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
    CliExitCode,
)
from ..documents import _load_json_document
from ..errors import _classify_workflow_command_error
from ..paths import _resolve_path_from_repository_root
from ..rendering import (
    _render_simulation_coarse_interval_human_output,
    _render_simulation_seeded_ticks_human_output,
    _write_cli_error,
    _write_command_output,
)
from ...errors import JaniformError
from ...simulation import (
    CoarseSimulationRunRequest,
    SeededTickSimulationRequest,
    run_coarse_simulation,
    run_seeded_tick_event_simulation,
)
from .shared import _attach_runtime_hourly_factor_contract, _build_calendar_registry_from_document


def _run_simulation_coarse_interval_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    request_json: Path,
    **_: object,
) -> int:
    request_path = _resolve_path_from_repository_root(cli_context.repository_root, request_json)

    try:
        request_document = _load_json_document(request_path, "coarse simulation request")
        request = CoarseSimulationRunRequest.from_mapping(request_document)
        response = run_coarse_simulation(request)
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_SIMULATION_COARSE_INTERVAL_ENTRYPOINT,
                validation_error_id=CLI_SIMULATION_COARSE_INTERVAL_VALIDATION_ERROR_ID,
                internal_error_id=CLI_SIMULATION_COARSE_INTERVAL_INTERNAL_ERROR_ID,
                file_field_path="request_json",
                document_field_path="request_document",
            )
        )

    payload = response.to_payload()
    _write_command_output(
        output,
        json_payload=payload,
        human_output_factory=lambda: _render_simulation_coarse_interval_human_output(payload),
    )
    return int(CliExitCode.SUCCESS)


def _run_simulation_seeded_ticks_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    request_json: Path,
    calendar_registry_json: Path,
    **_: object,
) -> int:
    request_path = _resolve_path_from_repository_root(cli_context.repository_root, request_json)
    calendar_registry_path = _resolve_path_from_repository_root(
        cli_context.repository_root,
        calendar_registry_json,
    )

    try:
        request_document = _load_json_document(request_path, "simulation request")
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        request = SeededTickSimulationRequest.from_mapping(request_document)
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        request_with_runtime_contract = _attach_runtime_hourly_factor_contract(
            request=request,
            calendar_registry=calendar_registry,
        )
        response = run_seeded_tick_event_simulation(request_with_runtime_contract)
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
                validation_error_id=CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
                internal_error_id=CLI_SIMULATION_SEEDED_TICKS_INTERNAL_ERROR_ID,
                file_field_path="request_json",
                document_field_path="request_document",
            )
        )

    payload = response.to_payload()
    _write_command_output(
        output,
        json_payload=payload,
        human_output_factory=lambda: _render_simulation_seeded_ticks_human_output(payload),
    )
    return int(CliExitCode.SUCCESS)


__all__ = [
    "_run_simulation_coarse_interval_command",
    "_run_simulation_seeded_ticks_command",
    "run_coarse_simulation",
    "run_seeded_tick_event_simulation",
]
