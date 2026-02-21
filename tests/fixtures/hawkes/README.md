# Hawkes Invariant Catalog Fixture

This fixture stores the canonical, machine-readable Hawkes invariant catalog used by regression and invariant test suites.

## File

- `hawkes_invariant_catalog.json`

## Contract

- `schema_version`: `hawkes.invariant.catalog.v1`
- `catalog_id`: `affine_hawkes_invariant_catalog_v1`
- `classification_rules`: deterministic equation vs statistical-property classification policy
- `categories`: invariant grouping taxonomy
- `invariants`: unique invariant IDs, rationale, and suite test references

## Coverage

- Constraint, dynamics, boundary behavior, sampling properties, and scale-consistency categories
- Explicit invariant classes for non-grid event-time semantics and coarse-vs-aggregated scale consistency
