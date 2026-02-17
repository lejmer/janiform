import json
from pathlib import Path

import pytest

from tests.fixture_validation import (
    CALENDAR_REGRESSION_FIXTURE_SET_ID,
    CalendarRegressionFixtureDocument,
    ValidationFixtureManifestDocument,
    ValidationFixtureManifestEntry,
    compute_content_hash_hex,
    load_calendar_regression_fixture_document,
    load_manifest_entry_payload,
    sorted_manifest_entries,
    validate_calendar_regression_fixture_document,
    validate_validation_fixture_manifest_document,
)


def test_manifest_calendar_fixture_entry_has_multiple_exchanges(
    validation_fixture_manifest_entries: list[ValidationFixtureManifestEntry],
) -> None:
    calendar_entry: ValidationFixtureManifestEntry = _find_entry_by_fixture_id(
        validation_fixture_manifest_entries,
        CALENDAR_REGRESSION_FIXTURE_SET_ID,
    )

    assert len(calendar_entry["exchange_codes"]) >= 2


def test_manifest_loading_order_and_content_hashes_are_deterministic(
    fixtures_root: Path,
    validation_fixture_manifest_document: ValidationFixtureManifestDocument,
) -> None:
    first_entries: list[ValidationFixtureManifestEntry] = sorted_manifest_entries(
        validation_fixture_manifest_document,
    )
    second_entries: list[ValidationFixtureManifestEntry] = sorted_manifest_entries(
        validation_fixture_manifest_document,
    )

    first_snapshot: list[tuple[str, str]] = _manifest_entry_content_hash_snapshot(fixtures_root, first_entries)
    second_snapshot: list[tuple[str, str]] = _manifest_entry_content_hash_snapshot(fixtures_root, second_entries)

    assert first_snapshot == second_snapshot


def test_calendar_fixture_loading_order_and_content_hash_are_deterministic(
    calendar_regression_fixture_path: Path,
) -> None:
    fixture_payload_bytes: bytes = calendar_regression_fixture_path.read_bytes()

    first_document: CalendarRegressionFixtureDocument = load_calendar_regression_fixture_document(
        fixture_payload_bytes,
    )
    second_document: CalendarRegressionFixtureDocument = load_calendar_regression_fixture_document(
        fixture_payload_bytes,
    )

    first_case_order: list[str] = [regression_case["fixture_id"] for regression_case in first_document["cases"]]
    second_case_order: list[str] = [regression_case["fixture_id"] for regression_case in second_document["cases"]]

    first_content_hash: str = compute_content_hash_hex(fixture_payload_bytes)
    second_content_hash: str = compute_content_hash_hex(fixture_payload_bytes)

    assert first_case_order == second_case_order
    assert first_content_hash == second_content_hash


def test_validation_manifest_rejects_missing_required_fields(
    validation_fixture_manifest_document: ValidationFixtureManifestDocument,
) -> None:
    invalid_document: dict[str, object] = _copy_document(validation_fixture_manifest_document)
    invalid_document.pop("schema_version")

    with pytest.raises(ValueError, match="missing required field 'schema_version'"):
        validate_validation_fixture_manifest_document(invalid_document)


def test_validation_manifest_rejects_invalid_domain_values(
    validation_fixture_manifest_document: ValidationFixtureManifestDocument,
) -> None:
    invalid_document: dict[str, object] = _copy_document(validation_fixture_manifest_document)
    fixture_entries: list[dict[str, object]] = invalid_document["fixtures"]
    fixture_entries[0]["domain"] = "python_only"

    with pytest.raises(ValueError, match="unsupported fixture domain"):
        validate_validation_fixture_manifest_document(invalid_document)


def test_calendar_fixture_rejects_missing_required_fields(
    calendar_regression_fixture_document: CalendarRegressionFixtureDocument,
) -> None:
    invalid_document: dict[str, object] = _copy_document(calendar_regression_fixture_document)
    invalid_document.pop("cases")

    with pytest.raises(ValueError, match="missing required field 'cases'"):
        validate_calendar_regression_fixture_document(invalid_document)


def test_calendar_fixture_rejects_invalid_expected_state_values(
    calendar_regression_fixture_document: CalendarRegressionFixtureDocument,
) -> None:
    invalid_document: dict[str, object] = _copy_document(calendar_regression_fixture_document)
    regression_cases: list[dict[str, object]] = invalid_document["cases"]
    regression_cases[0]["expected_state"] = "invalid"

    with pytest.raises(ValueError, match="unsupported expected_state"):
        validate_calendar_regression_fixture_document(invalid_document)


def _manifest_entry_content_hash_snapshot(
    fixtures_root: Path,
    manifest_entries: list[ValidationFixtureManifestEntry],
) -> list[tuple[str, str]]:
    snapshot: list[tuple[str, str]] = []
    for manifest_entry in manifest_entries:
        payload_bytes: bytes = load_manifest_entry_payload(fixtures_root, manifest_entry)
        snapshot.append((manifest_entry["fixture_id"], compute_content_hash_hex(payload_bytes)))

    return snapshot


def _find_entry_by_fixture_id(
    validation_fixture_manifest_entries: list[ValidationFixtureManifestEntry],
    fixture_id: str,
) -> ValidationFixtureManifestEntry:
    for manifest_entry in validation_fixture_manifest_entries:
        if manifest_entry["fixture_id"] == fixture_id:
            return manifest_entry

    raise ValueError(f"missing fixture manifest entry with id: {fixture_id}")


def _copy_document(document: object) -> dict[str, object]:
    return json.loads(json.dumps(document))
