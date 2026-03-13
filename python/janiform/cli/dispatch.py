from .parsing.command_tree import _parse_cli_arguments
from .rendering import _write_cli_error
from ..errors import JaniformError


def main() -> None:
    try:
        parsed_arguments = _parse_cli_arguments()
    except JaniformError as e:
        raise SystemExit(_write_cli_error(e)) from e

    handler = parsed_arguments.handler
    if handler is not None:
        raise SystemExit(handler(parsed_arguments))

    parsed_arguments.help_target.print_help()


__all__ = ["main"]
