import json
import sys
from argparse import ArgumentParser, Namespace
from datetime import timedelta
from pathlib import Path
from typing import Any, cast, Optional

from . import __version__
from .calibration import (
    CalibrationExecutionMode,
    CalibrationStagedExecutionRuntimeResources,
    StagedCalibrationExecutionInput,
    run_staged_calibration,
    validate_staged_calibration_execution_input,
)
from .calendar import (
    CalendarInvalidTimestampError,
    CalendarInvalidTimezoneError,
    CalendarMissingExchangeError,
    CalendarRegistry,
    CalendarRegistryLoadError,
)
from .errors import (
    ContractValidationFailure,
    JaniformError,
    JaniformExecutionError,
    JaniformErrorDetails,
    JaniformErrorCategory,
    JaniformValidationError,
)
from .hourly_factors import (
    HourlyNewsIntensityProviderExplicitSource,
    HourlySeasonalityProviderExplicitSource,
)
from .pricing import run_vanilla_option_pricing, validate_vanilla_option_pricing_execution_input
from .seasonality import SeasonalityInputValidationError
from .simulation import (
    SeededTickSimulationHourlyFactorSummary,
    SeededTickSimulationRequest,
    load_hourly_factor_contract_for_simulation,
    run_seeded_tick_event_simulation,
)
from .validation import run_open_closed_gap_leakage_regression_harness

DEFAULT_CALENDAR_FIXTURE_PATH: str = "tests/fixtures/calendar/calendar_service_regression_matrix.json"
DEFAULT_GAP_BEHAVIOR_FIXTURE_PATH: str = (
    "tests/fixtures/gap_behavior/open_closed_gap_behavior_scenarios.json"
)
CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT = "simulation.seeded_ticks"
CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID = "cli.validation.simulation.seeded_ticks.invalid_input"
CLI_SIMULATION_SEEDED_TICKS_EXECUTION_ERROR_ID = "cli.execution.simulation.seeded_ticks.failed"
CLI_PRICING_VANILLA_OPTION_ENTRYPOINT = "pricing.vanilla_option"
CLI_PRICING_VANILLA_OPTION_VALIDATION_ERROR_ID = "cli.validation.pricing.vanilla_option.invalid_input"
CLI_PRICING_VANILLA_OPTION_EXECUTION_ERROR_ID = "cli.execution.pricing.vanilla_option.failed"
CLI_CALIBRATION_STAGED_ENTRYPOINT = "calibration.staged"
CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID = "cli.validation.calibration.staged.invalid_input"
CLI_CALIBRATION_STAGED_EXECUTION_ERROR_ID = "cli.execution.calibration.staged.failed"
CLI_OPEN_CLOSED_GAP_ENTRYPOINT = "dev.regression.open_closed_gap_leakage"
CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID = "cli.validation.open_closed_gap_leakage.invalid_input"
CLI_OPEN_CLOSED_GAP_EXECUTION_ERROR_ID = "cli.execution.open_closed_gap_leakage.failed"


def _add_required_json_path_argument(
    parser: ArgumentParser,
    *,
    flag_name: str,
    help_text: str,
) -> None:
    parser.add_argument(
        flag_name,
        type=Path,
        required=True,
        help=help_text,
    )


def _build_parser() -> ArgumentParser:
    parser: ArgumentParser = ArgumentParser(
        prog="janiform",
        description="Command line interface for Janiform workflows and developer utilities.",
    )
    parser.add_argument("--version", action="version", version=f"%(prog)s {__version__}")
    parser.set_defaults(
        command=None,
        handler=None,
        help_target=parser,
    )

    subparsers = parser.add_subparsers(dest="command")

    simulation_parser = subparsers.add_parser(
        "simulation",
        help="Simulation workflows.",
        description="Simulation workflows.",
    )
    simulation_parser.set_defaults(help_target=simulation_parser)
    simulation_subparsers = simulation_parser.add_subparsers(dest="simulation_command")

    simulation_seeded_ticks_parser = simulation_subparsers.add_parser(
        "seeded-ticks",
        help="Seeded tick event simulation workflow.",
        description="Seeded tick event simulation workflow.",
    )
    simulation_seeded_ticks_parser.set_defaults(
        handler=_run_simulation_seeded_ticks_command,
        help_target=simulation_seeded_ticks_parser,
    )

    _add_required_json_path_argument(
        simulation_seeded_ticks_parser,
        flag_name="--request-json",
        help_text=(
            "Path to a seeded-tick simulation request JSON object. "
            "The request summary must preserve open-jump versus closed-gap hourly factors."
        ),
    )
    _add_required_json_path_argument(
        simulation_seeded_ticks_parser,
        flag_name="--calendar-registry-json",
        help_text="Path to a calendar registry JSON object used to rehydrate hourly factors.",
    )

    calibration_parser = subparsers.add_parser(
        "calibration",
        help="Calibration workflows.",
        description="Calibration workflows.",
    )
    calibration_parser.set_defaults(help_target=calibration_parser)
    calibration_subparsers = calibration_parser.add_subparsers(dest="calibration_command")

    calibration_staged_parser = calibration_subparsers.add_parser(
        "staged",
        help="Staged calibration workflow.",
        description="Staged calibration workflow.",
    )
    calibration_staged_parser.set_defaults(
        handler=_run_calibration_staged_command,
        help_target=calibration_staged_parser,
    )

    _add_required_json_path_argument(
        calibration_staged_parser,
        flag_name="--execution-input-json",
        help_text=(
            "Path to a staged calibration execution-input JSON object with explicit known-news "
            "return samples and raw close-open boundary observations for internal gap detection."
        ),
    )
    _add_required_json_path_argument(
        calibration_staged_parser,
        flag_name="--calendar-registry-json",
        help_text="Path to a calendar registry JSON object consumed by the staged calibration runner.",
    )

    calibration_staged_parser.add_argument(
        "--execution-mode",
        choices=("disk", "memory"),
        default="disk",
        help="Calibration execution mode. CLI default is disk.",
    )
    calibration_staged_parser.add_argument(
        "--disk-checkpoint-directory",
        type=Path,
        help="Checkpoint directory required for disk execution mode when not already present in the input JSON.",
    )

    pricing_parser = subparsers.add_parser(
        "pricing",
        help="Pricing workflows.",
        description="Pricing workflows.",
    )
    pricing_parser.set_defaults(help_target=pricing_parser)
    pricing_subparsers = pricing_parser.add_subparsers(dest="pricing_command")

    pricing_vanilla_option_parser = pricing_subparsers.add_parser(
        "vanilla-option",
        help="Vanilla option pricing workflow.",
        description="Vanilla option pricing workflow.",
    )
    pricing_vanilla_option_parser.set_defaults(
        handler=_run_pricing_vanilla_option_command,
        help_target=pricing_vanilla_option_parser,
    )

    _add_required_json_path_argument(
        pricing_vanilla_option_parser,
        flag_name="--execution-input-json",
        help_text=(
            "Path to a vanilla-option pricing execution-input JSON object. "
            "Interval factors keep known and surprise news intensity separate."
        ),
    )

    developer_parser = subparsers.add_parser(
        "dev",
        help="Developer-only utilities.",
        description="Developer-only utilities.",
    )
    developer_parser.set_defaults(help_target=developer_parser)
    developer_subparsers = developer_parser.add_subparsers(dest="developer_command")

    regression_parser = developer_subparsers.add_parser(
        "regression",
        help="Regression validation suites.",
        description="Regression validation suites.",
    )
    regression_parser.set_defaults(help_target=regression_parser)
    regression_subparsers = regression_parser.add_subparsers(dest="regression_command")

    gap_leakage_parser = regression_subparsers.add_parser(
        "open-closed-gap-leakage",
        help="Run open and closed gap leakage regression harness.",
        description="Run open and closed gap leakage regression harness.",
    )
    gap_leakage_parser.set_defaults(
        handler=_run_open_closed_gap_leakage_regression_command,
        help_target=gap_leakage_parser,
    )
    gap_leakage_parser.add_argument(
        "--calendar-fixture-path",
        type=Path,
        default=Path(DEFAULT_CALENDAR_FIXTURE_PATH),
        help="Path to calendar regression fixture JSON.",
    )
    gap_leakage_parser.add_argument(
        "--gap-behavior-fixture-path",
        type=Path,
        default=Path(DEFAULT_GAP_BEHAVIOR_FIXTURE_PATH),
        help="Path to open and closed gap behavior fixture JSON.",
    )
    gap_leakage_parser.add_argument(
        "--component-absolute-tolerance",
        type=float,
        default=1e-12,
        help="Absolute tolerance for component-level semantic mismatch detection.",
    )

    return parser


def main() -> None:
    parser: ArgumentParser = _build_parser()
    parsed_arguments: Namespace = parser.parse_args()
    handler = parsed_arguments.handler
    if handler is not None:
        raise SystemExit(handler(parsed_arguments))

    parsed_arguments.help_target.print_help()


def _run_simulation_seeded_ticks_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[2]
    request_path = _resolve_path_from_repository_root(repository_root, cast(Path, parsed_arguments.request_json))
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
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
    except JaniformError as error:
        sys.stderr.write(f"{error}\n")
        return 1
    except Exception as error:
        sys.stderr.write(
            f"{_classify_workflow_command_error(
                error,
                entrypoint=CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
                validation_error_id=CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
                execution_error_id=CLI_SIMULATION_SEEDED_TICKS_EXECUTION_ERROR_ID,
                file_field_path='request_json',
                document_field_path='request_document',
            )}\n"
        )
        return 1

    _write_contract_json(response.to_payload())
    return 0


def _run_pricing_vanilla_option_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[2]
    execution_input_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.execution_input_json),
    )

    try:
        execution_input_document = _load_json_document(execution_input_path, "pricing execution input")
        execution_input = validate_vanilla_option_pricing_execution_input(execution_input_document)
        run_result = run_vanilla_option_pricing(execution_input)
    except JaniformError as error:
        sys.stderr.write(f"{error}\n")
        return 1
    except Exception as error:
        sys.stderr.write(
            f"{_classify_workflow_command_error(
                error,
                entrypoint=CLI_PRICING_VANILLA_OPTION_ENTRYPOINT,
                validation_error_id=CLI_PRICING_VANILLA_OPTION_VALIDATION_ERROR_ID,
                execution_error_id=CLI_PRICING_VANILLA_OPTION_EXECUTION_ERROR_ID,
                file_field_path='execution_input_json',
                document_field_path='execution_input_document',
            )}\n"
        )
        return 1

    _write_contract_json(run_result.to_payload())
    return 0


def _run_calibration_staged_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[2]
    execution_input_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.execution_input_json),
    )
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )

    try:
        execution_input_document = _load_json_document(execution_input_path, "calibration execution input")
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        execution_input = validate_staged_calibration_execution_input(execution_input_document)
        normalized_execution_input = _override_calibration_execution_mode(
            execution_input=execution_input,
            execution_mode=CalibrationExecutionMode(cast(str, parsed_arguments.execution_mode)),
            disk_checkpoint_directory=cast(Path | None, parsed_arguments.disk_checkpoint_directory),
        )
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        response = run_staged_calibration(
            normalized_execution_input,
            calendar_registry=calendar_registry,
        )
    except JaniformError as error:
        sys.stderr.write(f"{error}\n")
        return 1
    except Exception as error:
        sys.stderr.write(
            f"{_classify_workflow_command_error(
                error,
                entrypoint=CLI_CALIBRATION_STAGED_ENTRYPOINT,
                validation_error_id=CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID,
                execution_error_id=CLI_CALIBRATION_STAGED_EXECUTION_ERROR_ID,
                file_field_path='execution_input_json',
                document_field_path='execution_input_document',
            )}\n"
        )
        return 1

    _write_contract_json(response.to_payload())
    return 0


def _run_open_closed_gap_leakage_regression_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[2]
    calendar_fixture_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_fixture_path),
    )
    gap_behavior_fixture_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.gap_behavior_fixture_path),
    )

    try:
        calendar_fixture_document = _load_json_document(calendar_fixture_path, "calendar fixture")
        gap_behavior_fixture_document = _load_json_document(
            gap_behavior_fixture_path,
            "open closed gap behavior fixture",
        )

        calendar_registry = _build_calendar_registry(calendar_fixture_document)
        gap_behavior_cases = _extract_gap_behavior_cases(gap_behavior_fixture_document)

        report = run_open_closed_gap_leakage_regression_harness(
            calendar_registry=calendar_registry,
            gap_behavior_cases=gap_behavior_cases,
            component_absolute_tolerance=float(parsed_arguments.component_absolute_tolerance),
        )
    except JaniformError as error:
        sys.stderr.write(f"{error}\n")
        return 1
    except Exception as error:
        sys.stderr.write(f"{_classify_open_closed_gap_command_error(error)}\n")
        return 1

    summary = report["summary"]
    behavior_diff_report_summary = cast(
        dict[str, int],
        cast(dict[str, Any], report["behavior_diff_report"])["summary"],
    )
    if summary["failed_case_count"] == 0:
        sys.stdout.write(
            "open closed gap leakage regression passed: "
            f"{summary['passed_case_count']}/{summary['total_case_count']} scenarios, "
            f"semantic_mismatches={summary['total_semantic_mismatch_failure_count']}, "
            f"diff_regression_cases={behavior_diff_report_summary['regression_case_count']}\n",
        )
        return 0

    sys.stderr.write(
        "open closed gap leakage regression failed: "
        f"{summary['failed_case_count']}/{summary['total_case_count']} scenarios, "
        f"semantic_mismatches={summary['total_semantic_mismatch_failure_count']}\n",
    )

    for case_report in report["case_reports"]:
        if case_report["status"] != "fail":
            continue

        sys.stderr.write(
            f"scenario_id={case_report['fixture_id']} exchange_code={case_report['exchange_code']} "
            f"semantic_mismatch_failure_count={case_report['semantic_mismatch_failure_count']} "
            f"max_absolute_mismatch={case_report['max_absolute_mismatch']}\n",
        )

        contribution_summary = cast(dict[str, Any], case_report["contribution_summary"])

        sys.stderr.write(
            "  contributions "
            f"activity_expected={contribution_summary['expected_activity_coupling_total']} "
            f"activity_observed={contribution_summary['observed_activity_coupling_total']} "
            f"trading_expected={contribution_summary['expected_trading_return_total']} "
            f"trading_observed={contribution_summary['observed_trading_return_total']} "
            f"open_jump_expected={contribution_summary['expected_open_jump_total']} "
            f"open_jump_observed={contribution_summary['observed_open_jump_total']} "
            f"closed_gap_expected={contribution_summary['expected_closed_gap_total']} "
            f"closed_gap_observed={contribution_summary['observed_closed_gap_total']}\n",
        )

        behavior_diff = cast(dict[str, Any], case_report["behavior_diff"])
        for diff_metric in cast(list[dict[str, Any]], behavior_diff["metrics"]):
            if not cast(bool, diff_metric["regression_detected"]):
                continue
            sys.stderr.write(
                "  diff_regression "
                f"metric_id={diff_metric['metric_id']} "
                f"semantic_bucket={diff_metric['semantic_bucket']} "
                f"expected={diff_metric['expected_value']} "
                f"observed={diff_metric['observed_value']} "
                f"absolute_difference={diff_metric['absolute_difference']} "
                f"absolute_tolerance={diff_metric['absolute_tolerance']}\n",
            )

        for failure in case_report["failures"]:
            failure_kind = cast(str, failure["failure_kind"])
            match failure_kind:
                case "semantic_mismatch":
                    sys.stderr.write(
                        f"  failure_kind={failure_kind} event_id={failure['event_id']} "
                        f"event_timestamp_utc={failure['event_timestamp_utc']} "
                        f"semantic_bucket={failure['semantic_bucket']} "
                        f"mismatch_kind={failure['mismatch_kind']} "
                        f"absolute_difference={failure['absolute_difference']}\n",
                    )
                case "dispatch_error":
                    sys.stderr.write(
                        f"  failure_kind={failure_kind} error_message={failure['error_message']}\n",
                    )
                case _:
                    sys.stderr.write(f"  failure_kind={failure_kind}\n")

    return 1


def _classify_open_closed_gap_command_error(error: Exception) -> JaniformError:
    if isinstance(error, JaniformError):
        return error

    if isinstance(error, FileNotFoundError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
                field_path="fixture_path",
                message="fixture file could not be found",
                context={"path": error.filename},
            )
        )

    if isinstance(error, json.JSONDecodeError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
                field_path="fixture_document",
                message="fixture file must contain valid JSON",
                context={
                    "column_number": error.colno,
                    "line_number": error.lineno,
                },
            )
        )

    if isinstance(
        error,
        (
            CalendarInvalidTimestampError,
            CalendarInvalidTimezoneError,
            CalendarMissingExchangeError,
            CalendarRegistryLoadError,
            SeasonalityInputValidationError,
            ValueError,
        ),
    ):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
                field_path="command_input",
                message=str(error),
            )
        )

    return JaniformExecutionError(
        JaniformErrorDetails.from_context(
            error_id=CLI_OPEN_CLOSED_GAP_EXECUTION_ERROR_ID,
            category=JaniformErrorCategory.EXECUTION,
            workflow="cli",
            entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
            message="open closed gap leakage regression command failed unexpectedly",
            context={"source_exception_type": type(error).__name__},
        )
    )


def _attach_runtime_hourly_factor_contract(
    *,
    request: SeededTickSimulationRequest,
    calendar_registry: CalendarRegistry,
) -> SeededTickSimulationRequest:
    range_end_utc = request.start_time_utc + timedelta(hours=request.horizon_interval_count)

    hourly_factor_contract = load_hourly_factor_contract_for_simulation(
        calendar_registry=calendar_registry,
        exchange_code=request.hourly_factor_summary.exchange_code,
        range_start_utc=request.start_time_utc,
        range_end_utc=range_end_utc,
        seasonality_source=HourlySeasonalityProviderExplicitSource(
            hourly_factors=request.hourly_factor_summary.s_h,
            materialization_method="cli.request_hourly_factor_summary.v1",
        ),
        news_intensity_source=HourlyNewsIntensityProviderExplicitSource(
            hourly_factors=request.hourly_factor_summary.nu_h,
            materialization_method="cli.request_hourly_factor_summary.v1",
        ),
    )

    derived_summary = SeededTickSimulationHourlyFactorSummary.from_contract(hourly_factor_contract)
    if derived_summary != request.hourly_factor_summary:
        raise JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
                field_path="request_json.hourly_factor_summary",
                message="request hourly_factor_summary must match the rehydrated hourly factor contract",
                context={
                    "derived_interval_count": derived_summary.interval_count,
                    "request_interval_count": request.hourly_factor_summary.interval_count,
                },
            )
        )

    return SeededTickSimulationRequest.from_runtime_inputs(
        seed=request.seed,
        start_time_utc=request.start_time_utc,
        horizon_interval_count=request.horizon_interval_count,
        kernel_parameters=request.kernel_parameters,
        initial_excitation_state=request.initial_excitation_state,
        hourly_factor_contract=hourly_factor_contract,
        debug_trace_enabled=request.debug_trace_enabled,
        sample_every_nth_transition=request.sample_every_nth_transition,
        max_records_per_chunk=request.max_records_per_chunk,
        state_snapshot_capture=request.state_snapshot_capture,
    )


def _override_calibration_execution_mode(
    *,
    execution_input: StagedCalibrationExecutionInput,
    execution_mode: CalibrationExecutionMode,
    disk_checkpoint_directory: Path | None,
) -> StagedCalibrationExecutionInput:
    if execution_mode == CalibrationExecutionMode.MEMORY and disk_checkpoint_directory is not None:
        raise JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_CALIBRATION_STAGED_ENTRYPOINT,
                field_path="disk_checkpoint_directory",
                message="--disk-checkpoint-directory requires --execution-mode disk",
            )
        )

    runtime_resources: Optional[CalibrationStagedExecutionRuntimeResources] = execution_input.runtime_resources
    if execution_mode == CalibrationExecutionMode.DISK and disk_checkpoint_directory is not None:
        runtime_resources = CalibrationStagedExecutionRuntimeResources(
            disk_checkpoint_directory=str(disk_checkpoint_directory),
        )
    if execution_mode == CalibrationExecutionMode.MEMORY:
        runtime_resources = None

    return validate_staged_calibration_execution_input(
        {
            **execution_input.to_payload(),
            "request": {
                **execution_input.request.to_payload(),
                "execution_mode": execution_mode.value,
            },
            "runtime_resources": None if runtime_resources is None else runtime_resources.to_payload(),
        }
    )


def _classify_workflow_command_error(
    error: Exception,
    *,
    entrypoint: str,
    validation_error_id: str,
    execution_error_id: str,
    file_field_path: str,
    document_field_path: str,
) -> JaniformError:
    if isinstance(error, JaniformError):
        return error

    if isinstance(error, FileNotFoundError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=validation_error_id,
                workflow="cli",
                entrypoint=entrypoint,
                field_path=file_field_path,
                message="input file could not be found",
                context={"path": error.filename},
            )
        )

    if isinstance(error, json.JSONDecodeError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=validation_error_id,
                workflow="cli",
                entrypoint=entrypoint,
                field_path=document_field_path,
                message="input file must contain valid JSON",
                context={
                    "column_number": error.colno,
                    "line_number": error.lineno,
                },
            )
        )

    if isinstance(
        error,
        (
            CalendarInvalidTimestampError,
            CalendarInvalidTimezoneError,
            CalendarMissingExchangeError,
            CalendarRegistryLoadError,
            SeasonalityInputValidationError,
            ValueError,
        ),
    ):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=validation_error_id,
                workflow="cli",
                entrypoint=entrypoint,
                field_path=document_field_path,
                message=str(error),
            )
        )

    return JaniformExecutionError(
        JaniformErrorDetails.from_context(
            error_id=execution_error_id,
            category=JaniformErrorCategory.EXECUTION,
            workflow="cli",
            entrypoint=entrypoint,
            message="workflow command failed unexpectedly",
            context={"source_exception_type": type(error).__name__},
        )
    )


def _resolve_path_from_repository_root(repository_root: Path, path: Path) -> Path:
    if path.is_absolute():
        return path

    return repository_root / path


def _load_json_document(path: Path, context: str) -> dict[str, Any]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise ValueError(f"{context} at {path} must decode to a JSON object")

    return cast(dict[str, Any], payload)


def _write_contract_json(payload: dict[str, Any]) -> None:
    sys.stdout.write(f"{json.dumps(payload, sort_keys=True)}\n")


def _build_calendar_registry(calendar_fixture_document: dict[str, Any]) -> CalendarRegistry:
    return _build_calendar_registry_from_document(calendar_fixture_document)


def _build_calendar_registry_from_document(calendar_registry_document: dict[str, Any]) -> CalendarRegistry:
    calendar_registry_payload: dict[str, Any]
    if "calendar_registry" in calendar_registry_document:
        calendar_registry_payload = _extract_calendar_registry_payload(
            calendar_registry_document,
            field_path="calendar_registry",
        )
    else:
        calendar_registry_payload = _extract_calendar_registry_payload(
            calendar_registry_document,
            field_path="calendar_registry_document",
        )

    return CalendarRegistry(json.dumps(calendar_registry_payload))


def _extract_calendar_registry_payload(
    calendar_registry_document: dict[str, Any],
    *,
    field_path: str,
) -> dict[str, Any]:
    schema_version = calendar_registry_document.get("schema_version")
    if isinstance(schema_version, str) and schema_version == "calendar.registry.v1":
        return calendar_registry_document

    calendar_registry_payload: object = calendar_registry_document.get("calendar_registry")
    if not isinstance(calendar_registry_payload, dict):
        raise ValueError(f"{field_path} must be a calendar.registry.v1 JSON object")

    return cast(dict[str, Any], calendar_registry_payload)


def _extract_gap_behavior_cases(
    gap_behavior_fixture_document: dict[str, Any],
) -> list[dict[str, Any]]:
    raw_cases: object = gap_behavior_fixture_document.get("cases")
    if not isinstance(raw_cases, list):
        raise ValueError("gap behavior fixture must include list field `cases`")

    parsed_cases: list[dict[str, Any]] = []
    for raw_case in raw_cases:
        if not isinstance(raw_case, dict):
            raise ValueError("gap behavior fixture case must be an object")

        initial_accumulator_state: object = raw_case.get("initial_accumulator_state")
        if not isinstance(initial_accumulator_state, dict):
            raise ValueError("gap behavior case must include object field `initial_accumulator_state`")

        injected_events: object = raw_case.get("injected_events")
        if not isinstance(injected_events, list):
            raise ValueError("gap behavior case must include list field `injected_events`")

        parsed_cases.append(
            {
                "fixture_id": raw_case.get("fixture_id"),
                "exchange_code": raw_case.get("exchange_code"),
                "scenario_class": raw_case.get("scenario_class"),
                "deterministic_replay_metadata": raw_case.get("deterministic_replay_metadata"),
                "issue_traceability": raw_case.get("issue_traceability"),
                "initial_j_t": initial_accumulator_state.get("j_t"),
                "initial_m_t": initial_accumulator_state.get("m_t"),
                "initial_g_t": initial_accumulator_state.get("g_t"),
                "injected_events": [
                    {
                        "event_id": (
                            raw_event.get("event_id")
                            if isinstance(raw_event, dict)
                            else None
                        ),
                        "timestamp_utc": (
                            raw_event.get("timestamp_utc")
                            if isinstance(raw_event, dict)
                            else None
                        ),
                        "shared_severity_mark": (
                            raw_event.get("shared_severity_mark")
                            if isinstance(raw_event, dict)
                            else None
                        ),
                        "news_arrival_component": (
                            raw_event.get("news_arrival_component")
                            if isinstance(raw_event, dict)
                            else None
                        ),
                        "coupled_return_jump_mark": (
                            raw_event.get("coupled_return_jump_mark")
                            if isinstance(raw_event, dict)
                            else None
                        ),
                    }
                    for raw_event in injected_events
                ],
            },
        )

    return parsed_cases
