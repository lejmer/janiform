from collections.abc import Callable, Mapping
from contextlib import AbstractContextManager
from pathlib import Path
from typing import Optional

import click

from .shared import attach_runtime_hourly_factor_contract, build_calendar_registry_from_document
from ..command_runtime import (
    JaniformCommand,
    command_config_option,
    invoke_command_handler,
    output_option,
    progress_option,
    required_json_path_option,
)
from ..context import CliCommandContext
from ..constants import (
    CLI_SIMULATION_COARSE_INTERVAL_ENTRYPOINT,
    CLI_SIMULATION_COARSE_INTERVAL_INTERNAL_ERROR_ID,
    CLI_SIMULATION_COARSE_INTERVAL_VALIDATION_ERROR_ID,
    CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
    CLI_SIMULATION_SEEDED_TICKS_INTERNAL_ERROR_ID,
    CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
    CliProgressMode,
    CliExitCode,
    CliOutputMode,
)
from ..documents import load_json_document_for_cli_field
from ..errors import classify_workflow_command_error
from ..rendering import (
    render_simulation_coarse_interval_human_output,
    render_simulation_seeded_ticks_human_output,
    write_cli_error,
    write_command_output,
)
from ...errors import JaniformError
from ..._workflow_progress import JaniformOutputController, NullProgress, WorkflowProgressMode
from ...simulation import (
    CoarseSimulationRunRequest,
    SeededTickSimulationRequest,
    SimulationProgressEvent,
    run_coarse_simulation,
    run_seeded_tick_event_simulation,
)


def build_simulation_output_controller(
    *,
    output: str,
    progress: str,
) -> JaniformOutputController:
    resolved_progress_mode = WorkflowProgressMode(progress)
    if resolved_progress_mode is WorkflowProgressMode.AUTO:
        resolved_progress_mode = (
            WorkflowProgressMode.HUMAN
            if output == CliOutputMode.HUMAN.value
            else WorkflowProgressMode.NONE
        )

    return JaniformOutputController(
        suppress_info_output=output == CliOutputMode.JSON.value,
        progress_mode=resolved_progress_mode,
        default_progress_mode=resolved_progress_mode,
    )


def simulation_progress_callback(
    progress_controller: JaniformOutputController,
) -> tuple[
    Optional[Callable[[SimulationProgressEvent], None]],
    AbstractContextManager[None],
]:
    progress = progress_controller.progress("simulate")
    if isinstance(progress, NullProgress):
        return None, progress

    def callback(event: SimulationProgressEvent) -> None:
        progress.update(
            detail=event.detail,
            completed_units=event.completed_units,
            total_units=event.total_units,
        )

    return callback, progress


def run_simulation_coarse_interval_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    progress: str,
    request_json: Path,
    **_,
) -> int:
    try:
        output_controller = build_simulation_output_controller(output=output, progress=progress)

        progress_callback, progress_scope = simulation_progress_callback(output_controller)

        request_document = load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=request_json,
            context="coarse simulation request",
            field_path="request_json",
            error_id=CLI_SIMULATION_COARSE_INTERVAL_VALIDATION_ERROR_ID,
            entrypoint=CLI_SIMULATION_COARSE_INTERVAL_ENTRYPOINT,
        )

        request = CoarseSimulationRunRequest.from_mapping(request_document)

        with progress_scope:
            if progress_callback is None:
                response = run_coarse_simulation(request)
            else:
                response = run_coarse_simulation(
                    request,
                    progress_callback=progress_callback,
                )
    except JaniformError as e:
        return write_cli_error(e)
    except Exception as e:
        return write_cli_error(
            classify_workflow_command_error(
                e,
                entrypoint=CLI_SIMULATION_COARSE_INTERVAL_ENTRYPOINT,
                validation_error_id=CLI_SIMULATION_COARSE_INTERVAL_VALIDATION_ERROR_ID,
                internal_error_id=CLI_SIMULATION_COARSE_INTERVAL_INTERNAL_ERROR_ID,
                file_field_path="request_json",
                document_field_path="request_document",
            )
        )

    payload = response.to_payload()

    write_command_output(
        output,
        json_payload=payload,
        human_output_factory=lambda: render_simulation_coarse_interval_human_output(payload),
    )

    return int(CliExitCode.SUCCESS)


def run_simulation_seeded_ticks_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    progress: str,
    request_json: Path,
    calendar_registry_json: Path,
    **_,
) -> int:
    try:
        output_controller = build_simulation_output_controller(output=output, progress=progress)

        progress_callback, progress_scope = simulation_progress_callback(output_controller)

        request_document = load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=request_json,
            context="simulation request",
            field_path="request_json",
            error_id=CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
        )

        calendar_registry_document = load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=calendar_registry_json,
            context="calendar registry",
            field_path="calendar_registry_json",
            error_id=CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
        )

        request = SeededTickSimulationRequest.from_mapping(request_document)

        calendar_registry = build_calendar_registry_from_document(calendar_registry_document)

        request_with_runtime_contract = attach_runtime_hourly_factor_contract(
            request=request,
            calendar_registry=calendar_registry,
        )

        with progress_scope:
            if progress_callback is None:
                response = run_seeded_tick_event_simulation(request_with_runtime_contract)
            else:
                response = run_seeded_tick_event_simulation(
                    request_with_runtime_contract,
                    progress_callback=progress_callback,
                )
    except JaniformError as e:
        return write_cli_error(e)
    except Exception as e:
        return write_cli_error(
            classify_workflow_command_error(
                e,
                entrypoint=CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
                validation_error_id=CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
                internal_error_id=CLI_SIMULATION_SEEDED_TICKS_INTERNAL_ERROR_ID,
                file_field_path="request_json",
                document_field_path="request_document",
            )
        )

    payload = response.to_payload()

    write_command_output(
        output,
        json_payload=payload,
        human_output_factory=lambda: render_simulation_seeded_ticks_human_output(payload),
    )

    return int(CliExitCode.SUCCESS)


def build_simulation_coarse_interval_command(
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
            progress_option(default_progress_mode=CliProgressMode.AUTO),
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
            handler=run_simulation_coarse_interval_command,
            command_path=("simulate", "coarse-interval"),
            command_arguments=dict(command_arguments),
        )

    return coarse_interval


def build_simulation_seeded_ticks_command(
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
            progress_option(default_progress_mode=CliProgressMode.AUTO),
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
            handler=run_simulation_seeded_ticks_command,
            command_path=("simulate", "seeded-ticks"),
            command_arguments=dict(command_arguments),
        )

    return seeded_ticks
