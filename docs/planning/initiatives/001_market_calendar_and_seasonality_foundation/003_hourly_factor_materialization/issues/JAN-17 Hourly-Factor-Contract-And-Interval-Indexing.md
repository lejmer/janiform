## Hourly Factor Contract And Interval Indexing

### Description
Define the canonical data contract for hourly factors and interval indexing used by downstream engines. The contract must represent hourly buckets, exchange context, and values for `s_h` and `nu_h`.

### Acceptance Criteria
- A documented schema exists for hourly factor outputs with required fields and metadata.
- Interval indexing rules are explicit for start-inclusive and end-exclusive bucket semantics.
- Contract supports deterministic representation across date ranges and exchanges.
- Invalid interval requests fail with actionable errors.

### Unit tests
- Schema validation tests for required fields and value types.
- Interval indexing tests for consecutive-hour coverage and no overlaps.
- Negative tests for invalid date ranges and unknown exchange identifiers.

### Notes/Constraints
- Keep contract independent from pricing or simulation internals.
- Include metadata fields needed for auditability of materialization choices.
