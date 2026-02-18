# Production Risk Mitigation Playbook

This playbook defines a general, stage-agnostic process for identifying and fixing production risk early. Use it for every meaningful change, regardless of current initiative, subsystem, or pain point.

## Objective

Catch risks early that can cause:

- incorrect model behavior
- broken integrations
- non-deterministic outputs
- operational incidents
- costly downstream rework

## When To Run This Review

Run this review at least:

- before implementation for medium or high impact work
- after a Project has been otherwise completed
- before release candidates

For urgent fixes, run a lightweight version first, then run the full review immediately after stabilization.

## Universal Review Workflow

1. Scope the blast radius.
2. Check contract and behavior stability.
3. Check correctness and failure modes.
4. Check determinism and reproducibility.
5. Check observability and operability.
6. Check performance and resource profile.
7. Check DRY and duplication risk.
8. Check test quality and coverage gaps.
9. Record findings with severity.
10. Implement fixes based on above findings.

## Risk Categories And What To Check

### 1. Contract And Compatibility Risk

Check for:

- unintentional API shape changes
- schema drift between Rust, PyO3, Python wrappers, fixtures, and docs
- renamed or removed fields without migration path
- inconsistent defaults across language boundaries

### 2. Correctness And Model Integrity Risk

Check for:

- invariant violations
- incorrect boundary semantics
- silent fallback behavior that changes outputs unexpectedly
- mismatch between implementation and documented model assumptions

### 3. Determinism And Reproducibility Risk

Check for:

- non-deterministic iteration order
- unstable serialization output
- hidden time or locale dependencies
- fixture loading paths that bypass integrity checks

### 4. Failure Handling And Diagnostics Risk

Check for:

- ambiguous error messages
- swallowed exceptions
- generic failures with no actionable context
- missing identifiers in diagnostics for triage

### 5. Data Integrity And Validation Risk

Check for:

- weak input validation at boundaries
- permissive parsing that hides malformed payloads
- unvalidated cast paths
- inconsistent unit or timestamp assumptions

### 6. Operational And Observability Risk

Check for:

- missing high-level logs at critical boundaries
- inability to trace failure to fixture, request, exchange, or interval
- missing machine-readable report artifacts for CI
- no clear policy for unknown or unsupported inputs

### 7. Performance And Capacity Risk

Check for:

- avoidable repeated parsing or conversions
- unnecessary copying in hot paths
- high fixed overhead in common workflows
- algorithmic complexity regressions

### 8. Test And Validation Risk

Check for:

- duplicated assertions in multiple stacks
- weak failure-path coverage
- missing determinism tests for repeated runs
- fixture drift not protected by content hash or schema checks

### 9. DRY And Duplication Risk

Check for:

- repeated logic inside functions/methods that can become hard to maintain
- repeated logic across Rust core, PyO3 bindings, and Python wrappers that can drift
- duplicate validation rules implemented in multiple layers with inconsistent behavior
- repeated constants or mapping tables that should have a single source of truth
- copy-pasted test setup or fixtures that hide shared behavior changes
- copy-pasted helpers that cause unnecessary code bloat

## Severity Policy

### Blocker

- can cause incorrect financial output in production
- can break external integration without explicit version bump
- can produce silent data corruption
- can cause non-deterministic results in deterministic workflows

### High

- likely to cause incidents under realistic usage
- materially degrades diagnosability or rollback safety
- creates major cross-language contract inconsistency

### Medium

- raises maintenance burden or increases probability of future defects
- creates avoidable duplication that can drift

### Low

- readability or consistency improvement with low near-term impact

## Required Output For Each Review

For each finding, record:

- `severity`
- `category`
- `location` (file and line)
- `impact`
- `trigger conditions`
- `summary of fixes`

If no findings are present, simply do nothing.

## Minimum Evidence Checklist

Before completion, ensure evidence exists for:

- contract compatibility checks
- deterministic rerun equivalence for relevant paths
- failure-path diagnostics quality
- fixture and schema integrity checks
- test coverage for changed behavior

## Maintenance Rule

Keep this playbook evergreen. If a new incident class appears, add a new check item here so the same failure mode is caught earlier next time.
