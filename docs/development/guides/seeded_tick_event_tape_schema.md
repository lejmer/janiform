# Seeded Tick Event Tape And Debug Trace Schema

This guide defines the deterministic event tape payload and optional debug trace payload emitted by
the seeded tick simulator in `crates/core/src/simulation.rs`.

## Schema versions

- `simulation.seeded_tick_event.path.v1`: top-level tape payload schema (`schema_version`)
- `simulation.seeded_tick_event.tape_metadata.v1`: metadata schema (`metadata.schema_version`)
- `simulation.seeded_tick_event.record.v1`: event record schema (`metadata.event_record_schema_version`)
- `simulation.seeded_tick_event.state_snapshot.v1`: optional state snapshot schema (`metadata.state_snapshot_schema_version`)
- `simulation.seeded_tick_event.debug_trace.v1`: optional debug trace top-level schema (`schema_version`)
- `simulation.seeded_tick_event.debug_trace_metadata.v1`: debug trace metadata schema (`metadata.schema_version`)
- `simulation.seeded_tick_event.debug_trace_record.v1`: debug trace record schema (`metadata.trace_record_schema_version`)
- `simulation.seeded_tick_event.debug_trace_chunk.v1`: debug trace chunk schema (`metadata.trace_chunk_schema_version`)

## Top-level payload

The canonical serialized payload contains:

- `schema_version`: path schema version
- `metadata`: machine-readable tape metadata
- `event_tape`: ordered list of event records

## Metadata fields

- `event_ordering_policy`: deterministic ordering policy for tied timestamps
- `timestamp_representation_policy`: confirms event-time nanosecond timestamps with no bucket identifier requirement
- `side_encoding_policy`: side encoding policy (`buy`, `sell`)
- `state_snapshot_capture`: snapshot capture mode (`disabled`, `post_event_transition`)
- `state_snapshot_capture_point_policy` (optional): populated when snapshots are enabled

## Hourly boundary semantics

The simulator consumes interval-aligned `s_h` and `nu_h` factors from the shared hourly factor contract.

- Interval boundary policy:
  - `hourly_factor_start_inclusive_end_exclusive_with_strict_consecutive_coverage.v1`
- State carryover policy:
  - `full_4d_hawkes_excitation_state_carryover_across_interval_boundaries.v1`
- Factor control policy:
  - `interval_aligned_s_h_nu_h_control_intensity_only_without_intra_interval_grid.v1`
- Intra-interval scheduling policy:
  - `event_time_updates_with_sequential_hawkes_transition_composition.v1`

Validation behavior:

- Hourly interval coverage must be consecutive and non-overlapping across the simulated horizon.
- Missing hourly intervals (gaps) and interval overlaps fail fast during run-config validation.
- Factor intervals control intensity scaling and news intensity only.
- Event timestamps are sampled in event time inside each interval and are never snapped to a fixed intra-interval lattice.

## Event record fields

Required:

- `event_index`
- `timestamp_utc_nanos`
- `side`

Optional:

- `state_snapshot`

## State snapshot fields

When enabled, each record snapshot contains:

- `y_plus_plus`
- `y_plus_minus`
- `y_minus_plus`
- `y_minus_minus`
- `deseasonalized_lambda_plus`
- `deseasonalized_lambda_minus`

Snapshot capture point:

- `post_event_transition_pre_following_same_timestamp_updates.v1`

This means the snapshot is taken immediately after the event transition is applied, before any later update at the same timestamp.

## Debug trace mode

Debug tracing is controlled through run config options:

- `debug_trace_enabled`: enables or disables trace emission without changing primary event outputs
- `debug_trace_options.sample_every_nth_transition`: captures every Nth transition record
- `debug_trace_options.max_records_per_chunk`: bounds per-chunk payload size

Debug traces always capture `interval_boundary_drift` transitions to preserve carryover diagnostics.

## Debug trace metadata fields

- `transition_ordering_policy`: deterministic transition ordering policy
- `state_carryover_policy`: explicit policy for interval-boundary state carryover
- `sampling_policy`: deterministic sampling policy string
- `chunking_policy`: deterministic chunking policy string
- `sample_every_nth_transition`: configured sampling stride
- `max_records_per_chunk`: configured chunk size bound
- `interval_boundary_drift_always_captured`: explicit carryover-capture flag

## Debug trace top-level fields

- `schema_version`: debug trace schema version
- `metadata`: machine-readable debug trace metadata
- `captured_transition_count`: number of captured transition records after sampling
- `trace_chunks`: ordered list of bounded-size chunks

## Debug trace chunk fields

- `schema_version`
- `chunk_index`
- `start_transition_index`
- `end_transition_index`
- `records`

## Debug trace record fields

Required:

- `transition_index`
- `interval_index`
- `previous_timestamp_utc_nanos`
- `timestamp_utc_nanos`
- `elapsed_hours_since_previous_update`
- `transition_kind` (`endogenous_buy_event`, `endogenous_sell_event`, `exogenous_news`, `interval_boundary_drift`)
- `seasonality_factor`
- `interval_news_intensity`
- `state_before_transition`
- `state_after_transition`

Optional:

- `generation_order`
- `event_index`
- `exogenous_news_severity_mark`
