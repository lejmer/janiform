# Stylized Fact Metric Catalog Schema

This guide documents the canonical stylized-fact metric catalog contract used by
validation workflows in Initiative 005 Project 002.

## Scope

- Rust entrypoints:
  - `janiform_core::verification::build_stylized_fact_metric_catalog`
  - `janiform_core::verification::load_stylized_fact_metric_catalog_from_json`
  - `janiform_core::verification::serialize_stylized_fact_metric_catalog_to_json`

The catalog is independent from plotting and report-rendering layers.

## Top-level catalog schema

- `schema_version`: `validation.stylized_fact.metric_catalog.v1`
- `catalog_id`
- `catalog_version`
- `schema_metadata`
  - `schema_family`: `validation.stylized_fact.metric_catalog`
  - `schema_generation`: `v1`
  - `forward_extension_policy`: `additive_fields_only.v1`
- `naming_conventions`
  - `naming_policy`: `snake_case_metric_ids_used_as_canonical_output_keys.v1`
  - `metric_id_format`: `snake_case`
  - `horizon_label_policy`: `explicit_horizon_bucket_labels_with_no_implicit_interpolation.v1`
- `uncertainty_policy`
  - `default_policy_id`: `dependent_return_series_default_block_bootstrap_with_approved_overrides.v1`
  - `dependent_return_series_default_method`: `block_bootstrap`
  - `approved_override_methods`: explicit allow-list for approved alternatives
- `insufficient_data_semantics`
  - `status_code`: `insufficient_data`
  - `decision_policy`: `emit_insufficient_data_with_coverage_and_skip_threshold_decision.v1`
  - `non_terminal_decision_state`: `true`
- `output_schema_metadata`
  - machine-readable required-field lists and canonical vocabularies
- `metrics[]`

## Metric schema

Each metric entry includes:

- `metric_id` (unique snake_case canonical output key)
- `metric_name` (human-readable label)
- `metric_class`: `primary_robust` or `secondary_moment`
- `metric_family`:
  - primary robust: `quantile_shape`, `tail_risk`, `distribution_distance`
  - secondary moment: `l_moment`, `central_moment`
- `domain`:
  - `activity_clustering`
  - `heavy_tails`
  - `skew`
  - `volatility_persistence`
  - `open_window_direct_jump_behavior`
  - `closed_window_gap_behavior`
- `definition`
- `unit`
- `horizon_applicability`
- `sample_dependence`
- `uncertainty`
- `sample_sufficiency`
- `tags`

Secondary-moment metrics additionally require:

- `estimator_metadata`
- `secondary_moment_variant`

Primary-robust metrics must not define secondary-only fields.

## Uncertainty and sample sufficiency contract

Per-metric `uncertainty` includes:

- `default_method`
- `confidence_level`
- `bootstrap_replication_count`
- `block_length_horizon_count`
- `override_policy`

For `sample_dependence=dependent_return_series`, validation enforces
`default_method=block_bootstrap`.

Per-metric `sample_sufficiency` includes:

- `minimum_sample_count`
- `minimum_effective_sample_count`
- `minimum_tail_observation_count`
- `minimum_horizon_coverage_ratio`

`tail_risk` families require `minimum_tail_observation_count`.

## Machine-readable output metadata

`output_schema_metadata` carries:

- required field lists for:
  - catalog
  - all metrics
  - primary metrics
  - secondary metrics
  - uncertainty blocks
  - sample-sufficiency blocks
  - estimator metadata blocks
- canonical vocabularies for:
  - metric classes
  - metric families
  - domains
  - uncertainty methods

This allows deterministic schema checks in downstream comparison and reporting
pipelines.
