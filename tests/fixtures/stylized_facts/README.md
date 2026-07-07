# Stylized Fact Metric Regression Fixtures

This directory contains the shared reference fixture set for stylized-fact metric regression
validation.

## Files

- `stylized_fact_metric_regression_fixture.json`: Canonical reference cases for deterministic
  exact-match and seeded bootstrap confidence-interval regression checks.

## Fixture Contract

Top-level fields:

- `schema_version`: `validation.stylized_fact.metric_regression.fixture.v1`
- `fixture_set_id`: `stylized_fact_metric_regression_fixture_v1`
- `fixture_set_version`: semantic fixture version for fixture content updates
- `schema_metadata`: schema family, generation, and forward-extension policy
- `determinism_metadata`: declared case ordering plus content-hash policies
- `tolerance_policies`: versioned regression tolerance policies
- `canonical_regression_metadata`: required tags, diagnostic kinds, policy ids, and stable fixture ids
- `cases`: ordered regression cases

Each case contains:

- `fixture_id`: Stable case identifier. Do not rename existing ids.
- `diagnostic_kind`: `activity_clustering`, `tail_skew`, `volatility_persistence`, or `gap_behavior`
- `tolerance_policy_id`: Reference to a declared tolerance policy
- `provenance`: baseline source, capture timestamp, and update guidance
- `request_payload`: domain-specific request payload consumed by the Rust regression harness
- `expected_metrics`: canonical metric expectations keyed by horizon, metric id, and optional variant
- `tags` and `description`: coverage and triage metadata

Each `expected_metrics` entry stores:

- metric identity: `horizon_label`, `metric_id`, and optional `metric_variant`
- `expected_status`: `available` or `insufficient_data`
- optional point-estimate expectations
- optional `expected_absolute_delta`
- optional empirical and simulated confidence intervals for seeded bootstrap regression cases

## Tolerance Policies

Two policy kinds are currently supported:

- `deterministic_exact_match`
- `bootstrap_confidence_interval`

Policies are versioned and explicit so intentional tolerance changes require fixture metadata
updates in the same change.

## Provenance And Update Workflow

Fixture values are derived from the controlled reference datasets already used by the Rust
stylized-fact diagnostic tests. Update the fixture only when the underlying diagnostic behavior is
intentionally changed.

When updating:

1. Edit `stylized_fact_metric_regression_fixture.json`.
2. Keep existing `fixture_id` values stable. Add new cases instead of renaming old ones.
3. If tolerances change intentionally, update the matching entry in `tolerance_policies`.
4. Refresh the manifest content hash in `tests/fixtures/validation/validation_fixture_manifest.json`.
5. Run verification:
   - `cargo test -p janiform stylized_fact_metric_regression`
   - `cargo test -p janiform validation_fixture`
   - `pytest tests/test_validation_fixtures.py -k stylized_fact`
   - `python3 -m py_compile python/janiform/*.py tests/conftest.py`
