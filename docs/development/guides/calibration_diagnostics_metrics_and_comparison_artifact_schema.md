# Calibration Diagnostics Metrics And Comparison Artifact Schema

This guide defines the persisted validation-metric artifact contract used for
calibration-quality diagnostics and comparison outputs in Initiative 005 Project
004.

## Scope

- The artifact applies only to calibration runs with `execution_mode=disk`.
- Persisted artifacts are optional overall.
- CLI-facing calibration workflows default to persisted artifacts ON through
  `execution_mode=disk`.
- Imported package usage defaults to persisted artifacts OFF through
  `execution_mode=memory` unless explicitly overridden.
- The schema supports two persisted artifact roles:
  - `diagnostics`
  - `comparison`
- The artifact links back to the run manifest and the persisted stage outputs
  used to produce the validation payload.

## Canonical schema

Schema version:

- `calibration.artifact_registry.validation_metric.v1`

Top-level fields:

- `schema_version`
- `run_id`
- `execution_mode`
- `artifact_role`
- `artifact_reference`
- `schema_metadata`
- `output_schema_metadata`
- `manifest_link`
- `stage_output_links`
- `metric_collections`

`schema_metadata` fields:

- `schema_family`: `calibration.artifact_registry.validation_metric`
- `schema_generation`: `v1`
- `forward_extension_policy`:
  `breaking_changes_allowed_with_lockstep_in_repo_updates_pre_v0_1.v1`

`manifest_link` fields:

- `artifact_reference`: `artifact://manifest/{run_id}`
- `schema_version`: `calibration.artifact_registry.manifest.v1`

`stage_output_links[]` fields:

- `stage`
- `artifact_reference`
- `stage_output_schema_version`

`metric_collections[]` fields:

- `collection_id`
- `source_kind`
- `source_schema_version`
- `status`
- `context`
- `primary_role_label`
- `secondary_role_label` when the source is pairwise
- `threshold_policy_id`
- `metrics`

`context` fields:

- `context_kind`
- `context_id`
- `context_label`

`metrics[]` fields:

- `metric_id`
- `metric_name`
- `metric_variant`
- `status`
- `primary_value`
- `secondary_value`
- `delta_value`
- `absolute_delta`
- `standardized_delta`
- `threshold_metadata`

`primary_value` and `secondary_value` fields:

- `value`
- `confidence_interval`

`confidence_interval` fields:

- `lower_bound`
- `upper_bound`
- `confidence_level`

`threshold_metadata` fields:

- `threshold_policy_id`
- `threshold_policy_version`
- `threshold_id`
- `threshold_rationale`
- `max_absolute_delta`
- `max_standardized_delta`
- `absolute_delta_exceeds_threshold`
- `standardized_delta_exceeds_threshold`

## Supported vocabularies

`artifact_role`:

- `diagnostics`
- `comparison`

`status`:

- `pass`
- `fail`
- `insufficient_data`
- `available`
- `missing`
- `error`

`source_kind`:

- `activity_clustering`
- `tail_skew`
- `volatility_persistence`
- `gap_behavior`
- `stylized_fact_horizon_comparison`
- `scale_consistency`

`context_kind`:

- `run`
- `batch`
- `horizon`
- `scenario`
- `stage`

## Linkage and integrity rules

- `manifest_link.schema_version` must match
  `calibration.artifact_registry.manifest.v1`.
- `manifest_link.artifact_reference` must equal `artifact://manifest/{run_id}`.
- `stage_output_links[]` must be non-empty.
- Every `stage_output_links[]` entry must resolve to a manifest entry with:
  - `artifact_kind = stage_output`
  - matching `stage`
  - matching `artifact_reference`
- `run_id` must match the linked manifest `run_id`.

## Metric completeness rules

Validation rejects:

- blank identifiers or labels
- blank threshold-policy identifiers
- missing threshold bounds
- threshold exceedance flags without the corresponding bound
- non-finite value, delta, or threshold fields
- invalid confidence intervals
- duplicate `(metric_id, metric_variant)` keys inside one collection
- `pass`, `fail`, or `available` metrics that contain no value or delta fields

## Persistence paths and manifest usage

For a disk-mode run with checkpoint root `{artifact_root}` and `run_id={run_id}`:

- Diagnostics role artifact:
  `{artifact_root}/artifacts/runs/{run_id}/diagnostics.validation_metric.artifact.json`
- Comparison role artifact:
  `{artifact_root}/artifacts/runs/{run_id}/comparison.validation_metric.artifact.json`

Pending exports use the same filenames under `{artifact_root}/artifacts/pending/{run_id}/` until the run-level commit publishes the full artifact set.

Canonical artifact references:

- Diagnostics role: `artifact://diagnostics/{run_id}`
- Comparison role: `artifact://comparison/{run_id}`

When linked from the calibration artifact manifest:

- diagnostics artifacts use `artifact_kind = diagnostics`
- comparison artifacts use `artifact_kind = comparison`

Writes use the same atomic tempfile then rename policy as the other calibration
artifact-registry outputs.
