# Hourly Factor Regression Fixtures

This directory contains the shared hourly factor regression matrix for Initiative 001, Project 003, Issue JAN-22.

## Files

- `hourly_factor_regression_matrix.json`: Canonical hourly regression matrix with expected `s_h` and `nu_h` outputs plus missing-data guard outcomes.

## Fixture Schema

`hourly_factor_regression_matrix.json` has the following top-level fields:

- `schema_version`: Fixture schema version string.
- `fixture_set_id`: Stable fixture set identifier used in test diagnostics.
- `calendar_fixture_set_id`: Reference to the shared calendar fixture set consumed by these tests.
- `acceptance_thresholds`: Absolute tolerances used by golden comparisons.
- `canonical_regression_metadata`: Canonical required-tag and stable fixture-id expectations used by Rust and Python tests.
- `canonical_output_cases`: Ordered canonical hourly output cases with expected `s_h` and `nu_h`.
- `missing_data_guard_cases`: Ordered sparse or missing source cases with defined fallback or failure outcomes.

Each `canonical_output_cases` entry contains:

- `fixture_id`: Stable case identifier. Do not rename existing IDs.
- `exchange_code`: Exchange identifier resolved by the shared calendar fixture.
- `range_start_utc` and `range_end_utc`: RFC3339 hourly boundaries defining the `[start, end)` request range.
- `seasonality_source` and `news_intensity_source`: Source configuration for deterministic materialization.
- `expected_s_h` and `expected_nu_h`: Golden interval outputs for the case.
- `expected_skipped_local_hour_interval_count` and `expected_repeated_local_hour_interval_count`: Daylight-saving boundary diagnostics.
- `tags`: Coverage classification labels used by regression tests.
- `description`: Human-readable triage context.

Each `missing_data_guard_cases` entry contains:

- `fixture_id`: Stable case identifier. Do not rename existing IDs.
- `exchange_code`: Exchange identifier resolved by the shared calendar fixture.
- `range_start_utc` and `range_end_utc`: RFC3339 hourly boundaries defining the `[start, end)` request range.
- `source_timestamps_utc` and `source_seasonality_factors`: Sparse or missing seasonality samples.
- `settings`: Deterministic interpolation, aggregation, and fallback policy configuration.
- `expected_outcome`: Either:
  - `outcome_kind: "success"` with expected `s_h` and sparse diagnostics, or
  - `outcome_kind: "failure"` with expected error variant and message substring.
- `tags`: Coverage classification labels used by regression tests.
- `description`: Human-readable triage context.

## Canonical Metadata

`canonical_regression_metadata` stores the canonical expectations consumed by both test stacks:

- `required_tags`: Tags that must be represented across canonical and missing-data guard cases.
- `stable_canonical_output_fixture_ids`: Stable case IDs expected in `canonical_output_cases`.
- `stable_missing_data_guard_fixture_ids`: Stable case IDs expected in `missing_data_guard_cases`.

## Acceptance Thresholds

- `s_h_absolute_tolerance`: Maximum absolute per-interval deviation allowed in `s_h` comparisons.
- `nu_h_absolute_tolerance`: Maximum absolute per-interval deviation allowed in `nu_h` comparisons.

Current thresholds are exact-equality friendly (`1e-12`) and intended to detect deterministic drift.

## Update Process

1. Edit `hourly_factor_regression_matrix.json`.
2. Keep existing `fixture_id` values stable. Add new cases instead of renaming old ones.
3. If behavior changes intentionally, update:
   - expected outputs or expected error outcomes in affected cases
   - `description` text for affected cases
   - this README if tolerance policy or fixture schema changed
4. Run verification:
   - `cargo test -p janiform-core hourly_factor_regression`
   - `pytest tests/test_hourly_factor_regression_matrix.py`
   - `pytest tests/test_hourly_factors.py`

These cases intentionally reuse the shared calendar regression fixture registry to avoid duplicate exchange schedule artifacts.
