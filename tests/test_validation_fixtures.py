import json
from pathlib import Path

import pytest

from tests.fixture_validation import (
    CALENDAR_REGRESSION_FIXTURE_SET_ID,
    HOURLY_FACTOR_REGRESSION_FIXTURE_SET_ID,
    CalendarRegressionCase,
    CalendarRegressionFixtureDocument,
    HourlyFactorRegressionFixtureDocument,
    ValidationFixtureManifestDocument,
    ValidationFixtureManifestEntry,
    compute_content_hash_hex,
    load_calendar_regression_fixture_document,
    load_hourly_factor_regression_fixture_document,
    load_manifest_entry_payload,
    sorted_manifest_entries,
    validate_calendar_regression_fixture_document,
    validate_hourly_factor_regression_fixture_document,
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


def test_manifest_hourly_fixture_entry_is_present(
    validation_fixture_manifest_entries: list[ValidationFixtureManifestEntry],
) -> None:
    hourly_entry: ValidationFixtureManifestEntry = _find_entry_by_fixture_id(
        validation_fixture_manifest_entries,
        HOURLY_FACTOR_REGRESSION_FIXTURE_SET_ID,
    )

    assert hourly_entry["domain"] == "hourly_factors"
    assert len(hourly_entry["exchange_codes"]) >= 1


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


def test_hourly_fixture_loading_order_and_content_hash_are_deterministic(
    hourly_factor_regression_fixture_path: Path,
) -> None:
    fixture_payload_bytes: bytes = hourly_factor_regression_fixture_path.read_bytes()

    first_document: HourlyFactorRegressionFixtureDocument = load_hourly_factor_regression_fixture_document(
        fixture_payload_bytes,
    )
    second_document: HourlyFactorRegressionFixtureDocument = load_hourly_factor_regression_fixture_document(
        fixture_payload_bytes,
    )

    first_canonical_case_order: list[str] = [
        regression_case["fixture_id"] for regression_case in first_document["canonical_output_cases"]
    ]
    second_canonical_case_order: list[str] = [
        regression_case["fixture_id"] for regression_case in second_document["canonical_output_cases"]
    ]

    first_content_hash: str = compute_content_hash_hex(fixture_payload_bytes)
    second_content_hash: str = compute_content_hash_hex(fixture_payload_bytes)

    assert first_canonical_case_order == second_canonical_case_order
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


def test_validation_manifest_rejects_malformed_manifest_entry_content_hash(
    validation_fixture_manifest_document: ValidationFixtureManifestDocument,
) -> None:
    invalid_document: dict[str, object] = _copy_document(validation_fixture_manifest_document)

    fixture_entries: list[dict[str, object]] = invalid_document["fixtures"]
    fixture_entries[0]["content_hash"] = "ABCDEF0123456789"

    with pytest.raises(ValueError, match="content_hash must be a lowercase fnv1a hex string"):
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


def test_calendar_fixture_historical_holiday_cases_have_unique_exchange_timestamp_pairs(
    calendar_regression_fixture_document: CalendarRegressionFixtureDocument,
) -> None:
    historical_holiday_cases: list[CalendarRegressionCase] = [
        regression_case
        for regression_case in calendar_regression_fixture_document["cases"]
        if "historical_holiday" in regression_case["tags"]
    ]

    assert historical_holiday_cases

    observed_pairs: set[tuple[str, str]] = set()
    for regression_case in historical_holiday_cases:
        exchange_code = regression_case["exchange_code"]
        timestamp_utc = regression_case["timestamp_utc"]

        assert regression_case["expected_state"] == "closed"
        assert isinstance(exchange_code, str)
        assert isinstance(timestamp_utc, str)

        case_pair: tuple[str, str] = (exchange_code, timestamp_utc)
        assert case_pair not in observed_pairs

        observed_pairs.add(case_pair)


def test_calendar_fixture_rejects_duplicate_exchange_timestamp_pairs(
    calendar_regression_fixture_document: CalendarRegressionFixtureDocument,
) -> None:
    invalid_document: dict[str, object] = _copy_document(calendar_regression_fixture_document)
    regression_cases: list[dict[str, object]] = invalid_document["cases"]

    duplicate_case: dict[str, object] = dict(regression_cases[0])
    duplicate_case["fixture_id"] = "duplicate_exchange_timestamp_case"

    regression_cases.append(duplicate_case)

    with pytest.raises(ValueError, match="duplicate exchange_code and timestamp_utc pair"):
        validate_calendar_regression_fixture_document(invalid_document)


def test_calendar_fixture_rejects_malformed_canonical_metadata(
    calendar_regression_fixture_document: CalendarRegressionFixtureDocument,
) -> None:
    invalid_document: dict[str, object] = _copy_document(calendar_regression_fixture_document)
    canonical_metadata: dict[str, object] = invalid_document["canonical_regression_metadata"]

    stable_fixture_ids_by_tag: dict[str, object] = canonical_metadata["stable_fixture_ids_by_tag"]
    stable_fixture_ids_by_tag["boundary"] = [1]

    with pytest.raises(
        ValueError,
        match="calendar canonical_regression_metadata.stable_fixture_ids_by_tag must only contain strings",
    ):
        validate_calendar_regression_fixture_document(invalid_document)


def test_hourly_fixture_rejects_malformed_canonical_metadata(
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
) -> None:
    invalid_document: dict[str, object] = _copy_document(hourly_factor_regression_fixture_document)
    canonical_metadata: dict[str, object] = invalid_document["canonical_regression_metadata"]
    canonical_metadata["stable_canonical_output_fixture_ids"] = ["missing_fixture_id"]

    with pytest.raises(
        ValueError,
        match="stable_canonical_output_fixture_ids does not match observed fixture ids",
    ):
        validate_hourly_factor_regression_fixture_document(invalid_document)


def test_hourly_fixture_rejects_unsupported_state_classification_strictness_policy(
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
) -> None:
    invalid_document: dict[str, object] = _copy_document(hourly_factor_regression_fixture_document)

    threshold_configuration: dict[str, object] = invalid_document["threshold_configuration"]
    threshold_configuration["state_classification_strictness"] = "fuzzy_match.v1"

    with pytest.raises(ValueError, match="unsupported state classification strictness policy"):
        validate_hourly_factor_regression_fixture_document(invalid_document)


def test_hourly_fixture_rejects_factor_range_policies_with_inverted_bounds(
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
) -> None:
    invalid_document: dict[str, object] = _copy_document(hourly_factor_regression_fixture_document)
    threshold_configuration: dict[str, object] = invalid_document["threshold_configuration"]

    factor_range_by_fixture_class: dict[str, dict[str, float]] = threshold_configuration[
        "factor_range_by_fixture_class"
    ]
    factor_range_by_fixture_class["normal_session"]["s_h_min"] = 2.5
    factor_range_by_fixture_class["normal_session"]["s_h_max"] = 2.0

    with pytest.raises(ValueError, match="s_h_min greater than s_h_max"):
        validate_hourly_factor_regression_fixture_document(invalid_document)


def test_hourly_fixture_rejects_threshold_change_log_without_current_policy_version(
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
) -> None:
    invalid_document: dict[str, object] = _copy_document(hourly_factor_regression_fixture_document)

    threshold_configuration: dict[str, object] = invalid_document["threshold_configuration"]
    threshold_configuration["policy_version"] = "2.0.0"

    with pytest.raises(ValueError, match="change_log must include an entry for policy_version"):
        validate_hourly_factor_regression_fixture_document(invalid_document)


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
