import json
from collections.abc import Mapping
from dataclasses import dataclass
from enum import StrEnum
from typing import Optional


type ErrorContextValue = Optional[str | int | float | bool]


class JaniformErrorCategory(StrEnum):
    VALIDATION = 'validation'
    EXECUTION = 'execution'
    INTEGRATION = 'integration'


@dataclass(frozen=True, slots=True)
class ContractValidationFailure:
    error_id: str
    workflow: str
    entrypoint: str
    field_path: str
    message: str
    context_items: tuple[tuple[str, ErrorContextValue], ...] = ()

    @staticmethod
    def from_context(
        *,
        error_id: str,
        workflow: str,
        entrypoint: str,
        field_path: str,
        message: str,
        context: Optional[Mapping[str, ErrorContextValue]] = None,
    ) -> ContractValidationFailure:
        return ContractValidationFailure(
            error_id=error_id,
            workflow=workflow,
            entrypoint=entrypoint,
            field_path=field_path,
            message=message,
            context_items=_sorted_context_items(context),
        )

    @property
    def context(self) -> dict[str, ErrorContextValue]:
        return dict(self.context_items)

    def to_error_details(self) -> JaniformErrorDetails:
        return JaniformErrorDetails(
            error_id=self.error_id,
            category=JaniformErrorCategory.VALIDATION,
            workflow=self.workflow,
            entrypoint=self.entrypoint,
            message=self.message,
            field_path=self.field_path,
            context_items=self.context_items,
        )


@dataclass(frozen=True, slots=True)
class JaniformErrorDetails:
    error_id: str
    category: JaniformErrorCategory
    workflow: str
    entrypoint: str
    message: str
    field_path: Optional[str] = None
    context_items: tuple[tuple[str, ErrorContextValue], ...] = ()

    @staticmethod
    def from_context(
        *,
        error_id: str,
        category: JaniformErrorCategory,
        workflow: str,
        entrypoint: str,
        message: str,
        field_path: Optional[str] = None,
        context: Optional[Mapping[str, ErrorContextValue]] = None,
    ) -> JaniformErrorDetails:
        return JaniformErrorDetails(
            error_id=error_id,
            category=category,
            workflow=workflow,
            entrypoint=entrypoint,
            message=message,
            field_path=field_path,
            context_items=_sorted_context_items(context),
        )

    @property
    def context(self) -> dict[str, ErrorContextValue]:
        return dict(self.context_items)


class JaniformError(Exception):
    def __init__(self, details: JaniformErrorDetails) -> None:
        self.details: JaniformErrorDetails = details
        super().__init__(format_janiform_error(details))

    @property
    def error_id(self) -> str:
        return self.details.error_id

    @property
    def category(self) -> JaniformErrorCategory:
        return self.details.category

    @property
    def workflow(self) -> str:
        return self.details.workflow

    @property
    def entrypoint(self) -> str:
        return self.details.entrypoint

    @property
    def field_path(self) -> Optional[str]:
        return self.details.field_path

    @property
    def context(self) -> dict[str, ErrorContextValue]:
        return self.details.context


class JaniformValidationError(JaniformError):
    def __init__(self, failure: ContractValidationFailure) -> None:
        self.failure: ContractValidationFailure = failure
        super().__init__(failure.to_error_details())


class JaniformExecutionError(JaniformError):
    pass


class JaniformIntegrationError(JaniformError):
    pass


def format_janiform_error(details: JaniformErrorDetails) -> str:
    formatted_parts: list[str] = [
        f"[{details.error_id}]",
        f"category={_format_error_string(details.category.value)}",
        f"workflow={_format_error_string(details.workflow)}",
        f"entrypoint={_format_error_string(details.entrypoint)}",
    ]

    if details.field_path is not None:
        formatted_parts.append(f"field_path={_format_error_string(details.field_path)}")

    if details.context_items:
        formatted_parts.append(
            "context="
            + json.dumps(
                {field_name: field_value for field_name, field_value in details.context_items},
                sort_keys=True,
            )
        )

    formatted_parts.append(f"message={_format_error_string(details.message)}")
    return " ".join(formatted_parts)


def _format_error_string(value: str) -> str:
    escaped_value = (
        value.replace('"', '\\"')
        .replace("\n", "\\n")
        .replace("\r", "\\r")
        .replace("\t", "\\t")
    )

    return f'"{escaped_value}"'


def _sorted_context_items(
    context: Optional[Mapping[str, ErrorContextValue]],
) -> tuple[tuple[str, ErrorContextValue], ...]:
    if context is None:
        return ()

    return tuple(sorted(context.items(), key=lambda item: item[0]))


__all__ = [
    "ContractValidationFailure",
    "ErrorContextValue",
    "JaniformError",
    "JaniformErrorCategory",
    "JaniformErrorDetails",
    "JaniformExecutionError",
    "JaniformIntegrationError",
    "JaniformValidationError",
    "format_janiform_error",
]
