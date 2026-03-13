import runpy
import sys

import pytest

import janiform.cli.dispatch as cli_dispatch_module
from tests.cli_test_support import normalize_help_output, render_help


def test_cli_entrypoints_share_the_same_top_level_help_surface(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    direct_help_output = render_help([])

    monkeypatch.setattr(sys, "argv", ["janiform", "--help"])
    with pytest.raises(SystemExit) as captured_dispatch_exit:
        cli_dispatch_module.main()

    assert captured_dispatch_exit.value.code == 0
    dispatch_output = capsys.readouterr().out

    monkeypatch.setattr(sys, "argv", ["janiform", "--help"])
    with pytest.raises(SystemExit) as captured_module_exit:
        runpy.run_module("janiform", run_name="__main__")

    assert captured_module_exit.value.code == 0
    module_output = capsys.readouterr().out

    normalized_direct_output = normalize_help_output(direct_help_output)
    assert normalize_help_output(dispatch_output) == normalized_direct_output
    assert normalize_help_output(module_output) == normalized_direct_output
