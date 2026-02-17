# Calendar Service Regression Fixtures

This directory contains the shared calendar regression fixture set for Initiative 001, Project 001, Issue JAN-10.
Issue JAN-23 extends this fixture with a shared schema and deterministic manifest contract that is reused by both Rust and Python tests.

## Files

- `calendar_service_regression_matrix.json`: Canonical calendar registry artifact plus a regression matrix of timestamp cases and expected `open` or `closed` outputs.
- `../validation/validation_fixture_manifest.json`: Canonical cross-language fixture manifest with deterministic loading and content-hash metadata.

## Fixture Schema

`calendar_service_regression_matrix.json` has the following top-level fields:

- `schema_version`: Fixture schema version string.
- `fixture_set_id`: Stable fixture set identifier used in test diagnostics.
- `calendar_registry`: The registry artifact consumed by the Rust core and Python adapter tests.
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

## Expected Output Semantics

- Session boundaries use half-open semantics `[open, close)`.
- Exact open timestamps resolve `open`.
- Exact close timestamps resolve `closed`.
- Holiday rules override session windows.
- Early-close windows override regular windows for their date.

## Update Process

1. Edit `calendar_service_regression_matrix.json`.
2. Keep existing `fixture_id` values stable. Add new cases instead of renaming old ones.
3. If a behavior change is intentional, update `expected_state` and `description` for affected cases.
4. Run verification:
   - `cargo test -p janiform-core calendar_regression`
   - `pytest tests/test_calendar.py`
5. When behavior changes by design, update this README and the related planning issue notes.

Downstream seasonality and gap-behavior test suites should reuse this matrix to avoid duplicate calendar fixtures.
