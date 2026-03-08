## Initiative 008: Seeded Simulation Semantics And Consistency Alignment

Align the seeded simulation implementation with `docs/design/janiform_compact_model_spec.tex` and `docs/design/janiform_design_brainstorm_notes.md`, specifically around market-state routing, exogenous news mark semantics, known-surprise event decomposition, and remaining seeded-model consistency constraints.

### Desired outcome
- Remove unresolved contradictions between the compact model contract and the seeded simulation code paths.
- Bring seeded tick event generation into the same shared-domain semantics as shared-news dispatch and return decomposition.
- Ensure model-level parameter constraints that affect skew and asymmetry are enforced consistently across core and Python validation boundaries.
- Ensure seeded fine-path event scheduling remains consistent with event-time Hawkes semantics expected by the model contract.
- Produce targeted issues and implementation outcomes that can be verified against concrete regression fixtures.

### Success signals
- Seeded and shared-news paths use the same market-state resolution logic for `M_t` and `G_t` routing.
- Exogenous news in seeded mode uses the same severity and return-jump model contract as the documented spec.
- Known and surprise news behavior in seeded mode matches explicit component decomposition without informal shortcuts.
- `delta_beta` skew-asymmetry constraints are enforced consistently across calibration, core model construction, and Python validation.
- Seeded fine-path arrival behavior does not rely on interval-level approximations that break event-time Hawkes consistency expectations.
- Completion of the project updates the issue tracker with clear acceptance criteria and test expectations.
