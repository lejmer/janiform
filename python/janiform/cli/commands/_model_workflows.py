"""Internal fit and simulate CLI command-group assembly."""

from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional, cast

import click

from ..._workflow_outputs import (
    WorkflowArtifactPaths,
    generate_workflow_run_id,
    resolve_fit_artifact_paths,
    resolve_simulation_artifact_paths,
    simulation_output_preferences_from_model,
)
from ..._workflow_progress import JaniformOutputController, WorkflowProgressMode
from .calibration import _build_fit_plan_command, _build_fit_staged_command
from .simulation import _build_simulation_coarse_interval_command, _build_simulation_seeded_ticks_command
from ._shared import (
    parse_utc_datetime_cli_argument,
    require_non_empty_cli_string,
    require_positive_integer_cli_argument,
)
from ..command_runtime import (
    JaniformCommand,
    JaniformGroup,
    command_config_option,
    invoke_command_handler,
    command_option,
    invoke_group_default_handler,
    optional_path_option,
    output_option,
    progress_option,
    preset_scope_option,
    required_datetime_option,
    user_preset_json_option,
)
from ..context import CliCommandContext
from .._constants import (
    CLI_CALIBRATION_DUPLICATE_RECORD_POLICY_CHOICES,
    CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
    CLI_CALIBRATION_MARKET_DATA_SCOPE_CHOICES,
    CLI_CALIBRATION_OPTION_QUOTE_POLICY_CHOICES,
    CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
    CLI_CALIBRATION_TICK_PRICE_POLICY_CHOICES,
    CLI_FIT_ENTRYPOINT,
    CLI_FIT_INSPECT_ENTRYPOINT,
    CLI_FIT_INSPECT_INTERNAL_ERROR_ID,
    CLI_FIT_INSPECT_RESULT_SCHEMA_VERSION,
    CLI_FIT_INSPECT_VALIDATION_ERROR_ID,
    CLI_FIT_INTERNAL_ERROR_ID,
    CLI_FIT_RESULT_SCHEMA_VERSION,
    CLI_FIT_VALIDATION_ERROR_ID,
    CLI_HIGH_LEVEL_SIMULATE_ENTRYPOINT,
    CLI_HIGH_LEVEL_SIMULATE_INTERNAL_ERROR_ID,
    CLI_HIGH_LEVEL_SIMULATE_VALIDATION_ERROR_ID,
    CLI_SIMULATE_RESULT_SCHEMA_VERSION,
    CliProgressMode,
    CliExitCode,
    CliOutputMode,
)
from .._documents import load_json_document_for_cli_field
from .._errors import classify_generic_cli_command_error, cli_validation_error
from ..paths import _resolve_path_from_repository_root
from .._rendering import (
    render_fit_human_output,
    render_fit_inspect_human_output,
    render_simulate_human_output,
    write_cli_error,
    write_command_output,
)
from ...calibration.fit import (
    JaniformEstimator,
    JaniformFitReport,
    JaniformModel,
)
from ...calibration.market_data import (
    CalibrationDuplicateRecordPolicy,
    CalibrationInputFileFormat,
    CalibrationMeanReversionConfiguration,
    CalibrationMarketDataOutputPreferences,
    CalibrationMarketDataWorkflowScope,
    CalibrationOptionQuoteInputPolicy,
    CalibrationOptionSurfaceColumnMapping,
    CalibrationTickPriceInputPolicy,
)
from ...calibration.presets import (
    KnownNewsExplicitConfiguration,
    KnownNewsPresetReference,
    MarketProfileReference,
    PresetCategory,
    PresetReferenceUsageMode,
    PresetRegistry,
    PresetRegistryScope,
    SessionPresetReference,
    deserialize_preset_artifact,
)
from ...calibration.fit.estimator import _load_explicit_session_configuration
from ...calibration.fit.input_resolution import (
    HighLevelFitInference,
    HighLevelNormalizationPolicyOverrides,
    HighLevelTickDataColumnOverrides,
    build_fit_inference_payload,
    infer_high_level_fit_inputs,
)
from ...calibration.staged import CalibrationExecutionMode
from ...calibration.staged.results import build_staged_failure_context
from ...mean_reversion import MeanReversionAnchorModelKind
from ...errors import (
    ErrorContextValue,
    JaniformError,
    JaniformErrorCategory,
    JaniformErrorDetails,
    JaniformExecutionError,
)


type CliCommandArguments = dict[str, Any]


def _default_workflow_progress_mode_for_output_mode(output_mode: str) -> WorkflowProgressMode:
    if output_mode == CliOutputMode.HUMAN.value:
        return WorkflowProgressMode.HUMAN

    return WorkflowProgressMode.NONE


def _resolved_cli_progress_mode(
    *,
    output_mode: str,
    command_arguments: CliCommandArguments,
) -> WorkflowProgressMode:
    if bool(command_arguments.get("silent")):
        return WorkflowProgressMode.NONE

    requested_progress_mode = WorkflowProgressMode(
        cast(str, command_arguments.get("progress", CliProgressMode.AUTO.value))
    )
    if requested_progress_mode is WorkflowProgressMode.AUTO:
        return _default_workflow_progress_mode_for_output_mode(output_mode)

    return requested_progress_mode


def _build_cli_output_controller(
    *,
    output_mode: str,
    command_arguments: CliCommandArguments,
) -> JaniformOutputController:
    silent = bool(command_arguments.get("silent"))
    progress_mode = _resolved_cli_progress_mode(
        output_mode=output_mode,
        command_arguments=command_arguments,
    )
    return JaniformOutputController(
        suppress_info_output=silent or output_mode == CliOutputMode.JSON.value,
        progress_mode=progress_mode,
        default_progress_mode=progress_mode,
    )


@dataclass(frozen=True, slots=True)
class ConfiguredCliFitCommand:
    estimator: JaniformEstimator
    input_inference: HighLevelFitInference
    tick_data_path: Path
    option_surface_path: Optional[Path]
    option_surface_format: CalibrationInputFileFormat
    option_surface_provenance_label: str
    output_dir: Path
    explain_requested: bool


def run_fit_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    **command_arguments,
) -> int:
    try:
        configured_command = configure_cli_fit_command(
            cli_context=cli_context,
            entrypoint=CLI_FIT_ENTRYPOINT,
            error_id=CLI_FIT_VALIDATION_ERROR_ID,
            command_arguments=command_arguments,
        )
        output_controller = _build_cli_output_controller(
            output_mode=output,
            command_arguments=command_arguments,
        )
        if output == CliOutputMode.HUMAN.value and not bool(command_arguments.get("silent")):
            output_controller.info(f"fit run_id: {configured_command.input_inference.run_id}")
            output_controller.info(f"tick data: {configured_command.tick_data_path}")
            output_controller.info(f"output directory: {configured_command.output_dir}")
            output_controller.info(f"model path: {configured_command.output_dir / 'model.json'}")

        model, fit_report, artifact_paths = execute_cli_fit(
            configured_command=configured_command,
            output=output,
            command_arguments=command_arguments,
            entrypoint=CLI_FIT_ENTRYPOINT,
            error_id=CLI_FIT_INTERNAL_ERROR_ID,
        )
        payload = build_cli_fit_result_payload(
            model=model,
            fit_report=fit_report,
            artifact_paths=artifact_paths,
            command=CLI_FIT_ENTRYPOINT,
            schema_version=CLI_FIT_RESULT_SCHEMA_VERSION,
            explain_requested=bool(command_arguments.get("explain")),
        )
    except JaniformError as e:
        return write_cli_error(e, output=output)
    except Exception as e:
        return write_cli_error(
            classify_generic_cli_command_error(
                e,
                entrypoint=CLI_FIT_ENTRYPOINT,
                validation_error_id=CLI_FIT_VALIDATION_ERROR_ID,
                internal_error_id=CLI_FIT_INTERNAL_ERROR_ID,
                file_field_path="tick_data_path",
                document_field_path="fit_workflow_configuration",
            ),
            output=output,
        )

    if not bool(command_arguments.get("silent")):
        write_command_output(
            output,
            json_payload=payload,
            human_output_factory=lambda: render_fit_human_output(payload),
        )

    return int(CliExitCode.SUCCESS)


def run_fit_inspect_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    **command_arguments,
) -> int:
    try:
        payload = build_cli_fit_inspect_payload(
            cli_context=cli_context,
            entrypoint=CLI_FIT_INSPECT_ENTRYPOINT,
            error_id=CLI_FIT_INSPECT_VALIDATION_ERROR_ID,
            command_arguments=command_arguments,
        )
    except JaniformError as e:
        return write_cli_error(e, output=output)
    except Exception as e:
        return write_cli_error(
            classify_generic_cli_command_error(
                e,
                entrypoint=CLI_FIT_INSPECT_ENTRYPOINT,
                validation_error_id=CLI_FIT_INSPECT_VALIDATION_ERROR_ID,
                internal_error_id=CLI_FIT_INSPECT_INTERNAL_ERROR_ID,
                file_field_path="tick_data_path",
                document_field_path="fit_inspection_configuration",
            ),
            output=output,
        )

    if not bool(command_arguments.get("silent")):
        write_command_output(
            output,
            json_payload=payload,
            human_output_factory=lambda: render_fit_inspect_human_output(payload),
        )

    return int(CliExitCode.SUCCESS)


def run_model_simulate_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    model_path: Path,
    **command_arguments,
) -> int:
    model_path = _resolve_path_from_repository_root(
        cli_context.repository_root,
        model_path,
    )

    try:
        model = JaniformModel.load(model_path)
        payload = build_cli_simulate_result_payload(
            model=model,
            command_arguments=command_arguments,
            model_path=model_path,
            command=CLI_HIGH_LEVEL_SIMULATE_ENTRYPOINT,
            schema_version=CLI_SIMULATE_RESULT_SCHEMA_VERSION,
            entrypoint=CLI_HIGH_LEVEL_SIMULATE_ENTRYPOINT,
            error_id=CLI_HIGH_LEVEL_SIMULATE_VALIDATION_ERROR_ID,
            output_mode=output,
        )
    except JaniformError as e:
        return write_cli_error(e, output=output)
    except RuntimeError as e:
        return write_cli_error(
            cli_validation_error(
                "model_path",
                str(e),
                error_id=CLI_HIGH_LEVEL_SIMULATE_VALIDATION_ERROR_ID,
                entrypoint=CLI_HIGH_LEVEL_SIMULATE_ENTRYPOINT,
            ),
            output=output,
        )
    except Exception as e:
        return write_cli_error(
            classify_generic_cli_command_error(
                e,
                entrypoint=CLI_HIGH_LEVEL_SIMULATE_ENTRYPOINT,
                validation_error_id=CLI_HIGH_LEVEL_SIMULATE_VALIDATION_ERROR_ID,
                internal_error_id=CLI_HIGH_LEVEL_SIMULATE_INTERNAL_ERROR_ID,
                file_field_path="model_path",
                document_field_path="model_payload",
            ),
            output=output,
        )

    if not bool(command_arguments.get("silent")):
        write_command_output(
            output,
            json_payload=payload,
            human_output_factory=lambda: render_simulate_human_output(payload),
        )

    return int(CliExitCode.SUCCESS)


def build_cli_preset_registry(
    *,
    cli_context: CliCommandContext,
    user_preset_json: tuple[Path, ...],
    entrypoint: str,
    error_id: str,
) -> PresetRegistry:
    registry = PresetRegistry()

    for path in user_preset_json:
        resolved_path = _resolve_path_from_repository_root(cli_context.repository_root, path)
        try:
            preset_json = resolved_path.read_text(encoding="utf-8")
        except FileNotFoundError as e:
            raise cli_validation_error(
                "user_preset_json",
                "input file could not be found",
                error_id=error_id,
                entrypoint=entrypoint,
                context={"path": str(resolved_path)},
            ) from e
        except OSError as e:
            raise cli_validation_error(
                "user_preset_json",
                f"input file could not be read: {e}",
                error_id=error_id,
                entrypoint=entrypoint,
                context={"path": str(resolved_path)},
            ) from e

        registry.register_user_preset(deserialize_preset_artifact(preset_json))

    return registry


def parse_cli_preset_category(raw_value: Optional[str]) -> Optional[PresetCategory]:
    if raw_value is None:
        return None

    return PresetCategory(raw_value.replace("-", "_"))


def parse_cli_preset_scope(raw_value: Optional[str]) -> Optional[PresetRegistryScope]:
    if raw_value is None or raw_value == "auto":
        return None

    return PresetRegistryScope(raw_value)


def resolve_named_preset_scope(
    *,
    registry: PresetRegistry,
    category: PresetCategory,
    preset_name: str,
    raw_scope: Optional[str],
) -> PresetRegistryScope:
    explicit_scope = parse_cli_preset_scope(raw_scope)
    resolved_scope, _ = registry.resolve(
        category=category,
        preset_name=preset_name,
        registry_scope=explicit_scope,
    )
    return resolved_scope


def resolve_preset_artifact(
    *,
    registry: PresetRegistry,
    category: PresetCategory,
    preset_name: str,
    registry_scope: PresetRegistryScope,
) -> Any:
    _, artifact = registry.resolve(
        category=category,
        preset_name=preset_name,
        registry_scope=registry_scope,
    )
    return artifact


def configure_cli_fit_command(
    *,
    cli_context: CliCommandContext,
    entrypoint: str,
    error_id: str,
    command_arguments: CliCommandArguments,
) -> ConfiguredCliFitCommand:
    registry = build_cli_preset_registry(
        cli_context=cli_context,
        user_preset_json=cast(tuple[Path, ...], command_arguments.get("user_preset_json", ())),
        entrypoint=entrypoint,
        error_id=error_id,
    )
    market_profile = build_market_profile_reference_from_arguments(
        command_arguments,
        registry=registry,
        entrypoint=entrypoint,
        error_id=error_id,
    )
    session_reference = build_session_reference_from_arguments(
        command_arguments,
        repository_root=cli_context.repository_root,
        registry=registry,
        entrypoint=entrypoint,
        error_id=error_id,
    )
    known_news_reference = build_known_news_reference_from_arguments(
        command_arguments,
        repository_root=cli_context.repository_root,
        registry=registry,
        entrypoint=entrypoint,
        error_id=error_id,
    )

    tick_data_path = resolve_required_tick_data_path(
        cli_context=cli_context,
        entrypoint=entrypoint,
        error_id=error_id,
        raw_tick_data_path=cast(Optional[Path], command_arguments.get("tick_data_path")),
    )
    option_surface_path = cast(Optional[Path], command_arguments.get("option_surface_path"))
    resolved_option_surface_path = (
        _resolve_path_from_repository_root(cli_context.repository_root, option_surface_path)
        if option_surface_path is not None
        else None
    )
    option_surface_format = (
        CalibrationInputFileFormat.PARQUET
        if command_arguments.get("option_surface_format") is None
        else CalibrationInputFileFormat(cast(str, command_arguments["option_surface_format"]))
    )
    option_surface_provenance_label = (
        "option_surface"
        if command_arguments.get("option_surface_provenance_label") is None
        else cast(str, command_arguments["option_surface_provenance_label"])
    )

    reject_orphan_market_data_arguments(
        resolved_option_surface_path is not None,
        (
            ("option_surface_format", command_arguments.get("option_surface_format")),
            ("option_surface_provenance_label", command_arguments.get("option_surface_provenance_label")),
            ("option_quote_input_policy", command_arguments.get("option_quote_input_policy")),
            ("option_quote_timestamp_column", command_arguments.get("option_quote_timestamp_column")),
            ("option_expiry_column", command_arguments.get("option_expiry_column")),
            ("option_strike_column", command_arguments.get("option_strike_column")),
            ("option_type_column", command_arguments.get("option_type_column")),
            ("option_mid_price_column", command_arguments.get("option_mid_price_column")),
            ("option_bid_column", command_arguments.get("option_bid_column")),
            ("option_ask_column", command_arguments.get("option_ask_column")),
            ("option_implied_volatility_column", command_arguments.get("option_implied_volatility_column")),
        ),
        message="option-surface flags require --option-surface-path",
    )

    workflow_scope = CalibrationMarketDataWorkflowScope(
        cast(
            str,
            command_arguments.get("workflow_scope", "full_without_option_surface"),
        )
    )
    option_surface_column_mapping = build_option_surface_column_mapping_from_arguments(
        command_arguments,
        entrypoint=entrypoint,
        error_id=error_id,
    )
    output_preferences = build_model_workflow_output_preferences_from_arguments(
        command_arguments,
        repository_root=cli_context.repository_root,
        entrypoint=entrypoint,
        error_id=error_id,
    )
    mean_reversion = build_mean_reversion_configuration_from_arguments(
        command_arguments,
        workflow_scope=workflow_scope,
        entrypoint=entrypoint,
        error_id=error_id,
    )

    preliminary_estimator = JaniformEstimator(
        workflow_scope=workflow_scope,
        option_surface_column_mapping=option_surface_column_mapping,
        output_preferences=output_preferences,
        market_profile=market_profile,
        session=session_reference,
        known_news=known_news_reference,
        exchange_code=cast(Optional[str], command_arguments.get("exchange_code")),
        session_timezone=cast(Optional[str], command_arguments.get("session_timezone")),
        registry=registry,
        cpu_count=cpu_count_from_arguments(
            command_arguments,
            entrypoint=entrypoint,
            error_id=error_id,
        ),
        silent=bool(command_arguments.get("silent")),
    )
    input_inference = infer_high_level_fit_inputs(
        tick_data_path=tick_data_path,
        tick_data_format=optional_tick_data_format_from_arguments(command_arguments),
        tick_data_provenance_label=tick_data_provenance_label_from_arguments(command_arguments),
        workflow_scope=workflow_scope,
        run_id=cast(Optional[str], command_arguments.get("run_id")),
        column_overrides=build_high_level_tick_data_column_overrides_from_arguments(command_arguments),
        normalization_overrides=build_high_level_normalization_overrides_from_arguments(command_arguments),
        market_profile_configuration=preliminary_estimator._resolve_market_profile_configuration(),
    )

    estimator = JaniformEstimator(
        workflow_scope=workflow_scope,
        tick_data_column_mapping=input_inference.tick_data_column_mapping,
        normalization_policy=input_inference.normalization_policy,
        option_surface_column_mapping=option_surface_column_mapping,
        output_preferences=output_preferences,
        mean_reversion=mean_reversion,
        market_profile=market_profile,
        session=session_reference,
        known_news=known_news_reference,
        exchange_code=cast(Optional[str], command_arguments.get("exchange_code")),
        session_timezone=cast(Optional[str], command_arguments.get("session_timezone")),
        registry=registry,
        cpu_count=cpu_count_from_arguments(
            command_arguments,
            entrypoint=entrypoint,
            error_id=error_id,
        ),
        silent=bool(command_arguments.get("silent")),
    )

    raw_output_dir = cast(Optional[Path], command_arguments.get("output_dir"))
    output_dir = (
        _resolve_path_from_repository_root(cli_context.repository_root, raw_output_dir)
        if raw_output_dir is not None
        else resolve_fit_artifact_paths(run_id=input_inference.run_id, output_dir=None).run_directory
    )

    return ConfiguredCliFitCommand(
        estimator=estimator,
        input_inference=input_inference,
        tick_data_path=tick_data_path,
        option_surface_path=resolved_option_surface_path,
        option_surface_format=option_surface_format,
        option_surface_provenance_label=option_surface_provenance_label,
        output_dir=output_dir,
        explain_requested=bool(command_arguments.get("explain")),
    )


def resolve_required_tick_data_path(
    *,
    cli_context: CliCommandContext,
    entrypoint: str,
    error_id: str,
    raw_tick_data_path: Optional[Path],
) -> Path:
    if raw_tick_data_path is None:
        raise cli_validation_error(
            "tick_data_path",
            "Please provide a tick-data path as `janiform fit PATH ...` or with --tick-data-path.",
            error_id=error_id,
            entrypoint=entrypoint,
        )

    return _resolve_path_from_repository_root(cli_context.repository_root, raw_tick_data_path)


def optional_tick_data_format_from_arguments(
    command_arguments: CliCommandArguments,
) -> Optional[CalibrationInputFileFormat]:
    raw_tick_data_format = cast(Optional[str], command_arguments.get("tick_data_format"))
    if raw_tick_data_format is None:
        return None

    return CalibrationInputFileFormat(raw_tick_data_format)


def tick_data_provenance_label_from_arguments(command_arguments: CliCommandArguments) -> str:
    return cast(str, command_arguments.get("tick_data_provenance_label", "tick_data"))


def cpu_count_from_arguments(
    command_arguments: CliCommandArguments,
    *,
    entrypoint: str,
    error_id: str,
) -> Optional[int]:
    raw_cpu_count = cast(Optional[int], command_arguments.get("cpu_count"))
    if raw_cpu_count is None:
        return None

    return require_positive_integer_cli_argument(
        raw_cpu_count,
        field_path="cpu_count",
        error_id=error_id,
        entrypoint=entrypoint,
    )


def build_high_level_tick_data_column_overrides_from_arguments(
    parsed_arguments: CliCommandArguments,
) -> Optional[HighLevelTickDataColumnOverrides]:
    overrides = HighLevelTickDataColumnOverrides(
        timestamp_column=cast(Optional[str], parsed_arguments.get("tick_timestamp_column")),
        price_column=cast(Optional[str], parsed_arguments.get("tick_price_column")),
        trade_sign_column=cast(Optional[str], parsed_arguments.get("tick_trade_sign_column")),
        bid_column=cast(Optional[str], parsed_arguments.get("tick_bid_column")),
        ask_column=cast(Optional[str], parsed_arguments.get("tick_ask_column")),
    )

    if all(
        field_value is None
        for field_value in (
            overrides.timestamp_column,
            overrides.price_column,
            overrides.trade_sign_column,
            overrides.bid_column,
            overrides.ask_column,
        )
    ):
        return None

    return overrides


def build_high_level_normalization_overrides_from_arguments(
    parsed_arguments: CliCommandArguments,
) -> Optional[HighLevelNormalizationPolicyOverrides]:
    duplicate_record_policy_raw = cast(Optional[str], parsed_arguments.get("duplicate_record_policy"))
    tick_price_input_policy_raw = cast(Optional[str], parsed_arguments.get("tick_price_input_policy"))
    option_quote_input_policy_raw = cast(Optional[str], parsed_arguments.get("option_quote_input_policy"))

    overrides = HighLevelNormalizationPolicyOverrides(
        timestamp_timezone=cast(Optional[str], parsed_arguments.get("timestamp_timezone")),
        sort_records_by_timestamp=(
            False
            if bool(parsed_arguments.get("disable_timestamp_sort"))
            else None
        ),
        duplicate_record_policy=(
            CalibrationDuplicateRecordPolicy(duplicate_record_policy_raw)
            if duplicate_record_policy_raw is not None
            else None
        ),
        tick_price_input_policy=(
            CalibrationTickPriceInputPolicy(tick_price_input_policy_raw)
            if tick_price_input_policy_raw is not None
            else None
        ),
        option_quote_input_policy=(
            CalibrationOptionQuoteInputPolicy(option_quote_input_policy_raw)
            if option_quote_input_policy_raw is not None
            else None
        ),
    )

    if all(
        field_value is None
        for field_value in (
            overrides.timestamp_timezone,
            overrides.sort_records_by_timestamp,
            overrides.duplicate_record_policy,
            overrides.tick_price_input_policy,
            overrides.option_quote_input_policy,
        )
    ):
        return None

    return overrides


def build_cli_fit_input_inference_payload(prepared_workflow: Any) -> dict[str, Any]:
    return build_fit_inference_payload(
        inference=prepared_workflow.input_inference,
        workflow_scope=prepared_workflow.prepared.request.workflow_scope,
        market_profile_name=(
            prepared_workflow.resolved_configuration.market_profile.selected_preset.preset_name
            if (
                prepared_workflow.resolved_configuration.market_profile is not None
                and prepared_workflow.resolved_configuration.market_profile.selected_preset is not None
            )
            else None
        ),
        known_news_preset_name=(
            prepared_workflow.resolved_configuration.known_news.selected_preset.preset_name
            if prepared_workflow.resolved_configuration.known_news.selected_preset is not None
            else None
        ),
        known_news_recommended_name=(
            prepared_workflow.resolved_configuration.known_news.recommended_reference.preset_name
            if prepared_workflow.resolved_configuration.known_news.recommended_reference is not None
            else None
        ),
        known_news_recommendation_applied=(
            prepared_workflow.resolved_configuration.known_news.recommendation_applied
        ),
        session_source_kind=prepared_workflow.resolved_configuration.session.source_kind.value,
        known_news_source_kind=prepared_workflow.resolved_configuration.known_news.source_kind.value,
    )


def execute_cli_fit(
    *,
    configured_command: ConfiguredCliFitCommand,
    output: str,
    command_arguments: CliCommandArguments,
    entrypoint: str,
    error_id: str,
) -> tuple[JaniformModel, JaniformFitReport, WorkflowArtifactPaths]:
    execution_silent = bool(command_arguments.get("silent")) or output == CliOutputMode.JSON.value

    model_result, fit_report = cast(
        tuple[Optional[JaniformModel], JaniformFitReport],
        configured_command.estimator.fit(
            run_id=configured_command.input_inference.run_id,
            tick_data_path=configured_command.tick_data_path,
            tick_data_format=configured_command.input_inference.tick_data_source.file_format,
            tick_data_provenance_label=configured_command.input_inference.tick_data_source.provenance_label,
            option_surface_path=configured_command.option_surface_path,
            option_surface_format=configured_command.option_surface_format,
            option_surface_provenance_label=configured_command.option_surface_provenance_label,
            output_dir=configured_command.output_dir,
            silent=execution_silent,
            report=True,
        ),
    )
    if model_result is None:
        failure_context = build_cli_fit_failure_context(fit_report)
        raise JaniformExecutionError(
            JaniformErrorDetails.from_context(
                error_id=error_id,
                category=JaniformErrorCategory.EXECUTION,
                workflow="cli",
                entrypoint=entrypoint,
                message="high-level fit failed before a JaniformModel could be materialized",
                context=failure_context,
            )
        )

    artifact_paths = resolve_fit_artifact_paths(
        run_id=model_result.run_id,
        output_dir=configured_command.output_dir,
    )

    return model_result, fit_report, artifact_paths


def build_cli_fit_failure_context(fit_report: JaniformFitReport) -> dict[str, ErrorContextValue]:
    return build_staged_failure_context(fit_report.staged_response)


def build_cli_fit_inspect_payload(
    *,
    cli_context: CliCommandContext,
    entrypoint: str,
    error_id: str,
    command_arguments: CliCommandArguments,
) -> dict[str, Any]:
    configured_command = configure_cli_fit_command(
        cli_context=cli_context,
        entrypoint=entrypoint,
        error_id=error_id,
        command_arguments=command_arguments,
    )

    with configured_command.estimator._stdout_suppression_context():
        prepared_workflow = configured_command.estimator._prepare_fit_workflow(
            run_id=configured_command.input_inference.run_id,
            tick_data_path=configured_command.tick_data_path,
            tick_data_format=configured_command.input_inference.tick_data_source.file_format,
            tick_data_provenance_label=configured_command.input_inference.tick_data_source.provenance_label,
            option_surface_path=configured_command.option_surface_path,
            option_surface_format=configured_command.option_surface_format,
            option_surface_provenance_label=configured_command.option_surface_provenance_label,
        )

    try:
        return {
            "schema_version": CLI_FIT_INSPECT_RESULT_SCHEMA_VERSION,
            "command": CLI_FIT_INSPECT_ENTRYPOINT,
            "run_id": prepared_workflow.run_id,
            "tick_data_path": str(configured_command.tick_data_path),
            "output_directory": str(configured_command.output_dir),
            "model_output_path": str(configured_command.output_dir / "model.json"),
            "report_path": str(configured_command.output_dir / "fit_report.json"),
            "input_inference": build_cli_fit_input_inference_payload(prepared_workflow),
            "resolved_configuration": prepared_workflow.resolved_configuration.to_payload(),
            "session_resolution": prepared_workflow.prepared.session_resolution.to_payload(),
            "preparation_diagnostics": prepared_workflow.prepared.diagnostics.to_payload(),
            "preparation_provenance": prepared_workflow.prepared.provenance.to_payload(),
            "execution_mode": prepared_workflow.prepared.staged_request.execution_mode.value,
            "temporary_workspace_materialized": prepared_workflow.temporary_directory is not None,
        }
    finally:
        if prepared_workflow.temporary_directory is not None:
            prepared_workflow.temporary_directory.cleanup()


def build_market_profile_reference_from_arguments(
    command_arguments: CliCommandArguments,
    *,
    registry: PresetRegistry,
    entrypoint: str,
    error_id: str,
) -> Optional[MarketProfileReference]:
    market_name = cast(Optional[str], command_arguments.get("market"))
    raw_scope = cast(Optional[str], command_arguments.get("market_scope"))

    if market_name is None:
        if raw_scope not in (None, "auto"):
            raise cli_validation_error(
                "market_scope",
                "--market-scope requires --market",
                error_id=error_id,
                entrypoint=entrypoint,
            )

        return None

    resolved_scope = resolve_named_preset_scope(
        registry=registry,
        category=PresetCategory.MARKET_PROFILE,
        preset_name=market_name,
        raw_scope=raw_scope,
    )
    return MarketProfileReference(
        usage_mode=PresetReferenceUsageMode.NAMED,
        registry_scope=resolved_scope,
        preset_name=market_name,
    )


def build_session_reference_from_arguments(
    command_arguments: CliCommandArguments,
    *,
    repository_root: Path,
    registry: PresetRegistry,
    entrypoint: str,
    error_id: str,
) -> Optional[SessionPresetReference]:
    session_preset = cast(Optional[str], command_arguments.get("session_preset"))
    session_json = cast(Optional[Path], command_arguments.get("session_json"))
    raw_scope = cast(Optional[str], command_arguments.get("session_scope"))

    if session_preset is None and session_json is None:
        if raw_scope not in (None, "auto"):
            raise cli_validation_error(
                "session_scope",
                "--session-scope requires --session-preset",
                error_id=error_id,
                entrypoint=entrypoint,
            )

        return None

    if session_json is not None:
        resolved_session_path = _resolve_path_from_repository_root(repository_root, session_json)
        return SessionPresetReference(
            usage_mode=PresetReferenceUsageMode.EXPLICIT,
            configuration=_load_explicit_session_configuration(
                resolved_session_path,
                exchange_code=cast(Optional[str], command_arguments.get("exchange_code")),
            ),
        )

    resolved_scope = resolve_named_preset_scope(
        registry=registry,
        category=PresetCategory.SESSION,
        preset_name=cast(str, session_preset),
        raw_scope=raw_scope,
    )

    return SessionPresetReference(
        usage_mode=PresetReferenceUsageMode.NAMED,
        registry_scope=resolved_scope,
        preset_name=cast(str, session_preset),
    )


def build_known_news_reference_from_arguments(
    command_arguments: CliCommandArguments,
    *,
    repository_root: Path,
    registry: PresetRegistry,
    entrypoint: str,
    error_id: str,
) -> Optional[KnownNewsPresetReference]:
    known_news = cast(Optional[str], command_arguments.get("known_news"))
    known_news_json = cast(Optional[Path], command_arguments.get("known_news_json"))
    raw_scope = cast(Optional[str], command_arguments.get("known_news_scope"))

    if known_news is None and known_news_json is None:
        if raw_scope not in (None, "auto"):
            raise cli_validation_error(
                "known_news_scope",
                "--known-news-scope requires --known-news",
                error_id=error_id,
                entrypoint=entrypoint,
            )

        return None

    if known_news_json is not None:
        explicit_configuration_document = load_json_document_for_cli_field(
            repository_root=repository_root,
            path=known_news_json,
            context="known-news configuration",
            field_path="known_news_json",
            error_id=error_id,
            entrypoint=entrypoint,
        )
        return KnownNewsPresetReference(
            usage_mode=PresetReferenceUsageMode.EXPLICIT,
            configuration=KnownNewsExplicitConfiguration.from_mapping(
                explicit_configuration_document,
                field_path="known_news_json",
            ),
        )

    resolved_scope = resolve_named_preset_scope(
        registry=registry,
        category=PresetCategory.KNOWN_NEWS,
        preset_name=cast(str, known_news),
        raw_scope=raw_scope,
    )

    return KnownNewsPresetReference(
        usage_mode=PresetReferenceUsageMode.NAMED,
        registry_scope=resolved_scope,
        preset_name=cast(str, known_news),
    )

def build_option_surface_column_mapping_from_arguments(
    parsed_arguments: CliCommandArguments,
    *,
    entrypoint: str,
    error_id: str,
) -> Optional[CalibrationOptionSurfaceColumnMapping]:
    option_surface_path = cast(Optional[Path], parsed_arguments.get("option_surface_path"))
    if option_surface_path is None:
        return None

    return CalibrationOptionSurfaceColumnMapping(
        quote_timestamp_column=require_non_empty_cli_string(
            cast(str, parsed_arguments["option_quote_timestamp_column"]),
            field_path="option_quote_timestamp_column",
            error_id=error_id,
            entrypoint=entrypoint,
        ),
        expiry_timestamp_column=require_non_empty_cli_string(
            cast(str, parsed_arguments["option_expiry_column"]),
            field_path="option_expiry_column",
            error_id=error_id,
            entrypoint=entrypoint,
        ),
        strike_column=require_non_empty_cli_string(
            cast(str, parsed_arguments["option_strike_column"]),
            field_path="option_strike_column",
            error_id=error_id,
            entrypoint=entrypoint,
        ),
        option_type_column=require_non_empty_cli_string(
            cast(str, parsed_arguments["option_type_column"]),
            field_path="option_type_column",
            error_id=error_id,
            entrypoint=entrypoint,
        ),
        mid_price_column=cast(Optional[str], parsed_arguments.get("option_mid_price_column")),
        bid_column=cast(Optional[str], parsed_arguments.get("option_bid_column")),
        ask_column=cast(Optional[str], parsed_arguments.get("option_ask_column")),
        implied_volatility_column=cast(Optional[str], parsed_arguments.get("option_implied_volatility_column")),
    )

def build_model_workflow_output_preferences_from_arguments(
    parsed_arguments: CliCommandArguments,
    *,
    repository_root: Path,
    entrypoint: str,
    error_id: str,
) -> CalibrationMarketDataOutputPreferences:
    selected_execution_mode = CalibrationExecutionMode(cast(str, parsed_arguments["execution_mode"]))

    raw_disk_checkpoint_directory = cast(Optional[Path], parsed_arguments.get("disk_checkpoint_directory"))

    disk_checkpoint_directory = (
        _resolve_path_from_repository_root(repository_root, raw_disk_checkpoint_directory)
        if raw_disk_checkpoint_directory is not None
        else None
    )

    if selected_execution_mode.value == "disk" and disk_checkpoint_directory is None:
        raise cli_validation_error(
            "disk_checkpoint_directory",
            "--disk-checkpoint-directory is required when --execution-mode disk is selected",
            error_id=error_id,
            entrypoint=entrypoint,
        )
    if selected_execution_mode.value == "memory" and disk_checkpoint_directory is not None:
        raise cli_validation_error(
            "disk_checkpoint_directory",
            "--disk-checkpoint-directory requires --execution-mode disk",
            error_id=error_id,
            entrypoint=entrypoint,
        )

    return CalibrationMarketDataOutputPreferences(
        execution_mode=selected_execution_mode,
        disk_checkpoint_directory=None if disk_checkpoint_directory is None else str(disk_checkpoint_directory),
        emit_derived_staged_execution_input=bool(parsed_arguments.get("emit_derived_staged_execution_input")),
        emit_input_provenance_report=not bool(parsed_arguments.get("omit_input_provenance_report")),
    )


def build_cli_fit_result_payload(
    *,
    model: JaniformModel,
    fit_report: JaniformFitReport,
    artifact_paths: WorkflowArtifactPaths,
    command: str,
    schema_version: str,
    explain_requested: bool,
) -> dict[str, Any]:
    return {
        "schema_version": schema_version,
        "command": command,
        "run_id": model.run_id,
        "run_directory": str(artifact_paths.run_directory),
        "model_output_path": str(artifact_paths.primary_output_path),
        "report_path": str(artifact_paths.report_path),
        "manifest_path": str(artifact_paths.manifest_path),
        "model_summary": model.summary(),
        "input_inference": cast(dict[str, Any], model.summary()["input_inference"]),
        "explain_requested": explain_requested,
        "fit_report_summary": fit_report.summary(),
        "resolved_configuration": fit_report.resolved_configuration.to_payload(),
        "session_resolution": fit_report.session_resolution.to_payload(),
        "preparation_diagnostics": fit_report.preparation_diagnostics.to_payload(),
        "preparation_provenance": fit_report.preparation_provenance.to_payload(),
        "staged_status": fit_report.staged_response.status.value,
    }


def build_cli_simulate_result_payload(
    *,
    model: JaniformModel,
    command_arguments: CliCommandArguments,
    model_path: Optional[Path],
    command: str,
    schema_version: str,
    entrypoint: str,
    error_id: str,
    output_mode: str,
) -> dict[str, Any]:
    start_time_utc = parse_utc_datetime_cli_argument(
        cast(str, command_arguments["start_time_utc"]),
        field_path="start_time_utc",
        error_id=error_id,
        entrypoint=entrypoint,
    )

    horizon_interval_count = require_positive_integer_cli_argument(
        int(cast(int, command_arguments["horizon_interval_count"])),
        field_path="horizon_interval_count",
        error_id=error_id,
        entrypoint=entrypoint,
    )
    sample_every_nth_transition = require_positive_integer_cli_argument(
        int(cast(int, command_arguments["sample_every_nth_transition"])),
        field_path="sample_every_nth_transition",
        error_id=error_id,
        entrypoint=entrypoint,
    )
    max_records_per_chunk = require_positive_integer_cli_argument(
        int(cast(int, command_arguments["max_records_per_chunk"])),
        field_path="max_records_per_chunk",
        error_id=error_id,
        entrypoint=entrypoint,
    )

    interval_value = cast(Optional[str], command_arguments.get("interval"))
    simulation_run_id = generate_workflow_run_id(f"{model.run_id}_simulate")
    output_path_argument = cast(Optional[Path], command_arguments.get("output_path"))
    resolved_output_path_argument = (
        _resolve_path_from_repository_root(Path.cwd(), output_path_argument)
        if output_path_argument is not None
        else None
    )
    output_preferences = simulation_output_preferences_from_model(model)
    artifact_paths = resolve_simulation_artifact_paths(
        run_id=simulation_run_id,
        output_path=resolved_output_path_argument,
        default_file_format=output_preferences.file_format,
        interval_label=None if interval_value is None else interval_value,
    )

    resolved_progress_mode = _resolved_cli_progress_mode(
        output_mode=output_mode,
        command_arguments=command_arguments,
    )
    output_controller = _build_cli_output_controller(
        output_mode=output_mode,
        command_arguments=command_arguments,
    )
    if output_mode == CliOutputMode.HUMAN.value and not bool(command_arguments.get("silent")):
        output_controller.info(f"simulate run_id: {simulation_run_id}")
        output_controller.info(f"model path: {model_path}")
        output_controller.info(f"primary output: {artifact_paths.primary_output_path}")
        output_controller.info(f"report path: {artifact_paths.report_path}")

    simulation_result = model.simulate(
        seed=int(cast(int, command_arguments["seed"])),
        start_time_utc=start_time_utc,
        horizon_interval_count=horizon_interval_count,
        interval=interval_value,
        output_path=artifact_paths.primary_output_path,
        debug_trace_enabled=bool(command_arguments.get("debug_trace_enabled")),
        sample_every_nth_transition=sample_every_nth_transition,
        max_records_per_chunk=max_records_per_chunk,
        silent=bool(command_arguments.get("silent")),
        progress_mode=resolved_progress_mode,
    )

    return {
        "schema_version": schema_version,
        "command": command,
        "model_path": None if model_path is None else str(model_path),
        "run_id": simulation_run_id,
        "simulation_kind": simulation_result.simulation_kind,
        "primary_output_path": str(artifact_paths.primary_output_path),
        "report_path": str(artifact_paths.report_path),
        "manifest_path": str(artifact_paths.manifest_path),
        "run_directory": str(artifact_paths.run_directory),
        "model_summary": model.summary(),
        "resolved_configuration": model.resolved_configuration.to_payload(),
        "session_resolution": model.session_resolution.to_payload(),
        "simulation_diagnostics": simulation_result.diagnostics,
    }


def reject_orphan_market_data_arguments(
    has_primary_path: bool,
    optional_arguments: tuple[tuple[str, Any], ...],
    *,
    message: str,
) -> None:
    if has_primary_path:
        return

    for _, argument_value in optional_arguments:
        if argument_value is not None:
            raise ValueError(message)


def build_mean_reversion_configuration_from_arguments(
    command_arguments: CliCommandArguments,
    *,
    workflow_scope: CalibrationMarketDataWorkflowScope,
    entrypoint: str,
    error_id: str,
) -> Optional[CalibrationMeanReversionConfiguration]:
    raw_anchor_model = cast(Optional[str], command_arguments.get("mean_reversion_anchor_model"))
    raw_grid_interval_hours = cast(Optional[float], command_arguments.get("mean_reversion_grid_interval_hours"))

    if workflow_scope == CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY:
        if raw_grid_interval_hours is not None:
            raise cli_validation_error(
                "mean_reversion_grid_interval_hours",
                "--mean-reversion-grid-interval-hours is not supported when --workflow-scope is seasonality_only",
                error_id=error_id,
                entrypoint=entrypoint,
            )
        if raw_anchor_model is None:
            return None

        raise cli_validation_error(
            "mean_reversion_anchor_model",
            "--mean-reversion-anchor-model is not supported when --workflow-scope is seasonality_only",
            error_id=error_id,
            entrypoint=entrypoint,
        )

    anchor_model_kind = (
        MeanReversionAnchorModelKind.ENDOGENOUS_CORE_PRESSURE_LEVEL
        if raw_anchor_model is None
        else MeanReversionAnchorModelKind(raw_anchor_model)
    )

    try:
        return CalibrationMeanReversionConfiguration(
            anchor_model_kind=anchor_model_kind,
            calibration_grid_interval_hours=raw_grid_interval_hours,
        )
    except Exception as e:
        raise cli_validation_error(
            "mean_reversion_anchor_model",
            str(e),
            error_id=error_id,
            entrypoint=entrypoint,
        ) from e


def build_fit_group(*, environment: Optional[Mapping[str, str]] = None) -> click.Group:
    @click.group(
        cls=JaniformGroup,
        command_path=("fit",),
        environment=environment,
        help=(
            "Fit a Janiform model from market data.\n\n"
            "Run `janiform fit` with the options below for the default end-to-end fitting "
            "workflow. Use `fit plan` to inspect the derived fit plan before execution, and "
            "use `fit staged` only when you need direct control over the staged execution boundary."
        ),
        invoke_without_command=True,
        params=model_workflow_fit_options(enforce_required=False),
    )
    @click.pass_context
    def fit(ctx: click.Context, **command_arguments) -> Optional[int]:
        return invoke_group_default_handler(
            ctx,
            handler=run_fit_command,
            command_path=("fit",),
            command_arguments=dict(command_arguments),
            required_option_names=("tick_data_path",),
            exclusivity_groups=(("session_preset", "session_json"), ("known_news", "known_news_json")),
        )

    fit_group = fit

    @click.command(
        "inspect",
        cls=JaniformCommand,
        command_path=("fit", "inspect"),
        environment=environment,
        help="Inspect the inferred fit configuration without running calibration.",
        params=model_workflow_fit_options(
            enforce_required=False,
            include_output_dir=True,
            include_explain=False,
        ),
    )
    @click.pass_context
    def fit_inspect(ctx: click.Context, **command_arguments) -> int:
        return invoke_command_handler(
            ctx,
            handler=run_fit_inspect_command,
            command_path=("fit", "inspect"),
            command_arguments=dict(command_arguments),
            exclusivity_groups=(("session_preset", "session_json"), ("known_news", "known_news_json")),
        )

    fit_group.add_command(fit_inspect)
    fit_group.add_command(_build_fit_plan_command(environment=environment))
    fit_group.add_command(_build_fit_staged_command(environment=environment))

    return fit_group


def build_simulate_group(*, environment: Optional[Mapping[str, str]] = None) -> click.Group:
    @click.group(
        cls=JaniformGroup,
        command_path=("simulate",),
        environment=environment,
        help=(
            "Simulate from a persisted Janiform model.\n\n"
            "Run `janiform simulate` without --interval for the default seeded tick workflow. "
            "Provide --interval for the coarse interval workflow. Use `simulate coarse-interval` "
            "or `simulate seeded-ticks` when you need direct control over those explicit contracts."
        ),
        invoke_without_command=True,
        params=model_workflow_simulate_options(
            require_model_path=True,
            enforce_required=False,
        ),
    )
    @click.pass_context
    def simulate(ctx: click.Context, **command_arguments) -> Optional[int]:
        return invoke_group_default_handler(
            ctx,
            handler=run_model_simulate_command,
            command_path=("simulate",),
            command_arguments=dict(command_arguments),
            required_option_names=("model_path", "seed", "start_time_utc", "horizon_interval_count"),
        )

    simulate_group = simulate

    simulate_group.add_command(_build_simulation_coarse_interval_command(environment=environment))
    simulate_group.add_command(_build_simulation_seeded_ticks_command(environment=environment))

    return simulate_group


def model_workflow_fit_options(
    *,
    enforce_required: bool = True,
    include_output_dir: bool = True,
    include_explain: bool = True,
) -> list[click.Parameter]:
    return [
        command_config_option(),
        output_option(default_output_mode=CliOutputMode.HUMAN),
        user_preset_json_option(),
        command_option(
            "--market",
            help_text="Optional market-profile preset name used as the high-level workflow baseline.",
        ),
        preset_scope_option(
            "--market-scope",
            help_text="Registry scope for --market. Default: auto.",
            include_auto=True,
        ),
        optional_path_option(
            "--session-json",
            help_text=(
                "Optional explicit session JSON override. Accepts a calendar.registry.v1 document or "
                "a SessionExplicitConfiguration JSON object."
            ),
        ),
        command_option(
            "--session-preset",
            help_text="Optional session preset name. When omitted, Janiform may infer the session from data.",
        ),
        preset_scope_option(
            "--session-scope",
            help_text="Registry scope for --session-preset. Default: auto.",
            include_auto=True,
        ),
        command_option(
            "--known-news",
            help_text=(
                "Optional known-news preset name. Use `none` to keep the bundled off-by-default "
                "baseline explicit."
            ),
        ),
        optional_path_option(
            "--known-news-json",
            help_text=(
                "Optional explicit known-news configuration JSON override using the "
                "KnownNewsExplicitConfiguration shape."
            ),
        ),
        preset_scope_option(
            "--known-news-scope",
            help_text="Registry scope for --known-news. Default: auto.",
            include_auto=True,
        ),
        command_option(
            "--exchange-code",
            help_text=(
                "Optional exchange-code hint for session inference when neither the selected market "
                "profile nor explicit session input already supplies it."
            ),
        ),
        command_option(
            "--session-timezone",
            help_text="Optional timezone hint for automatic session inference.",
        ),
        command_option(
            "--run-id",
            help_text=(
                "Stable run identifier recorded on the fitted model and fit report. "
                "Default: inferred from the tick-data file stem plus a UTC timestamp."
            ),
        ),
        command_option(
            "--workflow-scope",
            choice_options=CLI_CALIBRATION_MARKET_DATA_SCOPE_CHOICES,
            default="full_without_option_surface",
            help_text=(
                "High-level calibration scope. Use seasonality_only for activity-only estimation, "
                "full_without_option_surface for full calibration from tick data only, and "
                "full_with_option_surface when option-surface data is supplied. "
                "Default: full_without_option_surface."
            ),
        ),
        command_option(
            "--tick-data-path",
            help_text="Path to the input tick-data file.",
            path_type=True,
            required=enforce_required,
        ),
        command_option(
            "--tick-data-format",
            choice_options=CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
            help_text="Tick-data file format. Default: inferred from the file extension.",
        ),
        command_option(
            "--tick-data-provenance-label",
            default="tick_data",
            help_text="Stable provenance label recorded for the tick-data source. Default: tick_data.",
        ),
        command_option(
            "--tick-timestamp-column",
            help_text="Tick-data timestamp column name override. Default: auto-detected from common aliases.",
        ),
        command_option(
            "--tick-price-input-policy",
            choice_options=CLI_CALIBRATION_TICK_PRICE_POLICY_CHOICES,
            help_text="Explicit tick-price normalization policy for non-seasonality workflows.",
        ),
        command_option("--tick-price-column", help_text="Tick-data explicit price column."),
        command_option(
            "--tick-trade-sign-column",
            help_text="Tick-data trade sign column required for canonical full-model Hawkes calibration.",
        ),
        command_option("--tick-bid-column", help_text="Tick-data bid column."),
        command_option("--tick-ask-column", help_text="Tick-data ask column."),
        command_option(
            "--timestamp-timezone",
            help_text=(
                "Timezone used to normalize tick-data timestamps before fitting. "
                "Default: inferred from the selected market profile, otherwise UTC."
            ),
        ),
        command_option(
            "--duplicate-record-policy",
            choice_options=CLI_CALIBRATION_DUPLICATE_RECORD_POLICY_CHOICES,
            help_text="Optional tick-data deduplication policy. Omit to preserve duplicate tick rows.",
        ),
        command_option(
            "--disable-timestamp-sort",
            help_text="Record that source data should not be re-sorted by timestamp before translation.",
            is_flag=True,
        ),
        command_option(
            "--silent",
            help_text="Suppress successful command output and silence non-error stdout from the high-level fit path.",
            is_flag=True,
        ),
        command_option(
            "--cpu-count",
            help_text=(
                "CPU cores to use when evaluating calibration candidates. "
                "Default: all available logical CPUs."
            ),
            value_type=int,
        ),
        command_option(
            "--mean-reversion-anchor-model",
            choice_options=tuple(anchor_model_kind.value for anchor_model_kind in MeanReversionAnchorModelKind),
            help_text=(
                "Optional explicit endogenous anchor policy selector for high-level mean-reversion calibration. "
                "Default for non-seasonality workflows: endogenous_core_pressure_level."
            ),
        ),
        command_option(
            "--mean-reversion-grid-interval-hours",
            help_text=(
                "Optional calibration-grid interval in hours for high-level mean-reversion calibration. "
                "Default: derived from retained session-open boundary observations."
            ),
            value_type=float,
        ),
        *(
            [
                command_option(
                    "--explain",
                    help_text="Include a human-readable summary of the inferred configuration.",
                    is_flag=True,
                ),
            ]
            if include_explain
            else []
        ),
        optional_path_option("--option-surface-path", help_text="Optional path to the option-surface file."),
        command_option(
            "--option-surface-format",
            choice_options=CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
            help_text="Option-surface file format. Default when provided: parquet.",
        ),
        command_option(
            "--option-surface-provenance-label",
            help_text="Stable provenance label recorded for the option-surface source. Default when provided: option_surface.",
        ),
        command_option(
            "--option-quote-input-policy",
            choice_options=CLI_CALIBRATION_OPTION_QUOTE_POLICY_CHOICES,
            help_text="Explicit option-quote normalization policy.",
        ),
        command_option("--option-quote-timestamp-column", help_text="Option-surface quote timestamp column name."),
        command_option("--option-expiry-column", help_text="Option-surface expiry column name."),
        command_option("--option-strike-column", help_text="Option-surface strike column name."),
        command_option("--option-type-column", help_text="Option-surface option type column name."),
        command_option("--option-mid-price-column", help_text="Option-surface mid-price column name."),
        command_option("--option-bid-column", help_text="Option-surface bid column name."),
        command_option("--option-ask-column", help_text="Option-surface ask column name."),
        command_option(
            "--option-implied-volatility-column",
            help_text="Option-surface implied volatility column name.",
        ),
        command_option(
            "--execution-mode",
            choice_options=CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
            default="memory",
            help_text=(
                "Execution mode recorded in the high-level fit workflow. Default: memory. "
                "Use disk when you want checkpoint artifacts in addition to the fitted model."
            ),
        ),
        optional_path_option(
            "--disk-checkpoint-directory",
            directory=True,
            help_text=(
                "Disk checkpoint directory recorded in output_preferences. Required when "
                "--execution-mode disk is selected."
            ),
        ),
        command_option(
            "--emit-derived-staged-execution-input",
            help_text="Request emission of the derived staged execution input for replay or debugging.",
            is_flag=True,
        ),
        command_option(
            "--omit-input-provenance-report",
            help_text="Disable provenance-report emission in output_preferences.",
            is_flag=True,
        ),
        *(
            [
                command_option(
                    "--output-dir",
                    help_text="Directory for model.json, fit_report.json, and run.json.",
                    directory=True,
                    path_type=True,
                ),
            ]
            if include_output_dir
            else []
        ),
    ]


def model_workflow_simulate_options(
    *,
    require_model_path: bool,
    include_shared_arguments: bool = True,
    enforce_required: bool = True,
) -> list[click.Parameter]:
    parameters: list[click.Parameter] = []
    if include_shared_arguments:
        parameters.append(command_config_option())
        parameters.append(output_option(default_output_mode=CliOutputMode.HUMAN))
        parameters.append(progress_option(default_progress_mode=CliProgressMode.AUTO))

    if require_model_path:
        parameters.append(
            command_option(
                "--model-path",
                help_text="Path to a persisted JaniformModel JSON payload.",
                path_type=True,
                required=enforce_required,
            )
        )

    parameters.extend(
        [
            command_option("--seed", help_text="Simulation random seed.", required=enforce_required, value_type=int),
            required_datetime_option(
                "--start-time-utc",
                help_text="Simulation start time in UTC.",
                required=enforce_required,
            ),
            command_option(
                "--horizon-interval-count",
                help_text="Number of simulation intervals to generate.",
                required=enforce_required,
                value_type=int,
            ),
            command_option(
                "--interval",
                help_text=(
                    "Optional coarse simulation interval such as `1min`, `15min`, `1h`, or `1D`. "
                    "When omitted, Janiform runs the default seeded tick simulation path."
                ),
            ),
            optional_path_option(
                "--output-path",
                help_text="Primary simulation output path. Default: ./.janiform/<run_id>/simulation.<csv|parquet>.",
            ),
            command_option(
                "--silent",
                help_text="Suppress non-error simulate output.",
                is_flag=True,
            ),
            command_option(
                "--debug-trace-enabled",
                help_text="Enable seeded-tick debug tracing for the seeded simulation path.",
                is_flag=True,
            ),
            command_option(
                "--sample-every-nth-transition",
                default=1,
                help_text="Keep every nth transition in the seeded-tick trace. Default: 1.",
                value_type=int,
            ),
            command_option(
                "--max-records-per-chunk",
                default=1024,
                help_text="Maximum seeded-tick records emitted per chunk. Default: 1024.",
                value_type=int,
            ),
        ]
    )

    return parameters
