import json
from collections.abc import Sequence
from pathlib import Path
from typing import Any, Final


REPOSITORY_ROOT: Final[Path] = Path(__file__).resolve().parents[1]
DATA_ROOT: Final[Path] = REPOSITORY_ROOT / "data"
TEST_FIXTURE_ROOT: Final[Path] = REPOSITORY_ROOT / "tests" / "fixtures"

SYNTHETIC_EURUSD_TICK_DATA_PATH: Final[Path] = (
    DATA_ROOT / "Synthetic_EURUSD_Data_1M_ticks.parquet"
)
CALENDAR_FIXTURE_PATH: Final[Path] = (
    TEST_FIXTURE_ROOT / "calendar" / "calendar_service_regression_matrix.json"
)
HOURLY_FACTOR_FIXTURE_PATH: Final[Path] = (
    TEST_FIXTURE_ROOT / "hourly_factors" / "hourly_factor_regression_matrix.json"
)
STAGED_PREPARED_EXECUTION_PAYLOAD_PATH: Final[Path] = (
    TEST_FIXTURE_ROOT / "calibration" / "staged_prepared_execution_payload.json"
)


def load_json_document(path: Path) -> dict[str, Any]:
    raw_document: Any = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(raw_document, dict):
        raise ValueError(f"expected {path} to contain a JSON object")

    return raw_document


def write_json_document(path: Path, document: dict[str, Any]) -> Path:
    path.write_text(
        json.dumps(document, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    return path


def write_csv_lines(path: Path, rows: Sequence[str]) -> Path:
    path.write_text("\n".join(rows) + "\n", encoding="utf-8")
    return path


def calendar_fixture_document() -> dict[str, Any]:
    return load_json_document(CALENDAR_FIXTURE_PATH)


def calendar_registry_payload() -> dict[str, Any]:
    raw_payload: Any = calendar_fixture_document()["calendar_registry"]
    if not isinstance(raw_payload, dict):
        raise ValueError("calendar fixture calendar_registry must be a JSON object")

    return raw_payload


def calendar_registry_json() -> str:
    return json.dumps(calendar_registry_payload())


def calendar_regression_cases() -> list[dict[str, Any]]:
    raw_cases: Any = calendar_fixture_document()["cases"]
    if not isinstance(raw_cases, list):
        raise ValueError("calendar fixture cases must be a JSON array")

    return [dict(case) for case in raw_cases]


def hourly_factor_fixture_document() -> dict[str, Any]:
    return load_json_document(HOURLY_FACTOR_FIXTURE_PATH)


def hourly_factor_regression_canonical_cases() -> list[dict[str, Any]]:
    raw_cases: Any = hourly_factor_fixture_document()["canonical_output_cases"]
    if not isinstance(raw_cases, list):
        raise ValueError("hourly-factor canonical_output_cases must be a JSON array")

    return [dict(case) for case in raw_cases]
