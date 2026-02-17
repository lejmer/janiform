import json
from pathlib import Path
from typing import cast, TypedDict

import pytest

from janiform.calendar import CalendarRegistry
from tests.fixture_validation import (
    CALENDAR_REGRESSION_FIXTURE_SET_ID,
    CalendarRegressionCase,
    CalendarRegressionFixtureDocument,
    ValidationFixtureManifestDocument,
    ValidationFixtureManifestEntry,
    load_calendar_regression_fixture_document,
    load_manifest_entry_payload,
    load_validation_fixture_manifest,
    sorted_manifest_entries,
)


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
def validation_fixture_manifest_path() -> Path:
    return (
        Path(__file__).resolve().parent
        / "fixtures"
        / "validation"
        / "validation_fixture_manifest.json"
    )


@pytest.fixture(scope="session")
def fixtures_root() -> Path:
    return Path(__file__).resolve().parent / "fixtures"


@pytest.fixture(scope="session")
def validation_fixture_manifest_document(
    validation_fixture_manifest_path: Path,
) -> ValidationFixtureManifestDocument:
    return load_validation_fixture_manifest(validation_fixture_manifest_path)


@pytest.fixture(scope="session")
def validation_fixture_manifest_entries(
    validation_fixture_manifest_document: ValidationFixtureManifestDocument,
) -> list[ValidationFixtureManifestEntry]:
    return sorted_manifest_entries(validation_fixture_manifest_document)


@pytest.fixture(scope="session")
def calendar_regression_manifest_entry(
    validation_fixture_manifest_entries: list[ValidationFixtureManifestEntry],
) -> ValidationFixtureManifestEntry:
    for manifest_entry in validation_fixture_manifest_entries:
        if manifest_entry["fixture_id"] == CALENDAR_REGRESSION_FIXTURE_SET_ID:
            return manifest_entry

    raise ValueError(f"missing manifest fixture entry: {CALENDAR_REGRESSION_FIXTURE_SET_ID}")


@pytest.fixture(scope="session")
def calendar_regression_fixture_path(
    fixtures_root: Path,
    calendar_regression_manifest_entry: ValidationFixtureManifestEntry,
) -> Path:
    return fixtures_root / calendar_regression_manifest_entry["relative_path"]


@pytest.fixture(scope="session")
def calendar_regression_fixture_document(
    fixtures_root: Path,
    calendar_regression_manifest_entry: ValidationFixtureManifestEntry,
) -> CalendarRegressionFixtureDocument:
    fixture_payload_bytes: bytes = load_manifest_entry_payload(
        fixtures_root,
        calendar_regression_manifest_entry,
    )
    return load_calendar_regression_fixture_document(fixture_payload_bytes)


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
