from collections.abc import Mapping
from importlib import import_module
from types import ModuleType


def native_module() -> ModuleType:
    return import_module("janiform._native")


def native_exception_matches(
    native: ModuleType,
    error: Exception,
    exception_name: str,
) -> bool:
    native_exception_type = getattr(native, exception_name, None)
    return (
        isinstance(native_exception_type, type)
        and issubclass(native_exception_type, BaseException)
        and isinstance(error, native_exception_type)
    )


def remap_native_exception_by_type(
    native: ModuleType,
    error: Exception,
    mappings: Mapping[str, type[Exception]],
) -> Exception | None:
    for exception_name, error_type in mappings.items():
        if native_exception_matches(native, error, exception_name):
            return error_type(str(error))

    return None
