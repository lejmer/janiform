import argparse
import os
import subprocess
import sys
import sysconfig
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Install and smoke test a Janiform wheel in the active Python environment.",
    )

    parser.add_argument("--wheel", type=Path)
    parser.add_argument("--expected-version")

    args = parser.parse_args()

    if args.wheel is not None:
        _run([sys.executable, "-m", "pip", "install", "--force-reinstall", str(args.wheel)])

    observed_version = _run_python("import janiform; print(janiform.__version__)").strip()
    if args.expected_version is not None and observed_version != args.expected_version:
        print(
            f"installed janiform.__version__ is {observed_version}, expected {args.expected_version}",
            file=sys.stderr,
        )
        return 1

    _run_python("import janiform; import janiform._native")
    _run([sys.executable, "-m", "janiform", "--help"])
    _run([str(_console_script_path()), "--help"])

    print(f"installed wheel smoke passed for janiform {observed_version}")
    return 0


def _run_python(command: str) -> str:
    result = subprocess.run(
        [
            sys.executable,
            "-c",
            command,
        ],
        check=True,
        capture_output=True,
        text=True,
    )

    return result.stdout


def _run(command: list[str]) -> None:
    subprocess.run(command, check=True)


def _console_script_path() -> Path:
    scripts_directory = Path(sysconfig.get_path("scripts"))
    executable_name = "janiform.exe" if os.name == "nt" else "janiform"

    executable_path = scripts_directory / executable_name
    if not executable_path.exists():
        raise SystemExit(f"console script not found at {executable_path}")

    return executable_path


if __name__ == "__main__":
    raise SystemExit(main())
