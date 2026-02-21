import json
from pathlib import Path

import pytest

from janiform.calendar import CalendarRegistry
from tests.fixture_validation import (
    CALENDAR_REGRESSION_FIXTURE_SET_ID,
    HOURLY_FACTOR_REGRESSION_FIXTURE_SET_ID,
    CalendarRegressionCase,
    CalendarRegressionFixtureDocument,
    HourlyFactorRegressionCanonicalOutputCase,
    HourlyFactorRegressionFixtureDocument,
    HourlyFactorRegressionMissingDataGuardCase,
    SeededTickSimulationParityCase,
    SeededTickSimulationParityFixtureDocument,
    ValidationFixtureManifestDocument,
    ValidationFixtureManifestEntry,
    load_calendar_regression_fixture_document,
    load_hourly_factor_regression_fixture_document,
    load_seeded_tick_simulation_parity_fixture_document,
    load_manifest_entry_payload,
    load_validation_fixture_manifest,
    sorted_manifest_entries,
)


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
def hourly_factor_regression_manifest_entry(
    validation_fixture_manifest_entries: list[ValidationFixtureManifestEntry],
) -> ValidationFixtureManifestEntry:
    for manifest_entry in validation_fixture_manifest_entries:
        if manifest_entry["fixture_id"] == HOURLY_FACTOR_REGRESSION_FIXTURE_SET_ID:
            return manifest_entry

    raise ValueError(f"missing manifest fixture entry: {HOURLY_FACTOR_REGRESSION_FIXTURE_SET_ID}")


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
def hourly_factor_regression_fixture_path(
    fixtures_root: Path,
    hourly_factor_regression_manifest_entry: ValidationFixtureManifestEntry,
) -> Path:
    return fixtures_root / hourly_factor_regression_manifest_entry["relative_path"]


@pytest.fixture(scope="session")
def hourly_factor_regression_fixture_document(
    fixtures_root: Path,
    hourly_factor_regression_manifest_entry: ValidationFixtureManifestEntry,
) -> HourlyFactorRegressionFixtureDocument:
    fixture_payload_bytes: bytes = load_manifest_entry_payload(
        fixtures_root,
        hourly_factor_regression_manifest_entry,
    )
    return load_hourly_factor_regression_fixture_document(fixture_payload_bytes)


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


@pytest.fixture(scope="session")
def seeded_tick_simulation_parity_fixture_path(fixtures_root: Path) -> Path:
    return fixtures_root / "simulation" / "seeded_tick_simulation_parity_fixture.json"


@pytest.fixture(scope="session")
def seeded_tick_simulation_parity_fixture_document(
    seeded_tick_simulation_parity_fixture_path: Path,
) -> SeededTickSimulationParityFixtureDocument:
    return load_seeded_tick_simulation_parity_fixture_document(
        seeded_tick_simulation_parity_fixture_path.read_bytes(),
    )


@pytest.fixture(scope="session")
def seeded_tick_simulation_parity_cases(
    seeded_tick_simulation_parity_fixture_document: SeededTickSimulationParityFixtureDocument,
) -> list[SeededTickSimulationParityCase]:
    return list(seeded_tick_simulation_parity_fixture_document["cases"])
