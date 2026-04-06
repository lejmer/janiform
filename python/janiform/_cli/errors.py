"""Internal CLI error normalization helpers."""

import json
from typing import Any, Optional

from .constants import (
    CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT,
    CLI_CALIBRATION_MARKET_DATA_INTERNAL_ERROR_ID,
    CLI_CALIBRATION_MARKET_DATA_VALIDATION_ERROR_ID,
    CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
    CLI_OPEN_CLOSED_GAP_INTERNAL_ERROR_ID,
    CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
    CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
    CLI_SEASONALITY_DIAGNOSTICS_INTERNAL_ERROR_ID,
    CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
    CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_ENTRYPOINT,
    CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_INTERNAL_ERROR_ID,
    CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_VALIDATION_ERROR_ID,
)
from ..calendar import (
    CalendarInvalidTimestampError,
    CalendarInvalidTimezoneError,
    CalendarMissingExchangeError,
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
from ..seasonality import SeasonalityInputValidationError


def cli_validation_error(
    field_path: str,
    message: str,
    *,
    error_id: str,
    entrypoint: str,
    context: Optional[dict[str, Any]] = None,
) -> JaniformValidationError:
    return JaniformValidationError(
        ContractValidationFailure.from_context(
            error_id=error_id,
            workflow="cli",
            entrypoint=entrypoint,
            field_path=field_path,
            message=message,
            context=context,
        )
    )


def classify_workflow_command_error(
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
        return cli_validation_error(
            file_field_path,
            "input file could not be found",
            error_id=validation_error_id,
            entrypoint=entrypoint,
            context={"path": error.filename},
        )

    if isinstance(error, OSError):
        return cli_validation_error(
            file_field_path,
            f"input file could not be read: {error}",
            error_id=validation_error_id,
            entrypoint=entrypoint,
            context={"path": error.filename},
        )

    if isinstance(error, json.JSONDecodeError):
        return cli_validation_error(
            document_field_path,
            "input file must contain valid JSON",
            error_id=validation_error_id,
            entrypoint=entrypoint,
            context={
                "column_number": error.colno,
                "line_number": error.lineno,
            },
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
        return cli_validation_error(
            document_field_path,
            str(error),
            error_id=validation_error_id,
            entrypoint=entrypoint,
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


def classify_generic_cli_command_error(
    error: Exception,
    *,
    entrypoint: str,
    validation_error_id: str,
    internal_error_id: str,
    file_field_path: str,
    document_field_path: str,
) -> JaniformError:
    return classify_workflow_command_error(
        error,
        entrypoint=entrypoint,
        validation_error_id=validation_error_id,
        internal_error_id=internal_error_id,
        file_field_path=file_field_path,
        document_field_path=document_field_path,
    )


def classify_open_closed_gap_command_error(error: Exception) -> JaniformError:
    if isinstance(error, JaniformError):
        return error

    if isinstance(error, FileNotFoundError):
        return cli_validation_error(
            "fixture_path",
            "fixture file could not be found",
            error_id=CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
            entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
            context={"path": error.filename},
        )

    if isinstance(error, json.JSONDecodeError):
        return cli_validation_error(
            "fixture_document",
            "fixture file must contain valid JSON",
            error_id=CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
            entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
            context={
                "column_number": error.colno,
                "line_number": error.lineno,
            },
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
        return cli_validation_error(
            "command_input",
            str(error),
            error_id=CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
            entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
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


def classify_seasonality_preprocess_command_error(
    error: Exception,
    *,
    file_field_path: str,
    document_field_path: str,
) -> JaniformError:
    if isinstance(error, JaniformError):
        return error

    if isinstance(error, CalendarMissingExchangeError):
        return cli_validation_error(
            "exchange_code",
            str(error),
            error_id=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_ENTRYPOINT,
        )

    if isinstance(error, CalendarInvalidTimezoneError):
        return cli_validation_error(
            "calendar_registry_document",
            str(error),
            error_id=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_ENTRYPOINT,
        )

    if isinstance(error, SeasonalityInputValidationError):
        return cli_validation_error(
            "event_records_document",
            str(error),
            error_id=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_ENTRYPOINT,
        )

    return classify_workflow_command_error(
        error,
        entrypoint=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_ENTRYPOINT,
        validation_error_id=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_VALIDATION_ERROR_ID,
        internal_error_id=CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_INTERNAL_ERROR_ID,
        file_field_path=file_field_path,
        document_field_path=document_field_path,
    )


def classify_seasonality_diagnostics_command_error(
    error: Exception,
    *,
    file_field_path: str,
    document_field_path: str,
) -> JaniformError:
    if isinstance(error, JaniformError):
        return error

    if isinstance(error, CalendarMissingExchangeError):
        return cli_validation_error(
            "exchange_code",
            str(error),
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        )

    if isinstance(error, CalendarInvalidTimezoneError):
        return cli_validation_error(
            "calendar_registry_document",
            str(error),
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        )

    if isinstance(error, SeasonalityInputValidationError):
        return cli_validation_error(
            document_field_path,
            str(error),
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        )

    return classify_workflow_command_error(
        error,
        entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        validation_error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
        internal_error_id=CLI_SEASONALITY_DIAGNOSTICS_INTERNAL_ERROR_ID,
        file_field_path=file_field_path,
        document_field_path=document_field_path,
    )


def classify_market_data_command_error(error: Exception) -> JaniformError:
    if isinstance(error, JaniformError):
        return error

    if isinstance(error, ValueError):
        return cli_validation_error(
            "command_input",
            str(error),
            error_id=CLI_CALIBRATION_MARKET_DATA_VALIDATION_ERROR_ID,
            entrypoint=CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT,
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
