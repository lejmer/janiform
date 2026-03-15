from collections.abc import Mapping
from pathlib import Path
from typing import Optional

import click

from .shared import _attach_runtime_hourly_factor_contract, _build_calendar_registry_from_document
from ..command_runtime import (
    JaniformCommand,
    command_config_option,
    invoke_command_handler,
    output_option,
    required_json_path_option,
)
from ..context import CliCommandContext
from .._constants import (
    CLI_SIMULATION_COARSE_INTERVAL_ENTRYPOINT,
    CLI_SIMULATION_COARSE_INTERVAL_INTERNAL_ERROR_ID,
    CLI_SIMULATION_COARSE_INTERVAL_VALIDATION_ERROR_ID,
    CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
    CLI_SIMULATION_SEEDED_TICKS_INTERNAL_ERROR_ID,
    CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
    CliExitCode,
    CliOutputMode,
)
from ..documents import _load_json_document_for_cli_field
from ..errors import _classify_workflow_command_error
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


def _run_simulation_coarse_interval_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    request_json: Path,
    **_,
) -> int:
    try:
        request_document = _load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=request_json,
            context="coarse simulation request",
            field_path="request_json",
            error_id=CLI_SIMULATION_COARSE_INTERVAL_VALIDATION_ERROR_ID,
            entrypoint=CLI_SIMULATION_COARSE_INTERVAL_ENTRYPOINT,
        )
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
    **_,
) -> int:
    try:
        request_document = _load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=request_json,
            context="simulation request",
            field_path="request_json",
            error_id=CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
        )
        calendar_registry_document = _load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=calendar_registry_json,
            context="calendar registry",
            field_path="calendar_registry_json",
            error_id=CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
        )
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


def _build_simulation_coarse_interval_command(
    *,
    environment: Optional[Mapping[str, str]] = None,
) -> click.Command:
    @click.command(
        "coarse-interval",
        cls=JaniformCommand,
        command_path=("simulate", "coarse-interval"),
        environment=environment,
        help=(
            "Run the explicit coarse-interval simulation workflow when you want the canonical "
            "coarse path artifact without replaying fine-scale seeded ticks."
        ),
        params=[
            command_config_option(),
            output_option(default_output_mode=CliOutputMode.JSON),
            required_json_path_option(
                "--request-json",
                contract_label="a coarse-interval simulation request JSON object",
                semantic_note=(
                    "Use coarse-interval simulation for direct Delta-spaced paths. Use "
                    "seeded-ticks when you need a priced fine path or tick replay."
                ),
            ),
        ],
    )
    @click.pass_context
    def coarse_interval(ctx: click.Context, **command_arguments) -> int:
        return invoke_command_handler(
            ctx,
            handler=_run_simulation_coarse_interval_command,
            command_path=("simulate", "coarse-interval"),
            command_arguments=dict(command_arguments),
        )

    return coarse_interval


def _build_simulation_seeded_ticks_command(
    *,
    environment: Optional[Mapping[str, str]] = None,
) -> click.Command:
    @click.command(
        "seeded-ticks",
        cls=JaniformCommand,
        command_path=("simulate", "seeded-ticks"),
        environment=environment,
        help=(
            "Run the explicit seeded-tick simulation workflow when you need the fine-scale "
            "priced path, event replay, or direct component realizations."
        ),
        params=[
            command_config_option(),
            output_option(default_output_mode=CliOutputMode.JSON),
            required_json_path_option(
                "--request-json",
                contract_label="a seeded-tick simulation request JSON object",
                semantic_note="The request summary must preserve open-jump versus closed-gap hourly factors.",
            ),
            required_json_path_option(
                "--calendar-registry-json",
                contract_label="a calendar registry JSON object used to rehydrate hourly factors",
            ),
        ],
    )
    @click.pass_context
    def seeded_ticks(ctx: click.Context, **command_arguments) -> int:
        return invoke_command_handler(
            ctx,
            handler=_run_simulation_seeded_ticks_command,
            command_path=("simulate", "seeded-ticks"),
            command_arguments=dict(command_arguments),
        )

    return seeded_ticks


__all__ = [
    "run_coarse_simulation",
    "run_seeded_tick_event_simulation",
]
