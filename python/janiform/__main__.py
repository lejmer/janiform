from .cli.command_tree import _invoke_application


def main() -> None:
    raise SystemExit(_invoke_application())


if __name__ == "__main__":
    main()
