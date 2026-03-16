# Calendar and Seasonality API Contracts

This guide documents the calendar and seasonality interfaces used by Janiform.
It contains the detailed contract material moved from the repo root `README.md`.

## Calendar Query Example

```python
from datetime import UTC, datetime

from janiform.calendar import CalendarRegistry, MarketState

calendar_registry_json = """
{
  "schema_version": "calendar.registry.v1",
  "artifact_date": "2026-01-31",
  "exchanges": [
    {
      "schema_version": "calendar.v1",
      "exchange_code": "XNYS",
      "exchange_name": "New York Stock Exchange",
      "timezone_name": "America/New_York",
      "rule_metadata": {
        "holiday_calendar_code": "US_EQUITIES",
        "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
      },
      "regular_sessions": [
        {
          "weekday": "Monday",
          "windows": [
            {"label": "regular", "open_time": "09:30", "close_time": "16:00"}
          ]
        }
      ],
      "holiday_dates": [],
      "early_close_sessions": []
    }
  ]
}
"""

calendar_registry = CalendarRegistry(calendar_registry_json)
market_state = calendar_registry.resolve_market_state(
    "XNYS",
    datetime(2026, 2, 2, 14, 30, 0, tzinfo=UTC),
)
assert market_state == MarketState.OPEN
```

## Seasonality Timestamp Event Input Schema

Use `preprocess_timestamp_events(...)` before nonparametric seasonality estimation.

- Schema version: `seasonality.timestamp_event.v1`
- Required event field: `'timestamp'`
- Accepted timestamp types:
  - timezone-aware `datetime`
  - RFC3339 or ISO-8601 string with timezone offset
- Exchange context:
  - `exchange_code` is required
  - session classification is resolved through `CalendarRegistry`
  - timestamps are normalized to UTC and exchange-local timezone
- Determinism:
  - output events are sorted by normalized UTC timestamp and input position
  - repeated runs with identical input produce identical output
- Sparse coverage diagnostics:
  - preprocessing returns missing and sparse hourly buckets for the selected session filter
- Output layout:
  - Rust returns compact columnar data suitable for large datasets
  - `market_state_codes` and `exchange_local_day_of_week` are compact `bytes` (`u8` values)

## Intraday Seasonality Estimation

Use `estimate_intraday_seasonality(...)` with preprocessed events to estimate `f_intra(tod)` from
timestamp density with explicit smoothing controls.

- Input:
  - preprocessed event output (`exchange_local_minute_of_day`)
  - estimator settings (`minutes_per_bin`, kernel, bandwidth, pseudocount)
- Output:
  - deterministic machine-readable bins (`bin_start_minute_of_day`, `intraday_factor_by_bin`)
  - normalized component with anchored mean (`mean(intraday_factor_by_bin) = 1`)

## Day Of Week, Day Of Year, And Composition

Use `estimate_day_of_week_seasonality(...)` and `estimate_day_of_year_seasonality(...)` on the
same preprocessed event output to estimate deterministic weekly and yearly effects.

- Day-of-week output:
  - fixed weekday buckets (`day_of_week_number` values `1..7`, Monday-first)
  - sparse bucket flags and fallback flags for deterministic missing-data handling
  - normalized factor component (`day_of_week_factor`) and additive log component
- Day-of-year output:
  - fixed daily buckets (`day_of_year_number` values `1..366`)
  - sparse bucket flags and fallback flags for deterministic missing-data handling
  - normalized factor component (`day_of_year_factor`) and additive log component

Use `compose_deterministic_seasonality_signal(...)` to combine intraday, day-of-week, and
day-of-year log components into a unified deterministic signal per accepted timestamp event.

- Combined signal normalization:
  - applies a deterministic mean-anchor policy over the fit window
  - enforces `mean(combined_factor_by_event) = 1` for identifiability with baseline intensity
  - fails fast if normalization assumptions are violated
- Normalization metadata:
  - policy identifier, fit-window event count, pre and post means
  - normalization constant and log offset used to anchor `s(t)`
- Scheduled-event overlay support:
  - optional `scheduled_event_overlays` can be passed to the same composition API
  - overlay config records can be loaded with `load_scheduled_event_overlay_definitions(...)`
  - each overlay record supports:
    - `overlay_id`, `start_timestamp_utc`, `end_timestamp_utc`, `log_adjustment`
    - optional `precedence`, `enabled`, and `source_label`
  - precedence and merge policy:
    - highest precedence wins when windows overlap
    - overlays at the same precedence merge additively in log-space
    - windows are `start` inclusive and `end` exclusive
  - output artifacts expose overlay provenance metadata:
    - configured and applied overlay ids
    - per-event overlay ids and active flags
    - deterministic precedence, merge, and window-boundary policy identifiers

## Seasonality Diagnostics Reports

Use `generate_seasonality_diagnostics_report(...)` to produce a stable machine-readable report
for one dataset, and `generate_seasonality_diagnostics_reports_batch(...)` to emit a batch report.

- Report artifacts:
  - schema versioned output (`seasonality.diagnostics.report.v1`)
  - intraday, day-of-week, day-of-year, and optional overlay component summaries
  - fit-quality metrics for downstream validation tools
  - reproducibility metadata:
    - input snapshot fingerprint and event window
    - estimator configuration snapshot
    - estimator version label
- Batch artifacts:
  - schema versioned output (`seasonality.diagnostics.batch.v1`)
  - deterministic dataset ordering and per-dataset report entries

## Hourly Factor Contract And Interval Indexing

Use `materialize_hourly_seasonality_factors(...)` to convert source seasonality samples into
interval-aligned `s_h` values on the canonical fixed-width UTC interval grid. Hourly calibration
inputs may still be materialized onto sub-hour or multi-hour runtime grids.

- Materialization schema:
  - schema version: `seasonality.hourly_s_h.materialization.v1`
  - exchange context and UTC interval fields match the canonical hourly contract
  - output values: `s_h`
- Configurable deterministic policies:
  - interpolation method: `previous_observation_carry_forward.v1` or `linear_between_observations.v1`
  - aggregation method: `mean_of_observations.v1` or `last_observation.v1`
  - sparse fallback policy: `global_mean.v1` or `previous_materialized_then_global_mean.v1`
  - sparse interval threshold: `sparse_hour_minimum_observation_count`
- Auditability metadata:
  - materialization policy identifiers and sparse threshold are emitted with output
  - sparse, interpolated, and fallback interval counts are emitted in diagnostics

Use `materialize_hourly_news_intensity_factors(...)` to convert closed-window news baseline policy
into interval-aligned `nu_h` values on the same canonical fixed-width UTC interval grid.

- Materialization schema:
  - schema version: `seasonality.hourly_nu_h.materialization.v1`
  - exchange context and UTC interval fields match the canonical hourly contract
  - output values: `nu_h`
- Deterministic policy:
  - closed-window baseline intensity is applied from calendar market-state resolution
  - optional open-window overrides are keyed by interval index for scenario studies
- Auditability metadata:
  - `materialization_method`
  - `calendar_market_state_dependency`
  - `open_window_override_policy`
  - `closed_window_baseline_intensity`

Use `build_hourly_factor_contract(...)` to emit the canonical factor contract consumed by
downstream engines.

- Contract schema:
  - schema version: `seasonality.hourly_factor.contract.v1`
  - exchange context: `exchange_code`, `exchange_timezone_name`
  - interval-aligned values: `s_h` and `nu_h`
  - explicit interval width: `interval_duration_nanos`
  - UTC interval fields: `interval_indices`, `interval_start_utc_nanos`, `interval_end_utc_nanos`
- Interval semantics:
  - request range and buckets are half-open intervals `[start, end)`
  - interval boundary policy identifier: `start_inclusive_end_exclusive.v1`
  - interval alignment policy identifier: `request_start_anchored_fixed_width_utc_interval_grid.v1`
  - timezone transition policy identifier: `fixed_width_utc_interval_projected_to_exchange_local.v1`
  - spring-forward policy identifier: `skip_nonexistent_local_hour_labels.v1`
  - fall-back policy identifier: `preserve_repeated_local_hour_labels_with_numeric_utc_offset.v1`
  - request duration must divide exactly by `interval_duration_nanos`
  - local timestamp labels are projected from UTC bucket boundaries and include numeric UTC offsets
  - missing local hours during spring-forward emit no local-hour bucket
  - repeated local hours during fall-back are disambiguated by offset in labels
- Auditability metadata:
  - `seasonality_materialization_method`
  - `news_intensity_materialization_method`
  - deterministic policy identifiers for interval boundary and alignment choices

## Shared Hourly Factor Provider Integration

Use `provide_hourly_factor_contract(...)` from `janiform::hourly_factors` as the
single integration point for hourly factor delivery.

- Provider API behavior:
  - accepts one request with optional seasonality and news-intensity sources
  - supports explicit hourly values or deterministic materialization in the same API
  - derives the target runtime grid from `range_start_utc`, `range_end_utc`, and interval width
  - returns canonical interval-aligned `s_h` and `nu_h` contract output
  - returns structured provider errors for missing inputs and unsupported ranges
- Downstream engine usage:
  - canonical call: `janiform::hourly_factors::provide_hourly_factor_contract(...)`
  - module entrypoints:
    - `janiform::pricing::load_hourly_factor_contract(...)`
    - `janiform::simulation::load_hourly_factor_contract(...)`
- Python user-facing API:
  - shared provider entrypoint: `janiform.provide_hourly_factor_contract(...)`
  - pricing adapter: `janiform.load_hourly_factor_contract_for_pricing(...)`
  - simulation adapter: `janiform.load_hourly_factor_contract_for_simulation(...)`

## Validation Fallback Policy

Deterministic regression harness outputs include a machine-readable `fallback_policy` block
that captures unknown-date, unsupported-segment, and incomplete-fixture handling rules.

- Rust entrypoint: `janiform::verification::run_deterministic_regression_harness(...)`
- Python entrypoint: `janiform.validation.run_deterministic_regression_harness(...)`

See `docs/development/guides/validation_fallback_policy.md` for policy identifiers,
failure modes, and triage workflow.
