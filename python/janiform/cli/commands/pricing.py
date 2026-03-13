from collections.abc import Mapping
from pathlib import Path
from typing import cast

import click

from ..command_runtime import (
    JaniformCommand,
    JaniformGroup,
    command_config_option,
    group_help_callback,
    invoke_command_handler,
    output_option,
    required_json_path_option,
)
from ..context import CliCommandContext
from .._constants import (
    CLI_PRICING_VANILLA_OPTION_ENTRYPOINT,
    CLI_PRICING_VANILLA_OPTION_INTERNAL_ERROR_ID,
    CLI_PRICING_VANILLA_OPTION_VALIDATION_ERROR_ID,
    CliExitCode,
    CliOutputMode,
)
from ..documents import _load_json_document
from ..errors import _classify_workflow_command_error
from ..paths import _resolve_path_from_repository_root
from ..rendering import _render_pricing_vanilla_option_human_output, _write_cli_error, _write_command_output
from ...errors import JaniformError
from ...pricing import run_vanilla_option_pricing, validate_vanilla_option_pricing_execution_input


def _run_pricing_vanilla_option_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    execution_input_json: Path,
    **_: object,
) -> int:
    execution_input_path = _resolve_path_from_repository_root(
        cli_context.repository_root,
        execution_input_json,
    )

    try:
        execution_input_document = _load_json_document(execution_input_path, "pricing execution input")
        execution_input = validate_vanilla_option_pricing_execution_input(execution_input_document)
        run_result = run_vanilla_option_pricing(execution_input)
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_PRICING_VANILLA_OPTION_ENTRYPOINT,
                validation_error_id=CLI_PRICING_VANILLA_OPTION_VALIDATION_ERROR_ID,
                internal_error_id=CLI_PRICING_VANILLA_OPTION_INTERNAL_ERROR_ID,
                file_field_path="execution_input_json",
                document_field_path="execution_input_document",
            )
        )

    payload = run_result.to_payload()

    _write_command_output(
        output,
        json_payload=payload,
        human_output_factory=lambda: _render_pricing_vanilla_option_human_output(payload),
    )

    return int(CliExitCode.SUCCESS)


def _build_pricing_group(*, environment: Mapping[str, str] | None = None) -> click.Group:
    @click.group(
        cls=JaniformGroup,
        command_path=("pricing",),
        environment=environment,
        help="Pricing workflows.",
        invoke_without_command=True,
    )
    @click.pass_context
    def pricing(ctx: click.Context) -> None:
        group_help_callback(ctx)

    @pricing.command(
        "vanilla-option",
        cls=JaniformCommand,
        command_path=("pricing", "vanilla-option"),
        environment=environment,
        help="Vanilla option pricing workflow.",
        params=[
            command_config_option(),
            output_option(default_output_mode=CliOutputMode.JSON),
            required_json_path_option(
                "--execution-input-json",
                contract_label="a vanilla-option pricing execution-input JSON object",
                semantic_note="Interval factors keep known and surprise news intensity separate.",
            ),
        ],
    )
    @click.pass_context
    def vanilla_option(ctx: click.Context, **command_arguments: object) -> int:
        return invoke_command_handler(
            ctx,
            handler=_run_pricing_vanilla_option_command,
            command_path=("pricing", "vanilla-option"),
            command_arguments=dict(command_arguments),
        )

    return pricing


__all__ = [
    "_run_pricing_vanilla_option_command",
    "run_vanilla_option_pricing",
]
