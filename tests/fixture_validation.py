import json
import math
from datetime import UTC, datetime
from pathlib import Path
from typing import TypedDict, cast

from janiform.calendar import CalendarRegistry

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
HOURLY_FACTOR_THRESHOLD_CONFIGURATION_SCHEMA_VERSION: str = "validation.hourly_factor.thresholds.v1"
STATE_CLASSIFICATION_STRICTNESS_POLICY: str = "exact_match_required.v1"
OPEN_CLOSED_GAP_BEHAVIOR_FIXTURE_SCHEMA_VERSION: str = "gap.behavior.open_closed.scenario_fixture.v1"
OPEN_CLOSED_GAP_BEHAVIOR_FIXTURE_SET_ID: str = "open_closed_gap_behavior_scenarios_v1"
OPEN_CLOSED_GAP_BEHAVIOR_SCHEMA_FAMILY: str = "validation.gap_behavior.open_closed"
OPEN_CLOSED_GAP_BEHAVIOR_SCHEMA_GENERATION: str = "v0.x"
OPEN_CLOSED_GAP_BEHAVIOR_FORWARD_EXTENSION_POLICY: str = "additive_fields_only.v1"
OPEN_CLOSED_GAP_BEHAVIOR_CASE_ORDER_POLICY: str = "preserve_declared_case_order.v1"
OPEN_CLOSED_GAP_BEHAVIOR_EVENT_ORDER_POLICY: str = "preserve_declared_event_order.v1"
OPEN_CLOSED_GAP_BEHAVIOR_CONTENT_HASH_SCOPE_POLICY: str = "document_payload_bytes.v1"
OPEN_CLOSED_GAP_BEHAVIOR_OUTPUT_SCHEMA_VERSION: str = (
    "validation.gap_behavior.open_closed.case_output.v1"
)
SEEDED_TICK_SIMULATION_PARITY_FIXTURE_SCHEMA_VERSION: str = (
    "simulation.seeded_tick.parity_fixture.v1"
)
SEEDED_TICK_SIMULATION_PARITY_FIXTURE_SET_ID: str = "seeded_tick_simulation_parity_fixture_v1"
SEEDED_TICK_SIMULATION_CASE_ORDER_POLICY: str = "preserve_declared_case_order.v1"
SEEDED_TICK_SIMULATION_CONTENT_HASH_SCOPE_POLICY: str = "document_payload_bytes.v1"
SEEDED_TICK_SIMULATION_EVENT_ORDERING_POLICY: str = (
    "timestamp_then_endogenous_before_exogenous_then_buy_before_sell_then_generation_order.v1"
)
SUPPORTED_MANIFEST_DOMAINS: set[str] = {"calendar", "seasonality", "hourly_factors", "gap_behavior"}


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


class HourlyFactorRegressionToleranceConfig(TypedDict):
    s_h_absolute_tolerance: float
    nu_h_absolute_tolerance: float


class HourlyFactorRegressionFactorRange(TypedDict):
    s_h_min: float
    s_h_max: float
    nu_h_min: float
    nu_h_max: float


class HourlyFactorRegressionThresholdChangeLogEntry(TypedDict):
    policy_version: str
    recorded_at_utc: str
    rationale: str


class HourlyFactorRegressionThresholdConfiguration(TypedDict):
    schema_version: str
    policy_version: str
    state_classification_strictness: str
    tolerance_config: HourlyFactorRegressionToleranceConfig
    factor_range_by_fixture_class: dict[str, HourlyFactorRegressionFactorRange]
    change_log: list[HourlyFactorRegressionThresholdChangeLogEntry]


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
    threshold_configuration: HourlyFactorRegressionThresholdConfiguration
    canonical_regression_metadata: HourlyFactorRegressionCanonicalMetadata
    canonical_output_cases: list[HourlyFactorRegressionCanonicalOutputCase]
    missing_data_guard_cases: list[HourlyFactorRegressionMissingDataGuardCase]


class OpenClosedGapBehaviorSchemaMetadata(TypedDict):
    schema_family: str
    schema_generation: str
    forward_extension_policy: str


class OpenClosedGapBehaviorDeterminismMetadata(TypedDict):
    fixture_loading_order: str
    event_loading_order: str
    content_hash_scope: str
    content_hash_algorithm: str


class OpenClosedGapBehaviorOutputSchemaMetadata(TypedDict):
    schema_version: str
    required_case_output_fields: list[str]
    required_summary_fields: list[str]


class OpenClosedGapBehaviorCanonicalMetadata(TypedDict):
    required_tags: list[str]
    stable_fixture_ids_by_tag: dict[str, list[str]]


class OpenClosedGapBehaviorAccumulatorState(TypedDict):
    j_t: float
    g_t: float


class OpenClosedGapBehaviorInjectedEvent(TypedDict):
    event_id: str
    timestamp_utc: str
    shared_severity_mark: float
    coupled_gap_jump_mark: float


class OpenClosedGapBehaviorEventOutcome(TypedDict):
    event_id: str
    expected_market_state: str
    expected_j_t_increment: float
    expected_g_t_increment: float
    expected_direct_gap_applied: bool


class OpenClosedGapBehaviorSummary(TypedDict):
    expected_open_event_count: int
    expected_closed_event_count: int
    expected_total_j_t_increment: float
    expected_total_g_t_increment: float
    expected_open_window_direct_gap_increment_count: int


class OpenClosedGapBehaviorExpectedOutcome(TypedDict):
    event_outcomes: list[OpenClosedGapBehaviorEventOutcome]
    summary: OpenClosedGapBehaviorSummary


class OpenClosedGapBehaviorCase(TypedDict):
    fixture_id: str
    exchange_code: str
    calendar_fixture_set_id: str
    calendar_state_fixture_references: list[str]
    range_start_utc: str
    range_end_utc: str
    initial_accumulator_state: OpenClosedGapBehaviorAccumulatorState
    injected_events: list[OpenClosedGapBehaviorInjectedEvent]
    expected_outcome: OpenClosedGapBehaviorExpectedOutcome
    tags: list[str]
    description: str


class OpenClosedGapBehaviorFixtureDocument(TypedDict):
    schema_version: str
    fixture_set_id: str
    fixture_set_version: str
    schema_metadata: OpenClosedGapBehaviorSchemaMetadata
    determinism_metadata: OpenClosedGapBehaviorDeterminismMetadata
    output_schema_metadata: OpenClosedGapBehaviorOutputSchemaMetadata
    canonical_regression_metadata: OpenClosedGapBehaviorCanonicalMetadata
    cases: list[OpenClosedGapBehaviorCase]


class SeededTickSimulationParityDeterminismMetadata(TypedDict):
    fixture_loading_order: str
    event_ordering_policy: str
    content_hash_scope: str
    content_hash_algorithm: str


class SeededTickSimulationParityCanonicalMetadata(TypedDict):
    required_tags: list[str]
    stable_fixture_ids: list[str]


class SeededTickSimulationParityKernelLoadings(TypedDict):
    plus_plus: float
    plus_minus: float
    minus_plus: float
    minus_minus: float


class SeededTickSimulationParityKernelParameters(TypedDict):
    lambda_bar: float
    beta: float
    alpha: SeededTickSimulationParityKernelLoadings
    rho: SeededTickSimulationParityKernelLoadings


class SeededTickSimulationParityInitialExcitationState(TypedDict):
    y_plus_plus: float
    y_plus_minus: float
    y_minus_plus: float
    y_minus_minus: float


class SeededTickSimulationParityCase(TypedDict):
    fixture_id: str
    seed: int
    exchange_code: str
    range_start_utc: str
    range_end_utc: str
    horizon_interval_count: int
    kernel_parameters: SeededTickSimulationParityKernelParameters
    initial_excitation_state: SeededTickSimulationParityInitialExcitationState
    seasonality_hourly_factors: list[float]
    news_intensity_hourly_factors: list[float]
    expected_timestamps_utc_nanos: list[int]
    expected_side_sequence: list[str]
    expected_total_event_count: int
    expected_interval_event_counts: list[int]
    expected_interval_buy_counts: list[int]
    expected_interval_sell_counts: list[int]
    expected_min_positive_timestamp_gap_nanos: int
    expected_has_non_grid_arrival: bool
    tags: list[str]
    description: str


class SeededTickSimulationParityFixtureDocument(TypedDict):
    schema_version: str
    fixture_set_id: str
    fixture_set_version: str
    determinism_metadata: SeededTickSimulationParityDeterminismMetadata
    canonical_regression_metadata: SeededTickSimulationParityCanonicalMetadata
    cases: list[SeededTickSimulationParityCase]


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

    threshold_configuration = _validate_hourly_factor_threshold_configuration(
        _require_json_object_field(
            document_map,
            "threshold_configuration",
            "hourly factor fixture",
        ),
        "hourly factor fixture.threshold_configuration",
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
        "threshold_configuration": threshold_configuration,
        "canonical_regression_metadata": canonical_regression_metadata,
        "canonical_output_cases": canonical_output_cases,
        "missing_data_guard_cases": missing_data_guard_cases,
    }


def load_hourly_factor_regression_fixture_document(
    fixture_payload_bytes: bytes,
) -> HourlyFactorRegressionFixtureDocument:
    payload_value: object = json.loads(fixture_payload_bytes.decode("utf-8"))
    return validate_hourly_factor_regression_fixture_document(payload_value)


def validate_open_closed_gap_behavior_fixture_document(
    raw_document: object,
) -> OpenClosedGapBehaviorFixtureDocument:
    document_map: JsonObject = _require_json_object(raw_document, "open and closed gap behavior fixture")
    schema_version: str = _require_non_empty_string(document_map, "schema_version", "open and closed gap fixture")
    if schema_version != OPEN_CLOSED_GAP_BEHAVIOR_FIXTURE_SCHEMA_VERSION:
        raise ValueError(f"unsupported open and closed gap fixture schema version: {schema_version}")

    fixture_set_id: str = _require_non_empty_string(document_map, "fixture_set_id", "open and closed gap fixture")
    if fixture_set_id != OPEN_CLOSED_GAP_BEHAVIOR_FIXTURE_SET_ID:
        raise ValueError(f"unexpected open and closed gap fixture set id: {fixture_set_id}")

    fixture_set_version: str = _require_non_empty_string(
        document_map,
        "fixture_set_version",
        "open and closed gap fixture",
    )

    schema_metadata: JsonObject = _require_json_object_field(document_map, "schema_metadata", "open and closed gap")
    schema_family: str = _require_non_empty_string(schema_metadata, "schema_family", "open and closed gap schema")
    if schema_family != OPEN_CLOSED_GAP_BEHAVIOR_SCHEMA_FAMILY:
        raise ValueError(f"unexpected open and closed gap schema family: {schema_family}")

    schema_generation: str = _require_non_empty_string(
        schema_metadata,
        "schema_generation",
        "open and closed gap schema",
    )
    if schema_generation != OPEN_CLOSED_GAP_BEHAVIOR_SCHEMA_GENERATION:
        raise ValueError(f"unexpected open and closed gap schema generation: {schema_generation}")

    forward_extension_policy: str = _require_non_empty_string(
        schema_metadata,
        "forward_extension_policy",
        "open and closed gap schema",
    )
    if forward_extension_policy != OPEN_CLOSED_GAP_BEHAVIOR_FORWARD_EXTENSION_POLICY:
        raise ValueError(
            f"unexpected open and closed gap forward extension policy: {forward_extension_policy}",
        )

    determinism_metadata: JsonObject = _require_json_object_field(
        document_map,
        "determinism_metadata",
        "open and closed gap fixture",
    )
    fixture_loading_order: str = _require_non_empty_string(
        determinism_metadata,
        "fixture_loading_order",
        "open and closed gap determinism metadata",
    )
    if fixture_loading_order != OPEN_CLOSED_GAP_BEHAVIOR_CASE_ORDER_POLICY:
        raise ValueError(f"unexpected open and closed gap fixture_loading_order: {fixture_loading_order}")

    event_loading_order: str = _require_non_empty_string(
        determinism_metadata,
        "event_loading_order",
        "open and closed gap determinism metadata",
    )
    if event_loading_order != OPEN_CLOSED_GAP_BEHAVIOR_EVENT_ORDER_POLICY:
        raise ValueError(f"unexpected open and closed gap event_loading_order: {event_loading_order}")

    content_hash_scope: str = _require_non_empty_string(
        determinism_metadata,
        "content_hash_scope",
        "open and closed gap determinism metadata",
    )
    if content_hash_scope != OPEN_CLOSED_GAP_BEHAVIOR_CONTENT_HASH_SCOPE_POLICY:
        raise ValueError(f"unexpected open and closed gap content_hash_scope: {content_hash_scope}")

    content_hash_algorithm: str = _require_non_empty_string(
        determinism_metadata,
        "content_hash_algorithm",
        "open and closed gap determinism metadata",
    )
    if content_hash_algorithm != CONTENT_HASH_ALGORITHM_POLICY:
        raise ValueError(f"unexpected open and closed gap content_hash_algorithm: {content_hash_algorithm}")

    output_schema_metadata = _validate_open_closed_gap_behavior_output_schema_metadata(
        _require_json_object_field(
            document_map,
            "output_schema_metadata",
            "open and closed gap fixture",
        ),
    )

    canonical_metadata = _validate_open_closed_gap_behavior_canonical_metadata(
        _require_json_object_field(
            document_map,
            "canonical_regression_metadata",
            "open and closed gap fixture",
        ),
    )

    cases_raw: JsonArray = _require_json_array_field(document_map, "cases", "open and closed gap fixture")
    if not cases_raw:
        raise ValueError("open and closed gap fixture requires at least one case")

    validated_cases: list[OpenClosedGapBehaviorCase] = [
        _validate_open_closed_gap_behavior_case(
            _require_json_object(raw_case, "open and closed gap case"),
            f"open and closed gap case[{case_index}]",
        )
        for case_index, raw_case in enumerate(cases_raw)
    ]

    _require_unique_fixture_ids(
        [fixture_case["fixture_id"] for fixture_case in validated_cases],
        "open and closed gap fixture cases",
    )

    observed_tags: set[str] = {tag for fixture_case in validated_cases for tag in fixture_case["tags"]}
    required_tags = set(canonical_metadata["required_tags"])
    if not required_tags.issubset(observed_tags):
        raise ValueError(
            "open and closed gap canonical_regression_metadata.required_tags must be present in case tags",
        )

    for tag, stable_fixture_ids in canonical_metadata["stable_fixture_ids_by_tag"].items():
        observed_fixture_ids_for_tag: set[str] = {
            fixture_case["fixture_id"]
            for fixture_case in validated_cases
            if tag in fixture_case["tags"]
        }
        if observed_fixture_ids_for_tag != set(stable_fixture_ids):
            raise ValueError(
                "open and closed gap canonical_regression_metadata.stable_fixture_ids_by_tag does not match "
                f"observed fixture ids for tag '{tag}'",
            )

    return {
        "schema_version": schema_version,
        "fixture_set_id": fixture_set_id,
        "fixture_set_version": fixture_set_version,
        "schema_metadata": cast(OpenClosedGapBehaviorSchemaMetadata, schema_metadata),
        "determinism_metadata": cast(OpenClosedGapBehaviorDeterminismMetadata, determinism_metadata),
        "output_schema_metadata": output_schema_metadata,
        "canonical_regression_metadata": canonical_metadata,
        "cases": validated_cases,
    }


def load_open_closed_gap_behavior_fixture_document(
    fixture_payload_bytes: bytes,
) -> OpenClosedGapBehaviorFixtureDocument:
    payload_value: object = json.loads(fixture_payload_bytes.decode("utf-8"))
    return validate_open_closed_gap_behavior_fixture_document(payload_value)


def validate_open_closed_gap_behavior_calendar_integration(
    open_closed_gap_behavior_fixture_document: OpenClosedGapBehaviorFixtureDocument,
    calendar_regression_fixture_document: CalendarRegressionFixtureDocument,
) -> None:
    expected_calendar_fixture_set_id = calendar_regression_fixture_document["fixture_set_id"]
    calendar_fixture_ids: set[str] = {
        calendar_case["fixture_id"] for calendar_case in calendar_regression_fixture_document["cases"]
    }
    calendar_registry_json = json.dumps(calendar_regression_fixture_document["calendar_registry"])
    calendar_registry = CalendarRegistry(calendar_registry_json)

    observed_window_tags: set[str] = set()
    required_window_tags: set[str] = {"intraday", "overnight", "weekend"}
    for case_index, fixture_case in enumerate(open_closed_gap_behavior_fixture_document["cases"]):
        case_context = f"open and closed gap case[{case_index}]"

        if fixture_case["calendar_fixture_set_id"] != expected_calendar_fixture_set_id:
            raise ValueError(
                f"{case_context}.calendar_fixture_set_id must match loaded calendar fixture set id "
                f"{expected_calendar_fixture_set_id}, found {fixture_case['calendar_fixture_set_id']}",
            )

        observed_window_tags.update(
            case_tag
            for case_tag in fixture_case["tags"]
            if case_tag in required_window_tags
        )
        for reference_index, referenced_fixture_id in enumerate(fixture_case["calendar_state_fixture_references"]):
            if referenced_fixture_id not in calendar_fixture_ids:
                raise ValueError(
                    f"{case_context}.calendar_state_fixture_references[{reference_index}] references "
                    f"missing calendar fixture id: {referenced_fixture_id}",
                )

        expected_market_state_by_event_id: dict[str, str] = {
            expected_event["event_id"]: expected_event["expected_market_state"]
            for expected_event in fixture_case["expected_outcome"]["event_outcomes"]
        }
        for event_index, injected_event in enumerate(fixture_case["injected_events"]):
            event_context = f"{case_context}.injected_events[{event_index}]"
            timestamp_utc = _parse_utc_timestamp(injected_event["timestamp_utc"])
            resolved_market_state = calendar_registry.resolve_market_state(
                fixture_case["exchange_code"],
                timestamp_utc,
            )
            expected_market_state = expected_market_state_by_event_id[injected_event["event_id"]]
            if resolved_market_state.value != expected_market_state:
                raise ValueError(
                    f"{event_context} expected_market_state={expected_market_state} but "
                    f"calendar resolved {resolved_market_state.value}",
                )

    missing_window_tags = required_window_tags - observed_window_tags
    if missing_window_tags:
        sorted_missing_tags = ", ".join(sorted(missing_window_tags))
        raise ValueError(
            "open and closed gap fixture calendar_state_fixture_references must cover intraday, "
            f"overnight, and weekend windows, missing tags: {sorted_missing_tags}",
        )


def validate_seeded_tick_simulation_parity_fixture_document(
    raw_document: object,
) -> SeededTickSimulationParityFixtureDocument:
    document_map: JsonObject = _require_json_object(raw_document, "seeded tick simulation parity fixture")
    schema_version: str = _require_non_empty_string(
        document_map,
        "schema_version",
        "seeded tick simulation parity fixture",
    )
    if schema_version != SEEDED_TICK_SIMULATION_PARITY_FIXTURE_SCHEMA_VERSION:
        raise ValueError(f"unsupported seeded tick simulation parity schema version: {schema_version}")

    fixture_set_id: str = _require_non_empty_string(
        document_map,
        "fixture_set_id",
        "seeded tick simulation parity fixture",
    )
    if fixture_set_id != SEEDED_TICK_SIMULATION_PARITY_FIXTURE_SET_ID:
        raise ValueError(f"unexpected seeded tick simulation parity fixture set id: {fixture_set_id}")

    fixture_set_version: str = _require_non_empty_string(
        document_map,
        "fixture_set_version",
        "seeded tick simulation parity fixture",
    )

    determinism_metadata: JsonObject = _require_json_object_field(
        document_map,
        "determinism_metadata",
        "seeded tick simulation parity fixture",
    )
    fixture_loading_order: str = _require_non_empty_string(
        determinism_metadata,
        "fixture_loading_order",
        "seeded tick simulation parity fixture determinism_metadata",
    )
    if fixture_loading_order != SEEDED_TICK_SIMULATION_CASE_ORDER_POLICY:
        raise ValueError(f"unexpected seeded simulation case order policy: {fixture_loading_order}")

    event_ordering_policy: str = _require_non_empty_string(
        determinism_metadata,
        "event_ordering_policy",
        "seeded tick simulation parity fixture determinism_metadata",
    )
    if event_ordering_policy != SEEDED_TICK_SIMULATION_EVENT_ORDERING_POLICY:
        raise ValueError(f"unexpected seeded simulation event ordering policy: {event_ordering_policy}")

    content_hash_scope: str = _require_non_empty_string(
        determinism_metadata,
        "content_hash_scope",
        "seeded tick simulation parity fixture determinism_metadata",
    )
    if content_hash_scope != SEEDED_TICK_SIMULATION_CONTENT_HASH_SCOPE_POLICY:
        raise ValueError(f"unexpected seeded simulation content hash scope policy: {content_hash_scope}")

    content_hash_algorithm: str = _require_non_empty_string(
        determinism_metadata,
        "content_hash_algorithm",
        "seeded tick simulation parity fixture determinism_metadata",
    )
    if content_hash_algorithm != CONTENT_HASH_ALGORITHM_POLICY:
        raise ValueError(f"unexpected seeded simulation content hash algorithm: {content_hash_algorithm}")

    cases_raw: JsonArray = _require_json_array_field(
        document_map,
        "cases",
        "seeded tick simulation parity fixture",
    )
    if not cases_raw:
        raise ValueError("seeded tick simulation parity fixture must include at least one case")

    validated_cases: list[SeededTickSimulationParityCase] = []
    observed_fixture_ids: set[str] = set()
    observed_tags: set[str] = set()
    for raw_case in cases_raw:
        validated_case = _validate_seeded_tick_simulation_parity_case(
            _require_json_object(raw_case, "seeded tick simulation parity case"),
        )
        fixture_id = validated_case["fixture_id"]
        if fixture_id in observed_fixture_ids:
            raise ValueError(f"duplicate seeded tick simulation parity fixture_id: {fixture_id}")

        observed_fixture_ids.add(fixture_id)
        observed_tags.update(validated_case["tags"])
        validated_cases.append(validated_case)

    canonical_regression_metadata_raw: JsonObject = _require_json_object_field(
        document_map,
        "canonical_regression_metadata",
        "seeded tick simulation parity fixture",
    )
    required_tags: list[str] = _require_non_empty_string_list(
        canonical_regression_metadata_raw,
        "required_tags",
        "seeded tick simulation parity canonical_regression_metadata",
    )
    stable_fixture_ids: list[str] = _require_non_empty_string_list(
        canonical_regression_metadata_raw,
        "stable_fixture_ids",
        "seeded tick simulation parity canonical_regression_metadata",
    )
    _require_unique_fixture_ids(
        stable_fixture_ids,
        "seeded tick simulation parity canonical_regression_metadata.stable_fixture_ids",
    )

    if not set(required_tags).issubset(observed_tags):
        raise ValueError(
            "seeded tick simulation parity canonical_regression_metadata.required_tags must be present in case tags",
        )

    if set(stable_fixture_ids) != observed_fixture_ids:
        raise ValueError(
            "seeded tick simulation parity canonical_regression_metadata.stable_fixture_ids does not match observed case fixture ids",
        )

    return {
        "schema_version": schema_version,
        "fixture_set_id": fixture_set_id,
        "fixture_set_version": fixture_set_version,
        "determinism_metadata": cast(
            SeededTickSimulationParityDeterminismMetadata,
            determinism_metadata,
        ),
        "canonical_regression_metadata": {
            "required_tags": required_tags,
            "stable_fixture_ids": stable_fixture_ids,
        },
        "cases": validated_cases,
    }


def load_seeded_tick_simulation_parity_fixture_document(
    fixture_payload_bytes: bytes,
) -> SeededTickSimulationParityFixtureDocument:
    payload_value: object = json.loads(fixture_payload_bytes.decode("utf-8"))
    return validate_seeded_tick_simulation_parity_fixture_document(payload_value)


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


def _validate_hourly_factor_threshold_configuration(
    raw_configuration: JsonObject,
    context: str,
) -> HourlyFactorRegressionThresholdConfiguration:
    schema_version: str = _require_non_empty_string(raw_configuration, "schema_version", context)
    if schema_version != HOURLY_FACTOR_THRESHOLD_CONFIGURATION_SCHEMA_VERSION:
        raise ValueError(
            f"unsupported threshold configuration schema version: {schema_version}",
        )

    policy_version: str = _require_non_empty_string(raw_configuration, "policy_version", context)
    strictness_policy: str = _require_non_empty_string(
        raw_configuration,
        "state_classification_strictness",
        context,
    )
    if strictness_policy != STATE_CLASSIFICATION_STRICTNESS_POLICY:
        raise ValueError(f"unsupported state classification strictness policy: {strictness_policy}")

    tolerance_config_raw: JsonObject = _require_json_object_field(
        raw_configuration,
        "tolerance_config",
        context,
    )
    tolerance_config: HourlyFactorRegressionToleranceConfig = {
        "s_h_absolute_tolerance": _require_non_negative_float(
            tolerance_config_raw,
            "s_h_absolute_tolerance",
            f"{context}.tolerance_config",
        ),
        "nu_h_absolute_tolerance": _require_non_negative_float(
            tolerance_config_raw,
            "nu_h_absolute_tolerance",
            f"{context}.tolerance_config",
        ),
    }

    factor_range_map_raw: JsonObject = _require_json_object_field(
        raw_configuration,
        "factor_range_by_fixture_class",
        context,
    )
    if not factor_range_map_raw:
        raise ValueError(f"{context}.factor_range_by_fixture_class must not be empty")

    factor_range_by_fixture_class: dict[str, HourlyFactorRegressionFactorRange] = {}
    for raw_fixture_class, raw_range_policy in factor_range_map_raw.items():
        if not isinstance(raw_fixture_class, str) or not raw_fixture_class.strip():
            raise ValueError(f"{context}.factor_range_by_fixture_class keys must be non-empty strings")

        fixture_class: str = raw_fixture_class.strip()
        range_policy: JsonObject = _require_json_object(
            raw_range_policy,
            f"{context}.factor_range_by_fixture_class.{fixture_class}",
        )
        factor_range_by_fixture_class[fixture_class] = _validate_hourly_factor_range_policy(
            range_policy,
            fixture_class,
            context,
        )

    change_log_raw: JsonArray = _require_json_array_field(raw_configuration, "change_log", context)
    if not change_log_raw:
        raise ValueError(f"{context}.change_log must include at least one entry")

    change_log: list[HourlyFactorRegressionThresholdChangeLogEntry] = []
    observed_policy_versions: set[str] = set()
    has_current_policy_version = False
    for change_entry_value in change_log_raw:
        change_entry: JsonObject = _require_json_object(change_entry_value, f"{context}.change_log entry")
        change_entry_policy_version: str = _require_non_empty_string(
            change_entry,
            "policy_version",
            f"{context}.change_log entry",
        )
        if change_entry_policy_version in observed_policy_versions:
            raise ValueError(
                f"{context}.change_log contains duplicate policy_version: {change_entry_policy_version}",
            )

        observed_policy_versions.add(change_entry_policy_version)
        has_current_policy_version = has_current_policy_version or (
            change_entry_policy_version == policy_version
        )
        change_log.append(
            {
                "policy_version": change_entry_policy_version,
                "recorded_at_utc": _require_non_empty_string(
                    change_entry,
                    "recorded_at_utc",
                    f"{context}.change_log entry",
                ),
                "rationale": _require_non_empty_string(
                    change_entry,
                    "rationale",
                    f"{context}.change_log entry",
                ),
            },
        )

    if not has_current_policy_version:
        raise ValueError(
            f"{context}.change_log must include an entry for policy_version '{policy_version}'",
        )

    return {
        "schema_version": schema_version,
        "policy_version": policy_version,
        "state_classification_strictness": strictness_policy,
        "tolerance_config": tolerance_config,
        "factor_range_by_fixture_class": factor_range_by_fixture_class,
        "change_log": change_log,
    }


def _validate_hourly_factor_range_policy(
    raw_range_policy: JsonObject,
    fixture_class: str,
    context: str,
) -> HourlyFactorRegressionFactorRange:
    s_h_min: float = _require_non_negative_float(
        raw_range_policy,
        "s_h_min",
        f"{context}.factor_range_by_fixture_class.{fixture_class}",
    )
    s_h_max: float = _require_non_negative_float(
        raw_range_policy,
        "s_h_max",
        f"{context}.factor_range_by_fixture_class.{fixture_class}",
    )
    nu_h_min: float = _require_non_negative_float(
        raw_range_policy,
        "nu_h_min",
        f"{context}.factor_range_by_fixture_class.{fixture_class}",
    )
    nu_h_max: float = _require_non_negative_float(
        raw_range_policy,
        "nu_h_max",
        f"{context}.factor_range_by_fixture_class.{fixture_class}",
    )

    if s_h_min > s_h_max:
        raise ValueError(
            f"{context}.factor_range_by_fixture_class.{fixture_class} has s_h_min greater than s_h_max",
        )
    if nu_h_min > nu_h_max:
        raise ValueError(
            f"{context}.factor_range_by_fixture_class.{fixture_class} has nu_h_min greater than nu_h_max",
        )

    return {
        "s_h_min": s_h_min,
        "s_h_max": s_h_max,
        "nu_h_min": nu_h_min,
        "nu_h_max": nu_h_max,
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


def _validate_open_closed_gap_behavior_output_schema_metadata(
    raw_metadata: JsonObject,
) -> OpenClosedGapBehaviorOutputSchemaMetadata:
    schema_version: str = _require_non_empty_string(
        raw_metadata,
        "schema_version",
        "open and closed gap output_schema_metadata",
    )
    if schema_version != OPEN_CLOSED_GAP_BEHAVIOR_OUTPUT_SCHEMA_VERSION:
        raise ValueError(f"unexpected open and closed gap output schema version: {schema_version}")

    required_case_output_fields: list[str] = _require_non_empty_string_list(
        raw_metadata,
        "required_case_output_fields",
        "open and closed gap output_schema_metadata",
    )
    required_summary_fields: list[str] = _require_non_empty_string_list(
        raw_metadata,
        "required_summary_fields",
        "open and closed gap output_schema_metadata",
    )

    return {
        "schema_version": schema_version,
        "required_case_output_fields": required_case_output_fields,
        "required_summary_fields": required_summary_fields,
    }


def _validate_open_closed_gap_behavior_canonical_metadata(
    raw_metadata: JsonObject,
) -> OpenClosedGapBehaviorCanonicalMetadata:
    required_tags: list[str] = _require_non_empty_string_list(
        raw_metadata,
        "required_tags",
        "open and closed gap canonical_regression_metadata",
    )
    _require_unique_fixture_ids(required_tags, "open and closed gap canonical_regression_metadata.required_tags")

    stable_fixture_ids_by_tag_raw: JsonObject = _require_json_object_field(
        raw_metadata,
        "stable_fixture_ids_by_tag",
        "open and closed gap canonical_regression_metadata",
    )
    if not stable_fixture_ids_by_tag_raw:
        raise ValueError("open and closed gap canonical_regression_metadata.stable_fixture_ids_by_tag must not be empty")

    stable_fixture_ids_by_tag: dict[str, list[str]] = {}
    for raw_tag, raw_fixture_ids in stable_fixture_ids_by_tag_raw.items():
        if not isinstance(raw_tag, str) or not raw_tag.strip():
            raise ValueError(
                "open and closed gap canonical_regression_metadata.stable_fixture_ids_by_tag keys must be non-empty strings",
            )
        if not isinstance(raw_fixture_ids, list):
            raise ValueError(
                "open and closed gap canonical_regression_metadata.stable_fixture_ids_by_tag values must be string arrays",
            )

        tag = raw_tag.strip()
        normalized_fixture_ids = _normalize_non_empty_string_list(
            raw_fixture_ids,
            "open and closed gap canonical_regression_metadata.stable_fixture_ids_by_tag values",
        )
        _require_unique_fixture_ids(
            normalized_fixture_ids,
            f"open and closed gap canonical_regression_metadata.stable_fixture_ids_by_tag.{tag}",
        )
        stable_fixture_ids_by_tag[tag] = normalized_fixture_ids

    return {
        "required_tags": required_tags,
        "stable_fixture_ids_by_tag": stable_fixture_ids_by_tag,
    }


def _validate_open_closed_gap_behavior_case(
    raw_case: JsonObject,
    context: str,
) -> OpenClosedGapBehaviorCase:
    fixture_id: str = _require_non_empty_string(raw_case, "fixture_id", context)
    exchange_code: str = _require_non_empty_string(raw_case, "exchange_code", context)
    calendar_fixture_set_id: str = _require_non_empty_string(raw_case, "calendar_fixture_set_id", context)
    if calendar_fixture_set_id != CALENDAR_REGRESSION_FIXTURE_SET_ID:
        raise ValueError(
            f"{context}.calendar_fixture_set_id must be {CALENDAR_REGRESSION_FIXTURE_SET_ID}, found {calendar_fixture_set_id}",
        )

    calendar_state_fixture_references: list[str] = _require_non_empty_string_list(
        raw_case,
        "calendar_state_fixture_references",
        context,
    )
    _require_unique_fixture_ids(
        calendar_state_fixture_references,
        f"{context}.calendar_state_fixture_references",
    )

    range_start_utc: str = _require_non_empty_string(raw_case, "range_start_utc", context)
    range_end_utc: str = _require_non_empty_string(raw_case, "range_end_utc", context)
    initial_accumulator_state: JsonObject = _require_json_object_field(raw_case, "initial_accumulator_state", context)
    initial_state: OpenClosedGapBehaviorAccumulatorState = {
        "j_t": _require_number(initial_accumulator_state, "j_t", f"{context}.initial_accumulator_state"),
        "g_t": _require_number(initial_accumulator_state, "g_t", f"{context}.initial_accumulator_state"),
    }

    injected_events_raw: JsonArray = _require_json_array_field(raw_case, "injected_events", context)
    if not injected_events_raw:
        raise ValueError(f"{context}.injected_events must include at least one event")

    injected_events: list[OpenClosedGapBehaviorInjectedEvent] = []
    for event_index, raw_event in enumerate(injected_events_raw):
        event_map: JsonObject = _require_json_object(raw_event, f"{context}.injected_events[{event_index}]")
        injected_events.append(
            {
                "event_id": _require_non_empty_string(event_map, "event_id", f"{context}.injected_events[{event_index}]"),
                "timestamp_utc": _require_non_empty_string(
                    event_map,
                    "timestamp_utc",
                    f"{context}.injected_events[{event_index}]",
                ),
                "shared_severity_mark": _require_non_negative_float(
                    event_map,
                    "shared_severity_mark",
                    f"{context}.injected_events[{event_index}]",
                ),
                "coupled_gap_jump_mark": _require_number(
                    event_map,
                    "coupled_gap_jump_mark",
                    f"{context}.injected_events[{event_index}]",
                ),
            },
        )

    _require_unique_fixture_ids(
        [injected_event["event_id"] for injected_event in injected_events],
        f"{context}.injected_events",
    )

    expected_outcome = _validate_open_closed_gap_behavior_expected_outcome(
        _require_json_object_field(raw_case, "expected_outcome", context),
        context,
        injected_events,
    )

    tags: list[str] = _require_non_empty_string_list(raw_case, "tags", context)
    description: str = _require_non_empty_string(raw_case, "description", context)

    return {
        "fixture_id": fixture_id,
        "exchange_code": exchange_code,
        "calendar_fixture_set_id": calendar_fixture_set_id,
        "calendar_state_fixture_references": calendar_state_fixture_references,
        "range_start_utc": range_start_utc,
        "range_end_utc": range_end_utc,
        "initial_accumulator_state": initial_state,
        "injected_events": injected_events,
        "expected_outcome": expected_outcome,
        "tags": tags,
        "description": description,
    }


def _validate_open_closed_gap_behavior_expected_outcome(
    raw_expected_outcome: JsonObject,
    case_context: str,
    injected_events: list[OpenClosedGapBehaviorInjectedEvent],
) -> OpenClosedGapBehaviorExpectedOutcome:
    event_outcomes_raw: JsonArray = _require_json_array_field(
        raw_expected_outcome,
        "event_outcomes",
        f"{case_context}.expected_outcome",
    )
    if not event_outcomes_raw:
        raise ValueError(f"{case_context}.expected_outcome.event_outcomes must include at least one outcome")

    event_outcomes: list[OpenClosedGapBehaviorEventOutcome] = []
    for event_index, raw_event_outcome in enumerate(event_outcomes_raw):
        event_outcome: JsonObject = _require_json_object(
            raw_event_outcome,
            f"{case_context}.expected_outcome.event_outcomes[{event_index}]",
        )
        expected_market_state: str = _require_non_empty_string(
            event_outcome,
            "expected_market_state",
            f"{case_context}.expected_outcome.event_outcomes[{event_index}]",
        )
        if expected_market_state not in {"open", "closed"}:
            raise ValueError(
                f"{case_context}.expected_outcome.event_outcomes[{event_index}].expected_market_state "
                "must be either 'open' or 'closed'",
            )

        expected_g_t_increment: float = _require_number(
            event_outcome,
            "expected_g_t_increment",
            f"{case_context}.expected_outcome.event_outcomes[{event_index}]",
        )
        expected_direct_gap_applied: bool = _require_bool(
            event_outcome,
            "expected_direct_gap_applied",
            f"{case_context}.expected_outcome.event_outcomes[{event_index}]",
        )
        if expected_market_state == "open" and expected_direct_gap_applied:
            raise ValueError(
                f"{case_context}.expected_outcome.event_outcomes[{event_index}] cannot set expected_direct_gap_applied=true for open market state",
            )
        if expected_market_state == "open" and expected_g_t_increment != 0.0:
            raise ValueError(
                f"{case_context}.expected_outcome.event_outcomes[{event_index}] must set expected_g_t_increment=0.0 for open market state",
            )

        event_outcomes.append(
            {
                "event_id": _require_non_empty_string(
                    event_outcome,
                    "event_id",
                    f"{case_context}.expected_outcome.event_outcomes[{event_index}]",
                ),
                "expected_market_state": expected_market_state,
                "expected_j_t_increment": _require_non_negative_float(
                    event_outcome,
                    "expected_j_t_increment",
                    f"{case_context}.expected_outcome.event_outcomes[{event_index}]",
                ),
                "expected_g_t_increment": expected_g_t_increment,
                "expected_direct_gap_applied": expected_direct_gap_applied,
            },
        )

    _require_unique_fixture_ids(
        [event_outcome["event_id"] for event_outcome in event_outcomes],
        f"{case_context}.expected_outcome.event_outcomes",
    )

    injected_event_ids: set[str] = {injected_event["event_id"] for injected_event in injected_events}
    expected_event_ids: set[str] = {event_outcome["event_id"] for event_outcome in event_outcomes}
    if injected_event_ids != expected_event_ids:
        raise ValueError(
            f"{case_context}.expected_outcome.event_outcomes event_id set must match injected_events event_id set",
        )

    summary = _validate_open_closed_gap_behavior_summary(
        _require_json_object_field(
            raw_expected_outcome,
            "summary",
            f"{case_context}.expected_outcome",
        ),
        event_outcomes,
        case_context,
    )

    return {
        "event_outcomes": event_outcomes,
        "summary": summary,
    }


def _validate_open_closed_gap_behavior_summary(
    raw_summary: JsonObject,
    event_outcomes: list[OpenClosedGapBehaviorEventOutcome],
    case_context: str,
) -> OpenClosedGapBehaviorSummary:
    summary: OpenClosedGapBehaviorSummary = {
        "expected_open_event_count": _require_non_negative_int(
            raw_summary,
            "expected_open_event_count",
            f"{case_context}.expected_outcome.summary",
        ),
        "expected_closed_event_count": _require_non_negative_int(
            raw_summary,
            "expected_closed_event_count",
            f"{case_context}.expected_outcome.summary",
        ),
        "expected_total_j_t_increment": _require_number(
            raw_summary,
            "expected_total_j_t_increment",
            f"{case_context}.expected_outcome.summary",
        ),
        "expected_total_g_t_increment": _require_number(
            raw_summary,
            "expected_total_g_t_increment",
            f"{case_context}.expected_outcome.summary",
        ),
        "expected_open_window_direct_gap_increment_count": _require_non_negative_int(
            raw_summary,
            "expected_open_window_direct_gap_increment_count",
            f"{case_context}.expected_outcome.summary",
        ),
    }

    observed_open_event_count: int = sum(
        1 for event_outcome in event_outcomes if event_outcome["expected_market_state"] == "open"
    )
    observed_closed_event_count: int = sum(
        1 for event_outcome in event_outcomes if event_outcome["expected_market_state"] == "closed"
    )
    observed_open_window_direct_gap_increment_count: int = sum(
        1
        for event_outcome in event_outcomes
        if event_outcome["expected_market_state"] == "open" and event_outcome["expected_direct_gap_applied"]
    )
    observed_total_j_t_increment: float = sum(event_outcome["expected_j_t_increment"] for event_outcome in event_outcomes)
    observed_total_g_t_increment: float = sum(event_outcome["expected_g_t_increment"] for event_outcome in event_outcomes)

    if summary["expected_open_event_count"] != observed_open_event_count:
        raise ValueError(
            f"{case_context}.expected_outcome.summary.expected_open_event_count must match event_outcomes",
        )
    if summary["expected_closed_event_count"] != observed_closed_event_count:
        raise ValueError(
            f"{case_context}.expected_outcome.summary.expected_closed_event_count must match event_outcomes",
        )
    if summary["expected_open_window_direct_gap_increment_count"] != observed_open_window_direct_gap_increment_count:
        raise ValueError(
            f"{case_context}.expected_outcome.summary.expected_open_window_direct_gap_increment_count must match event_outcomes",
        )
    if summary["expected_open_event_count"] + summary["expected_closed_event_count"] != len(event_outcomes):
        raise ValueError(
            f"{case_context}.expected_outcome.summary open and closed event counts must sum to event_outcomes length",
        )
    if not math.isclose(summary["expected_total_j_t_increment"], observed_total_j_t_increment, rel_tol=0.0, abs_tol=1e-12):
        raise ValueError(
            f"{case_context}.expected_outcome.summary.expected_total_j_t_increment must match event_outcomes",
        )
    if not math.isclose(summary["expected_total_g_t_increment"], observed_total_g_t_increment, rel_tol=0.0, abs_tol=1e-12):
        raise ValueError(
            f"{case_context}.expected_outcome.summary.expected_total_g_t_increment must match event_outcomes",
        )

    return summary


def _validate_seeded_tick_simulation_parity_case(
    raw_case: JsonObject,
) -> SeededTickSimulationParityCase:
    fixture_id: str = _require_non_empty_string(raw_case, "fixture_id", "seeded tick simulation parity case")
    seed: int = _require_non_negative_int(raw_case, "seed", "seeded tick simulation parity case")
    exchange_code: str = _require_non_empty_string(raw_case, "exchange_code", "seeded tick simulation parity case")
    range_start_utc: str = _require_non_empty_string(
        raw_case,
        "range_start_utc",
        "seeded tick simulation parity case",
    )
    range_end_utc: str = _require_non_empty_string(
        raw_case,
        "range_end_utc",
        "seeded tick simulation parity case",
    )
    horizon_interval_count: int = _require_non_negative_int(
        raw_case,
        "horizon_interval_count",
        "seeded tick simulation parity case",
    )
    if horizon_interval_count == 0:
        raise ValueError("seeded tick simulation parity case.horizon_interval_count must be greater than 0")

    kernel_parameters = _validate_seeded_tick_simulation_kernel_parameters(
        _require_json_object_field(raw_case, "kernel_parameters", "seeded tick simulation parity case"),
        "seeded tick simulation parity case.kernel_parameters",
    )
    initial_excitation_state = _validate_seeded_tick_simulation_initial_excitation_state(
        _require_json_object_field(raw_case, "initial_excitation_state", "seeded tick simulation parity case"),
        "seeded tick simulation parity case.initial_excitation_state",
    )

    seasonality_hourly_factors: list[float] = _require_float_list(
        raw_case,
        "seasonality_hourly_factors",
        "seeded tick simulation parity case",
    )
    news_intensity_hourly_factors: list[float] = _require_float_list(
        raw_case,
        "news_intensity_hourly_factors",
        "seeded tick simulation parity case",
    )

    if len(seasonality_hourly_factors) != horizon_interval_count:
        raise ValueError(
            "seeded tick simulation parity case seasonality_hourly_factors length must match horizon_interval_count",
        )
    if len(news_intensity_hourly_factors) != horizon_interval_count:
        raise ValueError(
            "seeded tick simulation parity case news_intensity_hourly_factors length must match horizon_interval_count",
        )

    expected_timestamps_utc_nanos_raw: JsonArray = _require_json_array_field(
        raw_case,
        "expected_timestamps_utc_nanos",
        "seeded tick simulation parity case",
    )
    expected_timestamps_utc_nanos: list[int] = []
    for timestamp_position, raw_timestamp in enumerate(expected_timestamps_utc_nanos_raw):
        if not isinstance(raw_timestamp, int) or isinstance(raw_timestamp, bool):
            raise ValueError(
                "seeded tick simulation parity case.expected_timestamps_utc_nanos must only contain integers",
            )
        if timestamp_position > 0 and raw_timestamp < expected_timestamps_utc_nanos[timestamp_position - 1]:
            raise ValueError(
                "seeded tick simulation parity case.expected_timestamps_utc_nanos must be non-decreasing",
            )
        expected_timestamps_utc_nanos.append(raw_timestamp)

    expected_side_sequence_raw: JsonArray = _require_json_array_field(
        raw_case,
        "expected_side_sequence",
        "seeded tick simulation parity case",
    )
    expected_side_sequence: list[str] = []
    for raw_side in expected_side_sequence_raw:
        if not isinstance(raw_side, str):
            raise ValueError("seeded tick simulation parity case.expected_side_sequence must only contain strings")

        normalized_side = raw_side.strip()
        if normalized_side not in {"buy", "sell"}:
            raise ValueError(
                f"seeded tick simulation parity case.expected_side_sequence contains unsupported side: {raw_side}",
            )
        expected_side_sequence.append(normalized_side)

    if len(expected_side_sequence) != len(expected_timestamps_utc_nanos):
        raise ValueError(
            "seeded tick simulation parity case expected_timestamps_utc_nanos and expected_side_sequence must have equal lengths",
        )

    expected_total_event_count: int = _require_non_negative_int(
        raw_case,
        "expected_total_event_count",
        "seeded tick simulation parity case",
    )
    if expected_total_event_count != len(expected_timestamps_utc_nanos):
        raise ValueError(
            "seeded tick simulation parity case expected_total_event_count must match expected_timestamps_utc_nanos length",
        )

    expected_interval_event_counts = _require_non_negative_int_list(
        raw_case,
        "expected_interval_event_counts",
        "seeded tick simulation parity case",
    )
    expected_interval_buy_counts = _require_non_negative_int_list(
        raw_case,
        "expected_interval_buy_counts",
        "seeded tick simulation parity case",
    )
    expected_interval_sell_counts = _require_non_negative_int_list(
        raw_case,
        "expected_interval_sell_counts",
        "seeded tick simulation parity case",
    )
    if len(expected_interval_event_counts) != horizon_interval_count:
        raise ValueError(
            "seeded tick simulation parity case expected_interval_event_counts length must match horizon_interval_count",
        )
    if len(expected_interval_buy_counts) != horizon_interval_count:
        raise ValueError(
            "seeded tick simulation parity case expected_interval_buy_counts length must match horizon_interval_count",
        )
    if len(expected_interval_sell_counts) != horizon_interval_count:
        raise ValueError(
            "seeded tick simulation parity case expected_interval_sell_counts length must match horizon_interval_count",
        )
    if sum(expected_interval_event_counts) != expected_total_event_count:
        raise ValueError(
            "seeded tick simulation parity case expected_interval_event_counts sum must match expected_total_event_count",
        )
    if sum(expected_interval_buy_counts) + sum(expected_interval_sell_counts) != expected_total_event_count:
        raise ValueError(
            "seeded tick simulation parity case expected_interval_buy_counts and expected_interval_sell_counts sums must match expected_total_event_count",
        )

    expected_min_positive_timestamp_gap_nanos: int = _require_non_negative_int(
        raw_case,
        "expected_min_positive_timestamp_gap_nanos",
        "seeded tick simulation parity case",
    )
    expected_has_non_grid_arrival: bool = _require_bool(
        raw_case,
        "expected_has_non_grid_arrival",
        "seeded tick simulation parity case",
    )

    tags: list[str] = _require_non_empty_string_list(raw_case, "tags", "seeded tick simulation parity case")
    description: str = _require_non_empty_string(raw_case, "description", "seeded tick simulation parity case")

    return {
        "fixture_id": fixture_id,
        "seed": seed,
        "exchange_code": exchange_code,
        "range_start_utc": range_start_utc,
        "range_end_utc": range_end_utc,
        "horizon_interval_count": horizon_interval_count,
        "kernel_parameters": kernel_parameters,
        "initial_excitation_state": initial_excitation_state,
        "seasonality_hourly_factors": seasonality_hourly_factors,
        "news_intensity_hourly_factors": news_intensity_hourly_factors,
        "expected_timestamps_utc_nanos": expected_timestamps_utc_nanos,
        "expected_side_sequence": expected_side_sequence,
        "expected_total_event_count": expected_total_event_count,
        "expected_interval_event_counts": expected_interval_event_counts,
        "expected_interval_buy_counts": expected_interval_buy_counts,
        "expected_interval_sell_counts": expected_interval_sell_counts,
        "expected_min_positive_timestamp_gap_nanos": expected_min_positive_timestamp_gap_nanos,
        "expected_has_non_grid_arrival": expected_has_non_grid_arrival,
        "tags": tags,
        "description": description,
    }


def _validate_seeded_tick_simulation_kernel_parameters(
    raw_kernel_parameters: JsonObject,
    context: str,
) -> SeededTickSimulationParityKernelParameters:
    alpha = _validate_seeded_tick_simulation_kernel_loadings(
        _require_json_object_field(raw_kernel_parameters, "alpha", context),
        f"{context}.alpha",
    )
    rho = _validate_seeded_tick_simulation_kernel_loadings(
        _require_json_object_field(raw_kernel_parameters, "rho", context),
        f"{context}.rho",
    )

    return {
        "lambda_bar": _require_number(raw_kernel_parameters, "lambda_bar", context),
        "beta": _require_number(raw_kernel_parameters, "beta", context),
        "alpha": alpha,
        "rho": rho,
    }


def _validate_seeded_tick_simulation_kernel_loadings(
    raw_loadings: JsonObject,
    context: str,
) -> SeededTickSimulationParityKernelLoadings:
    return {
        "plus_plus": _require_number(raw_loadings, "plus_plus", context),
        "plus_minus": _require_number(raw_loadings, "plus_minus", context),
        "minus_plus": _require_number(raw_loadings, "minus_plus", context),
        "minus_minus": _require_number(raw_loadings, "minus_minus", context),
    }


def _validate_seeded_tick_simulation_initial_excitation_state(
    raw_state: JsonObject,
    context: str,
) -> SeededTickSimulationParityInitialExcitationState:
    return {
        "y_plus_plus": _require_number(raw_state, "y_plus_plus", context),
        "y_plus_minus": _require_number(raw_state, "y_plus_minus", context),
        "y_minus_plus": _require_number(raw_state, "y_minus_plus", context),
        "y_minus_minus": _require_number(raw_state, "y_minus_minus", context),
    }


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


def _require_non_negative_int_list(parent: JsonObject, field_name: str, context: str) -> list[int]:
    field_values: JsonArray = _require_json_array_field(parent, field_name, context)
    if not field_values:
        raise ValueError(f"field '{field_name}' in {context} must include at least one value")

    normalized_values: list[int] = []
    for raw_value in field_values:
        if not isinstance(raw_value, int) or isinstance(raw_value, bool):
            raise ValueError(f"field '{field_name}' in {context} must only contain integers")
        if raw_value < 0:
            raise ValueError(f"field '{field_name}' in {context} must only contain non-negative integers")
        normalized_values.append(raw_value)

    return normalized_values


def _require_bool(parent: JsonObject, field_name: str, context: str) -> bool:
    raw_value: object = parent.get(field_name)
    if raw_value is None:
        raise ValueError(f"missing required field '{field_name}' in {context}")

    if not isinstance(raw_value, bool):
        raise ValueError(f"field '{field_name}' in {context} must be a boolean")

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


def _parse_utc_timestamp(raw_timestamp_utc: str) -> datetime:
    parsed_timestamp = datetime.fromisoformat(raw_timestamp_utc.replace("Z", "+00:00"))
    if parsed_timestamp.tzinfo is None:
        raise ValueError(f"timestamp must include timezone offset: {raw_timestamp_utc}")

    return parsed_timestamp.astimezone(UTC)
