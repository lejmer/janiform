## Known And Surprise Arrival Derivation

### Description
Normalize seeded news arrival generation so known and surprise components are modeled as explicit arrival components, matching the intended decomposition and superposition logic.

### Acceptance Criteria
- Replace residual-count based surprise derivation with an explicit implementation of known and surprise arrivals that is consistent with `nu_known + nu_surprise`.
- Ensure event ordering and timestamps remain stable under fixed-seed simulation.
- Add tests for edge cases when known and surprise overlap in expectation or when known-rate changes within an interval.

### Unit tests
- Property test over seeded intervals where known arrivals are fixed and surprise is sampled independently from the documented contract.
- Integration test that verifies total news event count and routed component totals match `nu_news` decomposition for controlled fixtures.

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Keep Poisson and mark generation assumptions explicit in contracts and code comments.
