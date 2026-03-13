from argparse import Namespace
from pathlib import Path
from typing import cast

from .._constants import (
    CLI_PRICING_VANILLA_OPTION_ENTRYPOINT,
    CLI_PRICING_VANILLA_OPTION_INTERNAL_ERROR_ID,
    CLI_PRICING_VANILLA_OPTION_VALIDATION_ERROR_ID,
    CliExitCode,
)
from ..documents import _load_json_document
from ..errors import _classify_workflow_command_error
from ..paths import _resolve_path_from_repository_root, resolve_repository_root
from ..rendering import _render_pricing_vanilla_option_human_output, _write_cli_error, _write_command_output
from ...errors import JaniformError
from ...pricing import run_vanilla_option_pricing, validate_vanilla_option_pricing_execution_input


def _run_pricing_vanilla_option_command(parsed_arguments: Namespace) -> int:
    repository_root = resolve_repository_root()
    execution_input_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.execution_input_json),
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
        parsed_arguments,
        json_payload=payload,
        human_output_factory=lambda: _render_pricing_vanilla_option_human_output(payload),
    )
    return int(CliExitCode.SUCCESS)


__all__ = [
    "_run_pricing_vanilla_option_command",
    "run_vanilla_option_pricing",
]
