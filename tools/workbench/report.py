import argparse
import json
import os
import tempfile

from pathlib import Path

from .artifacts import build_summary_payload, load_workbench_artifacts


if "MPLCONFIGDIR" not in os.environ:
    matplotlib_cache_directory = Path(tempfile.gettempdir()) / "janiform-mpl"
    matplotlib_cache_directory.mkdir(parents=True, exist_ok=True)
    os.environ["MPLCONFIGDIR"] = str(matplotlib_cache_directory)

from .plots import save_all_figures


def main() -> int:
    argument_parser = argparse.ArgumentParser(
        description="Generate a headless calibration diagnostics report."
    )
    argument_parser.add_argument(
        "--input",
        required=True,
        help="Path to a run directory, staged response JSON, fit report JSON, or diagnostics JSON file.",
    )
    argument_parser.add_argument(
        "--output-dir",
        required=True,
        help="Directory where summary JSON and plots should be written.",
    )
    arguments = argument_parser.parse_args()

    artifacts = load_workbench_artifacts(arguments.input)
    output_directory = Path(arguments.output_dir).expanduser().resolve()
    output_directory.mkdir(parents=True, exist_ok=True)

    summary_payload = build_summary_payload(artifacts)
    plot_paths = save_all_figures(artifacts, output_directory)
    summary_payload["plot_paths"] = plot_paths

    summary_path = output_directory / "summary.json"
    summary_path.write_text(
        json.dumps(summary_payload, indent=2, sort_keys=True),
        encoding="utf-8",
    )
    print(json.dumps(summary_payload, indent=2, sort_keys=True))

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
