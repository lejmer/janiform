from pathlib import Path
import subprocess
import sys


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]


def test_generated_crate_readmes_are_current() -> None:
    result = subprocess.run(
        [sys.executable, "scripts/generate_crate_readmes.py", "--check"],
        capture_output=True,
        cwd=REPOSITORY_ROOT,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stdout + result.stderr
