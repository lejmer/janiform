from collections.abc import Mapping
from pathlib import Path
from typing import Optional, cast

import click

from ..._workflow_outputs import generate_workflow_run_id, resolve_pricing_artifact_paths
from ..._workflow_progress import JaniformOutputController
from ...calibration import JaniformModel
from ...errors import JaniformError
from ...pricing import VanillaOptionPayoffType
from .shared import _parse_utc_datetime_cli_argument, _require_positive_integer_cli_argument
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
from ..errors import _classify_workflow_command_error
from ..paths import _resolve_path_from_repository_root
from ..rendering import _render_price_human_output, _write_cli_error, _write_command_output


def _run_price_command(
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
        resolved_output_path = (
            _resolve_path_from_repository_root(cli_context.repository_root, output_path_argument)
            if output_path_argument is not None
            else None
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
            valuation_time_utc=_parse_utc_datetime_cli_argument(
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
            include_transform_diagnostics=bool(command_arguments.get("include_transform_diagnostics")),
            include_integration_quality_diagnostics=bool(
                command_arguments.get("include_integration_quality_diagnostics")
            ),
            include_calibration_hooks=bool(command_arguments.get("include_calibration_hooks")),
            output_path=artifact_paths.primary_output_path,
            silent=bool(command_arguments.get("silent")) or output == CliOutputMode.JSON.value,
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
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
        _write_command_output(
            output,
            json_payload=payload,
            human_output_factory=lambda: _render_price_human_output(payload),
        )

    return int(CliExitCode.SUCCESS)


def _build_price_command(*, environment: Optional[Mapping[str, str]] = None) -> click.Command:
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
                "--output-path",
                help_text="Primary pricing output path. Default: ./janiform_runs/<run_id>/pricing.json.",
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
            handler=_run_price_command,
            command_path=("price",),
            command_arguments=dict(command_arguments),
        )

    return price


__all__ = ["_build_price_command"]
