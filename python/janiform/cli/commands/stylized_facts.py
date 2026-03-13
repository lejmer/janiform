from collections.abc import Mapping
from pathlib import Path
from typing import Any, Optional, cast

import click

from ..command_runtime import (
    JaniformCommand,
    JaniformGroup,
    command_config_option,
    command_option,
    group_help_callback,
    invoke_command_handler,
    optional_path_option,
    output_option,
    required_json_path_option,
)
from ..context import CliCommandContext
from .._constants import (
    CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
    CLI_STYLIZED_FACTS_HORIZON_COMPARISON_ENTRYPOINT,
    CLI_STYLIZED_FACTS_HORIZON_COMPARISON_INTERNAL_ERROR_ID,
    CLI_STYLIZED_FACTS_HORIZON_COMPARISON_VALIDATION_ERROR_ID,
    CliExitCode,
    CliOutputMode,
)
from ..documents import _load_json_document
from ..errors import _classify_workflow_command_error
from ..paths import _resolve_path_from_repository_root
from ..rendering import _render_stylized_facts_horizon_comparison_human_output, _write_cli_error, _write_command_output
from ...errors import JaniformError
from ...stylized_facts import run_stylized_fact_horizon_comparison_report_generator


def _run_stylized_facts_horizon_comparison_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    metric_catalog_json: Path,
    comparison_request_json: Path,
    artifact_export_json: Optional[Path],
    execution_mode: str,
    **_: object,
) -> int:
    metric_catalog_path = _resolve_path_from_repository_root(
        cli_context.repository_root,
        metric_catalog_json,
    )
    comparison_request_path = _resolve_path_from_repository_root(
        cli_context.repository_root,
        comparison_request_json,
    )
    artifact_export_path = (
        _resolve_path_from_repository_root(cli_context.repository_root, artifact_export_json)
        if artifact_export_json is not None
        else None
    )

    file_field_path = "metric_catalog_json"
    document_field_path = "metric_catalog_document"

    try:
        metric_catalog_document = _load_json_document(metric_catalog_path, "stylized-fact metric catalog")

        file_field_path = "comparison_request_json"
        document_field_path = "comparison_request_document"

        comparison_request_document = _load_json_document(comparison_request_path, "stylized-fact comparison request")

        artifact_export_document: Optional[dict[str, Any]] = None
        if artifact_export_path is not None:
            file_field_path = "artifact_export_json"
            document_field_path = "artifact_export_document"
            artifact_export_document = _load_json_document(
                artifact_export_path,
                "stylized-fact artifact export config",
            )

        document_field_path = "command_input"

        report_bundle = run_stylized_fact_horizon_comparison_report_generator(
            metric_catalog=metric_catalog_document,
            comparison_request=comparison_request_document,
            execution_mode=execution_mode,
            artifact_export=artifact_export_document,
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_STYLIZED_FACTS_HORIZON_COMPARISON_ENTRYPOINT,
                validation_error_id=CLI_STYLIZED_FACTS_HORIZON_COMPARISON_VALIDATION_ERROR_ID,
                internal_error_id=CLI_STYLIZED_FACTS_HORIZON_COMPARISON_INTERNAL_ERROR_ID,
                file_field_path=file_field_path,
                document_field_path=document_field_path,
            )
        )

    _write_command_output(
        output,
        json_payload=cast(dict[str, Any], report_bundle),
        human_output_factory=lambda: _render_stylized_facts_horizon_comparison_human_output(
            report_bundle,
            execution_mode=execution_mode,
        ),
    )

    return int(CliExitCode.SUCCESS)


def _build_stylized_facts_group(*, environment: Mapping[str, str] | None = None) -> click.Group:
    @click.group(
        cls=JaniformGroup,
        command_path=("stylized-facts",),
        environment=environment,
        help="Stylized-fact reporting workflows.",
        invoke_without_command=True,
    )
    @click.pass_context
    def stylized_facts(ctx: click.Context) -> None:
        group_help_callback(ctx)

    @stylized_facts.command(
        "horizon-comparison",
        cls=JaniformCommand,
        command_path=("stylized-facts", "horizon-comparison"),
        environment=environment,
        help=(
            "Generate a stylized-fact horizon-comparison report bundle from a metric "
            "catalog, a comparison request, and optional artifact-export settings."
        ),
        params=[
            command_config_option(),
            output_option(default_output_mode=CliOutputMode.JSON),
            required_json_path_option("--metric-catalog-json", contract_label="a stylized-fact metric catalog JSON object"),
            required_json_path_option("--comparison-request-json", contract_label="a stylized-fact horizon-comparison request JSON object"),
            optional_path_option(
                "--artifact-export-json",
                help_text=(
                    "Optional path to a stylized-fact artifact-export config JSON object. This "
                    "payload is passed through the public workflow validation path and requires "
                    "--execution-mode disk."
                ),
            ),
            command_option(
                "--execution-mode",
                choice_options=CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
                default="memory",
                help_text=(
                    "Execution mode passed through to janiform.stylized_facts. Default: memory. "
                    "Use disk when artifact export is requested."
                ),
            ),
        ],
    )
    @click.pass_context
    def horizon_comparison(ctx: click.Context, **command_arguments: object) -> int:
        return invoke_command_handler(
            ctx,
            handler=_run_stylized_facts_horizon_comparison_command,
            command_path=("stylized-facts", "horizon-comparison"),
            command_arguments=dict(command_arguments),
        )

    return stylized_facts


__all__ = ["_run_stylized_facts_horizon_comparison_command"]
