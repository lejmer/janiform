"""Internal pricing CLI command assembly."""

from collections.abc import Mapping
from pathlib import Path
from typing import Optional, cast

import click

from ._shared import parse_utc_datetime_cli_argument
from .._constants import (
    CLI_PRICE_ENTRYPOINT,
    CLI_PRICE_INTERNAL_ERROR_ID,
    CLI_PRICE_RESULT_SCHEMA_VERSION,
    CLI_PRICE_VALIDATION_ERROR_ID,
    CliExitCode,
    CliOutputMode,
)
from ..command_runtime import (
    JaniformCommand,
    command_config_option,
    command_option,
    invoke_command_handler,
    optional_path_option,
    output_option,
    required_datetime_option,
)
from ..context import CliCommandContext
from .._documents import load_json_document_for_cli_field
from .._errors import classify_workflow_command_error
from ..paths import _resolve_path_from_repository_root
from .._rendering import render_price_human_output, write_cli_error, write_command_output
from ..._workflow_outputs import generate_workflow_run_id, resolve_pricing_artifact_paths
from ..._workflow_progress import JaniformOutputController
from ...calibration.fit import JaniformModel
from ...errors import JaniformError
from ...pricing import MeanReversionBoundaryState
from ...pricing import VanillaOptionPayoffType


def run_price_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    model_path: Path,
    **command_arguments,
) -> int:
    resolved_model_path = _resolve_path_from_repository_root(cli_context.repository_root, model_path)

    try:
        model = JaniformModel.load(resolved_model_path)
        pricing_run_id = generate_workflow_run_id(f"{model.run_id}_price")
        output_path_argument = cast(Optional[Path], command_arguments.get("output_path"))
        observed_boundary_state_path = cast(
            Optional[Path],
            command_arguments.get("observed_boundary_state_json"),
        )
        resolved_output_path = (
            _resolve_path_from_repository_root(cli_context.repository_root, output_path_argument)
            if output_path_argument is not None
            else None
        )
        observed_boundary_state = None
        if observed_boundary_state_path is not None:
            observed_boundary_state_document = load_json_document_for_cli_field(
                repository_root=cli_context.repository_root,
                path=observed_boundary_state_path,
                context="an observed MeanReversionBoundaryState JSON payload",
                field_path="observed_boundary_state_json",
                error_id=CLI_PRICE_VALIDATION_ERROR_ID,
                entrypoint=CLI_PRICE_ENTRYPOINT,
            )
            observed_boundary_state = MeanReversionBoundaryState.from_mapping(
                observed_boundary_state_document,
                field_path="observed_boundary_state",
            )
        artifact_paths = resolve_pricing_artifact_paths(
            run_id=pricing_run_id,
            output_path=resolved_output_path,
        )

        output_controller = JaniformOutputController(silent=bool(command_arguments.get("silent")))
        if output == CliOutputMode.HUMAN.value and not bool(command_arguments.get("silent")):
            output_controller.info(f"price run_id: {pricing_run_id}")
            output_controller.info(f"model path: {resolved_model_path}")
            output_controller.info(f"primary output: {artifact_paths.primary_output_path}")
            output_controller.info(f"report path: {artifact_paths.report_path}")

        pricing_result = model.price_option(
            valuation_time_utc=parse_utc_datetime_cli_argument(
                cast(str, command_arguments["valuation_time_utc"]),
                field_path="valuation_time_utc",
                error_id=CLI_PRICE_VALIDATION_ERROR_ID,
                entrypoint=CLI_PRICE_ENTRYPOINT,
            ),
            spot=float(cast(float, command_arguments["spot"])),
            strike=float(cast(float, command_arguments["strike"])),
            maturity_interval=float(cast(float, command_arguments["maturity_interval"])),
            payoff_type=cast(str, command_arguments["payoff_type"]),
            risk_free_rate=float(cast(float, command_arguments["risk_free_rate"])),
            dividend_yield=float(cast(float, command_arguments["dividend_yield"])),
            observed_boundary_state=observed_boundary_state,
            include_transform_diagnostics=bool(command_arguments.get("include_transform_diagnostics")),
            include_integration_quality_diagnostics=bool(
                command_arguments.get("include_integration_quality_diagnostics")
            ),
            include_calibration_hooks=bool(command_arguments.get("include_calibration_hooks")),
            output_path=artifact_paths.primary_output_path,
            silent=bool(command_arguments.get("silent")) or output == CliOutputMode.JSON.value,
        )
    except JaniformError as e:
        return write_cli_error(e)
    except Exception as e:
        return write_cli_error(
            classify_workflow_command_error(
                e,
                entrypoint=CLI_PRICE_ENTRYPOINT,
                validation_error_id=CLI_PRICE_VALIDATION_ERROR_ID,
                internal_error_id=CLI_PRICE_INTERNAL_ERROR_ID,
                file_field_path="model_path",
                document_field_path="model_payload",
            )
        )

    payload = {
        "schema_version": CLI_PRICE_RESULT_SCHEMA_VERSION,
        "command": CLI_PRICE_ENTRYPOINT,
        "run_id": pricing_run_id,
        "model_path": str(resolved_model_path),
        "primary_output_path": str(artifact_paths.primary_output_path),
        "report_path": str(artifact_paths.report_path),
        "manifest_path": str(artifact_paths.manifest_path),
        "run_directory": str(artifact_paths.run_directory),
        "response": pricing_result.response.to_payload(),
        "run_diagnostics": (
            None if pricing_result.run_diagnostics is None else pricing_result.run_diagnostics.to_payload()
        ),
    }

    if not bool(command_arguments.get("silent")):
        write_command_output(
            output,
            json_payload=payload,
            human_output_factory=lambda: render_price_human_output(payload),
        )

    return int(CliExitCode.SUCCESS)


def build_price_command(*, environment: Optional[Mapping[str, str]] = None) -> click.Command:
    @click.command(
        "price",
        cls=JaniformCommand,
        command_path=("price",),
        environment=environment,
        help=(
            "Price a vanilla option from a persisted Janiform model. "
            "The command writes pricing.json, pricing_report.json, and run.json by default."
        ),
        params=[
            command_config_option(),
            output_option(default_output_mode=CliOutputMode.HUMAN),
            command_option(
                "--model-path",
                help_text="Path to a persisted JaniformModel JSON payload.",
                path_type=True,
                required=True,
            ),
            required_datetime_option(
                "--valuation-time-utc",
                help_text="Valuation timestamp in UTC.",
                required=True,
            ),
            command_option("--spot", help_text="Spot price.", required=True, value_type=float),
            command_option("--strike", help_text="Option strike.", required=True, value_type=float),
            command_option(
                "--maturity-interval",
                help_text="Option maturity interval in model time units.",
                required=True,
                value_type=float,
            ),
            command_option(
                "--payoff-type",
                choice_options=(VanillaOptionPayoffType.CALL.value, VanillaOptionPayoffType.PUT.value),
                default=VanillaOptionPayoffType.CALL.value,
                help_text="Option payoff type. Default: call.",
            ),
            command_option(
                "--risk-free-rate",
                default=0.0,
                help_text="Risk-free rate. Default: 0.0.",
                value_type=float,
            ),
            command_option(
                "--dividend-yield",
                default=0.0,
                help_text="Dividend yield. Default: 0.0.",
                value_type=float,
            ),
            optional_path_option(
                "--observed-boundary-state-json",
                help_text=(
                    "Optional MeanReversionBoundaryState JSON payload for anchored pricing. "
                    "Required when the persisted model enables mean reversion because the CLI "
                    "does not invent observed pricing state."
                ),
            ),
            optional_path_option(
                "--output-path",
                help_text="Primary pricing output path. Default: ./.janiform/<run_id>/pricing.json.",
            ),
            command_option(
                "--include-transform-diagnostics",
                help_text="Include transform diagnostics in pricing_report.json.",
                is_flag=True,
            ),
            command_option(
                "--include-integration-quality-diagnostics",
                help_text="Include integration quality diagnostics in pricing_report.json.",
                is_flag=True,
            ),
            command_option(
                "--include-calibration-hooks",
                help_text="Include calibration hooks in pricing.json.",
                is_flag=True,
            ),
            command_option(
                "--silent",
                help_text="Suppress non-error price output.",
                is_flag=True,
            ),
        ],
    )
    @click.pass_context
    def price(ctx: click.Context, **command_arguments) -> int:
        return invoke_command_handler(
            ctx,
            handler=run_price_command,
            command_path=("price",),
            command_arguments=dict(command_arguments),
        )

    return price
