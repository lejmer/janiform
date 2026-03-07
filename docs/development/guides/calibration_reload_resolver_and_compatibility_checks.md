# Calibration Reload Resolver And Compatibility Checks

This guide defines the run-level reload behavior for persisted calibration
artifacts in `execution_mode=disk`.

## Scope

- Reload applies only to runs with committed artifacts under the calibration
  artifact registry layout.
- Persisted artifacts remain optional overall.
- CLI calibration flows default to persisted artifacts ON through
  `execution_mode=disk`.
- Imported package usage defaults to persisted artifacts OFF through
  `execution_mode=memory` unless explicitly enabled.
- `execution_mode=memory` runs do not emit committed disk artifacts and are
  therefore outside reload scope.

The canonical Rust entrypoint is:

- `calibration::reload_calibration_artifact_run_from_disk(...)`

The reload path is read-only and must stay decoupled from calibration execution
side effects.

## Request model

Reload requests use:

- `CalibrationArtifactReloadRequest::complete_run(run_id)`
- `CalibrationArtifactReloadRequest::stage_subset(run_id, requested_stages)`

Selection behavior:

- `complete_run` loads the dataset snapshot, all manifest-linked stage parameter
  artifacts in deterministic stage order, and any manifest-linked diagnostics or
  comparison validation-metric artifacts.
- `stage_subset` loads the dataset snapshot plus only the requested stage
  parameter artifacts in deterministic stage order.
- Stage subset reload does not load optional diagnostics or comparison artifacts.

Deterministic order is always:

1. `seasonality`
2. `hawkes`
3. `return_components`

## Compatibility checks

Reload validates:

- committed run directory exists
- manifest schema version matches
  `calibration.artifact_registry.manifest.v1`
- dataset snapshot schema version matches
  `calibration.artifact_registry.dataset_snapshot.v1`
- stage parameter artifact schema version matches
  `calibration.artifact_registry.stage_parameter.v1`
- validation metric artifact schema version matches
  `calibration.artifact_registry.validation_metric.v1`
- required manifest references exist for the requested scope
- each loaded artifact `run_id` matches the requested run
- each stage artifact matches the expected stage and manifest-linked
  `artifact_reference`
- validation metric artifacts pass manifest linkage checks before being returned

Pre-v0.1 schema policy remains strict:

- unsupported schema generations fail clearly
- no fallback parsing or compatibility shim is attempted
- in-repo producers and consumers are updated in lockstep when contracts change

## Failure model

Reload returns structured errors through `CalibrationArtifactReloadError`.

Primary failure classes:

- `RunArtifactsUnavailable`
- `MissingArtifactFile`
- `ArtifactReadFailed`
- `UnsupportedSchema`
- `CorruptedArtifact`
- `MissingRequiredComponent`
- `DuplicateManifestComponentReference`
- `ArtifactRunIdMismatch`
- `ArtifactReferenceMismatch`
- `StageParameterStageMismatch`
- `ValidationMetricIntegrityFailed`

`UnsupportedSchema` is reserved for explicit schema-version incompatibility.
Malformed JSON, failed deserialization, and failed artifact validation are
reported as `CorruptedArtifact`.

## Workflow usage

The reloaded output is designed for downstream consumers that need persisted
calibration state without re-running calibration:

- validation workflows can consume full-run reloads including optional
  diagnostics or comparison artifacts when they are present in the manifest
- pricing or other model-consumption workflows can request stage-specific
  subsets when they only need selected fitted parameter artifacts plus dataset
  provenance

If a required component is missing, reload fails instead of synthesizing a
placeholder object.
