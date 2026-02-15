# Calendar Service Regression Fixtures

This directory contains the shared calendar regression fixture set for Initiative 001, Project 001, Issue JAN-10.

## Files

- `calendar_service_regression_matrix.json`: Canonical calendar registry artifact plus a regression matrix of timestamp cases and expected `open` or `closed` outputs.

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
