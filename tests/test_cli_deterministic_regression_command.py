import os
import subprocess
import sys
from pathlib import Path


def test_cli_deterministic_regression_command_smoke() -> None:
    repository_root = Path(__file__).resolve().parents[1]
    python_source_root = repository_root / "python"

    environment = dict(os.environ)
    existing_python_path = environment.get("PYTHONPATH", "")
    environment["PYTHONPATH"] = (
        f"{python_source_root}{os.pathsep}{existing_python_path}"
        if existing_python_path
        else str(python_source_root)
    )

    command_result = subprocess.run(
        [
            sys.executable,
            "-m",
            "janiform",
            "dev",
            "regression",
            "deterministic",
            "--calendar-registry-json",
            "tests/fixtures/calendar/calendar_service_regression_matrix.json",
            "--calendar-cases-json",
            "tests/fixtures/calendar/calendar_service_regression_matrix.json",
            "--seasonality-cases-json",
            "tests/fixtures/hourly_factors/hourly_factor_regression_matrix.json",
        ],
        cwd=repository_root,
        check=False,
        capture_output=True,
        text=True,
        env=environment,
    )

    assert command_result.returncode == 0, command_result.stderr
    assert "deterministic regression passed" in command_result.stdout
