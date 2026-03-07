# Calibration Experiment Index And Lookup Helpers

This guide defines the lightweight experiment index used to find persisted calibration runs by model version, dataset snapshot, validation outcomes, and commit timestamps.

## Scope

- The index applies only to calibration runs with `execution_mode=disk`.
- Persisted artifacts remain optional overall.
- CLI calibration flows default to persisted artifacts ON through `execution_mode=disk`.
- Imported package usage defaults to persisted artifacts OFF through `execution_mode=memory` unless explicitly enabled.
- `execution_mode=memory` runs do not emit committed disk artifacts and therefore do not appear in the experiment index.

The index is derived from committed manifest metadata. It is not a second source of truth and it does not require any external database.

## Storage layout

For persistence root `{artifact_root}`, the index file is stored at:

- `{artifact_root}/artifacts/index/calibration_experiment_index.json`

Committed run manifests remain under:

- `{artifact_root}/artifacts/runs/{run_id}/artifact_manifest.json`

## Schema

Schema version:

- `calibration.artifact_registry.experiment_index.v1`

Top-level fields:

- `schema_version`
- `schema_family`
- `schema_generation`
- `rebuild_policy`
- `ordering_policy`
- `records`

Each record contains:

- `run_id`
- `execution_mode`
- `manifest_schema_version`
- `index_metadata`

`index_metadata` is copied from the committed manifest and therefore includes:

- commit timestamps
- exchange code
- dataset snapshot checksum
- trade-events and optional option-surface dataset references plus checksums
- compact model-version summary
- optional diagnostics or comparison summaries

## Ordering and determinism

Records are sorted deterministically:

1. descending `index_metadata.committed_at_unix_nanos`
2. ascending `run_id`

Lookup responses preserve this ordering before pagination is applied.

## Lookup API

The canonical Rust lookup entrypoint is:

- `calibration::lookup_calibration_experiment_index_records(...)`

Supported exact-match filters include:

- exchange code
- dataset snapshot checksum
- trade-events data reference and checksum
- option-surface data reference and checksum
- run-contract and run-spec schema versions
- stage-output schema versions
- seasonality estimator version
- diagnostics or comparison overall status
- failing metric identifier

Supported range filters include:

- `committed_at_or_after_unix_nanos`
- `committed_before_unix_nanos`

Pagination uses:

- `offset`
- `limit`

`limit` must be positive.

## Commit and rebuild behavior

Commit behavior:

- the pending run directory is renamed into the committed run directory first
- the committed manifest is then loaded and upserted into the index
- if index update fails after the rename, the committed run artifacts remain on disk and the failure is surfaced so the caller can rebuild the index explicitly

Rebuild behavior:

- `calibration::rebuild_calibration_experiment_index_from_disk(...)` rescans committed run manifests under `{artifact_root}/artifacts/runs/`
- each record is rederived from the committed manifest metadata
- the rebuilt document overwrites the previous index file atomically through the shared artifact writer path

## Recovery expectations

- If the index file is deleted or corrupted while committed run manifests remain intact, rebuild the index from disk.
- If a committed run manifest is itself corrupted, rebuild fails clearly instead of guessing or silently dropping fields.
- No compatibility shims are provided for unsupported schema variants during pre-v0.1 development.
