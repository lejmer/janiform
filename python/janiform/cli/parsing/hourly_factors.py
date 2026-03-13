from argparse import _SubParsersAction
from pathlib import Path

from .._constants import CLI_PATH_METAVAR
from ..commands.hourly_factors import (
    _run_hourly_factor_build_contract_command,
    _run_hourly_factor_materialize_news_intensity_command,
    _run_hourly_factor_materialize_seasonality_command,
)
from .shared import _add_hourly_factor_common_arguments, _add_required_json_path_argument


def _build_hourly_factors_parser(subparsers: _SubParsersAction) -> None:
    parser = subparsers.add_parser(
        "hourly-factors",
        help="Hourly factor materialization and contract-building workflows.",
        description="Hourly factor materialization and contract-building workflows.",
    )
    parser.set_defaults(help_target=parser)

    subcommands = parser.add_subparsers(dest="hourly_factors_command")

    seasonality_parser = subcommands.add_parser(
        "materialize-seasonality",
        help="Materialize interval-aligned seasonality factors from timestamped source samples.",
        description=(
            "Materialize interval-aligned seasonality factors from timestamped source "
            "samples. The command preserves the canonical seasonality materialization "
            "report shape."
        ),
    )
    seasonality_parser.set_defaults(
        handler=_run_hourly_factor_materialize_seasonality_command,
        help_target=seasonality_parser,
    )

    _add_hourly_factor_common_arguments(seasonality_parser)
    _add_required_json_path_argument(
        seasonality_parser,
        flag_name="--materialization-input-json",
        contract_label=(
            "an hourly seasonality materialization input document with source "
            "timestamps, source factors, and optional materialization settings"
        ),
    )

    news_parser = subcommands.add_parser(
        "materialize-news-intensity",
        help="Materialize interval-aligned news-intensity factors from known-news and surprise controls.",
        description=(
            "Materialize interval-aligned news-intensity factors from known-news and "
            "surprise controls. The command preserves the canonical news-intensity "
            "materialization report shape."
        ),
    )
    news_parser.set_defaults(
        handler=_run_hourly_factor_materialize_news_intensity_command,
        help_target=news_parser,
    )

    _add_hourly_factor_common_arguments(news_parser)
    _add_required_json_path_argument(
        news_parser,
        flag_name="--materialization-input-json",
        contract_label=(
            "an hourly news-intensity materialization input document with known-news "
            "timestamps, surprise baseline intensity, and optional interval multipliers"
        ),
    )

    contract_parser = subcommands.add_parser(
        "build-contract",
        help="Build the combined interval-factor contract for downstream simulation or pricing workflows.",
        description=(
            "Build the combined interval-factor contract for downstream simulation or "
            "pricing workflows. Seasonality and news-intensity inputs can come from "
            "explicit factor source documents or from canonical materialization report "
            "files produced by Janiform."
        ),
    )
    contract_parser.set_defaults(
        handler=_run_hourly_factor_build_contract_command,
        help_target=contract_parser,
    )

    _add_hourly_factor_common_arguments(contract_parser)

    seasonality_source_group = contract_parser.add_mutually_exclusive_group(required=True)
    seasonality_source_group.add_argument(
        "--seasonality-explicit-source-json",
        type=Path,
        metavar=CLI_PATH_METAVAR,
        help=(
            "Path to an explicit seasonality source document with `hourly_factors` and "
            "optional `materialization_method`. Relative paths are resolved from the "
            "repository root."
        ),
    )
    seasonality_source_group.add_argument(
        "--seasonality-materialized-source-json",
        type=Path,
        metavar=CLI_PATH_METAVAR,
        help=(
            "Path to a canonical hourly seasonality materialization report produced by "
            "`hourly-factors materialize-seasonality`. Relative paths are resolved from "
            "the repository root."
        ),
    )

    news_source_group = contract_parser.add_mutually_exclusive_group(required=True)
    news_source_group.add_argument(
        "--news-intensity-explicit-source-json",
        type=Path,
        metavar=CLI_PATH_METAVAR,
        help=(
            "Path to an explicit news-intensity source document with `hourly_factors` "
            "and optional `materialization_method`. Relative paths are resolved from "
            "the repository root."
        ),
    )
    news_source_group.add_argument(
        "--news-intensity-materialized-source-json",
        type=Path,
        metavar=CLI_PATH_METAVAR,
        help=(
            "Path to a canonical hourly news-intensity materialization report produced "
            "by `hourly-factors materialize-news-intensity`. Relative paths are "
            "resolved from the repository root."
        ),
    )


__all__ = ["_build_hourly_factors_parser"]
