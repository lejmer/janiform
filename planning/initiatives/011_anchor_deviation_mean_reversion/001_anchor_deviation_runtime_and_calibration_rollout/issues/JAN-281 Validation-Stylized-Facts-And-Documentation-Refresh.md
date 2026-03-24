## Validation Stylized Facts And Documentation Refresh

### Description
Refresh the validation suite, stylized-fact diagnostics, fixtures, examples, and working docs so the new anchor-deviation semantics are testable and understandable.

This issue closes the rollout. It must not invent new runtime semantics. It must update validation and documentation to match the contracts and engines implemented in earlier issues.

Once mean reversion is enabled, the library must distinguish between:
- Janiform core increments
- observed returns
- observed log-price state
- deviation from anchor

Validation and docs must make that split explicit everywhere.

### Required validation updates
- Update seeded and coarse regression fixtures to include observed-state fields.
- Update coarse versus seeded consistency harnesses to compare observed-state outputs on aligned grids.
- Update any path diagnostics, stylized-fact generators, or benchmark harnesses that currently assume `core cumulative return == observed log-price`.
- Add mean-reversion-specific diagnostics such as:
  - terminal deviation from anchor
  - maximum absolute deviation from anchor
  - deviation half-life implied by `kappa`
  - long-horizon variance-ratio stabilization relative to the nested `kappa = 0` case

The half-life diagnostic should use

```latex
\[
t_{1/2} = \frac{\log 2}{\kappa}
\]
```

for `kappa > 0`.

### Required documentation updates
- Update working model docs so the executable runtime semantics are unambiguous.
- Document the interval-grid anchor rule:

```latex
\[
X_{h+1} = a_{h+1} + e^{-\kappa \Delta_h}(X_h - a_h) + R_h.
\]
```

- Document the seeded runtime semantics:
  - anchor fixed inside interval
  - continuous deviation decay between seeded transitions
  - anchor reset only at interval boundaries
- Document pricing limitations for anchored configs if the pricing gate remains in place.
- Update examples so any mean-reverting simulation example shows:
  - anchor
  - deviation
  - observed price path

### Files and modules that should be updated
- Validation tests and fixtures under `tests/` and `crates/janiform-validation/`
- Stylized-fact and governance harnesses if they consume simulation outputs
- README and development guides
- `docs/design/janiform_compact_model_spec.tex`
- Examples under `examples/`

### Acceptance Criteria
- Validation fixtures and harnesses understand the new observed-state fields and no longer misread core return fields as observed price.
- Stylized-fact and diagnostic reports can surface anchor/deviation-aware metrics when mean reversion is enabled.
- The compact model spec and implementation-facing docs describe the same executable semantics as the runtime.
- Examples demonstrate the new layer without requiring conversational context or hidden assumptions.
- All tests and public docs pass after the earlier issues are implemented.

### Unit tests
- Fixture schema tests for any new observed-state fields.
- Stylized-fact adapter tests for new mean-reversion-aware diagnostic rows or plots.
- Export and reporting tests showing observed price is derived from observed-state fields.

### Integration tests
- End-to-end seeded and coarse simulation workflow examples for:
  - disabled mean reversion
  - constant-anchor mean reversion
- Documentation regression tests if command-reference or example snapshots exist for the updated surfaces.

### Notes/Constraints
- Do not defer docs and fixtures to a later cleanup. They must land in the same change that finalizes the rollout.
- Keep the docs implementation-facing and precise. The target reader must be able to implement the feature without relying on external conversation history.
