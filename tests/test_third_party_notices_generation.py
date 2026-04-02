import subprocess
import sys
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]


def test_generated_third_party_notices_are_current() -> None:
    result = subprocess.run(
        [sys.executable, "scripts/generate_third_party_notices.py", "--check"],
        capture_output=True,
        cwd=REPOSITORY_ROOT,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stdout + result.stderr
