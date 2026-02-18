# Calendar Service Regression Fixtures

This directory contains the shared calendar regression fixture set used by both Rust and Python tests.
The fixture includes deterministic schema and content-hash metadata, plus anomaly coverage for early-close,
delayed-open, and overlapping-rule precedence behavior.

## Files

- `calendar_service_regression_matrix.json`: Canonical calendar registry artifact plus a regression matrix of timestamp cases and expected `open` or `closed` outputs.
- `../validation/validation_fixture_manifest.json`: Canonical cross-language fixture manifest with deterministic loading and content-hash metadata.

## Fixture Schema

`calendar_service_regression_matrix.json` has the following top-level fields:

- `schema_version`: Fixture schema version string.
- `fixture_set_id`: Stable fixture set identifier used in test diagnostics.
- `calendar_registry`: The registry artifact consumed by the Rust core and Python adapter tests.
- `canonical_regression_metadata`: Canonical required-tag and stable fixture-id expectations used by Rust and Python tests.
- `cases`: Ordered regression cases with stable identifiers.

Each `cases` entry contains:

- `fixture_id`: Stable case identifier. Do not rename existing IDs.
- `exchange_code`: Exchange identifier resolved by the calendar service.
- `timestamp_utc`: RFC3339 timestamp used in resolver tests.
- `expected_state`: Expected resolver output, either `open` or `closed`.
- `tags`: Coverage classification labels used by regression tests.
- `description`: Human-readable triage context.

`calendar_service_regression_matrix.json` also includes machine-readable metadata blocks:

- `schema_metadata`:
  - `schema_family`
  - `schema_generation`
  - `forward_extension_policy`
- `determinism_metadata`:
  - `fixture_loading_order`
  - `content_hash_scope`
  - `content_hash_algorithm`
- `canonical_regression_metadata`:
  - `required_tags`
  - `stable_fixture_ids_by_tag`
  - `historical_holiday_expected_years`
  - `historical_holiday_expected_intraday_tags`

The shared manifest defines:

- `schema_version`, `manifest_id`, and `manifest_version`
- `versioning_policy` metadata for v0.x update strategy
- `determinism_policy` metadata for load-order and content-hash algorithms
- ordered `fixtures` entries containing:
  - `fixture_id`
  - `domain`
  - `fixture_schema_version`
  - `relative_path`
  - `exchange_codes`
  - `content_hash`

Rust and Python loaders sort manifest entries lexicographically by `fixture_id` and verify `content_hash` before fixture parsing.

## Versioning Strategy

- Breaking schema changes require a new `schema_version`.
- During v0.x, additive fields are allowed only when they preserve current required fields and determinism semantics.
- Fixture files remain immutable for a given `fixture_set_id`; any payload change must update `content_hash` in the shared manifest.
- The manifest contract is language-neutral and must not include runtime-only fields that only one stack can consume.

## Required Coverage Tags

The matrix must always include at least one case for each tag:

- `regular_session`
- `weekend`
- `holiday`
- `early_close`
- `boundary`
- `timezone_sensitive`
- `delayed_open`
- `session_anomaly`
- `precedence_conflict`

Additional holiday-regression tags:

- `historical_holiday`
- `holiday_intraday_open_boundary`
- `holiday_intraday_mid_session`
- `holiday_intraday_pre_close`
- `holiday_adjacent_non_holiday`

## Historical Holiday Provenance

Historical holiday closure cases are curated from NYSE weekday full-close conventions and kept in
the fixture as explicit `holiday_dates` plus timestamped regression cases. The current historical
set spans 2024 to 2026 and focuses on holidays that are unambiguous full-day closures:

- New Year's Day: `2025-01-01`, `2026-01-01`
- Independence Day: `2024-07-04`

Each historical holiday date includes key intraday timestamps covering regular-open boundary,
mid-session, and pre-close windows, all expected to resolve `closed`.
Adjacent weekday non-holiday timestamps are included to validate that normal `open` classification
is preserved immediately before and after each holiday closure.

## Expected Output Semantics

- Session boundaries use half-open semantics `[open, close)`.
- Exact open timestamps resolve `open`.
- Exact close timestamps resolve `closed`.
- Holiday rules override session windows.
- Early-close windows override regular windows for their date.
- Delayed-open anomalies are represented as date-specific override windows, and those windows override regular sessions for their date.

## Session Anomaly Rationale

The fixture includes timezone-aware RFC3339 UTC timestamps (`Z` offset) around anomaly transition boundaries:

- `xnys_early_close_pre_boundary`: validates that one second before early-close remains open.
- `xnys_early_close_at_boundary`: validates the exact half-open boundary close transition.
- `xnys_early_close_post_boundary`: validates that one second after early-close stays closed.
- `xnys_delayed_open_regular_open_conflict_at_regular_open`: validates deterministic precedence where date-specific delayed-open override wins over regular open.
- `xnys_delayed_open_pre_boundary`: validates that one second before delayed-open remains closed.
- `xnys_delayed_open_at_boundary`: validates exact delayed-open boundary transition to open.
- `xnys_holiday_overrides_delayed_open_at_boundary`: validates overlap conflict where holiday precedence keeps the market closed at delayed-open boundary.
- `xnys_holiday_overrides_delayed_open_mid_window`: validates overlap conflict where holiday precedence keeps the market closed inside delayed-open window.

## Update Process

1. Edit `calendar_service_regression_matrix.json`.
2. Keep existing `fixture_id` values stable. Add new cases instead of renaming old ones.
3. If a behavior change is intentional, update `expected_state` and `description` for affected cases.
4. Run verification:
   - `cargo test -p janiform-core calendar_regression`
   - `pytest tests/test_calendar.py`
5. When behavior changes by design, update this README.

Downstream seasonality and gap-behavior test suites should reuse this matrix to avoid duplicate calendar fixtures.
