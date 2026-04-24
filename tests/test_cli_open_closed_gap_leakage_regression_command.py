from tests.support.cli import invoke_cli


def test_cli_open_closed_gap_leakage_regression_command_smoke() -> None:
    exit_code, stdout, stderr = invoke_cli(
        [
            "dev",
            "regression",
            "open-closed-gap-leakage",
        ]
    )

    assert exit_code == 0, stderr
    assert "open closed gap leakage regression passed" in stdout
