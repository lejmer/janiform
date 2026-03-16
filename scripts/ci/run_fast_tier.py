import argparse
import json
import subprocess
import time
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Final


FAST_TIER_RESULT_PREFIX: Final[str] = "FAST_TIER_RESULT"
FAST_TIER_SCHEMA_VERSION: Final[str] = "ci.fast_tier.command_results.v1"


@dataclass(frozen=True)
class TierCommand:
    name: str
    command: str


@dataclass(frozen=True)
class TierCommandResult:
    name: str
    command: str
    status: str
    return_code: int
    duration_seconds: float
    log_path: str


def _default_fast_tier_commands() -> list[TierCommand]:
    return [
        TierCommand(
            name="rust_deterministic_regression",
            command="cargo test -p janiform --test deterministic_regression_harness",
        ),
        TierCommand(
            name="rust_tolerance_regression",
            command="cargo test -p janiform --test coarse_fine_consistency_regression_harness",
        ),
        TierCommand(
            name="rust_pricing_benchmark_regression",
            command="cargo test -p janiform --test pricing_benchmark_validation_regression",
        ),
        TierCommand(
            name="ci_workflow_lint_and_smoke",
            command="pytest ci/tests/test_ci_tiered_execution_strategy.py",
        ),
    ]


def _build_argument_parser() -> argparse.ArgumentParser:
    parser: argparse.ArgumentParser = argparse.ArgumentParser(
        description="Run fast deterministic CI tier commands and emit machine-readable output.",
    )
    parser.add_argument(
        "--output-dir",
        required=True,
        help="Directory where command logs and summary artifacts are written.",
    )
    parser.add_argument(
        "--command",
        action="append",
        default=[],
        help="Optional command override in the form '<name>::<command>'.",
    )

    return parser


def _parse_command_overrides(raw_commands: list[str]) -> list[TierCommand]:
    if not raw_commands:
        return _default_fast_tier_commands()

    parsed_commands: list[TierCommand] = []
    for raw_command in raw_commands:
        if "::" not in raw_command:
            raise ValueError(
                "invalid --command value "
                f"`{raw_command}` expected format '<name>::<command>'",
            )

        command_name, command_text = raw_command.split("::", 1)
        trimmed_name: str = command_name.strip()
        trimmed_command: str = command_text.strip()
        if not trimmed_name or not trimmed_command:
            raise ValueError(
                "invalid --command value "
                f"`{raw_command}` expected non-empty name and command",
            )

        parsed_commands.append(TierCommand(name=trimmed_name, command=trimmed_command))

    return parsed_commands


def _sanitize_file_component(raw_name: str) -> str:
    sanitized_parts: list[str] = []
    for character in raw_name.lower():
        if character.isalnum():
            sanitized_parts.append(character)
            continue

        sanitized_parts.append("_")

    return "".join(sanitized_parts).strip("_")


def _write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def _run_command(command: TierCommand, output_directory: Path) -> TierCommandResult:
    safe_name: str = _sanitize_file_component(command.name)
    command_log_path: Path = output_directory / f"{safe_name}.log"
    started_at: float = time.perf_counter()

    completed_process = subprocess.run(
        command.command,
        check=False,
        capture_output=True,
        shell=True,
        text=True,
    )

    duration_seconds: float = time.perf_counter() - started_at
    command_log = (
        f"$ {command.command}\n"
        f"return_code={completed_process.returncode}\n\n"
        f"--- stdout ---\n{completed_process.stdout}\n"
        f"--- stderr ---\n{completed_process.stderr}\n"
    )

    _write_text(command_log_path, command_log)

    status: str = "pass" if completed_process.returncode == 0 else "fail"

    return TierCommandResult(
        name=command.name,
        command=command.command,
        status=status,
        return_code=completed_process.returncode,
        duration_seconds=duration_seconds,
        log_path=command_log_path.as_posix(),
    )


def _write_results_artifacts(
    output_directory: Path,
    command_results: list[TierCommandResult],
) -> None:
    failed_count: int = sum(1 for result in command_results if result.status == "fail")

    report_payload: dict[str, Any] = {
        "schema_version": FAST_TIER_SCHEMA_VERSION,
        "tier_id": "fast_deterministic_regression",
        "generated_at_utc": datetime.now(UTC).isoformat(),
        "summary": {
            "total_command_count": len(command_results),
            "failed_command_count": failed_count,
            "passed_command_count": len(command_results) - failed_count,
        },
        "results": [asdict(result) for result in command_results],
    }

    json_path: Path = output_directory / "fast_tier_results.json"

    _write_text(json_path, json.dumps(report_payload, indent=2, sort_keys=True))

    markdown_lines: list[str] = [
        "## Fast Tier Results",
        "",
        "| Command | Status | Return Code | Duration (s) | Log |",
        "| --- | --- | --- | --- | --- |",
    ]

    for result in command_results:
        markdown_lines.append(
            "| "
            f"`{result.name}` | `{result.status}` | `{result.return_code}` | "
            f"{result.duration_seconds:.3f} | `{result.log_path}` |"
        )

    markdown_lines.extend(
        [
            "",
            f"- Total commands: `{len(command_results)}`",
            f"- Failed commands: `{failed_count}`",
        ],
    )

    _write_text(output_directory / "fast_tier_results.md", "\n".join(markdown_lines))


def main() -> int:
    parser: argparse.ArgumentParser = _build_argument_parser()
    parsed_arguments: argparse.Namespace = parser.parse_args()

    output_directory: Path = Path(parsed_arguments.output_dir)
    output_directory.mkdir(parents=True, exist_ok=True)

    commands: list[TierCommand] = _parse_command_overrides(parsed_arguments.command)
    command_results: list[TierCommandResult] = []
    for command in commands:
        command_result: TierCommandResult = _run_command(command, output_directory)
        command_results.append(command_result)

        print(
            f"{FAST_TIER_RESULT_PREFIX}|{command_result.name}|{command_result.status}|"
            f"{command_result.return_code}|{command_result.duration_seconds:.6f}|"
            f"{command_result.log_path}",
        )

    _write_results_artifacts(output_directory, command_results)

    failed_count: int = sum(1 for result in command_results if result.status == "fail")

    return 1 if failed_count > 0 else 0


if __name__ == "__main__":
    raise SystemExit(main())
