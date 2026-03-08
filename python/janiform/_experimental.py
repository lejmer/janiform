import os
from typing import Final


EXPERIMENTAL_API_ENV_VAR: Final[str] = "JANIFORM_EXPERIMENTAL"
EXPERIMENTAL_API_ENABLEMENT_POLICY: Final[str] = "python.experimental_api_enablement.v1"
EXPERIMENTAL_API_LIFECYCLE_POLICY: Final[str] = "python.experimental_api_lifecycle.v1"
_EXPERIMENTAL_API_ENABLED_VALUES: Final[frozenset[str]] = frozenset(
    {"1", "true", "t", "yes", "y", "on", "enable", "enabled"}
)


class ExperimentalApiDisabledError(ImportError):
    """Raised when an experimental namespace is imported without explicit opt-in."""


def experimental_api_enabled() -> bool:
    raw_value = os.environ.get(EXPERIMENTAL_API_ENV_VAR)
    if raw_value is None:
        return False

    return raw_value.strip().lower() in _EXPERIMENTAL_API_ENABLED_VALUES


def require_experimental_api_enabled(module_name: str) -> None:
    if experimental_api_enabled():
        return

    raise ExperimentalApiDisabledError(
        f"{module_name} is experimental and requires explicit opt-in via "
        f"{EXPERIMENTAL_API_ENV_VAR}=1"
    )
