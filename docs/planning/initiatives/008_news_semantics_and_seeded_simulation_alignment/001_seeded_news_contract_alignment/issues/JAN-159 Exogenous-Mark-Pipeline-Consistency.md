## Exogenous-Mark Pipeline Consistency

### Description
Align seeded exogenous news mark generation with the documented model contract used by the shared-news stack.

### Acceptance Criteria
- Ensure seeded activity coupling marks and return jump marks are produced from the same documented pipeline: Gamma severity mark and independent Kou shock composition where appropriate.
- Remove any simplified or ad hoc coupling transform that changes return mark moments relative to the documented contract.
- Add tests comparing seeded sampled moments or invariants against known-good shared-news behavior for equivalent seeds.

### Unit tests
- Unit test for seeded exogenous mark outputs with a fixed seed against a reference contract.
- Statistical sanity test that seeded exogenous marks preserve sign and scale properties expected from the shared gamma-Kou process.

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Preserve existing public-facing APIs and avoid changing unrelated function signatures unless required by this project.
