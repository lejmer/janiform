import json
import sys
from argparse import Namespace
from collections.abc import Callable
from datetime import UTC, datetime, timedelta
from enum import StrEnum
from pathlib import Path
from typing import Any, Optional, cast

from ._constants import (
    CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT,
    CLI_CALIBRATION_MARKET_DATA_INTERNAL_ERROR_ID,
    CLI_CALIBRATION_MARKET_DATA_VALIDATION_ERROR_ID,
    CLI_CALIBRATION_STAGED_ENTRYPOINT,
    CLI_CALIBRATION_STAGED_INTERNAL_ERROR_ID,
    CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID,
    CLI_DETERMINISTIC_REGRESSION_ENTRYPOINT,
    CLI_DETERMINISTIC_REGRESSION_INTERNAL_ERROR_ID,
    CLI_DETERMINISTIC_REGRESSION_VALIDATION_ERROR_ID,
    CLI_HOURLY_FACTORS_BUILD_CONTRACT_ENTRYPOINT,
    CLI_HOURLY_FACTORS_BUILD_CONTRACT_INTERNAL_ERROR_ID,
    CLI_HOURLY_FACTORS_BUILD_CONTRACT_VALIDATION_ERROR_ID,
    CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_ENTRYPOINT,
    CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_INTERNAL_ERROR_ID,
    CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_VALIDATION_ERROR_ID,
    CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_ENTRYPOINT,
    CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_INTERNAL_ERROR_ID,
    CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_VALIDATION_ERROR_ID,
    CLI_MARKET_DATA_SUMMARY_FIELD_SELECTION_POLICY,
    CLI_MARKET_DATA_SUMMARY_SCHEMA_VERSION,
    CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
    CLI_OPEN_CLOSED_GAP_INTERNAL_ERROR_ID,
    CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
    CLI_PRICING_VANILLA_OPTION_ENTRYPOINT,
    CLI_PRICING_VANILLA_OPTION_INTERNAL_ERROR_ID,
    CLI_PRICING_VANILLA_OPTION_VALIDATION_ERROR_ID,
    CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
    CLI_SEASONALITY_DIAGNOSTICS_INTERNAL_ERROR_ID,
    CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
    CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_ENTRYPOINT,
    CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_INTERNAL_ERROR_ID,
    CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_VALIDATION_ERROR_ID,
    CLI_SIMULATION_COARSE_INTERVAL_ENTRYPOINT,
    CLI_SIMULATION_COARSE_INTERVAL_INTERNAL_ERROR_ID,
    CLI_SIMULATION_COARSE_INTERVAL_VALIDATION_ERROR_ID,
    CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
    CLI_SIMULATION_SEEDED_TICKS_INTERNAL_ERROR_ID,
    CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
    CLI_STYLIZED_FACTS_HORIZON_COMPARISON_ENTRYPOINT,
    CLI_STYLIZED_FACTS_HORIZON_COMPARISON_INTERNAL_ERROR_ID,
    CLI_STYLIZED_FACTS_HORIZON_COMPARISON_VALIDATION_ERROR_ID,
    CliExitCode,
    CliOutputMode,
)
from ..calibration import (
    CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION,
    CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
    CalibrationExecutionMode,
    CalibrationStagedExecutionRuntimeResources,
    MarketDataCalibrationRequest,
    PreparedMarketDataCalibration,
    StagedCalibrationExecutionInput,
    prepare_market_data_calibration,
    run_staged_calibration,
    validate_market_data_calibration_request,
    validate_staged_calibration_execution_input,
)
from ..calendar import (
    CalendarInvalidTimestampError,
    CalendarInvalidTimezoneError,
    CalendarMissingExchangeError,
    CalendarRegistry,
    CalendarRegistryLoadError,
)
from ..errors import (
    ContractValidationFailure,
    JaniformError,
    JaniformErrorCategory,
    JaniformErrorDetails,
    JaniformIntegrationError,
    JaniformValidationError,
)
from .._timestamps import datetime_to_utc_nanos
from ..hourly_factors import (
    DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
    DEFAULT_SEASONALITY_AGGREGATION_METHOD,
    DEFAULT_SEASONALITY_INTERPOLATION_METHOD,
    DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY,
    HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
    HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
    HourlyFactorContract,
    HourlyNewsIntensityMaterialization,
    HourlyNewsIntensityMaterializationSettings,
    HourlyNewsIntensityProviderExplicitSource,
    HourlySeasonalityMaterialization,
    HourlySeasonalityMaterializationSettings,
    HourlySeasonalityProviderExplicitSource,
    build_hourly_factor_contract,
    materialize_hourly_news_intensity_factors,
    materialize_hourly_seasonality_factors,
)
from ..pricing import run_vanilla_option_pricing, validate_vanilla_option_pricing_execution_input
from ..seasonality import (
    DayOfWeekSeasonalityEstimatorSettings,
    DayOfYearSeasonalityEstimatorSettings,
    IntradaySeasonalityEstimatorSettings,
    IntradaySmoothingKernel,
    SeasonalityInputValidationError,
    SessionFilter,
    compose_deterministic_seasonality_signal,
    estimate_day_of_week_seasonality,
    estimate_day_of_year_seasonality,
    estimate_intraday_seasonality,
    generate_seasonality_diagnostics_report,
    preprocess_timestamp_events,
)
from ..simulation import (
    CoarseSimulationRunRequest,
    SeededTickSimulationHourlyFactorSummary,
    SeededTickSimulationRequest,
    load_hourly_factor_contract_for_simulation,
    run_coarse_simulation,
    run_seeded_tick_event_simulation,
)
from ..stylized_facts import run_stylized_fact_horizon_comparison_report_generator
from ..validation import (
    run_deterministic_regression_harness,
    run_open_closed_gap_leakage_regression_harness,
)


def _run_simulation_coarse_interval_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[3]
    request_path = _resolve_path_from_repository_root(repository_root, cast(Path, parsed_arguments.request_json))

    try:
        request_document = _load_json_document(request_path, "coarse simulation request")
        request = CoarseSimulationRunRequest.from_mapping(request_document)
        response = run_coarse_simulation(request)
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_SIMULATION_COARSE_INTERVAL_ENTRYPOINT,
                validation_error_id=CLI_SIMULATION_COARSE_INTERVAL_VALIDATION_ERROR_ID,
                internal_error_id=CLI_SIMULATION_COARSE_INTERVAL_INTERNAL_ERROR_ID,
                file_field_path="request_json",
                document_field_path="request_document",
            )
        )

    _write_command_output(
        parsed_arguments,
        json_payload=response.to_payload(),
        human_output_factory=lambda: _render_simulation_coarse_interval_human_output(response.to_payload()),
    )

    return int(CliExitCode.SUCCESS)


def _run_simulation_seeded_ticks_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[3]
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
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
                validation_error_id=CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
                internal_error_id=CLI_SIMULATION_SEEDED_TICKS_INTERNAL_ERROR_ID,
                file_field_path="request_json",
                document_field_path="request_document",
            )
        )

    _write_command_output(
        parsed_arguments,
        json_payload=response.to_payload(),
        human_output_factory=lambda: _render_simulation_seeded_ticks_human_output(response.to_payload()),
    )

    return int(CliExitCode.SUCCESS)


def _run_pricing_vanilla_option_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[3]
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

    _write_command_output(
        parsed_arguments,
        json_payload=run_result.to_payload(),
        human_output_factory=lambda: _render_pricing_vanilla_option_human_output(run_result.to_payload()),
    )

    return int(CliExitCode.SUCCESS)


def _run_seasonality_preprocess_timestamps_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[3]
    event_records_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.event_records_json),
    )
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )

    file_field_path = "event_records_json"
    document_field_path = "event_records_document"

    try:
        event_records_document = _load_json_document(event_records_path, "event records")
        event_records = _extract_event_records(event_records_document)
        file_field_path = "calendar_registry_json"
        document_field_path = "calendar_registry_document"
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        document_field_path = "event_records_document"
        preprocessed_events = preprocess_timestamp_events(
            event_records,
            exchange_code=cast(str, parsed_arguments.exchange_code),
            calendar_registry=calendar_registry,
            timestamp_field_name=cast(str, parsed_arguments.timestamp_field_name),
            session_filter=SessionFilter(cast(str, parsed_arguments.session_filter)),
            sparse_hour_event_threshold=int(parsed_arguments.sparse_hour_event_threshold),
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_seasonality_preprocess_command_error(
                e,
                file_field_path=file_field_path,
                document_field_path=document_field_path,
            )
        )

    payload = _build_timestamp_event_preprocessing_payload(preprocessed_events)

    _write_command_output(
        parsed_arguments,
        json_payload=payload,
        human_output_factory=lambda: _render_seasonality_preprocess_timestamps_human_output(payload),
    )

    return int(CliExitCode.SUCCESS)


def _run_seasonality_diagnostics_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[3]
    event_records_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.event_records_json),
    )
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )

    file_field_path = "event_records_json"
    document_field_path = "event_records_document"

    try:
        event_records_document = _load_json_document(event_records_path, "event records")
        event_records = _extract_event_records(event_records_document)
        file_field_path = "calendar_registry_json"
        document_field_path = "calendar_registry_document"
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        document_field_path = "command_input"
        dataset_id = _require_non_empty_cli_string(
            cast(str, parsed_arguments.dataset_id),
            field_path="dataset_id",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        )
        estimator_version = _require_non_empty_cli_string(
            cast(str, parsed_arguments.estimator_version),
            field_path="estimator_version",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        )
        sparse_hour_event_threshold = _require_positive_integer_cli_argument(
            int(parsed_arguments.sparse_hour_event_threshold),
            field_path="sparse_hour_event_threshold",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        )
        intraday_settings = _build_intraday_seasonality_settings_from_arguments(parsed_arguments)
        day_of_week_settings = _build_day_of_week_seasonality_settings_from_arguments(parsed_arguments)
        day_of_year_settings = _build_day_of_year_seasonality_settings_from_arguments(parsed_arguments)
        document_field_path = "event_records_document"
        preprocessed_events = preprocess_timestamp_events(
            event_records,
            exchange_code=cast(str, parsed_arguments.exchange_code),
            calendar_registry=calendar_registry,
            timestamp_field_name=cast(str, parsed_arguments.timestamp_field_name),
            session_filter=SessionFilter(cast(str, parsed_arguments.session_filter)),
            sparse_hour_event_threshold=sparse_hour_event_threshold,
        )
        document_field_path = "command_input"
        intraday_estimate = estimate_intraday_seasonality(
            preprocessed_events,
            settings=intraday_settings,
        )
        day_of_week_estimate = estimate_day_of_week_seasonality(
            preprocessed_events,
            settings=day_of_week_settings,
        )
        day_of_year_estimate = estimate_day_of_year_seasonality(
            preprocessed_events,
            settings=day_of_year_settings,
        )
        composition = compose_deterministic_seasonality_signal(
            preprocessed_events,
            intraday_estimate,
            day_of_week_estimate,
            day_of_year_estimate,
        )
        report = generate_seasonality_diagnostics_report(
            dataset_id,
            preprocessed_events,
            intraday_estimate,
            day_of_week_estimate,
            day_of_year_estimate,
            composition,
            estimator_version=estimator_version,
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_seasonality_diagnostics_command_error(
                e,
                file_field_path=file_field_path,
                document_field_path=document_field_path,
            )
        )

    _write_command_output(
        parsed_arguments,
        json_payload=report,
        human_output_factory=lambda: _render_seasonality_diagnostics_human_output(
            report,
            preprocessed_events=preprocessed_events,
            intraday_estimate=intraday_estimate,
            day_of_week_estimate=day_of_week_estimate,
            day_of_year_estimate=day_of_year_estimate,
        ),
    )

    return int(CliExitCode.SUCCESS)


def _run_hourly_factor_materialize_seasonality_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[3]
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )
    materialization_input_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.materialization_input_json),
    )

    file_field_path = "calendar_registry_json"
    document_field_path = "calendar_registry_document"

    try:
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        file_field_path = "materialization_input_json"
        document_field_path = "materialization_input_document"
        materialization_input_document = _load_json_document(
            materialization_input_path,
            "hourly seasonality materialization input",
        )

        document_field_path = "calendar_registry_document"
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        document_field_path = "command_input"
        range_start_utc = _parse_utc_datetime_cli_argument(
            cast(str, parsed_arguments.range_start_utc),
            field_path="range_start_utc",
            error_id=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_ENTRYPOINT,
        )
        range_end_utc = _parse_utc_datetime_cli_argument(
            cast(str, parsed_arguments.range_end_utc),
            field_path="range_end_utc",
            error_id=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_ENTRYPOINT,
        )

        document_field_path = "materialization_input_document"
        source_timestamps_utc, source_seasonality_factors, settings = (
            _extract_hourly_seasonality_materialization_input(materialization_input_document)
        )
        materialization = materialize_hourly_seasonality_factors(
            calendar_registry=calendar_registry,
            exchange_code=cast(str, parsed_arguments.exchange_code),
            range_start_utc=range_start_utc,
            range_end_utc=range_end_utc,
            interval=cast(str, parsed_arguments.interval),
            source_timestamps_utc=source_timestamps_utc,
            source_seasonality_factors=source_seasonality_factors,
            settings=settings,
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_ENTRYPOINT,
                validation_error_id=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_VALIDATION_ERROR_ID,
                internal_error_id=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_INTERNAL_ERROR_ID,
                file_field_path=file_field_path,
                document_field_path=document_field_path,
            )
        )

    payload = _build_hourly_seasonality_materialization_payload(materialization)

    _write_command_output(
        parsed_arguments,
        json_payload=payload,
        human_output_factory=lambda: _render_hourly_factor_materialize_seasonality_human_output(payload),
    )

    return int(CliExitCode.SUCCESS)


def _run_hourly_factor_materialize_news_intensity_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[3]
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )
    materialization_input_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.materialization_input_json),
    )

    file_field_path = "calendar_registry_json"
    document_field_path = "calendar_registry_document"

    try:
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        file_field_path = "materialization_input_json"
        document_field_path = "materialization_input_document"
        materialization_input_document = _load_json_document(
            materialization_input_path,
            "hourly news-intensity materialization input",
        )

        document_field_path = "calendar_registry_document"
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        document_field_path = "command_input"
        range_start_utc = _parse_utc_datetime_cli_argument(
            cast(str, parsed_arguments.range_start_utc),
            field_path="range_start_utc",
            error_id=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_ENTRYPOINT,
        )
        range_end_utc = _parse_utc_datetime_cli_argument(
            cast(str, parsed_arguments.range_end_utc),
            field_path="range_end_utc",
            error_id=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_ENTRYPOINT,
        )

        document_field_path = "materialization_input_document"
        settings = _extract_hourly_news_intensity_materialization_input(materialization_input_document)
        materialization = materialize_hourly_news_intensity_factors(
            calendar_registry=calendar_registry,
            exchange_code=cast(str, parsed_arguments.exchange_code),
            range_start_utc=range_start_utc,
            range_end_utc=range_end_utc,
            interval=cast(str, parsed_arguments.interval),
            settings=settings,
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_ENTRYPOINT,
                validation_error_id=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_VALIDATION_ERROR_ID,
                internal_error_id=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_INTERNAL_ERROR_ID,
                file_field_path=file_field_path,
                document_field_path=document_field_path,
            )
        )

    payload = _build_hourly_news_intensity_materialization_payload(materialization)

    _write_command_output(
        parsed_arguments,
        json_payload=payload,
        human_output_factory=lambda: _render_hourly_factor_materialize_news_intensity_human_output(payload),
    )

    return int(CliExitCode.SUCCESS)


def _run_hourly_factor_build_contract_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[3]
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )

    seasonality_explicit_source_path_argument = cast(
        Optional[Path],
        parsed_arguments.seasonality_explicit_source_json,
    )
    seasonality_materialized_source_path_argument = cast(
        Optional[Path],
        parsed_arguments.seasonality_materialized_source_json,
    )
    news_intensity_explicit_source_path_argument = cast(
        Optional[Path],
        parsed_arguments.news_intensity_explicit_source_json,
    )
    news_intensity_materialized_source_path_argument = cast(
        Optional[Path],
        parsed_arguments.news_intensity_materialized_source_json,
    )

    file_field_path = "calendar_registry_json"
    document_field_path = "calendar_registry_document"

    try:
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        document_field_path = "calendar_registry_document"
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        document_field_path = "command_input"
        range_start_utc = _parse_utc_datetime_cli_argument(
            cast(str, parsed_arguments.range_start_utc),
            field_path="range_start_utc",
            error_id=CLI_HOURLY_FACTORS_BUILD_CONTRACT_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_BUILD_CONTRACT_ENTRYPOINT,
        )
        range_end_utc = _parse_utc_datetime_cli_argument(
            cast(str, parsed_arguments.range_end_utc),
            field_path="range_end_utc",
            error_id=CLI_HOURLY_FACTORS_BUILD_CONTRACT_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_BUILD_CONTRACT_ENTRYPOINT,
        )

        seasonality_hourly_factors: list[float]
        seasonality_materialization_method: str
        if seasonality_explicit_source_path_argument is not None:
            file_field_path = "seasonality_explicit_source_json"
            document_field_path = "seasonality_source_document"
            seasonality_source_document = _load_json_document(
                _resolve_path_from_repository_root(repository_root, seasonality_explicit_source_path_argument),
                "explicit seasonality source",
            )
            seasonality_hourly_factors, seasonality_materialization_method = (
                _extract_explicit_hourly_factor_source_document(
                    seasonality_source_document,
                    field_prefix="seasonality source document",
                )
            )
        else:
            file_field_path = "seasonality_materialized_source_json"
            document_field_path = "seasonality_source_document"
            seasonality_source_document = _load_json_document(
                _resolve_path_from_repository_root(repository_root, seasonality_materialized_source_path_argument),
                "materialized seasonality source",
            )
            seasonality_hourly_factors, seasonality_materialization_method = (
                _extract_materialized_hourly_seasonality_source_document(
                    seasonality_source_document,
                    exchange_code=cast(str, parsed_arguments.exchange_code),
                    range_start_utc=range_start_utc,
                    range_end_utc=range_end_utc,
                )
            )

        news_intensity_hourly_factors: list[float]
        news_intensity_materialization_method: str
        if news_intensity_explicit_source_path_argument is not None:
            file_field_path = "news_intensity_explicit_source_json"
            document_field_path = "news_intensity_source_document"
            news_intensity_source_document = _load_json_document(
                _resolve_path_from_repository_root(repository_root, news_intensity_explicit_source_path_argument),
                "explicit news-intensity source",
            )
            news_intensity_hourly_factors, news_intensity_materialization_method = (
                _extract_explicit_hourly_factor_source_document(
                    news_intensity_source_document,
                    field_prefix="news intensity source document",
                )
            )
        else:
            file_field_path = "news_intensity_materialized_source_json"
            document_field_path = "news_intensity_source_document"
            news_intensity_source_document = _load_json_document(
                _resolve_path_from_repository_root(repository_root, news_intensity_materialized_source_path_argument),
                "materialized news-intensity source",
            )
            news_intensity_hourly_factors, news_intensity_materialization_method = (
                _extract_materialized_hourly_news_intensity_source_document(
                    news_intensity_source_document,
                    exchange_code=cast(str, parsed_arguments.exchange_code),
                    range_start_utc=range_start_utc,
                    range_end_utc=range_end_utc,
                )
            )

        contract = build_hourly_factor_contract(
            calendar_registry=calendar_registry,
            exchange_code=cast(str, parsed_arguments.exchange_code),
            range_start_utc=range_start_utc,
            range_end_utc=range_end_utc,
            interval=cast(str, parsed_arguments.interval),
            seasonality_hourly_factors=seasonality_hourly_factors,
            news_intensity_hourly_factors=news_intensity_hourly_factors,
            seasonality_materialization_method=seasonality_materialization_method,
            news_intensity_materialization_method=news_intensity_materialization_method,
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_HOURLY_FACTORS_BUILD_CONTRACT_ENTRYPOINT,
                validation_error_id=CLI_HOURLY_FACTORS_BUILD_CONTRACT_VALIDATION_ERROR_ID,
                internal_error_id=CLI_HOURLY_FACTORS_BUILD_CONTRACT_INTERNAL_ERROR_ID,
                file_field_path=file_field_path,
                document_field_path=document_field_path,
            )
        )

    payload = _build_hourly_factor_contract_payload(contract)

    _write_command_output(
        parsed_arguments,
        json_payload=payload,
        human_output_factory=lambda: _render_hourly_factor_build_contract_human_output(payload),
    )

    return int(CliExitCode.SUCCESS)


def _run_stylized_facts_horizon_comparison_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[3]
    metric_catalog_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.metric_catalog_json),
    )
    comparison_request_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.comparison_request_json),
    )
    artifact_export_path_argument = cast(Path | None, parsed_arguments.artifact_export_json)
    artifact_export_path = (
        _resolve_path_from_repository_root(repository_root, artifact_export_path_argument)
        if artifact_export_path_argument is not None
        else None
    )

    file_field_path = "metric_catalog_json"
    document_field_path = "metric_catalog_document"

    try:
        metric_catalog_document = _load_json_document(metric_catalog_path, "stylized-fact metric catalog")
        file_field_path = "comparison_request_json"
        document_field_path = "comparison_request_document"
        comparison_request_document = _load_json_document(
            comparison_request_path,
            "stylized-fact comparison request",
        )

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
            execution_mode=cast(str, parsed_arguments.execution_mode),
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
        parsed_arguments,
        json_payload=cast(dict[str, Any], report_bundle),
        human_output_factory=lambda: _render_stylized_facts_horizon_comparison_human_output(
            report_bundle,
            execution_mode=cast(str, parsed_arguments.execution_mode),
        ),
    )

    return int(CliExitCode.SUCCESS)


def _run_calibration_market_data_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[3]

    try:
        request = _build_market_data_calibration_request_from_arguments(
            parsed_arguments,
            repository_root=repository_root,
        )
        prepared_calibration = prepare_market_data_calibration(request)
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(_classify_market_data_command_error(e))

    _write_command_output(
        parsed_arguments,
        json_payload=_build_market_data_calibration_summary_payload(prepared_calibration),
        human_output_factory=lambda: _render_market_data_calibration_human_output(prepared_calibration),
    )

    return int(CliExitCode.SUCCESS)


def _run_calibration_staged_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[3]
    execution_input_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.execution_input_json),
    )
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )
    disk_checkpoint_directory_argument = cast(Path | None, parsed_arguments.disk_checkpoint_directory)
    disk_checkpoint_directory = (
        _resolve_path_from_repository_root(repository_root, disk_checkpoint_directory_argument)
        if disk_checkpoint_directory_argument is not None
        else None
    )

    try:
        execution_input_document = _load_json_document(execution_input_path, "calibration execution input")
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        execution_input = validate_staged_calibration_execution_input(execution_input_document)
        normalized_execution_input = _override_calibration_execution_mode(
            execution_input=execution_input,
            execution_mode=CalibrationExecutionMode(cast(str, parsed_arguments.execution_mode)),
            disk_checkpoint_directory=disk_checkpoint_directory,
        )
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        response = run_staged_calibration(
            normalized_execution_input,
            calendar_registry=calendar_registry,
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_CALIBRATION_STAGED_ENTRYPOINT,
                validation_error_id=CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID,
                internal_error_id=CLI_CALIBRATION_STAGED_INTERNAL_ERROR_ID,
                file_field_path="execution_input_json",
                document_field_path="execution_input_document",
            )
        )

    _write_command_output(
        parsed_arguments,
        json_payload=response.to_payload(),
        human_output_factory=lambda: _render_calibration_staged_human_output(response.to_payload()),
    )

    return int(CliExitCode.SUCCESS)


def _build_market_data_calibration_request_from_arguments(
    parsed_arguments: Namespace,
    *,
    repository_root: Path,
) -> MarketDataCalibrationRequest:
    tick_data_path = _require_path_argument(
        repository_root,
        cast(Path | None, parsed_arguments.tick_data_path),
        argument_name="tick_data_path",
    )
    session_definition_path_argument = cast(Path | None, parsed_arguments.session_definition_path)
    session_definition_path = (
        _require_path_argument(
            repository_root,
            session_definition_path_argument,
            argument_name="session_definition_path",
        )
        if session_definition_path_argument is not None
        else None
    )
    disk_checkpoint_directory_argument = cast(Path | None, parsed_arguments.disk_checkpoint_directory)
    disk_checkpoint_directory = (
        _resolve_path_from_repository_root(repository_root, disk_checkpoint_directory_argument)
        if disk_checkpoint_directory_argument is not None
        else None
    )

    option_surface_path_argument = cast(Path | None, parsed_arguments.option_surface_path)
    option_surface_path = (
        _resolve_path_from_repository_root(repository_root, option_surface_path_argument)
        if option_surface_path_argument is not None
        else None
    )
    known_news_path_argument = cast(Path | None, parsed_arguments.known_news_path)
    known_news_path = (
        _resolve_path_from_repository_root(repository_root, known_news_path_argument)
        if known_news_path_argument is not None
        else None
    )

    _reject_orphan_market_data_arguments(
        option_surface_path is not None,
        (
            ("option_surface_format", parsed_arguments.option_surface_format),
            ("option_surface_provenance_label", parsed_arguments.option_surface_provenance_label),
            ("option_quote_input_policy", parsed_arguments.option_quote_input_policy),
            ("option_quote_timestamp_column", parsed_arguments.option_quote_timestamp_column),
            ("option_expiry_column", parsed_arguments.option_expiry_column),
            ("option_strike_column", parsed_arguments.option_strike_column),
            ("option_type_column", parsed_arguments.option_type_column),
            ("option_mid_price_column", parsed_arguments.option_mid_price_column),
            ("option_bid_column", parsed_arguments.option_bid_column),
            ("option_ask_column", parsed_arguments.option_ask_column),
            ("option_implied_volatility_column", parsed_arguments.option_implied_volatility_column),
        ),
        message="option-surface flags require --option-surface-path",
    )
    _reject_orphan_market_data_arguments(
        known_news_path is not None,
        (
            ("known_news_format", parsed_arguments.known_news_format),
            ("known_news_provenance_label", parsed_arguments.known_news_provenance_label),
            ("known_news_timezone", parsed_arguments.known_news_timezone),
            ("known_news_timestamp_column", parsed_arguments.known_news_timestamp_column),
            ("known_news_event_id_column", parsed_arguments.known_news_event_id_column),
        ),
        message="known-news flags require --known-news-path",
    )
    _reject_orphan_market_data_arguments(
        session_definition_path is not None,
        (
            ("session_definition_format", parsed_arguments.session_definition_format),
            ("session_definition_provenance_label", parsed_arguments.session_definition_provenance_label),
        ),
        message="session-definition override metadata flags require --session-definition-path",
    )

    payload: dict[str, Any] = {
        "schema_version": CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
        "run_id": cast(str, parsed_arguments.run_id),
        "tick_data_source": {
            "path": str(tick_data_path),
            "file_format": cast(str, parsed_arguments.tick_data_format),
            "provenance_label": cast(str, parsed_arguments.tick_data_provenance_label),
        },
        "session_configuration": {
            "exchange_code": cast(str, parsed_arguments.exchange_code),
        },
        "column_mapping": {
            "tick_data": {
                "timestamp_column": cast(str, parsed_arguments.tick_timestamp_column),
                "price_column": cast(Optional[str], parsed_arguments.tick_price_column),
                "trade_price_column": cast(Optional[str], parsed_arguments.tick_trade_price_column),
                "trade_sign_column": cast(Optional[str], parsed_arguments.tick_trade_sign_column),
                "bid_column": cast(Optional[str], parsed_arguments.tick_bid_column),
                "ask_column": cast(Optional[str], parsed_arguments.tick_ask_column),
                "close_price_column": cast(Optional[str], parsed_arguments.tick_close_price_column),
            },
        },
        "workflow_scope": cast(str, parsed_arguments.workflow_scope),
        "normalization_policy": {
            "timestamp_timezone": cast(str, parsed_arguments.timestamp_timezone),
            "sort_records_by_timestamp": not bool(parsed_arguments.disable_timestamp_sort),
            "duplicate_record_policy": cast(str, parsed_arguments.duplicate_record_policy),
            "tick_price_input_policy": cast(Optional[str], parsed_arguments.tick_price_input_policy),
            "option_quote_input_policy": cast(Optional[str], parsed_arguments.option_quote_input_policy),
        },
        "output_preferences": {
            "execution_mode": cast(str, parsed_arguments.execution_mode),
            "disk_checkpoint_directory": (
                str(disk_checkpoint_directory)
                if disk_checkpoint_directory is not None
                else None
            ),
            "emit_derived_staged_execution_input": bool(parsed_arguments.emit_derived_staged_execution_input),
            "emit_input_provenance_report": not bool(parsed_arguments.omit_input_provenance_report),
        },
    }

    if parsed_arguments.session_timezone is not None:
        payload["session_configuration"]["session_timezone"] = cast(
            str,
            parsed_arguments.session_timezone,
        )

    if session_definition_path is not None:
        payload["session_configuration"]["session_definition_source"] = {
            "path": str(session_definition_path),
            "file_format": cast(str, parsed_arguments.session_definition_format),
            "provenance_label": cast(str, parsed_arguments.session_definition_provenance_label),
        }

    if option_surface_path is not None:
        payload["option_surface_source"] = {
            "path": str(option_surface_path),
            "file_format": cast(Optional[str], parsed_arguments.option_surface_format),
            "provenance_label": cast(Optional[str], parsed_arguments.option_surface_provenance_label),
        }
        payload["column_mapping"]["option_surface"] = {
            "quote_timestamp_column": cast(Optional[str], parsed_arguments.option_quote_timestamp_column),
            "expiry_timestamp_column": cast(Optional[str], parsed_arguments.option_expiry_column),
            "strike_column": cast(Optional[str], parsed_arguments.option_strike_column),
            "option_type_column": cast(Optional[str], parsed_arguments.option_type_column),
            "mid_price_column": cast(Optional[str], parsed_arguments.option_mid_price_column),
            "bid_column": cast(Optional[str], parsed_arguments.option_bid_column),
            "ask_column": cast(Optional[str], parsed_arguments.option_ask_column),
            "implied_volatility_column": cast(Optional[str], parsed_arguments.option_implied_volatility_column),
        }

    if known_news_path is not None:
        payload["known_news_inputs"] = {
            "calendar_source": {
                "path": str(known_news_path),
                "file_format": cast(Optional[str], parsed_arguments.known_news_format),
                "provenance_label": cast(Optional[str], parsed_arguments.known_news_provenance_label),
            },
            "calendar_timezone": cast(Optional[str], parsed_arguments.known_news_timezone),
        }

        if parsed_arguments.known_news_timestamp_column is not None:
            payload["column_mapping"]["known_news"] = {
                "event_timestamp_column": cast(str, parsed_arguments.known_news_timestamp_column),
                "event_identifier_column": cast(Optional[str], parsed_arguments.known_news_event_id_column),
            }

    return validate_market_data_calibration_request(payload)


def _build_market_data_calibration_summary_payload(
    prepared_calibration: PreparedMarketDataCalibration,
) -> dict[str, Any]:
    request = prepared_calibration.request

    return {
        "schema_version": CLI_MARKET_DATA_SUMMARY_SCHEMA_VERSION,
        "command": CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT,
        "request_schema_version": request.schema_version,
        "preparation_schema_version": prepared_calibration.schema_version,
        "run_id": request.run_id,
        "workflow_scope": request.workflow_scope.value,
        "session_context": {
            "exchange_code": request.session_configuration.exchange_code,
            "session_timezone": prepared_calibration.session_resolution.resolved_timezone_name,
            "resolution_kind": prepared_calibration.session_resolution.resolution_kind,
        },
        "input_files": {
            "tick_data": request.tick_data_source.to_payload(),
            "session_definition": (
                request.session_configuration.session_definition_source.to_payload()
                if request.session_configuration.session_definition_source is not None
                else None
            ),
            "known_news": (
                request.known_news_inputs.calendar_source.to_payload()
                if request.known_news_inputs is not None
                else None
            ),
            "option_surface": (
                request.option_surface_source.to_payload()
                if request.option_surface_source is not None
                else None
            ),
        },
        "column_mapping": request.column_mapping.to_payload(),
        "normalization_policy": request.normalization_policy.to_payload(),
        "output_artifact_locations": {
            "disk_checkpoint_directory": request.output_preferences.disk_checkpoint_directory,
        },
        "output_emission_policy": {
            "execution_mode": request.output_preferences.execution_mode.value,
            "emit_input_provenance_report": request.output_preferences.emit_input_provenance_report,
            "emit_derived_staged_execution_input": request.output_preferences.emit_derived_staged_execution_input,
        },
        "diagnostics_summary": prepared_calibration.diagnostics.to_payload(),
        "session_resolution": prepared_calibration.session_resolution.to_payload(),
        "provenance_summary": prepared_calibration.provenance.to_payload(),
        "output_schema_metadata": {
            "schema_family": "cli.workflow.command_summary",
            "field_selection_policy": CLI_MARKET_DATA_SUMMARY_FIELD_SELECTION_POLICY,
            "required_sections": [
                "run_id",
                "workflow_scope",
                "session_context",
                "input_files",
                "column_mapping",
                "normalization_policy",
                "output_artifact_locations",
                "output_emission_policy",
                "diagnostics_summary",
                "session_resolution",
                "provenance_summary",
            ],
            "optional_input_sections": [
                "input_files.known_news",
                "input_files.option_surface",
                "column_mapping.known_news",
                "column_mapping.option_surface",
                "provenance_summary.option_surface_summary",
                "provenance_summary.known_news_summary",
            ],
        },
    }


def _render_market_data_calibration_human_output(
    prepared_calibration: PreparedMarketDataCalibration,
) -> str:
    request = prepared_calibration.request

    selected_stages = ",".join(stage.value for stage in prepared_calibration.diagnostics.selected_stages)
    preprocessing_steps = ",".join(prepared_calibration.diagnostics.executed_preprocessing_steps)

    lines: list[str] = [
        (
            "calibration market-data request prepared: "
            f"run_id={request.run_id} "
            f"workflow_scope={request.workflow_scope.value} "
            f"execution_mode={request.output_preferences.execution_mode.value}"
        ),
        (
            "tick_data: "
            f"path={request.tick_data_source.path} "
            f"format={request.tick_data_source.file_format.value} "
            f"provenance_label={request.tick_data_source.provenance_label}"
        ),
        (
            "session_resolution: "
            f"exchange_code={request.session_configuration.exchange_code} "
            f"mode={prepared_calibration.session_resolution.resolution_kind} "
            f"timezone={prepared_calibration.session_resolution.resolved_timezone_name} "
            f"confidence={prepared_calibration.session_resolution.confidence_label}"
        ),
        (
            "workflow_diagnostics: "
            f"preparation_schema_version={CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION} "
            f"selected_stages={selected_stages} "
            f"preprocessing_steps={preprocessing_steps}"
        ),
        (
            "loaded_data_summary: "
            f"tick_records={prepared_calibration.diagnostics.tick_record_count} "
            f"known_news_events={prepared_calibration.diagnostics.known_news_event_count} "
            f"option_surface_records={prepared_calibration.diagnostics.option_surface_record_count}"
        ),
    ]

    if request.session_configuration.session_definition_source is not None:
        lines.append(
            (
                "session_definition_override: "
                f"path={request.session_configuration.session_definition_source.path} "
                f"format={request.session_configuration.session_definition_source.file_format.value} "
                f"provenance_label={request.session_configuration.session_definition_source.provenance_label}"
            )
        )

    if request.known_news_inputs is not None:
        lines.append(
            (
                "known_news: "
                f"path={request.known_news_inputs.calendar_source.path} "
                f"format={request.known_news_inputs.calendar_source.file_format.value} "
                f"provenance_label={request.known_news_inputs.calendar_source.provenance_label}"
            )
        )

    if request.option_surface_source is not None:
        lines.append(
            (
                "option_surface: "
                f"path={request.option_surface_source.path} "
                f"format={request.option_surface_source.file_format.value} "
                f"provenance_label={request.option_surface_source.provenance_label}"
            )
        )

    lines.append(
        (
            "derived_references: "
            f"trade_events={prepared_calibration.provenance.derived_trade_events_data_reference} "
            f"option_surface={prepared_calibration.provenance.derived_option_surface_data_reference or 'none'}"
        )
    )
    lines.append(
        (
            "normalization_policy: "
            f"timestamp_timezone={request.normalization_policy.timestamp_timezone} "
            f"sort_records_by_timestamp={str(request.normalization_policy.sort_records_by_timestamp).lower()} "
            f"duplicate_record_policy={request.normalization_policy.duplicate_record_policy.value} "
            f"tick_price_input_policy={_optional_enum_value(request.normalization_policy.tick_price_input_policy)} "
            f"option_quote_input_policy={_optional_enum_value(request.normalization_policy.option_quote_input_policy)}"
        )
    )
    lines.append(
        (
            "artifact_targets: "
            f"disk_checkpoint_directory={request.output_preferences.disk_checkpoint_directory or 'none'} "
            f"emit_input_provenance_report={str(request.output_preferences.emit_input_provenance_report).lower()} "
            f"emit_derived_staged_execution_input={str(request.output_preferences.emit_derived_staged_execution_input).lower()}"
        )
    )

    return "\n".join(lines) + "\n"


def _render_simulation_seeded_ticks_human_output(response_payload: dict[str, Any]) -> str:
    diagnostics = cast(dict[str, Any], response_payload["diagnostics"])
    return (
        "simulation seeded-ticks completed: "
        f"schema_version={response_payload['schema_version']} "
        f"transition_count={diagnostics['transition_count']} "
        f"path_record_schema_version={cast(dict[str, Any], response_payload['metadata'])['path_record_schema_version']}\n"
    )


def _render_simulation_coarse_interval_human_output(response_payload: dict[str, Any]) -> str:
    diagnostics = cast(dict[str, Any], response_payload["diagnostics"])
    return (
        "simulation coarse-interval completed: "
        f"schema_version={response_payload['schema_version']} "
        f"simulated_interval_count={response_payload['simulated_interval_count']} "
        f"interval_delta={response_payload['interval_delta']} "
        f"cumulative_total_return={diagnostics['cumulative_total_return']}\n"
    )


def _render_pricing_vanilla_option_human_output(run_result_payload: dict[str, Any]) -> str:
    response_payload = cast(dict[str, Any], run_result_payload["response"])
    return (
        "pricing vanilla-option completed: "
        f"schema_version={run_result_payload['schema_version']} "
        f"price={response_payload['price']} "
        f"delta={response_payload['delta']}\n"
    )


def _render_seasonality_preprocess_timestamps_human_output(payload: dict[str, Any]) -> str:
    lines: list[str] = [
        (
            "seasonality preprocess-timestamps completed: "
            f"schema_version={payload['schema_version']} "
            f"exchange_code={payload['exchange_code']} "
            f"exchange_timezone_name={payload['exchange_timezone_name']} "
            f"accepted_record_count={payload['accepted_record_count']} "
            f"filtered_record_count={payload['filtered_record_count']}"
        ),
        (
            "coverage_diagnostics: "
            f"hour_bucket_count={payload['hour_bucket_count']} "
            f"missing_hour_bucket_count={payload['missing_hour_bucket_count']} "
            f"sparse_hour_bucket_count={payload['sparse_hour_bucket_count']} "
            f"first_event_timestamp_utc_nanos={payload['first_event_timestamp_utc_nanos']} "
            f"last_event_timestamp_utc_nanos={payload['last_event_timestamp_utc_nanos']}"
        ),
    ]

    return "\n".join(lines) + "\n"


def _render_seasonality_diagnostics_human_output(
    report: dict[str, Any],
    *,
    preprocessed_events: object,
    intraday_estimate: object,
    day_of_week_estimate: object,
    day_of_year_estimate: object,
) -> str:
    fit_quality_metrics = cast(dict[str, Any], report["fit_quality_metrics"])
    reproducibility_metadata = cast(dict[str, Any], report["reproducibility_metadata"])
    input_snapshot = cast(dict[str, Any], reproducibility_metadata["input_snapshot"])
    overlay_component_summary = cast(Optional[dict[str, Any]], report["overlay_component_summary"])
    active_overlay_event_count = (
        overlay_component_summary["active_overlay_event_count"]
        if overlay_component_summary is not None
        else 0
    )

    lines: list[str] = [
        (
            "seasonality diagnostics completed: "
            f"schema_version={report['schema_version']} "
            f"dataset_id={report['dataset_id']} "
            f"exchange_code={preprocessed_events.exchange_code} "
            f"accepted_record_count={preprocessed_events.accepted_record_count} "
            f"filtered_record_count={preprocessed_events.filtered_record_count}"
        ),
        (
            "fit_quality: "
            f"combined_factor_mean={fit_quality_metrics['combined_factor_mean']} "
            f"normalization_anchor_error={fit_quality_metrics['normalization_anchor_error']} "
            f"missing_hour_bucket_count={preprocessed_events.missing_hour_bucket_count} "
            f"sparse_hour_bucket_count={preprocessed_events.sparse_hour_bucket_count}"
        ),
        (
            "component_sparsity: "
            f"intraday_sparse_bin_count={intraday_estimate.sparse_bin_count} "
            f"day_of_week_sparse_bucket_count={day_of_week_estimate.sparse_bucket_count} "
            f"day_of_year_sparse_bucket_count={day_of_year_estimate.sparse_bucket_count} "
            f"active_overlay_event_count={active_overlay_event_count}"
        ),
        (
            "reproducibility: "
            f"estimator_version={reproducibility_metadata['estimator_version']} "
            f"snapshot_fingerprint_hex={input_snapshot['snapshot_fingerprint_hex']}"
        ),
    ]

    return "\n".join(lines) + "\n"


def _render_hourly_factor_materialize_seasonality_human_output(payload: dict[str, Any]) -> str:
    lines: list[str] = [
        (
            "hourly-factors materialize-seasonality completed: "
            f"schema_version={payload['schema_version']} "
            f"exchange_code={payload['exchange_code']} "
            f"interval_count={payload['interval_count']} "
            f"source_point_count={payload['source_point_count']}"
        ),
        (
            "materialization_diagnostics: "
            f"sparse_interval_count={payload['sparse_interval_count']} "
            f"interpolated_interval_count={payload['interpolated_interval_count']} "
            f"fallback_interval_count={payload['fallback_interval_count']} "
            f"interpolation_method={payload['interpolation_method']}"
        ),
    ]

    return "\n".join(lines) + "\n"


def _render_hourly_factor_materialize_news_intensity_human_output(payload: dict[str, Any]) -> str:
    lines: list[str] = [
        (
            "hourly-factors materialize-news-intensity completed: "
            f"schema_version={payload['schema_version']} "
            f"exchange_code={payload['exchange_code']} "
            f"interval_count={payload['interval_count']} "
            f"known_scheduled_arrival_count={payload['known_scheduled_arrival_count']}"
        ),
        (
            "materialization_diagnostics: "
            f"combined_component_non_zero_interval_count={payload['combined_component_non_zero_interval_count']} "
            f"surprise_multiplier_override_interval_count={payload['surprise_multiplier_override_interval_count']} "
            f"surprise_baseline_intensity={payload['surprise_baseline_intensity']}"
        ),
    ]

    return "\n".join(lines) + "\n"


def _render_hourly_factor_build_contract_human_output(payload: dict[str, Any]) -> str:
    lines: list[str] = [
        (
            "hourly-factors build-contract completed: "
            f"schema_version={payload['schema_version']} "
            f"exchange_code={payload['exchange_code']} "
            f"interval_count={payload['interval_count']}"
        ),
        (
            "contract_metadata: "
            f"seasonality_materialization_method={payload['seasonality_materialization_method']} "
            f"news_intensity_materialization_method={payload['news_intensity_materialization_method']} "
            f"skipped_local_hour_interval_count={payload['skipped_local_hour_interval_count']} "
            f"repeated_local_hour_interval_count={payload['repeated_local_hour_interval_count']}"
        ),
    ]

    return "\n".join(lines) + "\n"


def _render_stylized_facts_horizon_comparison_human_output(
    report_bundle: dict[str, Any],
    *,
    execution_mode: str,
) -> str:
    comparison_report = cast(dict[str, Any], report_bundle["comparison_report"])
    summary = cast(dict[str, Any], comparison_report["summary"])
    lines: list[str] = [
        (
            "stylized-facts horizon-comparison completed: "
            f"schema_version={report_bundle['schema_version']} "
            f"execution_mode={execution_mode} "
            f"status={comparison_report['status']} "
            f"metric_catalog_id={comparison_report['metric_catalog_id']} "
            f"run_count={summary['total_run_count']} "
            f"table_row_count={len(cast(list[dict[str, Any]], report_bundle['table_summary']['rows']))}"
        ),
        (
            "decision_counts: "
            f"run_pass={summary['run_decision_counts']['pass_count']} "
            f"run_fail={summary['run_decision_counts']['fail_count']} "
            f"run_insufficient_data={summary['run_decision_counts']['insufficient_data_count']} "
            f"metric_pass={summary['metric_decision_counts']['pass_count']} "
            f"metric_fail={summary['metric_decision_counts']['fail_count']} "
            f"metric_insufficient_data={summary['metric_decision_counts']['insufficient_data_count']}"
        ),
    ]

    artifact_export = cast(Optional[dict[str, Any]], report_bundle["artifact_export"])
    if artifact_export is not None:
        lines.append(
            (
                "artifact_export: "
                f"export_directory={artifact_export['export_directory']} "
                f"file_stem={artifact_export['file_stem']} "
                f"exported_file_count={len(cast(list[dict[str, Any]], artifact_export['exported_files']))}"
            )
        )

    return "\n".join(lines) + "\n"


def _render_calibration_staged_human_output(response_payload: dict[str, Any]) -> str:
    return (
        "calibration staged completed: "
        f"run_id={response_payload['run_id']} "
        f"execution_mode={response_payload['execution_mode']} "
        f"status={response_payload['status']} "
        f"stage_report_count={len(cast(list[dict[str, Any]], response_payload['stage_reports']))} "
        f"checkpoint_count={len(cast(list[dict[str, Any]], response_payload['emitted_checkpoints']))}\n"
    )


def _run_deterministic_regression_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[3]
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )
    calendar_cases_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_cases_json),
    )
    seasonality_cases_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.seasonality_cases_json),
    )

    file_field_path = "calendar_registry_json"
    document_field_path = "calendar_registry_document"

    try:
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        file_field_path = "calendar_cases_json"
        document_field_path = "calendar_cases_document"
        calendar_cases_document = _load_json_document(calendar_cases_path, "calendar cases")
        file_field_path = "seasonality_cases_json"
        document_field_path = "seasonality_cases_document"
        seasonality_cases_document = _load_json_document(seasonality_cases_path, "seasonality cases")

        document_field_path = "calendar_registry_document"
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        document_field_path = "calendar_cases_document"
        calendar_cases = _extract_calendar_cases(calendar_cases_document)
        document_field_path = "seasonality_cases_document"
        seasonality_cases = _extract_seasonality_cases(seasonality_cases_document)
        report = run_deterministic_regression_harness(
            calendar_registry=calendar_registry,
            calendar_cases=calendar_cases,
            seasonality_cases=seasonality_cases,
            seasonality_absolute_tolerance=float(parsed_arguments.seasonality_absolute_tolerance),
            news_intensity_absolute_tolerance=float(parsed_arguments.news_intensity_absolute_tolerance),
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_DETERMINISTIC_REGRESSION_ENTRYPOINT,
                validation_error_id=CLI_DETERMINISTIC_REGRESSION_VALIDATION_ERROR_ID,
                internal_error_id=CLI_DETERMINISTIC_REGRESSION_INTERNAL_ERROR_ID,
                file_field_path=file_field_path,
                document_field_path=document_field_path,
            )
        )

    output_mode = CliOutputMode(cast(str, parsed_arguments.output))
    failed_case_count = cast(dict[str, int], report["summary"])["failed_case_count"]
    if output_mode == CliOutputMode.JSON:
        _write_contract_json(report)
        return int(CliExitCode.SUCCESS if failed_case_count == 0 else CliExitCode.RUNTIME_ERROR)

    _write_deterministic_regression_human_output(report)
    return int(CliExitCode.SUCCESS if failed_case_count == 0 else CliExitCode.RUNTIME_ERROR)


def _run_open_closed_gap_leakage_regression_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[3]
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
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(_classify_open_closed_gap_command_error(e))

    output_mode = CliOutputMode(cast(str, parsed_arguments.output))
    failed_case_count = cast(dict[str, int], report["summary"])["failed_case_count"]
    if output_mode == CliOutputMode.JSON:
        _write_contract_json(report)
        return int(CliExitCode.SUCCESS if failed_case_count == 0 else CliExitCode.RUNTIME_ERROR)

    _write_open_closed_gap_human_output(report)
    return int(CliExitCode.SUCCESS if failed_case_count == 0 else CliExitCode.RUNTIME_ERROR)


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

    return JaniformIntegrationError(
        JaniformErrorDetails.from_context(
            error_id=CLI_OPEN_CLOSED_GAP_INTERNAL_ERROR_ID,
            category=JaniformErrorCategory.INTEGRATION,
            workflow="cli",
            entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
            message="open closed gap leakage regression command failed unexpectedly",
            context={"source_exception_type": type(error).__name__},
        )
    )


def _classify_seasonality_preprocess_command_error(
    error: Exception,
    *,
    file_field_path: str,
    document_field_path: str,
) -> JaniformError:
    if isinstance(error, JaniformError):
        return error

    if isinstance(error, CalendarMissingExchangeError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_ENTRYPOINT,
                field_path="exchange_code",
                message=str(error),
            )
        )

    if isinstance(error, CalendarInvalidTimezoneError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_ENTRYPOINT,
                field_path="calendar_registry_document",
                message=str(error),
            )
        )

    if isinstance(error, SeasonalityInputValidationError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_ENTRYPOINT,
                field_path="event_records_document",
                message=str(error),
            )
        )

    return _classify_workflow_command_error(
        error,
        entrypoint=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_ENTRYPOINT,
        validation_error_id=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_VALIDATION_ERROR_ID,
        internal_error_id=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_INTERNAL_ERROR_ID,
        file_field_path=file_field_path,
        document_field_path=document_field_path,
    )


def _classify_seasonality_diagnostics_command_error(
    error: Exception,
    *,
    file_field_path: str,
    document_field_path: str,
) -> JaniformError:
    if isinstance(error, JaniformError):
        return error

    if isinstance(error, CalendarMissingExchangeError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
                field_path="exchange_code",
                message=str(error),
            )
        )

    if isinstance(error, CalendarInvalidTimezoneError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
                field_path="calendar_registry_document",
                message=str(error),
            )
        )

    if isinstance(error, SeasonalityInputValidationError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
                field_path=document_field_path,
                message=str(error),
            )
        )

    return _classify_workflow_command_error(
        error,
        entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        validation_error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
        internal_error_id=CLI_SEASONALITY_DIAGNOSTICS_INTERNAL_ERROR_ID,
        file_field_path=file_field_path,
        document_field_path=document_field_path,
    )


def _classify_market_data_command_error(error: Exception) -> JaniformError:
    if isinstance(error, JaniformError):
        return error

    if isinstance(error, ValueError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_CALIBRATION_MARKET_DATA_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT,
                field_path="command_input",
                message=str(error),
            )
        )

    return JaniformIntegrationError(
        JaniformErrorDetails.from_context(
            error_id=CLI_CALIBRATION_MARKET_DATA_INTERNAL_ERROR_ID,
            category=JaniformErrorCategory.INTEGRATION,
            workflow="cli",
            entrypoint=CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT,
            message="market-data calibration command failed unexpectedly",
            context={"source_exception_type": type(error).__name__},
        )
    )


def _attach_runtime_hourly_factor_contract(
    *,
    request: SeededTickSimulationRequest,
    calendar_registry: CalendarRegistry,
) -> SeededTickSimulationRequest:
    interval_duration_nanos = request.hourly_factor_summary.interval_duration_nanos
    range_end_utc_nanos = datetime_to_utc_nanos(request.start_time_utc) + (
        request.horizon_interval_count * interval_duration_nanos
    )
    whole_seconds, remainder_nanos = divmod(range_end_utc_nanos, 1_000_000_000)
    range_end_utc = datetime.fromtimestamp(whole_seconds, tz=UTC) + timedelta(
        microseconds=remainder_nanos // 1_000,
    )

    hourly_factor_contract = load_hourly_factor_contract_for_simulation(
        calendar_registry=calendar_registry,
        exchange_code=request.hourly_factor_summary.exchange_code,
        range_start_utc=request.start_time_utc,
        range_end_utc=range_end_utc,
        interval=f"{interval_duration_nanos}ns",
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
        nig_shared_plus_shift_configuration=request.nig_shared_plus_shift_configuration,
        news_process_configuration=request.news_process_configuration,
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
    internal_error_id: str,
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

    if isinstance(error, OSError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=validation_error_id,
                workflow="cli",
                entrypoint=entrypoint,
                field_path=file_field_path,
                message=f"input file could not be read: {error}",
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

    return JaniformIntegrationError(
        JaniformErrorDetails.from_context(
            error_id=internal_error_id,
            category=JaniformErrorCategory.INTEGRATION,
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


def _require_path_argument(
    repository_root: Path,
    path: Path | None,
    *,
    argument_name: str,
) -> Path:
    if path is None:
        raise ValueError(f"--{argument_name.replace('_', '-')} is required")

    return _resolve_path_from_repository_root(repository_root, path)


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


def _load_json_document(path: Path, context: str) -> dict[str, Any]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise ValueError(f"{context} at {path} must decode to a JSON object")

    return cast(dict[str, Any], payload)


def _write_contract_json(payload: dict[str, Any]) -> None:
    sys.stdout.write(f"{json.dumps(payload, sort_keys=True)}\n")


def _write_cli_error(error: JaniformError) -> int:
    sys.stderr.write(f"{error}\n")
    return int(_exit_code_for_error(error))


def _exit_code_for_error(error: JaniformError) -> CliExitCode:
    match error.category:
        case JaniformErrorCategory.VALIDATION:
            return CliExitCode.VALIDATION_ERROR
        case JaniformErrorCategory.EXECUTION:
            return CliExitCode.RUNTIME_ERROR
        case JaniformErrorCategory.INTEGRATION:
            return CliExitCode.INTERNAL_ERROR


def _write_command_output(
    parsed_arguments: Namespace,
    *,
    json_payload: dict[str, Any],
    human_output_factory: Callable[[], str],
) -> None:
    output_mode = CliOutputMode(cast(str, parsed_arguments.output))
    match output_mode:
        case CliOutputMode.JSON:
            _write_contract_json(json_payload)
        case CliOutputMode.HUMAN:
            sys.stdout.write(human_output_factory())


def _write_deterministic_regression_human_output(report: dict[str, Any]) -> None:
    summary = cast(dict[str, int], report["summary"])
    if summary["failed_case_count"] == 0:
        sys.stdout.write(
            "deterministic regression passed: "
            f"{summary['passed_case_count']}/{summary['total_case_count']} cases\n",
        )
        return

    sys.stderr.write(
        "deterministic regression failed: "
        f"{summary['failed_case_count']}/{summary['total_case_count']} cases\n",
    )

    for case_report in cast(list[dict[str, Any]], report["case_reports"]):
        if case_report["status"] != "fail":
            continue

        failure = cast(dict[str, Any] | None, case_report["failure"])
        failure_kind = failure["failure_kind"] if failure is not None else "unknown"
        sys.stderr.write(
            f"fixture_id={case_report['fixture_id']} "
            f"case_kind={case_report['case_kind']} "
            f"failure_kind={failure_kind} "
            f"summary={case_report['summary']}\n",
        )


def _write_open_closed_gap_human_output(report: dict[str, Any]) -> None:
    summary = cast(dict[str, int], report["summary"])
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
        return

    sys.stderr.write(
        "open closed gap leakage regression failed: "
        f"{summary['failed_case_count']}/{summary['total_case_count']} scenarios, "
        f"semantic_mismatches={summary['total_semantic_mismatch_failure_count']}\n",
    )

    for case_report in cast(list[dict[str, Any]], report["case_reports"]):
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

        for failure in cast(list[dict[str, Any]], case_report["failures"]):
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


def _optional_enum_value(value: Optional[StrEnum]) -> str:
    return value.value if value is not None else "none"


def _build_intraday_seasonality_settings_from_arguments(
    parsed_arguments: Namespace,
) -> IntradaySeasonalityEstimatorSettings:
    return IntradaySeasonalityEstimatorSettings(
        minutes_per_bin=_require_positive_integer_cli_argument(
            int(parsed_arguments.intraday_minutes_per_bin),
            field_path="intraday_minutes_per_bin",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        ),
        smoothing_kernel=IntradaySmoothingKernel(cast(str, parsed_arguments.intraday_smoothing_kernel)),
        smoothing_bandwidth_bins=_require_positive_float_cli_argument(
            float(parsed_arguments.intraday_smoothing_bandwidth_bins),
            field_path="intraday_smoothing_bandwidth_bins",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        ),
        minimum_bin_events=_require_positive_integer_cli_argument(
            int(parsed_arguments.intraday_minimum_bin_events),
            field_path="intraday_minimum_bin_events",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        ),
        pseudocount=_require_positive_float_cli_argument(
            float(parsed_arguments.intraday_pseudocount),
            field_path="intraday_pseudocount",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        ),
    )


def _build_day_of_week_seasonality_settings_from_arguments(
    parsed_arguments: Namespace,
) -> DayOfWeekSeasonalityEstimatorSettings:
    return DayOfWeekSeasonalityEstimatorSettings(
        minimum_bucket_events=_require_positive_integer_cli_argument(
            int(parsed_arguments.day_of_week_minimum_bucket_events),
            field_path="day_of_week_minimum_bucket_events",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        ),
        pseudocount=_require_positive_float_cli_argument(
            float(parsed_arguments.day_of_week_pseudocount),
            field_path="day_of_week_pseudocount",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        ),
    )


def _build_day_of_year_seasonality_settings_from_arguments(
    parsed_arguments: Namespace,
) -> DayOfYearSeasonalityEstimatorSettings:
    return DayOfYearSeasonalityEstimatorSettings(
        minimum_bucket_events=_require_positive_integer_cli_argument(
            int(parsed_arguments.day_of_year_minimum_bucket_events),
            field_path="day_of_year_minimum_bucket_events",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        ),
        pseudocount=_require_positive_float_cli_argument(
            float(parsed_arguments.day_of_year_pseudocount),
            field_path="day_of_year_pseudocount",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        ),
    )


def _require_non_empty_cli_string(
    value: str,
    *,
    field_path: str,
    error_id: str,
    entrypoint: str,
) -> str:
    if value.strip():
        return value

    raise JaniformValidationError(
        ContractValidationFailure.from_context(
            error_id=error_id,
            workflow="cli",
            entrypoint=entrypoint,
            field_path=field_path,
            message="must be a non-empty string",
        )
    )


def _require_positive_integer_cli_argument(
    value: int,
    *,
    field_path: str,
    error_id: str,
    entrypoint: str,
) -> int:
    if value > 0:
        return value

    raise JaniformValidationError(
        ContractValidationFailure.from_context(
            error_id=error_id,
            workflow="cli",
            entrypoint=entrypoint,
            field_path=field_path,
            message="must be strictly positive",
        )
    )


def _require_positive_float_cli_argument(
    value: float,
    *,
    field_path: str,
    error_id: str,
    entrypoint: str,
) -> float:
    if value > 0.0:
        return value

    raise JaniformValidationError(
        ContractValidationFailure.from_context(
            error_id=error_id,
            workflow="cli",
            entrypoint=entrypoint,
            field_path=field_path,
            message="must be strictly positive",
        )
    )


def _parse_utc_datetime_cli_argument(
    value: str,
    *,
    field_path: str,
    error_id: str,
    entrypoint: str,
) -> datetime:
    try:
        parsed_value = datetime.fromisoformat(value)
    except ValueError as e:
        raise JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=error_id,
                workflow="cli",
                entrypoint=entrypoint,
                field_path=field_path,
                message="must be a valid ISO 8601 timestamp",
            )
        ) from e

    if parsed_value.tzinfo is None:
        raise JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=error_id,
                workflow="cli",
                entrypoint=entrypoint,
                field_path=field_path,
                message="must be timezone-aware",
            )
        )

    return parsed_value.astimezone(UTC)


def _extract_hourly_seasonality_materialization_input(
    materialization_input_document: dict[str, Any],
) -> tuple[list[datetime], list[float], HourlySeasonalityMaterializationSettings]:
    source_timestamps_utc = _extract_datetime_sequence_field(
        materialization_input_document,
        field_name="source_timestamps_utc",
        field_prefix="hourly seasonality materialization input",
    )
    source_seasonality_factors = _extract_float_sequence_field(
        materialization_input_document,
        field_name="source_seasonality_factors",
        field_prefix="hourly seasonality materialization input",
    )
    sparse_hour_minimum_observation_count = _extract_int_field(
        materialization_input_document,
        field_name="sparse_hour_minimum_observation_count",
        field_prefix="hourly seasonality materialization input",
        default=1,
    )

    return (
        source_timestamps_utc,
        source_seasonality_factors,
        HourlySeasonalityMaterializationSettings(
            interpolation_method=_extract_string_field(
                materialization_input_document,
                field_name="interpolation_method",
                field_prefix="hourly seasonality materialization input",
                default=DEFAULT_SEASONALITY_INTERPOLATION_METHOD,
            ),
            aggregation_method=_extract_string_field(
                materialization_input_document,
                field_name="aggregation_method",
                field_prefix="hourly seasonality materialization input",
                default=DEFAULT_SEASONALITY_AGGREGATION_METHOD,
            ),
            sparse_fallback_policy=_extract_string_field(
                materialization_input_document,
                field_name="sparse_fallback_policy",
                field_prefix="hourly seasonality materialization input",
                default=DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY,
            ),
            sparse_hour_minimum_observation_count=sparse_hour_minimum_observation_count,
        ),
    )


def _extract_hourly_news_intensity_materialization_input(
    materialization_input_document: dict[str, Any],
) -> HourlyNewsIntensityMaterializationSettings:
    raw_interval_multipliers = materialization_input_document.get(
        "surprise_deterministic_time_multiplier_by_interval_index"
    )
    if raw_interval_multipliers is None:
        surprise_deterministic_time_multiplier_by_interval_index: dict[int, float] | None = None
    else:
        if not isinstance(raw_interval_multipliers, dict):
            raise ValueError(
                "hourly news-intensity materialization input field "
                "`surprise_deterministic_time_multiplier_by_interval_index` must be an object"
            )

        surprise_deterministic_time_multiplier_by_interval_index = {}
        for raw_interval_index, raw_multiplier in raw_interval_multipliers.items():
            try:
                interval_index = int(raw_interval_index)
            except (TypeError, ValueError) as e:
                raise ValueError(
                    "hourly news-intensity materialization input override keys must be integer strings"
                ) from e

            surprise_deterministic_time_multiplier_by_interval_index[interval_index] = _coerce_float(
                raw_multiplier,
                field_name=(
                    "hourly news-intensity materialization input field "
                    f"`surprise_deterministic_time_multiplier_by_interval_index[{raw_interval_index}]`"
                ),
            )

    return HourlyNewsIntensityMaterializationSettings(
        known_scheduled_event_timestamps_utc=_extract_datetime_sequence_field(
            materialization_input_document,
            field_name="known_scheduled_event_timestamps_utc",
            field_prefix="hourly news-intensity materialization input",
        ),
        surprise_baseline_intensity=_extract_required_float_field(
            materialization_input_document,
            field_name="surprise_baseline_intensity",
            field_prefix="hourly news-intensity materialization input",
        ),
        surprise_deterministic_time_multiplier_by_interval_index=(
            surprise_deterministic_time_multiplier_by_interval_index
        ),
    )


def _extract_explicit_hourly_factor_source_document(
    source_document: dict[str, Any],
    *,
    field_prefix: str,
) -> tuple[list[float], str]:
    return (
        _extract_float_sequence_field(
            source_document,
            field_name="hourly_factors",
            field_prefix=field_prefix,
        ),
        _extract_string_field(
            source_document,
            field_name="materialization_method",
            field_prefix=field_prefix,
            default=DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        ),
    )


def _extract_materialized_hourly_seasonality_source_document(
    source_document: dict[str, Any],
    *,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
) -> tuple[list[float], str]:
    _validate_hourly_materialization_source_document(
        source_document,
        expected_schema_version=HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
        exchange_code=exchange_code,
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        field_prefix="hourly seasonality materialization source",
    )

    return (
        _extract_float_sequence_field(
            source_document,
            field_name="s_h",
            field_prefix="hourly seasonality materialization source",
        ),
        HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
    )


def _extract_materialized_hourly_news_intensity_source_document(
    source_document: dict[str, Any],
    *,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
) -> tuple[list[float], str]:
    _validate_hourly_materialization_source_document(
        source_document,
        expected_schema_version=HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
        exchange_code=exchange_code,
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        field_prefix="hourly news-intensity materialization source",
    )

    return (
        _extract_float_sequence_field(
            source_document,
            field_name="nu_h",
            field_prefix="hourly news-intensity materialization source",
        ),
        HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
    )


def _validate_hourly_materialization_source_document(
    source_document: dict[str, Any],
    *,
    expected_schema_version: str,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
    field_prefix: str,
) -> None:
    schema_version = _extract_string_field(
        source_document,
        field_name="schema_version",
        field_prefix=field_prefix,
    )
    if schema_version != expected_schema_version:
        raise ValueError(f"{field_prefix} must have schema_version {expected_schema_version!r}")

    document_exchange_code = _extract_string_field(
        source_document,
        field_name="exchange_code",
        field_prefix=field_prefix,
    )
    if document_exchange_code != exchange_code:
        raise ValueError(
            f"{field_prefix} exchange_code {document_exchange_code!r} does not match {exchange_code!r}"
        )

    range_start_nanos = _extract_int_field(
        source_document,
        field_name="request_start_utc_nanos",
        field_prefix=field_prefix,
    )
    if range_start_nanos != datetime_to_utc_nanos(range_start_utc):
        raise ValueError(
            f"{field_prefix} request_start_utc_nanos does not match the requested range_start_utc"
        )

    range_end_nanos = _extract_int_field(
        source_document,
        field_name="request_end_utc_nanos",
        field_prefix=field_prefix,
    )
    if range_end_nanos != datetime_to_utc_nanos(range_end_utc):
        raise ValueError(
            f"{field_prefix} request_end_utc_nanos does not match the requested range_end_utc"
        )


def _extract_datetime_sequence_field(
    document: dict[str, Any],
    *,
    field_name: str,
    field_prefix: str,
) -> list[datetime]:
    raw_values = document.get(field_name)
    if not isinstance(raw_values, list):
        raise ValueError(f"{field_prefix} must include list field `{field_name}`")

    parsed_values: list[datetime] = []
    for raw_value in raw_values:
        if not isinstance(raw_value, str):
            raise ValueError(f"{field_prefix} field `{field_name}` must contain only timestamp strings")

        try:
            parsed_value = datetime.fromisoformat(raw_value)
        except ValueError as e:
            raise ValueError(f"{field_prefix} field `{field_name}` contains invalid timestamp {raw_value!r}") from e

        if parsed_value.tzinfo is None:
            raise ValueError(
                f"{field_prefix} field `{field_name}` contains naive timestamp {raw_value!r}"
            )

        parsed_values.append(parsed_value.astimezone(UTC))

    return parsed_values


def _extract_float_sequence_field(
    document: dict[str, Any],
    *,
    field_name: str,
    field_prefix: str,
) -> list[float]:
    raw_values = document.get(field_name)
    if not isinstance(raw_values, list):
        raise ValueError(f"{field_prefix} must include list field `{field_name}`")

    return [
        _coerce_float(
            raw_value,
            field_name=f"{field_prefix} field `{field_name}`",
        )
        for raw_value in raw_values
    ]


def _extract_string_field(
    document: dict[str, Any],
    *,
    field_name: str,
    field_prefix: str,
    default: str | None = None,
) -> str:
    raw_value = document.get(field_name, default)
    if not isinstance(raw_value, str) or not raw_value.strip():
        raise ValueError(f"{field_prefix} field `{field_name}` must be a non-empty string")

    return raw_value


def _extract_int_field(
    document: dict[str, Any],
    *,
    field_name: str,
    field_prefix: str,
    default: int | None = None,
) -> int:
    raw_value = document.get(field_name, default)
    if isinstance(raw_value, bool) or not isinstance(raw_value, int):
        raise ValueError(f"{field_prefix} field `{field_name}` must be an integer")

    return raw_value


def _extract_required_float_field(
    document: dict[str, Any],
    *,
    field_name: str,
    field_prefix: str,
) -> float:
    if field_name not in document:
        raise ValueError(f"{field_prefix} must include field `{field_name}`")

    return _coerce_float(document[field_name], field_name=f"{field_prefix} field `{field_name}`")


def _coerce_float(raw_value: object, *, field_name: str) -> float:
    if isinstance(raw_value, bool) or not isinstance(raw_value, int | float):
        raise ValueError(f"{field_name} must contain numeric values")

    return float(raw_value)


def _build_timestamp_event_preprocessing_payload(preprocessed_events: object) -> dict[str, Any]:
    return {
        "schema_version": cast(str, preprocessed_events.schema_version),
        "input_schema_version": cast(str, preprocessed_events.input_schema_version),
        "exchange_code": cast(str, preprocessed_events.exchange_code),
        "exchange_timezone_name": cast(str, preprocessed_events.exchange_timezone_name),
        "source_record_positions": list(cast(list[int], preprocessed_events.source_record_positions)),
        "timestamp_utc_nanos": list(cast(list[int], preprocessed_events.timestamp_utc_nanos)),
        "exchange_local_minute_of_day": list(cast(list[int], preprocessed_events.exchange_local_minute_of_day)),
        "exchange_local_day_of_week": list(cast(bytes, preprocessed_events.exchange_local_day_of_week)),
        "exchange_local_day_of_year": list(cast(list[int], preprocessed_events.exchange_local_day_of_year)),
        "market_state_codes": list(cast(bytes, preprocessed_events.market_state_codes)),
        "missing_hour_bucket_utc_nanos": list(cast(list[int], preprocessed_events.missing_hour_bucket_utc_nanos)),
        "sparse_hour_bucket_utc_nanos": list(cast(list[int], preprocessed_events.sparse_hour_bucket_utc_nanos)),
        "input_record_count": cast(int, preprocessed_events.input_record_count),
        "validated_record_count": cast(int, preprocessed_events.validated_record_count),
        "accepted_record_count": cast(int, preprocessed_events.accepted_record_count),
        "filtered_record_count": cast(int, preprocessed_events.filtered_record_count),
        "first_event_timestamp_utc_nanos": cast(Optional[int], preprocessed_events.first_event_timestamp_utc_nanos),
        "last_event_timestamp_utc_nanos": cast(Optional[int], preprocessed_events.last_event_timestamp_utc_nanos),
        "hour_bucket_count": cast(int, preprocessed_events.hour_bucket_count),
        "missing_hour_bucket_count": cast(int, preprocessed_events.missing_hour_bucket_count),
        "sparse_hour_bucket_count": cast(int, preprocessed_events.sparse_hour_bucket_count),
    }


def _build_hourly_factor_contract_payload(contract: HourlyFactorContract) -> dict[str, Any]:
    return {
        "schema_version": contract.schema_version,
        "exchange_code": contract.exchange_code,
        "exchange_timezone_name": contract.exchange_timezone_name,
        "request_start_utc_nanos": contract.request_start_utc_nanos,
        "request_end_utc_nanos": contract.request_end_utc_nanos,
        "interval_duration_nanos": contract.interval_duration_nanos,
        "interval_indices": list(contract.interval_indices),
        "interval_start_utc_nanos": list(contract.interval_start_utc_nanos),
        "interval_end_utc_nanos": list(contract.interval_end_utc_nanos),
        "interval_start_exchange_local": list(contract.interval_start_exchange_local),
        "interval_end_exchange_local": list(contract.interval_end_exchange_local),
        "s_h": list(contract.s_h),
        "nu_h": list(contract.nu_h),
        "interval_count": contract.interval_count,
        "first_interval_start_utc_nanos": contract.first_interval_start_utc_nanos,
        "last_interval_end_utc_nanos": contract.last_interval_end_utc_nanos,
        "skipped_local_hour_interval_count": contract.skipped_local_hour_interval_count,
        "repeated_local_hour_interval_count": contract.repeated_local_hour_interval_count,
        "interval_boundary_policy": contract.interval_boundary_policy,
        "interval_alignment_policy": contract.interval_alignment_policy,
        "timezone_transition_policy": contract.timezone_transition_policy,
        "spring_forward_policy": contract.spring_forward_policy,
        "fall_back_policy": contract.fall_back_policy,
        "seasonality_materialization_method": contract.seasonality_materialization_method,
        "news_intensity_materialization_method": contract.news_intensity_materialization_method,
    }


def _build_hourly_seasonality_materialization_payload(
    materialization: HourlySeasonalityMaterialization,
) -> dict[str, Any]:
    return {
        "schema_version": materialization.schema_version,
        "exchange_code": materialization.exchange_code,
        "exchange_timezone_name": materialization.exchange_timezone_name,
        "request_start_utc_nanos": materialization.request_start_utc_nanos,
        "request_end_utc_nanos": materialization.request_end_utc_nanos,
        "interval_duration_nanos": materialization.interval_duration_nanos,
        "interval_indices": list(materialization.interval_indices),
        "interval_start_utc_nanos": list(materialization.interval_start_utc_nanos),
        "interval_end_utc_nanos": list(materialization.interval_end_utc_nanos),
        "interval_start_exchange_local": list(materialization.interval_start_exchange_local),
        "interval_end_exchange_local": list(materialization.interval_end_exchange_local),
        "s_h": list(materialization.s_h),
        "source_point_count": materialization.source_point_count,
        "interval_count": materialization.interval_count,
        "sparse_interval_count": materialization.sparse_interval_count,
        "interpolated_interval_count": materialization.interpolated_interval_count,
        "fallback_interval_count": materialization.fallback_interval_count,
        "skipped_local_hour_interval_count": materialization.skipped_local_hour_interval_count,
        "repeated_local_hour_interval_count": materialization.repeated_local_hour_interval_count,
        "interpolation_method": materialization.interpolation_method,
        "aggregation_method": materialization.aggregation_method,
        "sparse_fallback_policy": materialization.sparse_fallback_policy,
        "sparse_hour_minimum_observation_count": materialization.sparse_hour_minimum_observation_count,
        "interval_boundary_policy": materialization.interval_boundary_policy,
        "interval_alignment_policy": materialization.interval_alignment_policy,
        "timezone_transition_policy": materialization.timezone_transition_policy,
        "spring_forward_policy": materialization.spring_forward_policy,
        "fall_back_policy": materialization.fall_back_policy,
    }


def _build_hourly_news_intensity_materialization_payload(
    materialization: HourlyNewsIntensityMaterialization,
) -> dict[str, Any]:
    return {
        "schema_version": materialization.schema_version,
        "exchange_code": materialization.exchange_code,
        "exchange_timezone_name": materialization.exchange_timezone_name,
        "request_start_utc_nanos": materialization.request_start_utc_nanos,
        "request_end_utc_nanos": materialization.request_end_utc_nanos,
        "interval_duration_nanos": materialization.interval_duration_nanos,
        "interval_indices": list(materialization.interval_indices),
        "interval_start_utc_nanos": list(materialization.interval_start_utc_nanos),
        "interval_end_utc_nanos": list(materialization.interval_end_utc_nanos),
        "interval_start_exchange_local": list(materialization.interval_start_exchange_local),
        "interval_end_exchange_local": list(materialization.interval_end_exchange_local),
        "known_component_nu_h": list(materialization.known_component_nu_h),
        "surprise_component_nu_h": list(materialization.surprise_component_nu_h),
        "nu_h": list(materialization.nu_h),
        "interval_count": materialization.interval_count,
        "known_component_non_zero_interval_count": materialization.known_component_non_zero_interval_count,
        "surprise_component_non_zero_interval_count": materialization.surprise_component_non_zero_interval_count,
        "combined_component_non_zero_interval_count": materialization.combined_component_non_zero_interval_count,
        "known_scheduled_arrival_count": materialization.known_scheduled_arrival_count,
        "surprise_multiplier_override_interval_count": materialization.surprise_multiplier_override_interval_count,
        "skipped_local_hour_interval_count": materialization.skipped_local_hour_interval_count,
        "repeated_local_hour_interval_count": materialization.repeated_local_hour_interval_count,
        "materialization_method": materialization.materialization_method,
        "component_aggregation_policy": materialization.component_aggregation_policy,
        "arrival_intensity_policy": materialization.arrival_intensity_policy,
        "known_news_calendar_anchor_policy": materialization.known_news_calendar_anchor_policy,
        "surprise_news_intensity_control_policy": materialization.surprise_news_intensity_control_policy,
        "known_news_calendar_anchor_count": materialization.known_news_calendar_anchor_count,
        "surprise_baseline_intensity": materialization.surprise_baseline_intensity,
        "interval_boundary_policy": materialization.interval_boundary_policy,
        "interval_alignment_policy": materialization.interval_alignment_policy,
        "timezone_transition_policy": materialization.timezone_transition_policy,
        "spring_forward_policy": materialization.spring_forward_policy,
        "fall_back_policy": materialization.fall_back_policy,
    }


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


def _extract_event_records(
    event_records_document: dict[str, Any],
) -> list[dict[str, Any]]:
    raw_event_records: object = event_records_document.get("event_records")
    if not isinstance(raw_event_records, list):
        raise ValueError("event records document must include list field `event_records`")

    parsed_event_records: list[dict[str, Any]] = []
    for raw_event_record in raw_event_records:
        if not isinstance(raw_event_record, dict):
            raise ValueError("event records document record must be an object")

        parsed_event_records.append(cast(dict[str, Any], raw_event_record))

    return parsed_event_records


def _extract_calendar_cases(
    calendar_cases_document: dict[str, Any],
) -> list[dict[str, Any]]:
    raw_cases: object = calendar_cases_document.get("cases")
    if not isinstance(raw_cases, list):
        raise ValueError("calendar cases document must include list field `cases`")

    parsed_cases: list[dict[str, Any]] = []
    for raw_case in raw_cases:
        if not isinstance(raw_case, dict):
            raise ValueError("calendar cases document case must be an object")

        parsed_cases.append(cast(dict[str, Any], raw_case))

    return parsed_cases


def _extract_seasonality_cases(
    seasonality_cases_document: dict[str, Any],
) -> list[dict[str, Any]]:
    raw_cases: object = seasonality_cases_document.get("canonical_output_cases")
    if not isinstance(raw_cases, list):
        raise ValueError("seasonality cases document must include list field `canonical_output_cases`")

    parsed_cases: list[dict[str, Any]] = []
    for raw_case in raw_cases:
        if not isinstance(raw_case, dict):
            raise ValueError("seasonality cases document case must be an object")

        parsed_cases.append(cast(dict[str, Any], raw_case))

    return parsed_cases


def main() -> None:
    from ._parser import _parse_cli_arguments

    try:
        parsed_arguments = _parse_cli_arguments()
    except JaniformError as e:
        raise SystemExit(_write_cli_error(e)) from e

    handler = parsed_arguments.handler
    if handler is not None:
        raise SystemExit(handler(parsed_arguments))

    parsed_arguments.help_target.print_help()

__all__ = [
    '_attach_runtime_hourly_factor_contract',
    '_build_calendar_registry',
    '_build_calendar_registry_from_document',
    '_build_hourly_factor_contract_payload',
    '_build_hourly_news_intensity_materialization_payload',
    '_build_hourly_seasonality_materialization_payload',
    '_build_day_of_week_seasonality_settings_from_arguments',
    '_build_day_of_year_seasonality_settings_from_arguments',
    '_build_intraday_seasonality_settings_from_arguments',
    '_build_market_data_calibration_request_from_arguments',
    '_build_market_data_calibration_summary_payload',
    '_build_timestamp_event_preprocessing_payload',
    '_classify_market_data_command_error',
    '_classify_open_closed_gap_command_error',
    '_classify_seasonality_diagnostics_command_error',
    '_classify_seasonality_preprocess_command_error',
    '_classify_workflow_command_error',
    '_exit_code_for_error',
    '_extract_calendar_cases',
    '_extract_calendar_registry_payload',
    '_extract_datetime_sequence_field',
    '_extract_event_records',
    '_extract_explicit_hourly_factor_source_document',
    '_extract_gap_behavior_cases',
    '_extract_hourly_news_intensity_materialization_input',
    '_extract_hourly_seasonality_materialization_input',
    '_extract_int_field',
    '_extract_materialized_hourly_news_intensity_source_document',
    '_extract_materialized_hourly_seasonality_source_document',
    '_extract_required_float_field',
    '_extract_seasonality_cases',
    '_extract_string_field',
    '_extract_float_sequence_field',
    '_load_json_document',
    '_optional_enum_value',
    '_override_calibration_execution_mode',
    '_parse_utc_datetime_cli_argument',
    '_reject_orphan_market_data_arguments',
    '_render_calibration_staged_human_output',
    '_render_hourly_factor_build_contract_human_output',
    '_render_hourly_factor_materialize_news_intensity_human_output',
    '_render_hourly_factor_materialize_seasonality_human_output',
    '_render_market_data_calibration_human_output',
    '_render_pricing_vanilla_option_human_output',
    '_render_simulation_coarse_interval_human_output',
    '_render_seasonality_diagnostics_human_output',
    '_render_seasonality_preprocess_timestamps_human_output',
    '_render_simulation_seeded_ticks_human_output',
    '_render_stylized_facts_horizon_comparison_human_output',
    '_require_non_empty_cli_string',
    '_require_path_argument',
    '_require_positive_float_cli_argument',
    '_require_positive_integer_cli_argument',
    '_resolve_path_from_repository_root',
    '_run_calibration_market_data_command',
    '_run_calibration_staged_command',
    '_run_deterministic_regression_command',
    '_run_hourly_factor_build_contract_command',
    '_run_hourly_factor_materialize_news_intensity_command',
    '_run_hourly_factor_materialize_seasonality_command',
    '_run_open_closed_gap_leakage_regression_command',
    '_run_pricing_vanilla_option_command',
    '_run_seasonality_diagnostics_command',
    '_run_seasonality_preprocess_timestamps_command',
    '_run_simulation_coarse_interval_command',
    '_run_simulation_seeded_ticks_command',
    '_run_stylized_facts_horizon_comparison_command',
    '_validate_hourly_materialization_source_document',
    '_write_cli_error',
    '_write_command_output',
    '_write_contract_json',
    '_write_deterministic_regression_human_output',
    '_write_open_closed_gap_human_output',
    'main',
]
