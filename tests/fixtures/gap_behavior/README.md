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
- `canonical_regression_metadata`: required tags, required scenario classes, and stable fixture IDs by tag
- `cases`: scenario definitions

Each case contains:

- Exchange context: `exchange_code`, `calendar_fixture_set_id`, `range_start_utc`, `range_end_utc`
- Scenario semantics: `scenario_class` with values `known_only`, `surprise_only`, or `mixed_news`
- Deterministic replay metadata: `deterministic_replay_metadata`
- Issue traceability metadata: `issue_traceability`
- Calendar linkage: `calendar_state_fixture_references` identifies case IDs from `calendar_service_regression_matrix_v1`
- Inputs: `initial_accumulator_state` and `injected_events`
- Expected outcomes: per-event `event_outcomes` and aggregate `summary`
- Metadata: `tags`, `description`

`initial_accumulator_state` includes `j_t`, `m_t`, and `g_t`.

Each injected event includes:

- `news_arrival_component` (`known` or `surprise`)
- `shared_severity_mark`
- `coupled_return_jump_mark`

Each expected event outcome includes:

- `expected_activity_coupling_j_t_increment`
- `expected_open_jump_m_t_increment`
- `expected_closed_gap_g_t_increment`
- `expected_trading_return_increment`

## Calendar Fixture Mapping

Open and closed scenario classification is resolved through the canonical calendar service using
the shared Initiative 001 calendar fixture set.

- `calendar_fixture_set_id` must reference `calendar_service_regression_matrix_v1`
- `calendar_state_fixture_references` must point to existing case IDs in
  `tests/fixtures/calendar/calendar_service_regression_matrix.json`
- Scenario `expected_outcome.event_outcomes[*].expected_market_state` is validated against
  `resolve_market_state` outputs for each injected event timestamp

Window coverage is intentional and explicit:

- `xnys_intraday_open_window_gap_suppression` maps to intraday references
- `xnys_overnight_closed_window_gap_emission` maps to overnight references
- `xtks_weekend_closed_window_gap_emission` maps to weekend references
- `xnys_overnight_close_open_boundary_transition` maps to overnight session-edge references
- `xnys_weekend_close_open_boundary_transition` maps to weekend session-edge references

Boundary coverage is required in addition to intraday, overnight, and weekend.

## Boundary Semantics

Session windows follow the canonical calendar boundary policy:

- Session open is inclusive
- Session close is exclusive
- Exact session-open timestamps resolve `open`
- Exact session-close timestamps resolve `closed`

The boundary transition scenarios include data-driven `event_pre_*`, `event_at_*`, and
`event_post_*` event IDs around close and open transitions so regression tests can validate
edge behavior without hardcoded timestamp logic.

## Determinism Rules

- Cases are consumed in declared order.
- Event lists are consumed in declared order.
- Content hash scope is the full document payload bytes.

## Output Schema Metadata

`output_schema_metadata` is intentionally explicit so harness and report implementations can validate output payloads consistently across engines.
