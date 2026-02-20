# Seeded Tick Event Tape Schema

This guide defines the deterministic event tape payload emitted by the seeded tick simulator in `crates/core/src/simulation.rs`.

## Schema versions

- `simulation.seeded_tick_event.path.v1`: top-level tape payload schema (`schema_version`)
- `simulation.seeded_tick_event.tape_metadata.v1`: metadata schema (`metadata.schema_version`)
- `simulation.seeded_tick_event.record.v1`: event record schema (`metadata.event_record_schema_version`)
- `simulation.seeded_tick_event.state_snapshot.v1`: optional state snapshot schema (`metadata.state_snapshot_schema_version`)

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
