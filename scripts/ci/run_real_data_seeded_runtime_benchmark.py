import argparse
from pathlib import Path
import sys


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
if str(REPOSITORY_ROOT) not in sys.path:
    sys.path.insert(0, str(REPOSITORY_ROOT))

from benches.python.support import (
    compare_real_data_seeded_runtime_benchmark_report_to_baseline,
    load_real_data_seeded_runtime_baseline_from_json,
    load_real_data_seeded_runtime_benchmark_fixture_document,
    load_real_data_seeded_runtime_benchmark_policy_from_json,
    render_real_data_seeded_runtime_comparison_summary_markdown,
    run_real_data_seeded_runtime_benchmark_fixture,
    serialize_real_data_seeded_runtime_comparison_report_to_json,
    serialize_real_data_seeded_runtime_benchmark_report_to_json,
)


def _build_argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Run the real-data seeded runtime benchmark fixture, compare it to the accepted "
            "baseline, and write machine-readable and human-readable artifacts."
        )
    )
    parser.add_argument(
        "--fixture-path",
        required=True,
        help="Path to the real-data seeded runtime benchmark fixture document.",
    )
    parser.add_argument(
        "--policy-path",
        required=True,
        help="Path to the real-data seeded runtime threshold policy.",
    )
    parser.add_argument(
        "--baseline-path",
        required=True,
        help="Path to the accepted real-data seeded runtime baseline artifact.",
    )
    parser.add_argument(
        "--output-dir",
        required=True,
        help="Directory where benchmark artifacts should be written.",
    )
    parser.add_argument(
        "--allow-regression",
        action="store_true",
        help="Write artifacts even when thresholds fail and exit successfully.",
    )
    return parser


def main() -> int:
    arguments = _build_argument_parser().parse_args()

    fixture_path = Path(arguments.fixture_path)
    policy_path = Path(arguments.policy_path)
    baseline_path = Path(arguments.baseline_path)
    output_dir = Path(arguments.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    fixture_document = load_real_data_seeded_runtime_benchmark_fixture_document(
        fixture_path.read_bytes()
    )
    policy = load_real_data_seeded_runtime_benchmark_policy_from_json(
        policy_path.read_text(encoding="utf-8")
    )
    baseline = load_real_data_seeded_runtime_baseline_from_json(
        baseline_path.read_text(encoding="utf-8")
    )

    reports = tuple(
        run_real_data_seeded_runtime_benchmark_fixture(fixture)
        for fixture in fixture_document.fixtures
    )
    comparison_report = compare_real_data_seeded_runtime_benchmark_report_to_baseline(
        candidate_reports=reports,
        baseline=baseline,
        policy=policy,
    )
    summary_markdown = render_real_data_seeded_runtime_comparison_summary_markdown(
        comparison_report
    )

    (output_dir / "real_data_seeded_runtime_benchmark_report.json").write_text(
        serialize_real_data_seeded_runtime_benchmark_report_to_json(reports[0])
        if len(reports) == 1
        else "[\n"
        + ",\n".join(
            serialize_real_data_seeded_runtime_benchmark_report_to_json(report)
            for report in reports
        )
        + "\n]\n",
        encoding="utf-8",
    )
    (output_dir / "real_data_seeded_runtime_benchmark_comparison.json").write_text(
        serialize_real_data_seeded_runtime_comparison_report_to_json(comparison_report),
        encoding="utf-8",
    )
    (output_dir / "real_data_seeded_runtime_benchmark_summary.md").write_text(
        summary_markdown,
        encoding="utf-8",
    )

    if arguments.allow_regression:
        return 0

    has_failure = any(
        fixture_comparison.status.value == "fail"
        for fixture_comparison in comparison_report.fixture_comparisons
    )
    return 1 if has_failure else 0


if __name__ == "__main__":
    raise SystemExit(main())
