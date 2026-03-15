import json
from typing import Any, Optional

from ._constants import (
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


def _cli_validation_error(
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


def _classify_generic_cli_command_error(
    error: Exception,
    *,
    entrypoint: str,
    validation_error_id: str,
    internal_error_id: str,
    file_field_path: str,
    document_field_path: str,
) -> JaniformError:
    return _classify_workflow_command_error(
        error,
        entrypoint=entrypoint,
        validation_error_id=validation_error_id,
        internal_error_id=internal_error_id,
        file_field_path=file_field_path,
        document_field_path=document_field_path,
    )


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

