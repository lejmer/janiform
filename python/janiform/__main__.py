from ._cli.command_tree import invoke_application


def main() -> None:
    raise SystemExit(invoke_application())


if __name__ == "__main__":
    main()
