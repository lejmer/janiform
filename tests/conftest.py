import json
from pathlib import Path
from typing import TypedDict, cast

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


class HourlyFactorRegressionAcceptanceThresholds(TypedDict):
    s_h_absolute_tolerance: float
    nu_h_absolute_tolerance: float


class HourlyFactorRegressionCanonicalOutputCase(TypedDict):
    fixture_id: str
    exchange_code: str
    range_start_utc: str
    range_end_utc: str
    seasonality_source: dict[str, object]
    news_intensity_source: dict[str, object]
    expected_s_h: list[float]
    expected_nu_h: list[float]
    expected_skipped_local_hour_interval_count: int
    expected_repeated_local_hour_interval_count: int
    tags: list[str]
    description: str


class HourlyFactorRegressionMissingDataGuardCase(TypedDict):
    fixture_id: str
    exchange_code: str
    range_start_utc: str
    range_end_utc: str
    source_timestamps_utc: list[str]
    source_seasonality_factors: list[float]
    settings: dict[str, object]
    expected_outcome: dict[str, object]
    tags: list[str]
    description: str


class HourlyFactorRegressionFixtureDocument(TypedDict):
    schema_version: str
    fixture_set_id: str
    calendar_fixture_set_id: str
    acceptance_thresholds: HourlyFactorRegressionAcceptanceThresholds
    canonical_output_cases: list[HourlyFactorRegressionCanonicalOutputCase]
    missing_data_guard_cases: list[HourlyFactorRegressionMissingDataGuardCase]


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


@pytest.fixture(scope="session")
def hourly_factor_regression_fixture_path() -> Path:
    return (
        Path(__file__).resolve().parent
        / "fixtures"
        / "hourly_factors"
        / "hourly_factor_regression_matrix.json"
    )


@pytest.fixture(scope="session")
def hourly_factor_regression_fixture_document(
    hourly_factor_regression_fixture_path: Path,
) -> HourlyFactorRegressionFixtureDocument:
    fixture_text: str = hourly_factor_regression_fixture_path.read_text(encoding="utf-8")
    fixture_value: object = json.loads(fixture_text)
    return cast(HourlyFactorRegressionFixtureDocument, fixture_value)


@pytest.fixture(scope="session")
def hourly_factor_regression_canonical_cases(
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
) -> list[HourlyFactorRegressionCanonicalOutputCase]:
    return list(hourly_factor_regression_fixture_document["canonical_output_cases"])


@pytest.fixture(scope="session")
def hourly_factor_regression_missing_data_guard_cases(
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
) -> list[HourlyFactorRegressionMissingDataGuardCase]:
    return list(hourly_factor_regression_fixture_document["missing_data_guard_cases"])
