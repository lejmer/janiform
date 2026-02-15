from pathlib import Path
from typing import TypedDict, cast

import json

import pytest

from janiform.calendar import CalendarRegistry


class CalendarRegressionCase(TypedDict):
    fixture_id: str
    exchange_code: str
    timestamp_utc: str
    expected_state: str
    tags: list[str]
    description: str


class CalendarRegressionFixtureDocument(TypedDict):
    schema_version: str
    fixture_set_id: str
    calendar_registry: dict[str, object]
    cases: list[CalendarRegressionCase]


@pytest.fixture(scope="session")
def project_root() -> Path:
    return Path(__file__).resolve().parents[2]


@pytest.fixture(scope="session")
def python_source_root(project_root: Path) -> Path:
    return project_root / "python"


@pytest.fixture(scope="session")
def calendar_regression_fixture_path() -> Path:
    return (
        Path(__file__).resolve().parent
        / "fixtures"
        / "calendar"
        / "calendar_service_regression_matrix.json"
    )


@pytest.fixture(scope="session")
def calendar_regression_fixture_document(
    calendar_regression_fixture_path: Path,
) -> CalendarRegressionFixtureDocument:
    fixture_text: str = calendar_regression_fixture_path.read_text(encoding="utf-8")
    fixture_value: object = json.loads(fixture_text)
    return cast(CalendarRegressionFixtureDocument, fixture_value)


@pytest.fixture(scope="session")
def calendar_regression_cases(
    calendar_regression_fixture_document: CalendarRegressionFixtureDocument,
) -> list[CalendarRegressionCase]:
    return list(calendar_regression_fixture_document["cases"])


@pytest.fixture(scope="session")
def calendar_registry_json(
    calendar_regression_fixture_document: CalendarRegressionFixtureDocument,
) -> str:
    registry_artifact: dict[str, object] = calendar_regression_fixture_document["calendar_registry"]
    return json.dumps(registry_artifact)


@pytest.fixture()
def calendar_registry(calendar_registry_json: str) -> CalendarRegistry:
    return CalendarRegistry(calendar_registry_json)
