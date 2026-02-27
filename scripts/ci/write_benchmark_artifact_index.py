import argparse
import json
from pathlib import Path
from typing import Any, Final


ARTIFACT_INDEX_SCHEMA_VERSION: Final[str] = "ci.benchmark_tier.artifact_index.v1"


def _build_argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Write a benchmark scenario and baseline index with repository and run links.",
    )
    parser.add_argument("--output-dir", required=True, help="Output directory for index files.")
    parser.add_argument("--repository", required=True, help="GitHub repository owner/name.")
    parser.add_argument("--git-sha", required=True, help="Commit SHA used to build blob links.")
    parser.add_argument("--run-id", required=True, help="GitHub Actions run id.")
    parser.add_argument(
        "--scenario-path",
        required=True,
        help="Repository relative path to scenario artifact.",
    )
    parser.add_argument(
        "--baseline-path",
        required=True,
        help="Repository relative path to baseline artifact.",
    )
    return parser


def _repository_blob_url(repository: str, git_sha: str, relative_path: str) -> str:
    return f"https://github.com/{repository}/blob/{git_sha}/{relative_path}"


def main() -> int:
    parser: argparse.ArgumentParser = _build_argument_parser()
    parsed_arguments: argparse.Namespace = parser.parse_args()

    output_directory = Path(parsed_arguments.output_dir)
    output_directory.mkdir(parents=True, exist_ok=True)
    run_url: str = f"https://github.com/{parsed_arguments.repository}/actions/runs/{parsed_arguments.run_id}"
    scenario_url: str = _repository_blob_url(
        parsed_arguments.repository,
        parsed_arguments.git_sha,
        parsed_arguments.scenario_path,
    )
    baseline_url: str = _repository_blob_url(
        parsed_arguments.repository,
        parsed_arguments.git_sha,
        parsed_arguments.baseline_path,
    )

    payload: dict[str, Any] = {
        "schema_version": ARTIFACT_INDEX_SCHEMA_VERSION,
        "run_url": run_url,
        "entries": [
            {
                "artifact_type": "scenario_catalog",
                "relative_path": parsed_arguments.scenario_path,
                "url": scenario_url,
            },
            {
                "artifact_type": "performance_baseline",
                "relative_path": parsed_arguments.baseline_path,
                "url": baseline_url,
            },
        ],
    }
    (output_directory / "benchmark_artifact_index.json").write_text(
        json.dumps(payload, indent=2, sort_keys=True),
        encoding="utf-8",
    )

    markdown_lines: list[str] = [
        "## Scenario And Baseline Artifacts",
        "",
        f"- Run artifacts: [{run_url}]({run_url})",
        (
            "- Scenario catalog: "
            f"[{parsed_arguments.scenario_path}]({scenario_url})"
        ),
        (
            "- Performance baseline: "
            f"[{parsed_arguments.baseline_path}]({baseline_url})"
        ),
    ]
    (output_directory / "benchmark_artifact_index.md").write_text(
        "\n".join(markdown_lines),
        encoding="utf-8",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
