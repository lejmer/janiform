from argparse import _SubParsersAction

from .shared import (
    _add_command_config_argument,
    _add_output_mode_argument,
    _add_required_json_path_argument,
)
from .._constants import (
    CLI_SEASONALITY_INTRADAY_SMOOTHING_KERNEL_CHOICES,
    CLI_SEASONALITY_SESSION_FILTER_CHOICES,
    CliOutputMode,
)
from ..commands.seasonality import (
    _run_seasonality_diagnostics_command,
    _run_seasonality_preprocess_timestamps_command,
)
from ...seasonality import IntradaySmoothingKernel, SessionFilter


def _build_seasonality_parser(subparsers: _SubParsersAction) -> None:
    seasonality_parser = subparsers.add_parser(
        "seasonality",
        help="Seasonality preprocessing workflows.",
        description="Seasonality preprocessing workflows.",
    )
    seasonality_parser.set_defaults(help_target=seasonality_parser)

    seasonality_subparsers = seasonality_parser.add_subparsers(dest="seasonality_command")

    diagnostics_parser = seasonality_subparsers.add_parser(
        "diagnostics",
        help="Run end-to-end seasonality diagnostics from raw timestamp events.",
        description=(
            "Run end-to-end seasonality diagnostics from raw timestamp events. This "
            "task-oriented workflow validates timestamp records, applies session "
            "filtering, estimates intraday, day-of-week, and day-of-year components, "
            "composes the deterministic seasonality signal, and emits the canonical "
            "diagnostics report."
        ),
    )
    diagnostics_parser.set_defaults(
        handler=_run_seasonality_diagnostics_command,
        help_target=diagnostics_parser,
    )

    _add_command_config_argument(diagnostics_parser)
    _add_output_mode_argument(diagnostics_parser, default_output_mode=CliOutputMode.JSON)
    _add_required_json_path_argument(
        diagnostics_parser,
        flag_name="--event-records-json",
        contract_label="an event-record document with a top-level `event_records` list",
        semantic_note=(
            "Each record must include a timezone-aware timestamp string compatible with "
            "the public janiform.seasonality preprocessing contract."
        ),
    )
    _add_required_json_path_argument(
        diagnostics_parser,
        flag_name="--calendar-registry-json",
        contract_label="a calendar registry JSON object used for exchange session classification",
    )

    diagnostics_parser.add_argument(
        "--dataset-id",
        required=True,
        help="Stable dataset identifier recorded in the diagnostics report.",
    )
    diagnostics_parser.add_argument(
        "--exchange-code",
        required=True,
        help="Exchange code used to resolve session state for all timestamp events.",
    )
    diagnostics_parser.add_argument(
        "--timestamp-field-name",
        default="timestamp",
        help="Timestamp field name inside each event record. Default: timestamp.",
    )
    diagnostics_parser.add_argument(
        "--session-filter",
        choices=CLI_SEASONALITY_SESSION_FILTER_CHOICES,
        default=SessionFilter.OPEN_ONLY.value,
        help="Session filter applied after exchange calendar classification. Default: open_only.",
    )
    diagnostics_parser.add_argument(
        "--sparse-hour-event-threshold",
        type=int,
        default=1,
        help="Minimum accepted events per hour before a bucket is flagged as sparse. Default: 1.",
    )
    diagnostics_parser.add_argument(
        "--intraday-minutes-per-bin",
        type=int,
        default=1,
        help="Intraday estimator bin size in minutes. Default: 1.",
    )
    diagnostics_parser.add_argument(
        "--intraday-smoothing-kernel",
        choices=CLI_SEASONALITY_INTRADAY_SMOOTHING_KERNEL_CHOICES,
        default=IntradaySmoothingKernel.GAUSSIAN.value,
        help="Intraday estimator smoothing kernel. Default: gaussian.",
    )
    diagnostics_parser.add_argument(
        "--intraday-smoothing-bandwidth-bins",
        type=float,
        default=3.0,
        help="Intraday estimator smoothing bandwidth measured in bins. Default: 3.0.",
    )
    diagnostics_parser.add_argument(
        "--intraday-minimum-bin-events",
        type=int,
        default=1,
        help="Minimum events per intraday bin before the bin is flagged as sparse. Default: 1.",
    )
    diagnostics_parser.add_argument(
        "--intraday-pseudocount",
        type=float,
        default=1e-6,
        help="Intraday estimator pseudocount added before normalization. Default: 1e-6.",
    )
    diagnostics_parser.add_argument(
        "--day-of-week-minimum-bucket-events",
        type=int,
        default=1,
        help="Minimum events per day-of-week bucket before the bucket is flagged as sparse. Default: 1.",
    )
    diagnostics_parser.add_argument(
        "--day-of-week-pseudocount",
        type=float,
        default=1e-6,
        help="Day-of-week estimator pseudocount added before normalization. Default: 1e-6.",
    )
    diagnostics_parser.add_argument(
        "--day-of-year-minimum-bucket-events",
        type=int,
        default=1,
        help="Minimum events per day-of-year bucket before the bucket is flagged as sparse. Default: 1.",
    )
    diagnostics_parser.add_argument(
        "--day-of-year-pseudocount",
        type=float,
        default=1e-6,
        help="Day-of-year estimator pseudocount added before normalization. Default: 1e-6.",
    )
    diagnostics_parser.add_argument(
        "--estimator-version",
        default="seasonality_estimator.v1",
        help="Estimator version label recorded in reproducibility metadata. Default: seasonality_estimator.v1.",
    )

    preprocess_parser = seasonality_subparsers.add_parser(
        "preprocess-timestamps",
        help="Preprocess timestamp events against the exchange calendar registry.",
        description=(
            "Preprocess timestamp events against the exchange calendar registry. This "
            "standalone step validates timestamp records, applies session filtering, and "
            "emits deterministic preprocessing diagnostics for downstream seasonality "
            "or calibration workflows."
        ),
    )
    preprocess_parser.set_defaults(
        handler=_run_seasonality_preprocess_timestamps_command,
        help_target=preprocess_parser,
    )

    _add_command_config_argument(preprocess_parser)
    _add_output_mode_argument(preprocess_parser, default_output_mode=CliOutputMode.JSON)
    _add_required_json_path_argument(
        preprocess_parser,
        flag_name="--event-records-json",
        contract_label="an event-record document with a top-level `event_records` list",
        semantic_note=(
            "Each record must include a timezone-aware timestamp string compatible with "
            "the public janiform.seasonality preprocessing contract."
        ),
    )
    _add_required_json_path_argument(
        preprocess_parser,
        flag_name="--calendar-registry-json",
        contract_label="a calendar registry JSON object used for exchange session classification",
    )

    preprocess_parser.add_argument(
        "--exchange-code",
        required=True,
        help="Exchange code used to resolve session state for all timestamp events.",
    )
    preprocess_parser.add_argument(
        "--timestamp-field-name",
        default="timestamp",
        help="Timestamp field name inside each event record. Default: timestamp.",
    )
    preprocess_parser.add_argument(
        "--session-filter",
        choices=CLI_SEASONALITY_SESSION_FILTER_CHOICES,
        default=SessionFilter.OPEN_ONLY.value,
        help="Session filter applied after exchange calendar classification. Default: open_only.",
    )
    preprocess_parser.add_argument(
        "--sparse-hour-event-threshold",
        type=int,
        default=1,
        help="Minimum accepted events per hour before a bucket is flagged as sparse. Default: 1.",
    )


__all__ = ["_build_seasonality_parser"]
