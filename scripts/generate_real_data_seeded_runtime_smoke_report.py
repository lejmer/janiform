import argparse
from pathlib import Path
import sys


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
if str(REPOSITORY_ROOT) not in sys.path:
    sys.path.insert(0, str(REPOSITORY_ROOT))

from benches.python.support import (
    load_real_data_seeded_runtime_benchmark_fixture_document,
    run_real_data_seeded_runtime_benchmark_fixture,
    serialize_real_data_seeded_runtime_benchmark_report_to_json,
)


def _build_argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Generate the checked-in smoke benchmark report artifact for the real-data seeded "
            "runtime test suite."
        )
    )
    parser.add_argument(
        "--fixture-path",
        default="tests/fixtures/benchmarks/real_data_seeded_runtime_smoke_fixture.json",
        help="Path to the smoke benchmark fixture document.",
    )
    parser.add_argument(
        "--output-path",
        default="tests/fixtures/benchmarks/real_data_seeded_runtime_smoke_report.json",
        help="Path where the generated smoke benchmark report should be written.",
    )

    return parser


def main() -> int:
    arguments = _build_argument_parser().parse_args()
    fixture_path = REPOSITORY_ROOT / arguments.fixture_path
    output_path = REPOSITORY_ROOT / arguments.output_path

    fixture_document = load_real_data_seeded_runtime_benchmark_fixture_document(
        fixture_path.read_bytes()
    )
    if len(fixture_document.fixtures) != 1:
        raise ValueError("smoke report generation expects exactly one fixture")

    report = run_real_data_seeded_runtime_benchmark_fixture(fixture_document.fixtures[0])
    output_path.write_text(
        serialize_real_data_seeded_runtime_benchmark_report_to_json(report),
        encoding="utf-8",
    )

    print(output_path.relative_to(REPOSITORY_ROOT))

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
