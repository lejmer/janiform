import argparse
import json
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Final


FAST_TIER_RESULT_PREFIX: Final[str] = "FAST_TIER_RESULT"
FAST_TIER_PARSED_OUTPUT_SCHEMA_VERSION: Final[str] = "ci.fast_tier.parsed_output.v1"


@dataclass(frozen=True)
class ParsedFastTierLine:
    command_name: str
    status: str
    return_code: int
    duration_seconds: float
    log_path: str


def _build_argument_parser() -> argparse.ArgumentParser:
    parser: argparse.ArgumentParser = argparse.ArgumentParser(
        description="Parse FAST_TIER_RESULT lines into a machine-readable json artifact.",
    )
    parser.add_argument("--input-log", required=True, help="Path to a log file to parse.")
    parser.add_argument(
        "--output-json",
        required=True,
        help="Path where parsed output JSON should be written.",
    )

    return parser


def _parse_fast_tier_result_lines(raw_log_text: str) -> list[ParsedFastTierLine]:
    parsed_lines: list[ParsedFastTierLine] = []
    for raw_line in raw_log_text.splitlines():
        if not raw_line.startswith(f"{FAST_TIER_RESULT_PREFIX}|"):
            continue

        line_parts: list[str] = raw_line.split("|", 5)
        if len(line_parts) != 6:
            raise ValueError(
                "invalid fast tier result line format "
                f"`{raw_line}` expected 6 pipe-delimited parts",
            )

        _, command_name, status, return_code_text, duration_text, log_path = line_parts
        parsed_lines.append(
            ParsedFastTierLine(
                command_name=command_name,
                status=status,
                return_code=int(return_code_text),
                duration_seconds=float(duration_text),
                log_path=log_path,
            ),
        )

    if not parsed_lines:
        raise ValueError("no FAST_TIER_RESULT lines were found in the provided log")

    return parsed_lines


def main() -> int:
    parser: argparse.ArgumentParser = _build_argument_parser()
    parsed_arguments: argparse.Namespace = parser.parse_args()
    input_log_path = Path(parsed_arguments.input_log)
    output_json_path = Path(parsed_arguments.output_json)
    output_json_path.parent.mkdir(parents=True, exist_ok=True)

    input_log_text: str = input_log_path.read_text(encoding="utf-8")
    parsed_results: list[ParsedFastTierLine] = _parse_fast_tier_result_lines(input_log_text)
    output_payload: dict[str, Any] = {
        "schema_version": FAST_TIER_PARSED_OUTPUT_SCHEMA_VERSION,
        "result_count": len(parsed_results),
        "results": [asdict(parsed_result) for parsed_result in parsed_results],
    }
    output_json_path.write_text(
        json.dumps(output_payload, indent=2, sort_keys=True),
        encoding="utf-8",
    )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
