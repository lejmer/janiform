# Calibration Artifact Manifest Schema And Versioning Policy

This guide defines the top-level manifest contract for persisted calibration artifacts.

## Scope

- The manifest applies only to calibration runs with `execution_mode=disk`.
- Persisted artifacts are optional overall.
- CLI workflows default to persisted artifacts ON.
- Imported package usage defaults to persisted artifacts OFF unless explicitly enabled.
- `execution_mode=memory` runs do not require or emit this manifest.

The manifest is intentionally minimal. It records lightweight references to persisted artifacts and does not embed large payloads directly.

## Canonical schema

Schema version:

- `calibration.artifact_registry.manifest.v1`

Top-level fields:

- `schema_version`: exact manifest schema identifier.
- `run_id`: calibration run identifier.
- `execution_mode`: must be `disk`.
- `index_metadata`: lightweight lookup summary derived from committed artifact metadata.
- `artifact_references`: non-empty list of referenced persisted artifacts.

Index metadata fields:

- `committed_at_utc`: RFC3339 UTC timestamp recorded when the run artifacts were committed.
- `committed_at_unix_nanos`: integer nanosecond timestamp paired with `committed_at_utc`.
- `exchange_code`: exchange code from the dataset snapshot artifact.
- `dataset_snapshot_checksum`: dataset snapshot checksum from the dataset snapshot artifact.
- `trade_events_data_reference`: canonical trade-events dataset reference string.
- `trade_events_reference_checksum`: checksum of `trade_events_data_reference`.
- `option_surface_data_reference`: optional option-surface dataset reference string.
- `option_surface_reference_checksum`: checksum of `option_surface_data_reference` when present.
- `model_version`: compact model-version summary used by experiment lookup.
- `validation_metric_summaries`: optional summaries for manifest-linked diagnostics or comparison artifacts.

Model-version fields:

- `run_contract_schema_version`
- `run_spec_schema_version`
- `seasonality_stage_output_schema_version`
- `hawkes_stage_output_schema_version`
- `return_components_stage_output_schema_version`
- `seasonality_estimator_version`

Validation-metric summary fields:

- `artifact_role`: `diagnostics` or `comparison`
- `artifact_reference`
- `overall_status`
- `collection_count`
- `metric_count`
- `collection_status_counts`
- `metric_status_counts`
- `failing_metric_ids`

Artifact reference fields:

- `artifact_id`: unique identifier within the manifest.
- `artifact_kind`: high-level category for the referenced artifact.
- `artifact_reference`: lightweight URI or path-like reference to the persisted artifact.
- `stage`: optional calibration stage when the artifact is stage-specific.

Supported `artifact_kind` values:

- `run_contract`
- `dataset_snapshot`
- `stage_output`
- `stage_checkpoint`
- `diagnostics`
- `comparison`
- `metadata`

Example:

```json
{
  "schema_version": "calibration.artifact_registry.manifest.v1",
  "run_id": "run-001",
  "execution_mode": "disk",
  "index_metadata": {
    "committed_at_utc": "2026-03-05T12:30:00.000000000Z",
    "committed_at_unix_nanos": 1772713800000000000,
    "exchange_code": "XNYS",
    "dataset_snapshot_checksum": "d2d24ac777f770eb",
    "trade_events_data_reference": "dataset://synthetic/trades/v1",
    "trade_events_reference_checksum": "388fa40f89d1d462",
    "option_surface_data_reference": "dataset://synthetic/options/v1",
    "option_surface_reference_checksum": "1f86174674bcc833",
    "model_version": {
      "run_contract_schema_version": "calibration.orchestration.run_contract.v1",
      "run_spec_schema_version": "calibration.orchestration.run_spec.v1",
      "seasonality_stage_output_schema_version": "calibration.orchestration.stage_output.seasonality.v1",
      "hawkes_stage_output_schema_version": "calibration.orchestration.stage_output.hawkes.v1",
      "return_components_stage_output_schema_version": "calibration.orchestration.stage_output.return_components.v1",
      "seasonality_estimator_version": "seasonality_estimator.v1"
    },
    "validation_metric_summaries": [
      {
        "artifact_role": "diagnostics",
        "artifact_reference": "artifact://diagnostics/run-001",
        "overall_status": "fail",
        "collection_count": 1,
        "metric_count": 2,
        "collection_status_counts": [
          {"status": "fail", "count": 1}
        ],
        "metric_status_counts": [
          {"status": "pass", "count": 1},
          {"status": "fail", "count": 1}
        ],
        "failing_metric_ids": ["tail_skew.hourly"]
      }
    ]
  },
  "artifact_references": [
    {
      "artifact_id": "run_contract",
      "artifact_kind": "run_contract",
      "artifact_reference": "artifact://run_contract/run-001"
    },
    {
      "artifact_id": "seasonality_stage_output",
      "artifact_kind": "stage_output",
      "artifact_reference": "artifact://seasonality/run-001",
      "stage": "seasonality"
    },
    {
      "artifact_id": "seasonality_checkpoint",
      "artifact_kind": "stage_checkpoint",
      "artifact_reference": "artifact://stage_checkpoint/seasonality/run-001",
      "stage": "seasonality"
    }
  ]
}
```

## Validation rules

Validation must reject:

- missing required top-level fields
- unsupported `schema_version`
- `execution_mode` values other than `disk`
- blank `run_id`
- malformed or inconsistent `index_metadata`
- empty `artifact_references`
- blank `artifact_id`
- blank `artifact_reference`
- duplicate `artifact_id` values within one manifest

## Persistence path and commit flow

For a disk-mode run with checkpoint root `{artifact_root}` and `run_id={run_id}`, the manifest is written to:

- committed artifact path: `{artifact_root}/artifacts/runs/{run_id}/artifact_manifest.json`
- pending run directory during execution: `{artifact_root}/artifacts/pending/{run_id}/`
- quarantine directory for recovered partial writes: `{artifact_root}/artifacts/quarantine/`
- resumable checkpoints remain separate under `{artifact_root}/checkpoints/{run_id}/`

Commit semantics:

- dataset snapshot, stage output artifacts, and the manifest are first written under the pending run directory
- successful completion atomically publishes the full artifact set by renaming the pending run directory to the committed run directory
- after the directory rename, the experiment index is upserted from the committed manifest metadata
- fresh non-resume runs quarantine stale pending or incomplete run directories instead of overwriting them
- committed run directories are never overwritten silently

## Versioning policy

Current implementation:

- Janiform currently supports manifest schema `calibration.artifact_registry.manifest.v1` only.
- In-repo producers and consumers are updated in lockstep during initial development.
- Unsupported schema generations fail validation explicitly.

Released-schema policy:

- Additive compatible fields may be introduced without changing the schema family.
- Breaking manifest changes must use an explicit schema version bump.
- Every breaking schema change must ship with release notes that describe the contract change, affected fields, and the required upgrade action.
- New readers must fail clearly on unsupported schema versions instead of guessing or silently coercing payloads.

## Upgrade and deprecation guidance

Upgrade guidance:

- Prefer regenerating manifests with the current Janiform writer instead of editing files manually.
- If migration tooling is provided in a future release, use that tooling to rewrite older manifests before reload.
- Treat manifest schema upgrades as part of the persisted calibration artifact upgrade, not as an isolated file patch.

Deprecation guidance:

- When a future released schema supersedes an older one, release notes should mark the older schema as deprecated and state the intended removal window.
- Deprecation notices should name the replacement schema version and any migration tooling.
- If correctness or safety requires immediate removal, the release notes must state that no compatibility bridge is provided.
