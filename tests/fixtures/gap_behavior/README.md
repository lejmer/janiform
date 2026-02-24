# Open Closed Gap Behavior Scenarios

This fixture set defines data-driven open and closed gap validation scenarios that can be consumed by Rust and Python regression suites.

## File

- `open_closed_gap_behavior_scenarios.json`

## Schema Contract

Top-level fields:

- `schema_version`: `gap.behavior.open_closed.scenario_fixture.v1`
- `fixture_set_id`: `open_closed_gap_behavior_scenarios_v1`
- `fixture_set_version`: semantic fixture version
- `schema_metadata`: machine-readable schema family, generation, and extension policy
- `determinism_metadata`: case and event ordering policies plus content hash metadata
- `output_schema_metadata`: machine-readable required output fields for downstream reports
- `canonical_regression_metadata`: required tags and stable fixture IDs by tag
- `cases`: scenario definitions

Each case contains:

- Exchange context: `exchange_code`, `calendar_fixture_set_id`, `range_start_utc`, `range_end_utc`
- Inputs: `initial_accumulator_state` and `injected_events`
- Expected outcomes: per-event `event_outcomes` and aggregate `summary`
- Metadata: `tags`, `description`

## Determinism Rules

- Cases are consumed in declared order.
- Event lists are consumed in declared order.
- Content hash scope is the full document payload bytes.

## Output Schema Metadata

`output_schema_metadata` is intentionally explicit so harness and report implementations can validate output payloads consistently across engines.
