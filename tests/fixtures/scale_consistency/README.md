# Scale Consistency Regression Fixtures

This directory contains the compact shared regression pack for the model-level scale-consistency
decision and reporting harness.

## Files

- `scale_consistency_regression_fixture.json`: Canonical fixed-input cases covering subhour and
  hourly interval bands, pass and fail outcomes, event-derived drift, and engine benchmark
  prerequisite failures.

## Fixture Contract

Top-level fields:

- `schema_version`: `validation.scale_consistency.regression.fixture.v1`
- `fixture_set_id`: `scale_consistency_regression_fixture_v1`
- `fixture_set_version`: semantic fixture version for intentional content updates
- `schema_metadata`: schema family, generation, and forward-extension policy
- `determinism_metadata`: declared case ordering, content-hash policy, and repeated-run policy
- `threshold_policy_reference`: expected scale-consistency threshold policy id and version
- `canonical_regression_metadata`: required coverage tags, interval-band coverage, expected-status
  coverage, and stable fixture ids
- `cases`: ordered regression cases

Each case contains:

- `fixture_id`, `scenario_id`, `parameter_set_id`, `horizon_label`
- `coarse_interval_delta` and `fine_interval_delta`
- `expected_applied_interval_threshold_band_label`
- `provenance`: baseline source, capture timestamp, and update guidance
- `engine_benchmark_input`: compact prerequisite input consumed by the decision engine
- `metric_delta_input`: deterministic metric deltas used to build a single-scenario batch report
- `expected_outcome`: versioned decision and report expectations, including rendered summary text
- `tags` and `description`: coverage and triage metadata

## Update Workflow

These fixtures intentionally sit above the stochastic simulation layer. Keep them compact and use
them only for deterministic regression coverage of the decision and reporting harness.

When updating:

1. Edit `scale_consistency_regression_fixture.json`.
2. Keep existing `fixture_id` values stable. Add new cases instead of renaming old ones.
3. If threshold semantics changed intentionally, update
   `tests/fixtures/validation/scale_consistency_threshold_policy.json` and then update the fixture
   `threshold_policy_reference` in the same change.
4. Refresh the manifest content hash in
   `tests/fixtures/validation/validation_fixture_manifest.json`.
5. Run verification:
   - `cargo test -p janiform-core scale_consistency_regression_fixture`
   - `cargo test -p janiform-core scale_consistency_reporting`
   - `cargo test -p janiform-core scale_consistency_threshold_decisions`
   - `pytest tests/test_validation_fixtures.py -k scale_consistency`
   - `python3 -m py_compile python/janiform/*.py tests/conftest.py tests/test_validation_fixtures.py`
