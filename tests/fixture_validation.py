import json
import math
from pathlib import Path
from typing import TypedDict, cast

VALIDATION_FIXTURE_MANIFEST_SCHEMA_VERSION: str = "validation.fixture.manifest.v1"
MANIFEST_DETERMINISM_ORDER_POLICY: str = "lexicographic_by_fixture_id.v1"
CONTENT_HASH_ALGORITHM_POLICY: str = "fnv1a_64_hex_utf8.v1"
MANIFEST_SCHEMA_FAMILY: str = "validation.fixture.manifest"
MANIFEST_V0_UPDATE_POLICY: str = "additive_only_and_major_on_breaking_change.v1"

CALENDAR_REGRESSION_FIXTURE_SCHEMA_VERSION: str = "calendar.service.regression.v1"
CALENDAR_REGRESSION_FIXTURE_SET_ID: str = "calendar_service_regression_matrix_v1"
CALENDAR_CASE_ORDER_POLICY: str = "preserve_declared_case_order.v1"
CALENDAR_CONTENT_HASH_SCOPE_POLICY: str = "document_payload_bytes.v1"
CALENDAR_SCHEMA_FAMILY: str = "validation.calendar.service.regression"
CALENDAR_SCHEMA_GENERATION: str = "v0.x"
CALENDAR_FORWARD_EXTENSION_POLICY: str = "additive_fields_only.v1"

HOURLY_FACTOR_REGRESSION_FIXTURE_SCHEMA_VERSION: str = "hourly.factor.regression.v1"
HOURLY_FACTOR_REGRESSION_FIXTURE_SET_ID: str = "hourly_factor_regression_matrix_v1"
SUPPORTED_MANIFEST_DOMAINS: set[str] = {"calendar", "seasonality", "hourly_factors"}


type JsonObject = dict[str, object]
type JsonArray = list[object]


class ValidationFixtureVersioningPolicy(TypedDict):
    schema_family: str
    v0_update_policy: str


class ValidationFixtureDeterminismPolicy(TypedDict):
    fixture_loading_order: str
    content_hash_algorithm: str


class ValidationFixtureManifestEntry(TypedDict):
    fixture_id: str
    domain: str
    fixture_schema_version: str
    relative_path: str
    exchange_codes: list[str]
    content_hash: str


class ValidationFixtureManifestDocument(TypedDict):
    schema_version: str
    manifest_id: str
    manifest_version: str
    versioning_policy: ValidationFixtureVersioningPolicy
    determinism_policy: ValidationFixtureDeterminismPolicy
    fixtures: list[ValidationFixtureManifestEntry]


class CalendarRegressionSchemaMetadata(TypedDict):
    schema_family: str
    schema_generation: str
    forward_extension_policy: str


class CalendarRegressionDeterminismMetadata(TypedDict):
    fixture_loading_order: str
    content_hash_scope: str
    content_hash_algorithm: str


class CalendarRegressionCase(TypedDict):
    fixture_id: str
    exchange_code: str
    timestamp_utc: str
    expected_state: str
    tags: list[str]
    description: str


class CalendarCanonicalRegressionMetadata(TypedDict):
    required_tags: list[str]
    stable_fixture_ids_by_tag: dict[str, list[str]]
    historical_holiday_expected_years: list[int]
    historical_holiday_expected_intraday_tags: list[str]


class CalendarRegressionFixtureDocument(TypedDict):
    schema_version: str
    fixture_set_id: str
    schema_metadata: CalendarRegressionSchemaMetadata
    determinism_metadata: CalendarRegressionDeterminismMetadata
    canonical_regression_metadata: CalendarCanonicalRegressionMetadata
    calendar_registry: dict[str, object]
    cases: list[CalendarRegressionCase]


class HourlyFactorRegressionCanonicalMetadata(TypedDict):
    required_tags: list[str]
    stable_canonical_output_fixture_ids: list[str]
    stable_missing_data_guard_fixture_ids: list[str]


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
    canonical_regression_metadata: HourlyFactorRegressionCanonicalMetadata
    canonical_output_cases: list[HourlyFactorRegressionCanonicalOutputCase]
    missing_data_guard_cases: list[HourlyFactorRegressionMissingDataGuardCase]


def compute_content_hash_hex(payload_bytes: bytes) -> str:
    hash_state: int = 0xCBF29CE484222325
    for payload_byte in payload_bytes:
        hash_state ^= payload_byte
        hash_state = (hash_state * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF

    return f"{hash_state:016x}"


def load_validation_fixture_manifest(manifest_path: Path) -> ValidationFixtureManifestDocument:
    return validate_validation_fixture_manifest_document(_read_json_document(manifest_path))


def validate_validation_fixture_manifest_document(
    raw_document: object,
) -> ValidationFixtureManifestDocument:
    document_map: JsonObject = _require_json_object(raw_document, "validation fixture manifest")
    schema_version: str = _require_non_empty_string(document_map, "schema_version", "manifest")
    if schema_version != VALIDATION_FIXTURE_MANIFEST_SCHEMA_VERSION:
        raise ValueError(f"unsupported validation fixture manifest schema version: {schema_version}")

    _require_non_empty_string(document_map, "manifest_id", "manifest")
    _require_non_empty_string(document_map, "manifest_version", "manifest")

    versioning_policy: JsonObject = _require_json_object_field(document_map, "versioning_policy", "manifest")
    schema_family: str = _require_non_empty_string(versioning_policy, "schema_family", "versioning_policy")
    if schema_family != MANIFEST_SCHEMA_FAMILY:
        raise ValueError(f"unexpected versioning schema family: {schema_family}")

    v0_update_policy: str = _require_non_empty_string(versioning_policy, "v0_update_policy", "versioning_policy")
    if v0_update_policy != MANIFEST_V0_UPDATE_POLICY:
        raise ValueError(f"unexpected v0 update policy: {v0_update_policy}")

    determinism_policy: JsonObject = _require_json_object_field(document_map, "determinism_policy", "manifest")
    fixture_loading_order: str = _require_non_empty_string(
        determinism_policy,
        "fixture_loading_order",
        "determinism_policy",
    )
    if fixture_loading_order != MANIFEST_DETERMINISM_ORDER_POLICY:
        raise ValueError(f"unexpected fixture loading order policy: {fixture_loading_order}")

    content_hash_algorithm: str = _require_non_empty_string(
        determinism_policy,
        "content_hash_algorithm",
        "determinism_policy",
    )
    if content_hash_algorithm != CONTENT_HASH_ALGORITHM_POLICY:
        raise ValueError(f"unexpected content hash algorithm policy: {content_hash_algorithm}")

    fixtures: JsonArray = _require_json_array_field(document_map, "fixtures", "manifest")
    if not fixtures:
        raise ValueError("validation fixture manifest must include at least one fixture entry")

    observed_fixture_ids: set[str] = set()
    validated_entries: list[ValidationFixtureManifestEntry] = []
    for fixture_entry_value in fixtures:
        fixture_entry_map: JsonObject = _require_json_object(fixture_entry_value, "manifest fixture entry")
        fixture_entry: ValidationFixtureManifestEntry = _validate_manifest_fixture_entry(fixture_entry_map)
        fixture_id: str = fixture_entry["fixture_id"]
        if fixture_id in observed_fixture_ids:
            raise ValueError(f"duplicate fixture_id in manifest: {fixture_id}")

        observed_fixture_ids.add(fixture_id)
        validated_entries.append(fixture_entry)

    return {
        "schema_version": schema_version,
        "manifest_id": cast(str, document_map["manifest_id"]),
        "manifest_version": cast(str, document_map["manifest_version"]),
        "versioning_policy": cast(ValidationFixtureVersioningPolicy, versioning_policy),
        "determinism_policy": cast(ValidationFixtureDeterminismPolicy, determinism_policy),
        "fixtures": validated_entries,
    }


def sorted_manifest_entries(
    manifest_document: ValidationFixtureManifestDocument,
) -> list[ValidationFixtureManifestEntry]:
    return sorted(
        manifest_document["fixtures"],
        key=lambda fixture_entry: fixture_entry["fixture_id"],
    )


def manifest_entry_resolved_path(
    fixture_root: Path,
    manifest_entry: ValidationFixtureManifestEntry,
) -> Path:
    return fixture_root / manifest_entry["relative_path"]


def load_manifest_entry_payload(
    fixture_root: Path,
    manifest_entry: ValidationFixtureManifestEntry,
) -> bytes:
    resolved_path: Path = manifest_entry_resolved_path(fixture_root, manifest_entry)
    payload_bytes: bytes = resolved_path.read_bytes()

    observed_hash: str = compute_content_hash_hex(payload_bytes)
    expected_hash: str = manifest_entry["content_hash"]
    if observed_hash != expected_hash:
        raise ValueError(
            f"fixture content hash mismatch for {manifest_entry['fixture_id']}: "
            f"expected {expected_hash}, observed {observed_hash}",
        )

    return payload_bytes


def validate_calendar_regression_fixture_document(
    raw_document: object,
) -> CalendarRegressionFixtureDocument:
    document_map: JsonObject = _require_json_object(raw_document, "calendar regression fixture")
    schema_version: str = _require_non_empty_string(document_map, "schema_version", "calendar fixture")
    if schema_version != CALENDAR_REGRESSION_FIXTURE_SCHEMA_VERSION:
        raise ValueError(f"unsupported calendar fixture schema version: {schema_version}")

    fixture_set_id: str = _require_non_empty_string(document_map, "fixture_set_id", "calendar fixture")
    if fixture_set_id != CALENDAR_REGRESSION_FIXTURE_SET_ID:
        raise ValueError(f"unexpected calendar fixture set id: {fixture_set_id}")

    schema_metadata: JsonObject = _require_json_object_field(document_map, "schema_metadata", "calendar fixture")
    schema_family: str = _require_non_empty_string(schema_metadata, "schema_family", "schema_metadata")
    if schema_family != CALENDAR_SCHEMA_FAMILY:
        raise ValueError(f"unexpected schema family: {schema_family}")

    schema_generation: str = _require_non_empty_string(
        schema_metadata,
        "schema_generation",
        "schema_metadata",
    )
    if schema_generation != CALENDAR_SCHEMA_GENERATION:
        raise ValueError(f"unexpected schema generation: {schema_generation}")

    forward_extension_policy: str = _require_non_empty_string(
        schema_metadata,
        "forward_extension_policy",
        "schema_metadata",
    )
    if forward_extension_policy != CALENDAR_FORWARD_EXTENSION_POLICY:
        raise ValueError(f"unexpected forward extension policy: {forward_extension_policy}")

    determinism_metadata: JsonObject = _require_json_object_field(
        document_map,
        "determinism_metadata",
        "calendar fixture",
    )
    fixture_loading_order: str = _require_non_empty_string(
        determinism_metadata,
        "fixture_loading_order",
        "determinism_metadata",
    )
    if fixture_loading_order != CALENDAR_CASE_ORDER_POLICY:
        raise ValueError(f"unexpected fixture loading order policy: {fixture_loading_order}")

    content_hash_scope: str = _require_non_empty_string(
        determinism_metadata,
        "content_hash_scope",
        "determinism_metadata",
    )
    if content_hash_scope != CALENDAR_CONTENT_HASH_SCOPE_POLICY:
        raise ValueError(f"unexpected content hash scope: {content_hash_scope}")

    content_hash_algorithm: str = _require_non_empty_string(
        determinism_metadata,
        "content_hash_algorithm",
        "determinism_metadata",
    )
    if content_hash_algorithm != CONTENT_HASH_ALGORITHM_POLICY:
        raise ValueError(f"unexpected content hash algorithm: {content_hash_algorithm}")

    calendar_registry: JsonObject = _require_json_object_field(document_map, "calendar_registry", "calendar fixture")
    exchange_codes_from_registry: set[str] = _extract_registry_exchange_codes(calendar_registry)

    cases: JsonArray = _require_json_array_field(document_map, "cases", "calendar fixture")
    if not cases:
        raise ValueError("calendar regression fixture requires at least one case")

    observed_fixture_ids: set[str] = set()
    observed_exchange_timestamp_pairs: set[tuple[str, str]] = set()
    validated_cases: list[CalendarRegressionCase] = []
    for regression_case_value in cases:
        regression_case_map: JsonObject = _require_json_object(regression_case_value, "calendar regression case")
        validated_case: CalendarRegressionCase = _validate_calendar_regression_case(regression_case_map)

        case_fixture_id: str = validated_case["fixture_id"]
        if case_fixture_id in observed_fixture_ids:
            raise ValueError(f"duplicate case fixture_id in calendar regression fixture: {case_fixture_id}")

        observed_fixture_ids.add(case_fixture_id)
        validated_cases.append(validated_case)

        case_exchange_code: str = validated_case["exchange_code"]
        if case_exchange_code not in exchange_codes_from_registry:
            raise ValueError(
                f"calendar regression case references unknown exchange_code: {case_exchange_code}",
            )

        case_timestamp_utc: str = validated_case["timestamp_utc"]
        exchange_timestamp_pair: tuple[str, str] = (case_exchange_code, case_timestamp_utc)
        if exchange_timestamp_pair in observed_exchange_timestamp_pairs:
            raise ValueError(
                "duplicate exchange_code and timestamp_utc pair in calendar regression fixture: "
                f"{case_exchange_code} at {case_timestamp_utc}",
            )

        observed_exchange_timestamp_pairs.add(exchange_timestamp_pair)

    canonical_regression_metadata = _validate_calendar_canonical_regression_metadata(
        _require_json_object_field(document_map, "canonical_regression_metadata", "calendar fixture"),
        validated_cases,
    )

    return {
        "schema_version": schema_version,
        "fixture_set_id": fixture_set_id,
        "schema_metadata": cast(CalendarRegressionSchemaMetadata, schema_metadata),
        "determinism_metadata": cast(CalendarRegressionDeterminismMetadata, determinism_metadata),
        "canonical_regression_metadata": canonical_regression_metadata,
        "calendar_registry": cast(dict[str, object], calendar_registry),
        "cases": validated_cases,
    }


def load_calendar_regression_fixture_document(
    fixture_payload_bytes: bytes,
) -> CalendarRegressionFixtureDocument:
    payload_value: object = json.loads(fixture_payload_bytes.decode("utf-8"))
    return validate_calendar_regression_fixture_document(payload_value)


def validate_hourly_factor_regression_fixture_document(
    raw_document: object,
) -> HourlyFactorRegressionFixtureDocument:
    document_map: JsonObject = _require_json_object(raw_document, "hourly factor regression fixture")
    schema_version: str = _require_non_empty_string(document_map, "schema_version", "hourly factor fixture")
    if schema_version != HOURLY_FACTOR_REGRESSION_FIXTURE_SCHEMA_VERSION:
        raise ValueError(f"unsupported hourly factor fixture schema version: {schema_version}")

    fixture_set_id: str = _require_non_empty_string(document_map, "fixture_set_id", "hourly factor fixture")
    if fixture_set_id != HOURLY_FACTOR_REGRESSION_FIXTURE_SET_ID:
        raise ValueError(f"unexpected hourly factor fixture set id: {fixture_set_id}")

    calendar_fixture_set_id: str = _require_non_empty_string(
        document_map,
        "calendar_fixture_set_id",
        "hourly factor fixture",
    )
    if calendar_fixture_set_id != CALENDAR_REGRESSION_FIXTURE_SET_ID:
        raise ValueError(
            f"unexpected hourly factor calendar_fixture_set_id: {calendar_fixture_set_id}",
        )

    acceptance_thresholds: JsonObject = _require_json_object_field(
        document_map,
        "acceptance_thresholds",
        "hourly factor fixture",
    )
    seasonality_absolute_tolerance: float = _require_non_negative_float(
        acceptance_thresholds,
        "s_h_absolute_tolerance",
        "acceptance_thresholds",
    )
    news_intensity_absolute_tolerance: float = _require_non_negative_float(
        acceptance_thresholds,
        "nu_h_absolute_tolerance",
        "acceptance_thresholds",
    )

    canonical_output_cases_raw: JsonArray = _require_json_array_field(
        document_map,
        "canonical_output_cases",
        "hourly factor fixture",
    )
    if not canonical_output_cases_raw:
        raise ValueError("hourly factor fixture requires at least one canonical_output_cases entry")

    missing_data_guard_cases_raw: JsonArray = _require_json_array_field(
        document_map,
        "missing_data_guard_cases",
        "hourly factor fixture",
    )
    if not missing_data_guard_cases_raw:
        raise ValueError("hourly factor fixture requires at least one missing_data_guard_cases entry")

    canonical_output_cases: list[HourlyFactorRegressionCanonicalOutputCase] = [
        _validate_hourly_factor_canonical_output_case(
            _require_json_object(case_value, "hourly canonical output case"),
        )
        for case_value in canonical_output_cases_raw
    ]
    missing_data_guard_cases: list[HourlyFactorRegressionMissingDataGuardCase] = [
        _validate_hourly_factor_missing_data_guard_case(
            _require_json_object(case_value, "hourly missing data guard case"),
        )
        for case_value in missing_data_guard_cases_raw
    ]

    _require_unique_fixture_ids(
        [fixture_case["fixture_id"] for fixture_case in canonical_output_cases],
        "canonical_output_cases",
    )
    _require_unique_fixture_ids(
        [fixture_case["fixture_id"] for fixture_case in missing_data_guard_cases],
        "missing_data_guard_cases",
    )

    canonical_regression_metadata = _validate_hourly_canonical_regression_metadata(
        _require_json_object_field(
            document_map,
            "canonical_regression_metadata",
            "hourly factor fixture",
        ),
        canonical_output_cases,
        missing_data_guard_cases,
    )

    return {
        "schema_version": schema_version,
        "fixture_set_id": fixture_set_id,
        "calendar_fixture_set_id": calendar_fixture_set_id,
        "acceptance_thresholds": {
            "s_h_absolute_tolerance": seasonality_absolute_tolerance,
            "nu_h_absolute_tolerance": news_intensity_absolute_tolerance,
        },
        "canonical_regression_metadata": canonical_regression_metadata,
        "canonical_output_cases": canonical_output_cases,
        "missing_data_guard_cases": missing_data_guard_cases,
    }


def load_hourly_factor_regression_fixture_document(
    fixture_payload_bytes: bytes,
) -> HourlyFactorRegressionFixtureDocument:
    payload_value: object = json.loads(fixture_payload_bytes.decode("utf-8"))
    return validate_hourly_factor_regression_fixture_document(payload_value)


def _validate_manifest_fixture_entry(raw_entry: JsonObject) -> ValidationFixtureManifestEntry:
    fixture_id: str = _require_non_empty_string(raw_entry, "fixture_id", "manifest fixture entry")
    domain: str = _require_non_empty_string(raw_entry, "domain", "manifest fixture entry")
    if domain not in SUPPORTED_MANIFEST_DOMAINS:
        raise ValueError(f"unsupported fixture domain: {domain}")

    fixture_schema_version: str = _require_non_empty_string(
        raw_entry,
        "fixture_schema_version",
        "manifest fixture entry",
    )
    relative_path: str = _require_non_empty_string(raw_entry, "relative_path", "manifest fixture entry")
    exchange_codes: list[str] = _require_non_empty_string_list(raw_entry, "exchange_codes", "manifest fixture entry")

    content_hash: str = _require_non_empty_string(raw_entry, "content_hash", "manifest fixture entry")
    if len(content_hash) != 16 or any(character not in "0123456789abcdef" for character in content_hash):
        raise ValueError(f"manifest fixture entry content_hash must be a lowercase fnv1a hex string: {content_hash}")

    return {
        "fixture_id": fixture_id,
        "domain": domain,
        "fixture_schema_version": fixture_schema_version,
        "relative_path": relative_path,
        "exchange_codes": exchange_codes,
        "content_hash": content_hash,
    }


def _validate_calendar_regression_case(raw_case: JsonObject) -> CalendarRegressionCase:
    fixture_id: str = _require_non_empty_string(raw_case, "fixture_id", "calendar regression case")
    exchange_code: str = _require_non_empty_string(raw_case, "exchange_code", "calendar regression case")
    timestamp_utc: str = _require_non_empty_string(raw_case, "timestamp_utc", "calendar regression case")

    expected_state: str = _require_non_empty_string(raw_case, "expected_state", "calendar regression case")
    if expected_state not in {"open", "closed"}:
        raise ValueError(f"unsupported expected_state in calendar regression case: {expected_state}")

    tags: list[str] = _require_non_empty_string_list(raw_case, "tags", "calendar regression case")
    description: str = _require_non_empty_string(raw_case, "description", "calendar regression case")

    return {
        "fixture_id": fixture_id,
        "exchange_code": exchange_code,
        "timestamp_utc": timestamp_utc,
        "expected_state": expected_state,
        "tags": tags,
        "description": description,
    }


def _validate_calendar_canonical_regression_metadata(
    raw_metadata: JsonObject,
    validated_cases: list[CalendarRegressionCase],
) -> CalendarCanonicalRegressionMetadata:
    required_tags: list[str] = _require_non_empty_string_list(
        raw_metadata,
        "required_tags",
        "calendar canonical_regression_metadata",
    )

    stable_fixture_ids_by_tag_raw: JsonObject = _require_json_object_field(
        raw_metadata,
        "stable_fixture_ids_by_tag",
        "calendar canonical_regression_metadata",
    )
    if not stable_fixture_ids_by_tag_raw:
        raise ValueError("calendar canonical_regression_metadata.stable_fixture_ids_by_tag must not be empty")

    stable_fixture_ids_by_tag: dict[str, list[str]] = {}
    for raw_tag, raw_fixture_ids in stable_fixture_ids_by_tag_raw.items():
        if not isinstance(raw_tag, str) or not raw_tag.strip():
            raise ValueError(
                "calendar canonical_regression_metadata.stable_fixture_ids_by_tag keys must be non-empty strings",
            )
        if not isinstance(raw_fixture_ids, list):
            raise ValueError(
                "calendar canonical_regression_metadata.stable_fixture_ids_by_tag values must be string arrays",
            )

        normalized_fixture_ids: list[str] = _normalize_non_empty_string_list(
            raw_fixture_ids,
            "calendar canonical_regression_metadata.stable_fixture_ids_by_tag",
        )
        _require_unique_fixture_ids(
            normalized_fixture_ids,
            f"calendar canonical_regression_metadata.stable_fixture_ids_by_tag.{raw_tag}",
        )
        stable_fixture_ids_by_tag[raw_tag.strip()] = normalized_fixture_ids

    historical_holiday_expected_years_raw: JsonArray = _require_json_array_field(
        raw_metadata,
        "historical_holiday_expected_years",
        "calendar canonical_regression_metadata",
    )
    if not historical_holiday_expected_years_raw:
        raise ValueError(
            "calendar canonical_regression_metadata.historical_holiday_expected_years must include at least one year",
        )

    historical_holiday_expected_years: list[int] = []
    for raw_year in historical_holiday_expected_years_raw:
        if not isinstance(raw_year, int) or isinstance(raw_year, bool):
            raise ValueError(
                "calendar canonical_regression_metadata.historical_holiday_expected_years must only include integers",
            )
        historical_holiday_expected_years.append(raw_year)

    historical_holiday_expected_intraday_tags: list[str] = _require_non_empty_string_list(
        raw_metadata,
        "historical_holiday_expected_intraday_tags",
        "calendar canonical_regression_metadata",
    )

    observed_tags: set[str] = {
        tag
        for fixture_case in validated_cases
        for tag in fixture_case["tags"]
    }
    if not set(required_tags).issubset(observed_tags):
        raise ValueError(
            "calendar canonical_regression_metadata.required_tags must be present in fixture cases",
        )

    for tag, expected_fixture_ids in stable_fixture_ids_by_tag.items():
        observed_fixture_ids: set[str] = {
            fixture_case["fixture_id"]
            for fixture_case in validated_cases
            if tag in fixture_case["tags"]
        }
        if observed_fixture_ids != set(expected_fixture_ids):
            raise ValueError(
                "calendar canonical_regression_metadata.stable_fixture_ids_by_tag does not match "
                f"observed fixture ids for tag '{tag}'",
            )

    return {
        "required_tags": required_tags,
        "stable_fixture_ids_by_tag": stable_fixture_ids_by_tag,
        "historical_holiday_expected_years": historical_holiday_expected_years,
        "historical_holiday_expected_intraday_tags": historical_holiday_expected_intraday_tags,
    }


def _validate_hourly_factor_canonical_output_case(
    raw_case: JsonObject,
) -> HourlyFactorRegressionCanonicalOutputCase:
    fixture_id: str = _require_non_empty_string(raw_case, "fixture_id", "hourly canonical output case")
    exchange_code: str = _require_non_empty_string(raw_case, "exchange_code", "hourly canonical output case")
    range_start_utc: str = _require_non_empty_string(raw_case, "range_start_utc", "hourly canonical output case")
    range_end_utc: str = _require_non_empty_string(raw_case, "range_end_utc", "hourly canonical output case")

    seasonality_source: JsonObject = _require_json_object_field(
        raw_case,
        "seasonality_source",
        "hourly canonical output case",
    )
    _validate_hourly_seasonality_source(seasonality_source, "hourly canonical output case.seasonality_source")

    news_intensity_source: JsonObject = _require_json_object_field(
        raw_case,
        "news_intensity_source",
        "hourly canonical output case",
    )

    _validate_hourly_news_intensity_source(news_intensity_source, "hourly canonical output case.news_intensity_source")

    expected_s_h: list[float] = _require_float_list(raw_case, "expected_s_h", "hourly canonical output case")
    expected_nu_h: list[float] = _require_float_list(raw_case, "expected_nu_h", "hourly canonical output case")
    expected_skipped_local_hour_interval_count: int = _require_non_negative_int(
        raw_case,
        "expected_skipped_local_hour_interval_count",
        "hourly canonical output case",
    )
    expected_repeated_local_hour_interval_count: int = _require_non_negative_int(
        raw_case,
        "expected_repeated_local_hour_interval_count",
        "hourly canonical output case",
    )
    tags: list[str] = _require_non_empty_string_list(raw_case, "tags", "hourly canonical output case")
    description: str = _require_non_empty_string(raw_case, "description", "hourly canonical output case")

    return {
        "fixture_id": fixture_id,
        "exchange_code": exchange_code,
        "range_start_utc": range_start_utc,
        "range_end_utc": range_end_utc,
        "seasonality_source": cast(dict[str, object], seasonality_source),
        "news_intensity_source": cast(dict[str, object], news_intensity_source),
        "expected_s_h": expected_s_h,
        "expected_nu_h": expected_nu_h,
        "expected_skipped_local_hour_interval_count": expected_skipped_local_hour_interval_count,
        "expected_repeated_local_hour_interval_count": expected_repeated_local_hour_interval_count,
        "tags": tags,
        "description": description,
    }


def _validate_hourly_factor_missing_data_guard_case(
    raw_case: JsonObject,
) -> HourlyFactorRegressionMissingDataGuardCase:
    fixture_id: str = _require_non_empty_string(raw_case, "fixture_id", "hourly missing data guard case")
    exchange_code: str = _require_non_empty_string(raw_case, "exchange_code", "hourly missing data guard case")
    range_start_utc: str = _require_non_empty_string(raw_case, "range_start_utc", "hourly missing data guard case")
    range_end_utc: str = _require_non_empty_string(raw_case, "range_end_utc", "hourly missing data guard case")
    source_timestamps_utc: list[str] = _require_string_list_allow_empty(
        raw_case,
        "source_timestamps_utc",
        "hourly missing data guard case",
    )
    source_seasonality_factors: list[float] = _require_float_list_allow_empty(
        raw_case,
        "source_seasonality_factors",
        "hourly missing data guard case",
    )

    settings: JsonObject = _require_json_object_field(raw_case, "settings", "hourly missing data guard case")
    _require_non_empty_string(settings, "interpolation_method", "hourly missing data guard case.settings")
    _require_non_empty_string(settings, "aggregation_method", "hourly missing data guard case.settings")
    _require_non_empty_string(settings, "sparse_fallback_policy", "hourly missing data guard case.settings")
    _require_non_negative_int(
        settings,
        "sparse_hour_minimum_observation_count",
        "hourly missing data guard case.settings",
    )

    expected_outcome: JsonObject = _require_json_object_field(
        raw_case,
        "expected_outcome",
        "hourly missing data guard case",
    )

    _validate_hourly_missing_data_expected_outcome(expected_outcome)

    tags: list[str] = _require_non_empty_string_list(raw_case, "tags", "hourly missing data guard case")
    description: str = _require_non_empty_string(raw_case, "description", "hourly missing data guard case")

    return {
        "fixture_id": fixture_id,
        "exchange_code": exchange_code,
        "range_start_utc": range_start_utc,
        "range_end_utc": range_end_utc,
        "source_timestamps_utc": source_timestamps_utc,
        "source_seasonality_factors": source_seasonality_factors,
        "settings": cast(dict[str, object], settings),
        "expected_outcome": cast(dict[str, object], expected_outcome),
        "tags": tags,
        "description": description,
    }


def _validate_hourly_canonical_regression_metadata(
    raw_metadata: JsonObject,
    canonical_output_cases: list[HourlyFactorRegressionCanonicalOutputCase],
    missing_data_guard_cases: list[HourlyFactorRegressionMissingDataGuardCase],
) -> HourlyFactorRegressionCanonicalMetadata:
    required_tags: list[str] = _require_non_empty_string_list(
        raw_metadata,
        "required_tags",
        "hourly canonical_regression_metadata",
    )
    stable_canonical_output_fixture_ids: list[str] = _require_non_empty_string_list(
        raw_metadata,
        "stable_canonical_output_fixture_ids",
        "hourly canonical_regression_metadata",
    )
    stable_missing_data_guard_fixture_ids: list[str] = _require_non_empty_string_list(
        raw_metadata,
        "stable_missing_data_guard_fixture_ids",
        "hourly canonical_regression_metadata",
    )

    _require_unique_fixture_ids(
        stable_canonical_output_fixture_ids,
        "hourly canonical_regression_metadata.stable_canonical_output_fixture_ids",
    )
    _require_unique_fixture_ids(
        stable_missing_data_guard_fixture_ids,
        "hourly canonical_regression_metadata.stable_missing_data_guard_fixture_ids",
    )

    observed_tags: set[str] = {
        tag
        for fixture_case in canonical_output_cases
        for tag in fixture_case["tags"]
    }
    observed_tags.update(
        tag
        for fixture_case in missing_data_guard_cases
        for tag in fixture_case["tags"]
    )

    if not set(required_tags).issubset(observed_tags):
        raise ValueError("hourly canonical_regression_metadata.required_tags must be present in fixture cases")

    observed_canonical_output_fixture_ids: set[str] = {
        fixture_case["fixture_id"] for fixture_case in canonical_output_cases
    }
    observed_missing_data_guard_fixture_ids: set[str] = {
        fixture_case["fixture_id"] for fixture_case in missing_data_guard_cases
    }

    if observed_canonical_output_fixture_ids != set(stable_canonical_output_fixture_ids):
        raise ValueError(
            "hourly canonical_regression_metadata.stable_canonical_output_fixture_ids does not match observed fixture ids",
        )
    if observed_missing_data_guard_fixture_ids != set(stable_missing_data_guard_fixture_ids):
        raise ValueError(
            "hourly canonical_regression_metadata.stable_missing_data_guard_fixture_ids does not match observed fixture ids",
        )

    return {
        "required_tags": required_tags,
        "stable_canonical_output_fixture_ids": stable_canonical_output_fixture_ids,
        "stable_missing_data_guard_fixture_ids": stable_missing_data_guard_fixture_ids,
    }


def _validate_hourly_missing_data_expected_outcome(expected_outcome: JsonObject) -> None:
    outcome_kind: str = _require_non_empty_string(expected_outcome, "outcome_kind", "expected_outcome")

    match outcome_kind:
        case "success":
            _require_float_list(expected_outcome, "expected_s_h", "expected_outcome")
            _require_non_negative_int(expected_outcome, "expected_sparse_interval_count", "expected_outcome")
            _require_non_negative_int(expected_outcome, "expected_interpolated_interval_count", "expected_outcome")
            _require_non_negative_int(expected_outcome, "expected_fallback_interval_count", "expected_outcome")
        case "failure":
            _require_non_empty_string(expected_outcome, "expected_error_variant", "expected_outcome")
            _require_non_empty_string(expected_outcome, "expected_error_message_substring", "expected_outcome")
        case _:
            raise ValueError(f"unsupported expected_outcome.outcome_kind: {outcome_kind}")


def _validate_hourly_seasonality_source(raw_source: JsonObject, context: str) -> None:
    source_kind: str = _require_non_empty_string(raw_source, "source_kind", context)

    match source_kind:
        case "explicit":
            _require_float_list(raw_source, "hourly_factors", context)
            _require_non_empty_string(raw_source, "materialization_method", context)
        case "materialized":
            if "source_timestamp_utc" in raw_source:
                raise ValueError(
                    f"{context} contains unsupported field 'source_timestamp_utc'; use 'source_timestamps_utc'",
                )

            _require_non_empty_string_list(raw_source, "source_timestamps_utc", context)
            _require_float_list(raw_source, "source_seasonality_factors", context)
            _require_non_empty_string(raw_source, "interpolation_method", context)
            _require_non_empty_string(raw_source, "aggregation_method", context)
            _require_non_empty_string(raw_source, "sparse_fallback_policy", context)
            _require_non_negative_int(raw_source, "sparse_hour_minimum_observation_count", context)
        case _:
            raise ValueError(f"{context} contains unsupported source_kind value: {source_kind}")


def _validate_hourly_news_intensity_source(raw_source: JsonObject, context: str) -> None:
    source_kind: str = _require_non_empty_string(raw_source, "source_kind", context)

    match source_kind:
        case "explicit":
            _require_float_list(raw_source, "hourly_factors", context)
            _require_non_empty_string(raw_source, "materialization_method", context)
        case "materialized":
            _require_number(raw_source, "closed_window_baseline_intensity", context)

            overrides: JsonObject = _require_json_object_field(
                raw_source,
                "open_window_override_by_interval_index",
                context,
            )

            for raw_interval_index, raw_override in overrides.items():
                if not isinstance(raw_interval_index, str) or not raw_interval_index.strip():
                    raise ValueError(f"{context}.open_window_override_by_interval_index keys must be non-empty strings")
                if not raw_interval_index.isdigit():
                    raise ValueError(
                        f"{context}.open_window_override_by_interval_index key '{raw_interval_index}' must be numeric",
                    )
                _require_float_like(raw_override, f"{context}.open_window_override_by_interval_index")
        case _:
            raise ValueError(f"{context} contains unsupported source_kind value: {source_kind}")


def _extract_registry_exchange_codes(calendar_registry: JsonObject) -> set[str]:
    exchanges: JsonArray = _require_json_array_field(calendar_registry, "exchanges", "calendar_registry")
    if not exchanges:
        raise ValueError("calendar registry must contain at least one exchange")

    exchange_codes: set[str] = set()
    for exchange_value in exchanges:
        exchange_map: JsonObject = _require_json_object(exchange_value, "calendar_registry exchange")
        exchange_code: str = _require_non_empty_string(exchange_map, "exchange_code", "calendar_registry exchange")
        exchange_codes.add(exchange_code)

    return exchange_codes


def _read_json_document(document_path: Path) -> object:
    return json.loads(document_path.read_text(encoding="utf-8"))


def _require_json_object(value: object, context: str) -> JsonObject:
    if not isinstance(value, dict):
        raise ValueError(f"expected JSON object for {context}")

    return cast(JsonObject, value)


def _require_json_object_field(parent: JsonObject, field_name: str, context: str) -> JsonObject:
    field_value: object = parent.get(field_name)
    if field_value is None:
        raise ValueError(f"missing required field '{field_name}' in {context}")

    return _require_json_object(field_value, f"{context}.{field_name}")


def _require_json_array_field(parent: JsonObject, field_name: str, context: str) -> JsonArray:
    field_value: object = parent.get(field_name)
    if field_value is None:
        raise ValueError(f"missing required field '{field_name}' in {context}")

    if not isinstance(field_value, list):
        raise ValueError(f"field '{field_name}' in {context} must be an array")

    return cast(JsonArray, field_value)


def _require_non_empty_string(parent: JsonObject, field_name: str, context: str) -> str:
    field_value: object = parent.get(field_name)
    if field_value is None:
        raise ValueError(f"missing required field '{field_name}' in {context}")

    if not isinstance(field_value, str):
        raise ValueError(f"field '{field_name}' in {context} must be a string")

    normalized_value: str = field_value.strip()
    if not normalized_value:
        raise ValueError(f"field '{field_name}' in {context} must be non-empty")

    return normalized_value


def _require_non_empty_string_list(parent: JsonObject, field_name: str, context: str) -> list[str]:
    field_values: JsonArray = _require_json_array_field(parent, field_name, context)
    if not field_values:
        raise ValueError(f"field '{field_name}' in {context} must include at least one value")

    return _normalize_non_empty_string_list(field_values, context)


def _normalize_non_empty_string_list(raw_values: JsonArray, context: str) -> list[str]:
    validated_values: list[str] = []
    for raw_value in raw_values:
        if not isinstance(raw_value, str):
            raise ValueError(f"{context} must only contain strings")

        normalized_value: str = raw_value.strip()
        if not normalized_value:
            raise ValueError(f"{context} cannot contain empty strings")

        validated_values.append(normalized_value)

    return validated_values


def _require_string_list_allow_empty(parent: JsonObject, field_name: str, context: str) -> list[str]:
    field_values: JsonArray = _require_json_array_field(parent, field_name, context)
    return _normalize_non_empty_string_list(field_values, f"field '{field_name}' in {context}")


def _require_float_list(parent: JsonObject, field_name: str, context: str) -> list[float]:
    field_values: JsonArray = _require_json_array_field(parent, field_name, context)
    if not field_values:
        raise ValueError(f"field '{field_name}' in {context} must include at least one value")

    return _normalize_float_list(field_values, f"field '{field_name}' in {context}")


def _require_float_list_allow_empty(parent: JsonObject, field_name: str, context: str) -> list[float]:
    field_values: JsonArray = _require_json_array_field(parent, field_name, context)
    return _normalize_float_list(field_values, f"field '{field_name}' in {context}")


def _normalize_float_list(raw_values: JsonArray, context: str) -> list[float]:
    validated_values: list[float] = []
    for raw_value in raw_values:
        validated_values.append(_require_float_like(raw_value, context))

    return validated_values


def _require_non_negative_float(parent: JsonObject, field_name: str, context: str) -> float:
    raw_value: object = parent.get(field_name)
    if raw_value is None:
        raise ValueError(f"missing required field '{field_name}' in {context}")

    normalized_value: float = _require_float_like(raw_value, f"field '{field_name}' in {context}")
    if normalized_value < 0.0:
        raise ValueError(f"field '{field_name}' in {context} must be non-negative")

    return normalized_value


def _require_non_negative_int(parent: JsonObject, field_name: str, context: str) -> int:
    raw_value: object = parent.get(field_name)
    if raw_value is None:
        raise ValueError(f"missing required field '{field_name}' in {context}")

    if not isinstance(raw_value, int) or isinstance(raw_value, bool):
        raise ValueError(f"field '{field_name}' in {context} must be an integer")

    if raw_value < 0:
        raise ValueError(f"field '{field_name}' in {context} must be non-negative")

    return raw_value


def _require_number(parent: JsonObject, field_name: str, context: str) -> float:
    raw_value: object = parent.get(field_name)
    if raw_value is None:
        raise ValueError(f"missing required field '{field_name}' in {context}")

    return _require_float_like(raw_value, f"field '{field_name}' in {context}")


def _require_float_like(raw_value: object, context: str) -> float:
    if not isinstance(raw_value, int | float) or isinstance(raw_value, bool):
        raise ValueError(f"{context} must be a finite number")

    normalized_value: float = float(raw_value)
    if not math.isfinite(normalized_value):
        raise ValueError(f"{context} must be a finite number")

    return normalized_value


def _require_unique_fixture_ids(fixture_ids: list[str], context: str) -> None:
    observed_fixture_ids: set[str] = set()
    for fixture_id in fixture_ids:
        if fixture_id in observed_fixture_ids:
            raise ValueError(f"duplicate fixture_id in {context}: {fixture_id}")

        observed_fixture_ids.add(fixture_id)
