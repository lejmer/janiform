from pathlib import Path
from typing import Any, Optional, cast

from .shared import (
    _parse_utc_datetime_cli_argument,
    _require_non_empty_cli_string,
    _require_positive_integer_cli_argument,
)
from ..context import CliCommandContext
from .._constants import (
    CLI_FIT_ENTRYPOINT,
    CLI_FIT_INTERNAL_ERROR_ID,
    CLI_FIT_RESULT_SCHEMA_VERSION,
    CLI_FIT_VALIDATION_ERROR_ID,
    CLI_HIGH_LEVEL_SIMULATE_ENTRYPOINT,
    CLI_HIGH_LEVEL_SIMULATE_INTERNAL_ERROR_ID,
    CLI_HIGH_LEVEL_SIMULATE_VALIDATION_ERROR_ID,
    CLI_SIMULATE_RESULT_SCHEMA_VERSION,
    CliExitCode,
)
from ..documents import _load_json_document_for_cli_field
from ..errors import _classify_generic_cli_command_error, _cli_validation_error
from ..paths import _resolve_path_from_repository_root
from ..rendering import (
    _render_fit_human_output,
    _render_simulate_human_output,
    _write_cli_error,
    _write_command_output,
)
from ...calibration import (
    CalibrationDuplicateRecordPolicy,
    CalibrationInputFileFormat,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataOutputPreferences,
    CalibrationMarketDataWorkflowScope,
    CalibrationOptionQuoteInputPolicy,
    CalibrationOptionSurfaceColumnMapping,
    CalibrationExecutionMode,
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
    JaniformEstimator,
    JaniformFitReport,
    JaniformModel,
    KnownNewsExplicitConfiguration,
    KnownNewsPresetReference,
    MarketProfileReference,
    PresetCategory,
    PresetReferenceUsageMode,
    PresetRegistry,
    PresetRegistryScope,
    SessionPresetReference,
)
from ...calibration.fit.estimator import _load_explicit_session_configuration
from ...errors import JaniformError, JaniformErrorCategory, JaniformErrorDetails, JaniformExecutionError


def _run_fit_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    **command_arguments: object,
) -> int:
    try:
        model, fit_report, persisted_model_path = _execute_cli_fit(
            cli_context=cli_context,
            entrypoint=CLI_FIT_ENTRYPOINT,
            error_id=CLI_FIT_VALIDATION_ERROR_ID,
            command_arguments=command_arguments,
        )
        payload = _build_cli_fit_result_payload(
            model=model,
            fit_report=fit_report,
            persisted_model_path=persisted_model_path,
            command=CLI_FIT_ENTRYPOINT,
            schema_version=CLI_FIT_RESULT_SCHEMA_VERSION,
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_generic_cli_command_error(
                e,
                entrypoint=CLI_FIT_ENTRYPOINT,
                validation_error_id=CLI_FIT_VALIDATION_ERROR_ID,
                internal_error_id=CLI_FIT_INTERNAL_ERROR_ID,
                file_field_path="tick_data_path",
                document_field_path="fit_workflow_configuration",
            )
        )

    _write_command_output(
        output,
        json_payload=payload,
        human_output_factory=lambda: _render_fit_human_output(payload),
    )
    return int(CliExitCode.SUCCESS)


def _run_model_simulate_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    model_path: Path,
    **command_arguments: object,
) -> int:
    model_path = _resolve_path_from_repository_root(
        cli_context.repository_root,
        model_path,
    )

    try:
        model = JaniformModel.load(model_path)
        payload = _build_cli_simulate_result_payload(
            model=model,
            command_arguments=command_arguments,
            model_path=model_path,
            command=CLI_HIGH_LEVEL_SIMULATE_ENTRYPOINT,
            schema_version=CLI_SIMULATE_RESULT_SCHEMA_VERSION,
            entrypoint=CLI_HIGH_LEVEL_SIMULATE_ENTRYPOINT,
            error_id=CLI_HIGH_LEVEL_SIMULATE_VALIDATION_ERROR_ID,
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except RuntimeError as e:
        return _write_cli_error(
            _cli_validation_error(
                "model_path",
                str(e),
                error_id=CLI_HIGH_LEVEL_SIMULATE_VALIDATION_ERROR_ID,
                entrypoint=CLI_HIGH_LEVEL_SIMULATE_ENTRYPOINT,
            )
        )
    except Exception as e:
        return _write_cli_error(
            _classify_generic_cli_command_error(
                e,
                entrypoint=CLI_HIGH_LEVEL_SIMULATE_ENTRYPOINT,
                validation_error_id=CLI_HIGH_LEVEL_SIMULATE_VALIDATION_ERROR_ID,
                internal_error_id=CLI_HIGH_LEVEL_SIMULATE_INTERNAL_ERROR_ID,
                file_field_path="model_path",
                document_field_path="model_payload",
            )
        )

    _write_command_output(
        output,
        json_payload=payload,
        human_output_factory=lambda: _render_simulate_human_output(payload),
    )
    return int(CliExitCode.SUCCESS)


def _build_cli_preset_registry(
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
            raise _cli_validation_error(
                "user_preset_json",
                "input file could not be found",
                error_id=error_id,
                entrypoint=entrypoint,
                context={"path": str(resolved_path)},
            ) from e
        except OSError as e:
            raise _cli_validation_error(
                "user_preset_json",
                f"input file could not be read: {e}",
                error_id=error_id,
                entrypoint=entrypoint,
                context={"path": str(resolved_path)},
            ) from e
        registry.load_user_preset_json(preset_json)
    return registry


def _parse_cli_preset_category(raw_value: Optional[str]) -> Optional[PresetCategory]:
    if raw_value is None:
        return None
    return PresetCategory(raw_value.replace("-", "_"))


def _parse_cli_preset_scope(raw_value: Optional[str]) -> Optional[PresetRegistryScope]:
    if raw_value is None or raw_value == "auto":
        return None
    return PresetRegistryScope(raw_value)


def _resolve_named_preset_scope(
    *,
    registry: PresetRegistry,
    category: PresetCategory,
    preset_name: str,
    raw_scope: Optional[str],
) -> PresetRegistryScope:
    explicit_scope = _parse_cli_preset_scope(raw_scope)
    match category:
        case PresetCategory.SESSION:
            resolved_scope, _ = registry.resolve_session_preset(preset_name, registry_scope=explicit_scope)
        case PresetCategory.KNOWN_NEWS:
            resolved_scope, _ = registry.resolve_known_news_preset(preset_name, registry_scope=explicit_scope)
        case PresetCategory.MARKET_PROFILE:
            resolved_scope, _ = registry.resolve_market_profile_preset(preset_name, registry_scope=explicit_scope)
    return resolved_scope


def _resolve_preset_artifact(
    *,
    registry: PresetRegistry,
    category: PresetCategory,
    preset_name: str,
    registry_scope: PresetRegistryScope,
) -> Any:
    match category:
        case PresetCategory.SESSION:
            _, artifact = registry.resolve_session_preset(preset_name, registry_scope=registry_scope)
        case PresetCategory.KNOWN_NEWS:
            _, artifact = registry.resolve_known_news_preset(preset_name, registry_scope=registry_scope)
        case PresetCategory.MARKET_PROFILE:
            _, artifact = registry.resolve_market_profile_preset(preset_name, registry_scope=registry_scope)
    return artifact


def _execute_cli_fit(
    *,
    cli_context: CliCommandContext,
    entrypoint: str,
    error_id: str,
    command_arguments: dict[str, object],
) -> tuple[JaniformModel, JaniformFitReport, Optional[Path]]:
    registry = _build_cli_preset_registry(
        cli_context=cli_context,
        user_preset_json=cast(tuple[Path, ...], command_arguments.get("user_preset_json", ())),
        entrypoint=entrypoint,
        error_id=error_id,
    )
    market_profile = _build_market_profile_reference_from_arguments(
        command_arguments,
        registry=registry,
        entrypoint=entrypoint,
        error_id=error_id,
    )
    session_reference = _build_session_reference_from_arguments(
        command_arguments,
        repository_root=cli_context.repository_root,
        registry=registry,
        entrypoint=entrypoint,
        error_id=error_id,
    )
    known_news_reference = _build_known_news_reference_from_arguments(
        command_arguments,
        repository_root=cli_context.repository_root,
        registry=registry,
        entrypoint=entrypoint,
        error_id=error_id,
    )
    estimator = JaniformEstimator(
        workflow_scope=CalibrationMarketDataWorkflowScope(cast(str, command_arguments["workflow_scope"])),
        tick_data_column_mapping=_build_tick_data_column_mapping_from_arguments(command_arguments),
        normalization_policy=_build_model_workflow_normalization_policy_from_arguments(command_arguments),
        option_surface_column_mapping=_build_option_surface_column_mapping_from_arguments(
            command_arguments,
            entrypoint=entrypoint,
            error_id=error_id,
        ),
        output_preferences=_build_model_workflow_output_preferences_from_arguments(
            command_arguments,
            repository_root=cli_context.repository_root,
            entrypoint=entrypoint,
            error_id=error_id,
        ),
        market_profile=market_profile,
        session=session_reference,
        known_news=known_news_reference,
        exchange_code=cast(Optional[str], command_arguments.get("exchange_code")),
        session_timezone=cast(Optional[str], command_arguments.get("session_timezone")),
        registry=registry,
    )

    tick_data_path = _resolve_path_from_repository_root(
        cli_context.repository_root,
        cast(Path, command_arguments["tick_data_path"]),
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

    _reject_orphan_market_data_arguments(
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

    model_result, fit_report = cast(
        tuple[Optional[JaniformModel], JaniformFitReport],
        estimator.fit(
            run_id=cast(str, command_arguments["run_id"]),
            tick_data_path=tick_data_path,
            tick_data_format=CalibrationInputFileFormat(cast(str, command_arguments["tick_data_format"])),
            tick_data_provenance_label=cast(str, command_arguments["tick_data_provenance_label"]),
            option_surface_path=resolved_option_surface_path,
            option_surface_format=option_surface_format,
            option_surface_provenance_label=option_surface_provenance_label,
            report=True,
        ),
    )
    if model_result is None:
        raise JaniformExecutionError(
            JaniformErrorDetails.from_context(
                error_id=f"{error_id}.fit_failed",
                category=JaniformErrorCategory.EXECUTION,
                workflow="cli",
                entrypoint=entrypoint,
                message="high-level fit failed before a JaniformModel could be materialized",
                context={"run_id": fit_report.run_id, "staged_status": fit_report.staged_response.status.value},
            )
        )

    raw_model_output_path = cast(Optional[Path], command_arguments.get("model_output_path"))
    persisted_model_path = (
        _resolve_path_from_repository_root(cli_context.repository_root, raw_model_output_path)
        if raw_model_output_path is not None
        else None
    )
    if persisted_model_path is not None:
        model_result.save(persisted_model_path)
    return model_result, fit_report, persisted_model_path


def _build_market_profile_reference_from_arguments(
    command_arguments: dict[str, object],
    *,
    registry: PresetRegistry,
    entrypoint: str,
    error_id: str,
) -> Optional[MarketProfileReference]:
    market_name = cast(Optional[str], command_arguments.get("market"))
    raw_scope = cast(Optional[str], command_arguments.get("market_scope"))

    if market_name is None:
        if raw_scope not in (None, "auto"):
            raise _cli_validation_error(
                "market_scope",
                "--market-scope requires --market",
                error_id=error_id,
                entrypoint=entrypoint,
            )

        return None

    resolved_scope = _resolve_named_preset_scope(
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


def _build_session_reference_from_arguments(
    command_arguments: dict[str, object],
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
            raise _cli_validation_error(
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

    resolved_scope = _resolve_named_preset_scope(
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


def _build_known_news_reference_from_arguments(
    command_arguments: dict[str, object],
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
            raise _cli_validation_error(
                "known_news_scope",
                "--known-news-scope requires --known-news",
                error_id=error_id,
                entrypoint=entrypoint,
            )

        return None

    if known_news_json is not None:
        explicit_configuration_document = _load_json_document_for_cli_field(
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

    resolved_scope = _resolve_named_preset_scope(
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


def _build_tick_data_column_mapping_from_arguments(
    parsed_arguments: dict[str, object],
) -> CalibrationTickDataColumnMapping:
    return CalibrationTickDataColumnMapping(
        timestamp_column=cast(str, parsed_arguments["tick_timestamp_column"]),
        price_column=cast(Optional[str], parsed_arguments.get("tick_price_column")),
        trade_price_column=cast(Optional[str], parsed_arguments.get("tick_trade_price_column")),
        trade_sign_column=cast(Optional[str], parsed_arguments.get("tick_trade_sign_column")),
        bid_column=cast(Optional[str], parsed_arguments.get("tick_bid_column")),
        ask_column=cast(Optional[str], parsed_arguments.get("tick_ask_column")),
        close_price_column=cast(Optional[str], parsed_arguments.get("tick_close_price_column")),
    )


def _build_option_surface_column_mapping_from_arguments(
    parsed_arguments: dict[str, object],
    *,
    entrypoint: str,
    error_id: str,
) -> Optional[CalibrationOptionSurfaceColumnMapping]:
    option_surface_path = cast(Optional[Path], parsed_arguments.get("option_surface_path"))
    if option_surface_path is None:
        return None
    return CalibrationOptionSurfaceColumnMapping(
        quote_timestamp_column=_require_non_empty_cli_string(
            cast(str, parsed_arguments["option_quote_timestamp_column"]),
            field_path="option_quote_timestamp_column",
            error_id=error_id,
            entrypoint=entrypoint,
        ),
        expiry_timestamp_column=_require_non_empty_cli_string(
            cast(str, parsed_arguments["option_expiry_column"]),
            field_path="option_expiry_column",
            error_id=error_id,
            entrypoint=entrypoint,
        ),
        strike_column=_require_non_empty_cli_string(
            cast(str, parsed_arguments["option_strike_column"]),
            field_path="option_strike_column",
            error_id=error_id,
            entrypoint=entrypoint,
        ),
        option_type_column=_require_non_empty_cli_string(
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


def _build_model_workflow_normalization_policy_from_arguments(
    parsed_arguments: dict[str, object],
) -> CalibrationMarketDataNormalizationPolicy:
    tick_price_input_policy_raw = cast(Optional[str], parsed_arguments.get("tick_price_input_policy"))
    option_quote_input_policy_raw = cast(Optional[str], parsed_arguments.get("option_quote_input_policy"))

    return CalibrationMarketDataNormalizationPolicy(
        timestamp_timezone=cast(str, parsed_arguments["timestamp_timezone"]),
        sort_records_by_timestamp=not bool(parsed_arguments.get("disable_timestamp_sort")),
        duplicate_record_policy=CalibrationDuplicateRecordPolicy(
            cast(str, parsed_arguments["duplicate_record_policy"]),
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


def _build_model_workflow_output_preferences_from_arguments(
    parsed_arguments: dict[str, object],
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
        raise _cli_validation_error(
            "disk_checkpoint_directory",
            "--disk-checkpoint-directory is required when --execution-mode disk is selected",
            error_id=error_id,
            entrypoint=entrypoint,
        )
    if selected_execution_mode.value == "memory" and disk_checkpoint_directory is not None:
        raise _cli_validation_error(
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


def _build_cli_fit_result_payload(
    *,
    model: JaniformModel,
    fit_report: JaniformFitReport,
    persisted_model_path: Optional[Path],
    command: str,
    schema_version: str,
) -> dict[str, Any]:
    return {
        "schema_version": schema_version,
        "command": command,
        "run_id": model.run_id,
        "model_output_path": None if persisted_model_path is None else str(persisted_model_path),
        "model_summary": model.summary(),
        "fit_report_summary": fit_report.summary(),
        "resolved_configuration": fit_report.resolved_configuration.to_payload(),
        "session_resolution": fit_report.session_resolution.to_payload(),
        "preparation_diagnostics": fit_report.preparation_diagnostics.to_payload(),
        "preparation_provenance": fit_report.preparation_provenance.to_payload(),
        "staged_status": fit_report.staged_response.status.value,
    }


def _build_cli_simulate_result_payload(
    *,
    model: JaniformModel,
    command_arguments: dict[str, object],
    model_path: Optional[Path],
    command: str,
    schema_version: str,
    entrypoint: str,
    error_id: str,
) -> dict[str, Any]:
    start_time_utc = _parse_utc_datetime_cli_argument(
        cast(str, command_arguments["start_time_utc"]),
        field_path="start_time_utc",
        error_id=error_id,
        entrypoint=entrypoint,
    )
    horizon_interval_count = _require_positive_integer_cli_argument(
        int(cast(int, command_arguments["horizon_interval_count"])),
        field_path="horizon_interval_count",
        error_id=error_id,
        entrypoint=entrypoint,
    )
    sample_every_nth_transition = _require_positive_integer_cli_argument(
        int(cast(int, command_arguments["sample_every_nth_transition"])),
        field_path="sample_every_nth_transition",
        error_id=error_id,
        entrypoint=entrypoint,
    )
    max_records_per_chunk = _require_positive_integer_cli_argument(
        int(cast(int, command_arguments["max_records_per_chunk"])),
        field_path="max_records_per_chunk",
        error_id=error_id,
        entrypoint=entrypoint,
    )
    output_mode = cast(str, command_arguments["simulation_output"])
    simulation_result = model.simulate(
        seed=int(cast(int, command_arguments["seed"])),
        start_time_utc=start_time_utc,
        horizon_interval_count=horizon_interval_count,
        interval=cast(str, command_arguments["interval"]),
        output=cast(Any, output_mode),
        debug_trace_enabled=bool(command_arguments.get("debug_trace_enabled")),
        sample_every_nth_transition=sample_every_nth_transition,
        max_records_per_chunk=max_records_per_chunk,
    )
    coarse_payload: Optional[dict[str, Any]] = None
    seeded_tick_payload: Optional[dict[str, Any]] = None
    match output_mode:
        case "coarse":
            coarse_payload = simulation_result.to_payload()
        case "ticks":
            seeded_tick_payload = simulation_result.to_payload()
        case "both":
            coarse_result, seeded_tick_result = cast(tuple[Any, Any], simulation_result)
            coarse_payload = coarse_result.to_payload()
            seeded_tick_payload = seeded_tick_result.to_payload()
    return {
        "schema_version": schema_version,
        "command": command,
        "model_path": None if model_path is None else str(model_path),
        "run_id": model.run_id,
        "simulation_output": output_mode,
        "model_summary": model.summary(),
        "resolved_configuration": model.resolved_configuration.to_payload(),
        "session_resolution": model.session_resolution.to_payload(),
        "coarse_simulation": coarse_payload,
        "seeded_tick_simulation": seeded_tick_payload,
    }


def _reject_orphan_market_data_arguments(
    has_primary_path: bool,
    optional_arguments: tuple[tuple[str, object], ...],
    *,
    message: str,
) -> None:
    if has_primary_path:
        return
    for _, argument_value in optional_arguments:
        if argument_value is not None:
            raise ValueError(message)


__all__ = [
    "_build_cli_fit_result_payload",
    "_build_cli_preset_registry",
    "_build_cli_simulate_result_payload",
    "_execute_cli_fit",
    "_parse_cli_preset_category",
    "_parse_cli_preset_scope",
    "_resolve_named_preset_scope",
    "_resolve_preset_artifact",
    "_run_fit_command",
    "_run_model_simulate_command",
]
