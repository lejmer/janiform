import json
from pathlib import Path
from typing import TypedDict, cast

VALIDATION_FIXTURE_MANIFEST_SCHEMA_VERSION: str = "validation.fixture.manifest.v1"
CALENDAR_REGRESSION_FIXTURE_SCHEMA_VERSION: str = "calendar.service.regression.v1"
CALENDAR_REGRESSION_FIXTURE_SET_ID: str = "calendar_service_regression_matrix_v1"
MANIFEST_DETERMINISM_ORDER_POLICY: str = "lexicographic_by_fixture_id.v1"
CONTENT_HASH_ALGORITHM_POLICY: str = "fnv1a_64_hex_utf8.v1"
CALENDAR_CASE_ORDER_POLICY: str = "preserve_declared_case_order.v1"
CALENDAR_CONTENT_HASH_SCOPE_POLICY: str = "document_payload_bytes.v1"
CALENDAR_SCHEMA_FAMILY: str = "validation.calendar.service.regression"
CALENDAR_SCHEMA_GENERATION: str = "v0.x"
CALENDAR_FORWARD_EXTENSION_POLICY: str = "additive_fields_only.v1"
MANIFEST_SCHEMA_FAMILY: str = "validation.fixture.manifest"
MANIFEST_V0_UPDATE_POLICY: str = "additive_only_and_major_on_breaking_change.v1"


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


class CalendarRegressionFixtureDocument(TypedDict):
    schema_version: str
    fixture_set_id: str
    schema_metadata: CalendarRegressionSchemaMetadata
    determinism_metadata: CalendarRegressionDeterminismMetadata
    calendar_registry: dict[str, object]
    cases: list[CalendarRegressionCase]


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

    return {
        "schema_version": schema_version,
        "fixture_set_id": fixture_set_id,
        "schema_metadata": cast(CalendarRegressionSchemaMetadata, schema_metadata),
        "determinism_metadata": cast(CalendarRegressionDeterminismMetadata, determinism_metadata),
        "calendar_registry": cast(dict[str, object], calendar_registry),
        "cases": validated_cases,
    }


def load_calendar_regression_fixture_document(
    fixture_payload_bytes: bytes,
) -> CalendarRegressionFixtureDocument:
    payload_value: object = json.loads(fixture_payload_bytes.decode("utf-8"))
    return validate_calendar_regression_fixture_document(payload_value)


def _validate_manifest_fixture_entry(raw_entry: JsonObject) -> ValidationFixtureManifestEntry:
    fixture_id: str = _require_non_empty_string(raw_entry, "fixture_id", "manifest fixture entry")
    domain: str = _require_non_empty_string(raw_entry, "domain", "manifest fixture entry")
    if domain not in {"calendar", "seasonality"}:
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

    validated_values: list[str] = []
    for field_value in field_values:
        if not isinstance(field_value, str):
            raise ValueError(f"field '{field_name}' in {context} must only contain strings")

        normalized_value: str = field_value.strip()
        if not normalized_value:
            raise ValueError(f"field '{field_name}' in {context} cannot contain empty strings")

        validated_values.append(normalized_value)

    return validated_values
