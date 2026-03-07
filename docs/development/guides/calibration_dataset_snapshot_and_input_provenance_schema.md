# Calibration Dataset Snapshot And Input Provenance Schema

This guide defines the machine-readable dataset snapshot artifact emitted for persisted calibration runs.

## Scope

- The artifact applies only to calibration runs with `execution_mode=disk`.
- Persisted artifacts are optional overall.
- CLI-facing calibration flows default to persisted artifacts ON through `execution_mode=disk`.
- Imported package usage defaults to persisted artifacts OFF through `execution_mode=memory` unless explicitly overridden.
- The artifact stores lightweight dataset references and provenance metadata only. Raw event tapes and option surface payloads are not embedded.

## Canonical schema

Schema version:

- `calibration.artifact_registry.dataset_snapshot.v1`

Top-level fields:

- `schema_version`
- `run_id`
- `execution_mode`
- `schema_metadata`
- `output_schema_metadata`
- `exchange_code`
- `provenance`
- `input_datasets`
- `snapshot_checksum`

`schema_metadata` fields:

- `schema_family`: `calibration.artifact_registry.dataset_snapshot`
- `schema_generation`: `v1`
- `forward_extension_policy`: `breaking_changes_allowed_with_lockstep_in_repo_updates_pre_v0_1.v1`

`output_schema_metadata` fields:

- `schema_version`: `calibration.artifact_registry.dataset_snapshot.output_schema_metadata.v1`
- `required_artifact_fields`
- `required_provenance_fields`
- `required_input_dataset_fields`

`provenance` fields:

- `artifact_manifest_schema_version`
- `run_contract_schema_version`
- `run_spec_schema_version`
- `data_inputs_schema_version`
- `execution_mode_default_policy`
- `persistence_scope_policy`
- `stage_input_naming_policy`
- `reference_checksum_algorithm`
- `snapshot_checksum_algorithm`

`input_datasets[]` fields:

- `input_name`
- `primary_stage`
- `data_reference`
- `reference_checksum`

Supported `input_name` values:

- `trade_events_data_reference`
- `option_surface_data_reference`

Stage mapping policy:

- `trade_events_data_reference` maps to `seasonality`
- `option_surface_data_reference` maps to `return_components`

Checksum policies:

- `reference_checksum_algorithm`: `fnv1a_64_hex_utf8.v1`
- `reference_checksum_scope`: `reference_string_utf8_bytes.v1`
- `snapshot_checksum_algorithm`: `fnv1a_64_hex_utf8.v1`
- `snapshot_checksum_scope`: `canonical_artifact_identity_reference_and_provenance_fields_utf8_bytes.v1`

Example:

```json
{
  "schema_version": "calibration.artifact_registry.dataset_snapshot.v1",
  "run_id": "run-001",
  "execution_mode": "disk",
  "schema_metadata": {
    "schema_family": "calibration.artifact_registry.dataset_snapshot",
    "schema_generation": "v1",
    "forward_extension_policy": "breaking_changes_allowed_with_lockstep_in_repo_updates_pre_v0_1.v1"
  },
  "output_schema_metadata": {
    "schema_version": "calibration.artifact_registry.dataset_snapshot.output_schema_metadata.v1",
    "required_artifact_fields": [
      "schema_version",
      "run_id",
      "execution_mode",
      "schema_metadata",
      "output_schema_metadata",
      "exchange_code",
      "provenance",
      "input_datasets",
      "snapshot_checksum"
    ],
    "required_provenance_fields": [
      "artifact_manifest_schema_version",
      "run_contract_schema_version",
      "run_spec_schema_version",
      "data_inputs_schema_version",
      "execution_mode_default_policy",
      "persistence_scope_policy",
      "stage_input_naming_policy",
      "reference_checksum_algorithm",
      "snapshot_checksum_algorithm"
    ],
    "required_input_dataset_fields": [
      "input_name",
      "primary_stage",
      "data_reference",
      "reference_checksum"
    ]
  },
  "exchange_code": "XNYS",
  "provenance": {
    "artifact_manifest_schema_version": "calibration.artifact_registry.manifest.v1",
    "run_contract_schema_version": "calibration.orchestration.run_contract.v1",
    "run_spec_schema_version": "calibration.orchestration.run_spec.v1",
    "data_inputs_schema_version": "calibration.orchestration.data_inputs.v1",
    "execution_mode_default_policy": "cli_defaults_to_disk_python_importable_api_defaults_to_memory.v1",
    "persistence_scope_policy": "artifact_required_only_for_execution_mode_disk_when_persistence_is_enabled.v1",
    "stage_input_naming_policy": "trade_events_data_reference_maps_to_seasonality_and_option_surface_data_reference_maps_to_return_components.v1",
    "reference_checksum_algorithm": "fnv1a_64_hex_utf8.v1",
    "snapshot_checksum_algorithm": "fnv1a_64_hex_utf8.v1"
  },
  "input_datasets": [
    {
      "input_name": "trade_events_data_reference",
      "primary_stage": "seasonality",
      "data_reference": "dataset://synthetic/trades/v1",
      "reference_checksum": "e0ce78f355bf943f"
    },
    {
      "input_name": "option_surface_data_reference",
      "primary_stage": "return_components",
      "data_reference": "dataset://synthetic/options/v1",
      "reference_checksum": "cda3581918e16fef"
    }
  ],
  "snapshot_checksum": "f15e6dc91df8f7dc"
}
```

## Validation rules

Validation rejects:

- unsupported `schema_version`
- `execution_mode` values other than `disk`
- blank `run_id` or `exchange_code`
- missing or blank provenance fields
- `output_schema_metadata` required field lists that do not match the canonical contract
- empty `input_datasets`
- duplicate `input_name` values
- missing `trade_events_data_reference`
- `primary_stage` values that do not match the canonical stage mapping
- malformed lowercase hex checksum fields
- `reference_checksum` values inconsistent with `data_reference`
- `snapshot_checksum` values inconsistent with the canonical artifact content

## Persistence path

For a disk-mode run with checkpoint root `{artifact_root}` and `run_id={run_id}`, the dataset snapshot artifact is written to:

- committed artifact path: `{artifact_root}/artifacts/runs/{run_id}/dataset_snapshot.artifact.json`
- pending write path before commit: `{artifact_root}/artifacts/pending/{run_id}/dataset_snapshot.artifact.json`

Writes remain atomic at the file level, and the full run is published atomically by renaming the pending run directory into the committed artifact directory.
