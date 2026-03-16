# Validation Fallback Policy

This guide defines the v0.1 fallback policy for calendar and seasonality validation flows.
The policy is deterministic and is surfaced in regression harness reports so outcomes stay auditable.

## Policy Scope

The policy applies to:

- Rust validation entrypoint: `janiform::verification::run_deterministic_regression_harness`
- Python validation entrypoint: `janiform.validation.run_deterministic_regression_harness`
- Shared fixture schema validation for `tests/fixtures/calendar` and `tests/fixtures/hourly_factors`

## Policy Contract

The harness report exposes `fallback_policy` with:

- `schema_version`: `validation.fallback_policy.v1`
- `unknown_date_policy`: `resolve_with_regular_sessions_when_no_date_override.v1`
- `unsupported_exchange_calendar_segment_policy`: `fail_case_with_explicit_calendar_resolution_error.v1`
- `incomplete_fixture_coverage_policy`: `fail_fast_on_fixture_contract_validation.v1`

## Unknown Date Behavior

Unknown dates are dates that are not explicitly listed in date-scoped override sets such as
`holiday_dates` or `early_close_sessions`.

Fallback behavior:

- Resolve using the exchange regular weekday session schedule.
- Do not silently invent holiday or anomaly overrides.
- Keep behavior deterministic from calendar definition plus UTC timestamp.

## Unsupported Exchange Calendar Segment Behavior

Unsupported segments include missing exchange identifiers and invalid exchange timezone
configuration that prevent resolver execution.

Fallback behavior:

- Validation case fails.
- Failure is explicit in harness output (`calendar_resolution_error`).
- No success result is emitted from fallback in this case.

## Incomplete Fixture Coverage Behavior

Incomplete fixture coverage means canonical fixture contracts are missing required stable
fixture IDs or required tags.

Fallback behavior:

- Fixture validation fails fast before regression execution.
- No synthetic fixture IDs are injected.
- No relaxed comparison mode is used.

## Failure Modes

- Calendar resolver input cannot be resolved for the requested exchange segment.
- Fixture metadata and observed case sets diverge.
- Fixture source data is empty or structurally incomplete in missing-data guard scenarios.

## Triage Steps

1. Confirm the failure category from harness report `failure_kind` or fixture validation error text.
2. For unknown-date surprises, inspect `holiday_dates` and `early_close_sessions` for the local exchange date.
3. For unsupported exchange segments, verify `exchange_code` and `timezone_name` in the shared registry artifact.
4. For incomplete fixture coverage, reconcile `canonical_regression_metadata` stable IDs with observed case IDs.
5. Update fixture payload plus manifest hash together when a behavior change is intentional.

## Examples

Unknown-date example:

- Input: `exchange_code="XNYS"`, timestamp `2027-02-02T14:30:00Z`.
- Date is not in `holiday_dates` and not in `early_close_sessions`.
- Result: resolved by regular weekday session schedule.

Incomplete-fixture example:

- Input: calendar fixture with one boundary case removed from `cases` while
  `stable_fixture_ids_by_tag.boundary` still references it.
- Result: fixture validation fails with stable-ID mismatch.
