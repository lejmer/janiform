from argparse import ArgumentParser, Namespace

from . import __version__


def _build_parser() -> ArgumentParser:
    parser: ArgumentParser = ArgumentParser(
        prog="janiform",
        description="Command line interface for Janiform.",
    )
    parser.add_argument("--version", action="version", version=f"%(prog)s {__version__}")

    return parser


def main() -> None:
    parser: ArgumentParser = _build_parser()
    parsed_arguments: Namespace = parser.parse_args()
    _ = parsed_arguments
