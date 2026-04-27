from collections.abc import Mapping
from importlib import import_module
from types import ModuleType
from typing import Optional


def native_module() -> ModuleType:
    return import_module("janiform._native")


def native_exception_matches(
    native: ModuleType,
    error: Exception,
    exception_name: str,
) -> bool:
    """
    Match native exceptions without binding `except native.SomeError` clauses.

    Public Python wrappers use this helper because direct `except native.SomeError`
    blocks require the extension module to expose every expected exception type at
    handler setup time. If the native module is partially loaded or contract-drifted,
    that lookup can raise `AttributeError` and mask the original runtime failure.
    """
    native_exception_type = getattr(native, exception_name, None)
    return (
        isinstance(native_exception_type, type)
        and issubclass(native_exception_type, BaseException)
        and isinstance(error, native_exception_type)
    )


def native_error_parts(raw_message: str, *, default_field_path: str) -> tuple[str, str]:
    separator_indexes = _native_error_separator_indexes(raw_message)
    if len(separator_indexes) == 1:
        separator_index = separator_indexes[0]
        field_path = raw_message[:separator_index]
        message = raw_message[separator_index + 2 :]

        if field_path.strip() and message.strip():
            return field_path, message

    for separator_index in reversed(separator_indexes):
        field_path_candidate = raw_message[:separator_index].rsplit(
            ": ",
            maxsplit=1,
        )[-1]
        message = raw_message[separator_index + 2 :]

        if _is_native_error_field_path(field_path_candidate) and message.strip():
            return field_path_candidate, message

    return default_field_path, raw_message


def _native_error_separator_indexes(raw_message: str) -> tuple[int, ...]:
    indexes: list[int] = []
    search_start = 0

    while True:
        separator_index = raw_message.find(": ", search_start)
        if separator_index < 0:
            return tuple(indexes)

        indexes.append(separator_index)
        search_start = separator_index + 2


def _is_native_error_field_path(value: str) -> bool:
    return value.strip() == value and bool(value) and all(
        character.isalnum() or character in "._[]"
        for character in value
    )


def remap_native_exception_by_type(
    native: ModuleType,
    error: Exception,
    mappings: Mapping[str, type[Exception]],
) -> Optional[Exception]:
    for exception_name, error_type in mappings.items():
        if native_exception_matches(native, error, exception_name):
            return error_type(str(error))

    return None
