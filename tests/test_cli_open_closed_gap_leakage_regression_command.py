import os
import subprocess
import sys
from pathlib import Path


def test_cli_open_closed_gap_leakage_regression_command_smoke() -> None:
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
            "regression",
            "open-closed-gap-leakage",
        ],
        cwd=repository_root,
        check=False,
        capture_output=True,
        text=True,
        env=environment,
    )

    assert command_result.returncode == 0, command_result.stderr
    assert "open closed gap leakage regression passed" in command_result.stdout
