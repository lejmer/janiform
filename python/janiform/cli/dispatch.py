from .command_tree import _invoke_application


def main() -> None:
    raise SystemExit(_invoke_application())


__all__ = ["main"]
