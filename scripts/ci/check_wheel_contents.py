import argparse
import sys
import zipfile
from pathlib import Path


REQUIRED_EXACT_SUFFIXES = (
    "janiform/py.typed",
    "janiform/calibration/_bundled_presets/generated/index.json",
)
FORBIDDEN_PARTS = (
    "__pycache__",
    ".artifacts/",
    "target/",
)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate Janiform wheel contents for release packaging.",
    )
    parser.add_argument("wheel", type=Path)
    args = parser.parse_args()

    with zipfile.ZipFile(args.wheel) as wheel:
        names = tuple(wheel.namelist())

    errors: list[str] = []
    for required_suffix in REQUIRED_EXACT_SUFFIXES:
        if required_suffix not in names:
            errors.append(f"missing required wheel member {required_suffix}")

    if not any(name.startswith("janiform/_native.") for name in names):
        errors.append("missing native extension member janiform/_native.*")
    if not any(name.endswith("/LICENSE") or name == "LICENSE" for name in names):
        errors.append("missing packaged LICENSE file")
    if not any(name.endswith("/THIRD_PARTY_NOTICES.txt") or name == "THIRD_PARTY_NOTICES.txt" for name in names):
        errors.append("missing packaged THIRD_PARTY_NOTICES.txt file")
    if not any(name.startswith("janiform/calibration/_bundled_presets/generated/sessions/") and name.endswith(".json") for name in names):
        errors.append("missing bundled session preset JSON")
    if not any(name.startswith("janiform/calibration/_bundled_presets/generated/market_profiles/") and name.endswith(".json") for name in names):
        errors.append("missing bundled market profile preset JSON")
    if not any(name.startswith("janiform/calibration/_bundled_presets/generated/known_news/") and name.endswith(".json") for name in names):
        errors.append("missing bundled known-news preset JSON")

    for name in names:
        if name.endswith(".pyc"):
            errors.append(f"forbidden bytecode member {name}")

        for forbidden_part in FORBIDDEN_PARTS:
            if forbidden_part in name:
                errors.append(f"forbidden wheel member {name}")

    if errors:
        for error in errors:
            print(f"wheel content check failed: {error}", file=sys.stderr)

        return 1

    print(f"wheel content check passed for {args.wheel}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
