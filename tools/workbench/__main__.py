import argparse

from .app import launch_workbench


def main() -> int:
    argument_parser = argparse.ArgumentParser(
        description="Open the Janiform calibration workbench."
    )
    argument_parser.add_argument(
        "input",
        help="Path to a run directory, fit report JSON, staged response JSON, or diagnostics JSON file.",
    )
    arguments = argument_parser.parse_args()
    return launch_workbench(arguments.input)


if __name__ == "__main__":
    raise SystemExit(main())
