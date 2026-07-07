import argparse
import os
import subprocess
import sys
from pathlib import Path
from typing import NoReturn


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]


def _default_venv_path() -> Path:
    if os.name == "nt":
        return REPOSITORY_ROOT / ".venv"

    return REPOSITORY_ROOT / ".venv-linux"


def _python_executable_for_venv(venv_path: Path) -> Path:
    if os.name == "nt":
        return venv_path / "Scripts" / "python.exe"

    return venv_path / "bin" / "python"


def _build_argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Run the local-only Python benchmark workloads and write their artifacts "
            "under a chosen output root."
        )
    )
    parser.add_argument(
        "--venv-path",
        default=str(_default_venv_path()),
        help="Path to the Python virtual environment to use.",
    )
    parser.add_argument(
        "--output-root",
        default=str(REPOSITORY_ROOT / ".artifacts" / "local-python-benchmarks"),
        help="Directory where workload artifacts should be written.",
    )
    parser.add_argument(
        "--skip-real-data-seeded-runtime",
        action="store_true",
        help="Skip the representative real-data seeded runtime benchmark workload.",
    )
    parser.add_argument(
        "--skip-microbenchmark-smoke",
        action="store_true",
        help="Skip the Python microbenchmark smoke workload.",
    )
    return parser


def _pythonpath() -> str:
    return os.pathsep.join(
        [
            str(REPOSITORY_ROOT / "python"),
            str(REPOSITORY_ROOT),
        ]
    )


def _base_environment() -> dict[str, str]:
    environment = dict(os.environ)
    environment["PYTHONPATH"] = _pythonpath()

    if os.name != "nt" and Path("/tmp").is_dir():
        environment.setdefault("TMPDIR", "/tmp")
        environment.setdefault("UV_CACHE_DIR", "/tmp/uv")

    return environment


def _exit_with_error(message: str) -> NoReturn:
    raise SystemExit(message)


def _run_and_tee(
    *,
    command: list[str],
    log_path: Path,
    environment: dict[str, str],
) -> None:
    log_path.parent.mkdir(parents=True, exist_ok=True)

    process = subprocess.Popen(
        command,
        cwd=REPOSITORY_ROOT,
        env=environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
    )

    assert process.stdout is not None
    with log_path.open("w", encoding="utf-8") as log_file:
        for line in process.stdout:
            sys.stdout.write(line)
            log_file.write(line)

    return_code = process.wait()
    if return_code != 0:
        _exit_with_error(
            f"command failed with exit code {return_code}: {' '.join(command)}"
        )


def _run_real_data_seeded_runtime_benchmark(
    *,
    python_executable: Path,
    output_root: Path,
    environment: dict[str, str],
) -> None:
    workload_output_root = output_root / "real-data-seeded-runtime-benchmark"
    workload_output_root.mkdir(parents=True, exist_ok=True)

    _run_and_tee(
        command=[
            str(python_executable),
            "scripts/ci/run_real_data_seeded_runtime_benchmark.py",
            "--fixture-path",
            "tests/fixtures/benchmarks/real_data_seeded_runtime_benchmark_fixture.json",
            "--policy-path",
            "tests/fixtures/benchmarks/real_data_seeded_runtime_benchmark_policy.json",
            "--baseline-path",
            "tests/fixtures/benchmarks/real_data_seeded_runtime_baseline_v1.json",
            "--output-dir",
            str(workload_output_root),
        ],
        log_path=workload_output_root / "run.log",
        environment=environment,
    )


def _run_microbenchmark_smoke(
    *,
    python_executable: Path,
    output_root: Path,
    environment: dict[str, str],
) -> None:
    workload_output_root = output_root / "microbenchmark-smoke"
    workload_output_root.mkdir(parents=True, exist_ok=True)

    rust_environment = dict(environment)
    if os.name != "nt" and Path("/tmp").is_dir():
        rust_environment.setdefault("TMPDIR", "/tmp")

    _run_and_tee(
        command=[
            "cargo",
            "bench",
            "-p",
            "janiform-governance-bench",
            "--bench",
            "rust_microbenchmarks",
            "--",
            "--list",
        ],
        log_path=workload_output_root / "rust-bench-list.log",
        environment=rust_environment,
    )
    _run_and_tee(
        command=[
            str(python_executable),
            "-m",
            "pytest",
            "--benchmark-disable",
            "benches/python/test_microbenchmark_tooling.py",
            "benches/python/python_microbenchmarks.py",
        ],
        log_path=workload_output_root / "python-microbench-smoke.log",
        environment=environment,
    )


def main() -> int:
    arguments = _build_argument_parser().parse_args()
    venv_path = Path(arguments.venv_path).resolve()
    python_executable = _python_executable_for_venv(venv_path)
    if not python_executable.is_file():
        _exit_with_error(
            (
                f"python executable not found at {python_executable}. "
                "Bootstrap the local environment first."
            )
        )

    output_root = Path(arguments.output_root).resolve()
    output_root.mkdir(parents=True, exist_ok=True)
    environment = _base_environment()

    if not arguments.skip_real_data_seeded_runtime:
        _run_real_data_seeded_runtime_benchmark(
            python_executable=python_executable,
            output_root=output_root,
            environment=environment,
        )

    if not arguments.skip_microbenchmark_smoke:
        _run_microbenchmark_smoke(
            python_executable=python_executable,
            output_root=output_root,
            environment=environment,
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
