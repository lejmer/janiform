## Seeded Event-Time Hawkes Scheduling Consistency

### Description
Remove or harden seeded fine-path scheduling shortcuts that derive per-interval arrival counts from static interval expectations and then place arrivals uniformly, so seeded tick simulation is consistent with event-time Hawkes semantics expected by the model contract.

### Acceptance Criteria
- Replace interval-level count plus uniform-timestamp scheduling for endogenous arrivals with an event-time compatible scheduling approach, or introduce an explicitly documented and test-gated compatibility mode that preserves model-consistent behavior by default.
- Ensure intra-interval self and cross-excitation can influence within-interval arrival behavior in the seeded fine path.
- Ensure coarse-vs-fine consistency harness inputs can exercise the updated fine-path scheduler without hidden assumptions about uniform intra-interval placement.

### Unit tests
- Deterministic seeded simulation fixture where event-time Hawkes updates change subsequent same-interval arrival dynamics relative to a no-feedback baseline.
- Regression test that fails if seeded endogenous arrivals revert to pure interval-level static-count plus uniform-placement behavior in model-consistent mode.
- Integration test tying seeded fine-path aggregation outputs to coarse sufficient-statistics diagnostics for controlled scenarios.

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Keep existing deterministic seeding guarantees and reproducibility artifacts intact.
- Avoid introducing duplicated scheduling logic if shared Hawkes transition helpers can be reused.
