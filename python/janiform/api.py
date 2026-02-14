from importlib import import_module
from types import ModuleType


def native_module() -> ModuleType:
    return import_module("janiform._native")


__all__ = ["native_module"]
