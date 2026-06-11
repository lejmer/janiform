"""Regenerate all paper figure artifacts."""

import argparse
import subprocess
import sys
from pathlib import Path
from typing import Final, Optional


REPOSITORY_ROOT: Final[Path] = Path(__file__).resolve().parents[2]
FIGURE_SCRIPT_DIR: Final[Path] = Path(__file__).resolve().parent / "figures"


def parse_args(available_figures: tuple[str, ...]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)

    parser.add_argument(
        "--figure",
        action="append",
        choices=available_figures,
        help="Run only the named figure script. May be supplied multiple times.",
    )

    parser.add_argument(
        "--python-executable",
        default=sys.executable,
        help="Python executable used to run each figure script.",
    )

    parser.add_argument(
        "--continue-on-error",
        action="store_true",
        help="Run remaining scripts after a figure script fails.",
    )

    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print the commands without running them.",
    )

    parser.add_argument(
        "--list",
        action="store_true",
        help="List discovered figure scripts and exit.",
    )

    parser.add_argument(
        "figure_args",
        nargs=argparse.REMAINDER,
        help="Arguments after -- are passed to every figure script.",
    )

    return parser.parse_args()


def main() -> int:
    figure_scripts = discover_figure_scripts()
    available_figures = tuple(script.stem for script in figure_scripts)
    arguments = parse_args(available_figures)

    if arguments.list:
        for figure_name in available_figures:
            print(figure_name)

        return 0

    selected_scripts = select_figure_scripts(
        figure_scripts,
        selected_names=arguments.figure,
    )

    figure_args = normalize_figure_args(arguments.figure_args)

    print(f"Discovered {len(figure_scripts)} figure scripts")
    print(f"Running {len(selected_scripts)} figure scripts")

    failed_scripts: list[Path] = []
    for script_path in selected_scripts:
        return_code = run_figure_script(
            script_path,
            python_executable=arguments.python_executable,
            figure_args=figure_args,
            dry_run=arguments.dry_run,
        )
        if return_code == 0:
            continue

        failed_scripts.append(script_path)
        if not arguments.continue_on_error:
            break

    if failed_scripts:
        print("Figure regeneration failed for:")
        for script_path in failed_scripts:
            print(f"  {script_path.relative_to(REPOSITORY_ROOT)}")

        return 1

    print("Figure regeneration complete")

    return 0


def discover_figure_scripts() -> tuple[Path, ...]:
    scripts = tuple(
        sorted(
            script_path
            for script_path in FIGURE_SCRIPT_DIR.glob("*.py")
            if script_path.name != "__init__.py"
        )
    )
    if len(scripts) == 0:
        raise RuntimeError(f"No figure scripts found in {FIGURE_SCRIPT_DIR}")

    return scripts


def select_figure_scripts(
    figure_scripts: tuple[Path, ...],
    *,
    selected_names: Optional[list[str]],
) -> tuple[Path, ...]:
    if selected_names is None:
        return figure_scripts

    selected_name_set = set(selected_names)

    return tuple(
        script_path for script_path in figure_scripts if script_path.stem in selected_name_set
    )


def normalize_figure_args(raw_figure_args: list[str]) -> tuple[str, ...]:
    if raw_figure_args[:1] == ["--"]:
        return tuple(raw_figure_args[1:])

    return tuple(raw_figure_args)


def run_figure_script(
    script_path: Path,
    *,
    python_executable: str,
    figure_args: tuple[str, ...],
    dry_run: bool,
) -> int:
    command = [
        python_executable,
        str(script_path.relative_to(REPOSITORY_ROOT)),
        *figure_args,
    ]

    print()
    print(f"Running: {' '.join(command)}")

    if dry_run:
        return 0

    completed_process = subprocess.run(command, cwd=REPOSITORY_ROOT, check=False)

    return completed_process.returncode


if __name__ == "__main__":
    raise SystemExit(main())
