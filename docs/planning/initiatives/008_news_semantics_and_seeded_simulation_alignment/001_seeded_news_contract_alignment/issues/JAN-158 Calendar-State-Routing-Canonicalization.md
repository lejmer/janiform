## Calendar-State Routing Canonicalization

### Description
Refactor seeded tick simulation news routing to use explicit market-state resolution from exchange calendar utilities instead of seasonality-factor inference.

### Acceptance Criteria
- Replace interval-level seasonality routing heuristics with calendar-backed open/closed state resolution in seeded pathways that produce `M_t` and `G_t` updates.
- Ensure news events in known-open and known-closed windows map consistently to open-jump and gap-jump channels.
- Add regression tests for interval boundaries and edge cases such as early closes and holiday closures.

### Unit tests
- Deterministic seeded interval fixture proving a known open-news timestamp updates `M_t` and not `G_t`.
- Deterministic seeded interval fixture proving a known closed-news timestamp updates `G_t` and not `M_t`.
- Boundary test for ambiguous factor values where calendar resolution is the only source of truth.

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Do not alter unrelated Poisson scheduling logic in this issue.
