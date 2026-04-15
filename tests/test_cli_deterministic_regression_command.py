from tests.support.cli import invoke_cli


def test_cli_deterministic_regression_command_smoke() -> None:
    exit_code, stdout, stderr = invoke_cli(
        [
            "dev",
            "regression",
            "deterministic",
            "--calendar-registry-json",
            "tests/fixtures/calendar/calendar_service_regression_matrix.json",
            "--calendar-cases-json",
            "tests/fixtures/calendar/calendar_service_regression_matrix.json",
            "--seasonality-cases-json",
            "tests/fixtures/hourly_factors/hourly_factor_regression_matrix.json",
        ]
    )

    assert exit_code == 0, stderr
    assert "deterministic regression passed" in stdout
