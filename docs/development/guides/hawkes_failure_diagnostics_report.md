# Hawkes Failure Diagnostics Report

This guide defines the machine-readable triage payload used by Hawkes invariant and regression
suites when failures are detected.

## Report schema

- `schema_version`: `hawkes.failure_diagnostic_report.v1`
- `determinism_policy`: `stable_category_classification_and_sorted_metadata.v1`
- `suite_id`: deterministic suite identifier, for example `hawkes_equation_reference_suite`
- `test_name`: deterministic test function name that produced the report
- `check_class`: `deterministic_equation` or `statistical_property`
- `invariant_ids`: sorted catalog IDs tied to the failing test
- `summary`:
  - `violation_count`
  - `category_counts[]` where each item has `category` and `count`
- `violations[]` where each item has:
  - `category`
  - `scope`
  - `detail`
  - `observed_value` (optional numeric)
  - `reference_lower_bound` (optional numeric)
  - `reference_upper_bound` (optional numeric)
  - `delta_relative_to_reference` (optional numeric)

## Category taxonomy

The triage categories are stable and deterministic:

- `constraint`
- `transition`
- `ordering`
- `statistical_drift`
- `scale_consistency_drift`
- `event_time_grid_leakage`
- `timestamp_snapping_regression`

`scale_consistency_drift` and `event_time_grid_leakage` are dedicated categories for
coarse-vs-aggregated drift and event-time grid leakage diagnostics.

## Example triage report

```json
{
  "schema_version": "hawkes.failure_diagnostic_report.v1",
  "determinism_policy": "stable_category_classification_and_sorted_metadata.v1",
  "suite_id": "hawkes_cluster_signature_statistical_regression",
  "test_name": "cluster_signature_scale_consistency_reports_metric_deltas_when_bands_are_violated",
  "check_class": "statistical_property",
  "invariant_ids": [
    "HK-SCALE-001"
  ],
  "summary": {
    "violation_count": 2,
    "category_counts": [
      {
        "category": "scale_consistency_drift",
        "count": 2
      }
    ]
  },
  "violations": [
    {
      "category": "scale_consistency_drift",
      "scope": "panel=scale_consistency horizon=1h metric=absolute_buy_share_delta",
      "detail": "observed=0.11 reference_band=[0.0, 0.08] delta_relative_to_band=0.03",
      "observed_value": 0.11,
      "reference_lower_bound": 0.0,
      "reference_upper_bound": 0.08,
      "delta_relative_to_reference": 0.03
    },
    {
      "category": "scale_consistency_drift",
      "scope": "panel=scale_consistency horizon=1h metric=absolute_sell_share_delta",
      "detail": "observed=0.11 reference_band=[0.0, 0.08] delta_relative_to_band=0.03",
      "observed_value": 0.11,
      "reference_lower_bound": 0.0,
      "reference_upper_bound": 0.08,
      "delta_relative_to_reference": 0.03
    }
  ]
}
```

## Usage notes

- Reports are designed for fast root-cause triage and avoid environment-specific paths.
- Deterministic and statistical suites emit the same report shape for consistent tooling.
- Category mapping is deterministic to keep repeated runs comparable.
