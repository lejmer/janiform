from importlib import import_module
import sys

import pytest

import janiform
import janiform.calibration as calibration_module
import janiform.pricing as pricing_module
import janiform.simulation as simulation_module
from janiform._api_contract_guardrails import build_public_api_contract_snapshot
from janiform._experimental import EXPERIMENTAL_API_ENV_VAR


def _purge_experimental_modules() -> None:
    module_names = [
        module_name
        for module_name in sys.modules
        if module_name == "janiform.experimental" or module_name.startswith("janiform.experimental.")
    ]
    for module_name in module_names:
        sys.modules.pop(module_name, None)


def test_experimental_namespace_is_not_part_of_primary_public_surface() -> None:
    assert "experimental" not in janiform.__all__
    assert "janiform.experimental" not in build_public_api_contract_snapshot()["public_modules"]
    assert "experimental" not in simulation_module.__all__
    assert "experimental" not in pricing_module.__all__
    assert "experimental" not in calibration_module.__all__


def test_preview_endpoint_namespace_requires_explicit_opt_in(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv(EXPERIMENTAL_API_ENV_VAR, raising=False)
    _purge_experimental_modules()

    with pytest.raises(ImportError, match="JANIFORM_EXPERIMENTAL=1"):
        import_module("janiform.experimental.api")


def test_preview_endpoint_namespace_imports_when_flag_is_enabled(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv(EXPERIMENTAL_API_ENV_VAR, "enabled")
    _purge_experimental_modules()

    experimental_module = import_module("janiform.experimental")
    preview_module = import_module("janiform.experimental.api")

    assert experimental_module.experimental_api_enabled() is True
    assert preview_module.__all__ == []


def test_primary_contract_snapshot_is_unchanged_when_preview_flag_toggles(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.delenv(EXPERIMENTAL_API_ENV_VAR, raising=False)
    _purge_experimental_modules()
    disabled_snapshot = build_public_api_contract_snapshot()

    monkeypatch.setenv(EXPERIMENTAL_API_ENV_VAR, "1")
    _purge_experimental_modules()
    enabled_snapshot = build_public_api_contract_snapshot()

    assert disabled_snapshot == enabled_snapshot
