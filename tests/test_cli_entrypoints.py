import runpy
import sys

import pytest

import janiform.__main__ as janiform_main_module
import janiform._native_api as native_api


def test_cli_entrypoints_delegate_to_the_same_native_cli_launcher(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured_arguments: list[tuple[str, ...]] = []

    def fake_run_cli() -> int:
        captured_arguments.append(tuple(sys.argv))
        return 0

    monkeypatch.setattr(janiform_main_module, "run_cli", fake_run_cli)
    monkeypatch.setattr(sys, "argv", ["janiform", "--help"])
    with pytest.raises(SystemExit) as captured_cli_exit:
        janiform_main_module.main()

    assert captured_cli_exit.value.code == 0

    sys.modules.pop("janiform.__main__", None)
    monkeypatch.setattr(native_api, "run_cli", fake_run_cli)
    monkeypatch.setattr(sys, "argv", ["janiform", "--help"])
    with pytest.raises(SystemExit) as captured_module_exit:
        runpy.run_module("janiform", run_name="__main__")

    assert captured_module_exit.value.code == 0
    assert captured_arguments == [
        ("janiform", "--help"),
        ("janiform", "--help"),
    ]
