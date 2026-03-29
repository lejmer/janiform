import runpy
import sys

import pytest

import janiform.__main__ as janiform_main_module
from tests.cli_test_support import normalize_help_output, render_help


def test_cli_entrypoints_share_the_same_top_level_help_surface(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    direct_help_output = render_help([])

    monkeypatch.setattr(sys, "argv", ["janiform", "--help"])
    with pytest.raises(SystemExit) as captured_cli_exit:
        janiform_main_module.main()

    assert captured_cli_exit.value.code == 0
    cli_output = capsys.readouterr().out

    sys.modules.pop("janiform.__main__", None)
    monkeypatch.setattr(sys, "argv", ["janiform", "--help"])
    with pytest.raises(SystemExit) as captured_module_exit:
        runpy.run_module("janiform", run_name="__main__")

    assert captured_module_exit.value.code == 0
    module_output = capsys.readouterr().out

    normalized_direct_output = normalize_help_output(direct_help_output)
    assert normalize_help_output(cli_output) == normalized_direct_output
    assert normalize_help_output(module_output) == normalized_direct_output
