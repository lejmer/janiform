## Augmented State Anchored Pricing Engine

### Description
Implement the actual anchored pricing engine for European vanilla options under the augmented state introduced in the previous issue.

The current pricing engine assumes one state-independent characteristic function for the full observed return over the pricing interval. That assumption breaks once the observed return depends on the boundary deviation state. This issue must replace that assumption with an engine that propagates value across the augmented state

```latex
\[
Z_h = (Y_h, D_h).
\]
```

The engine must continue to use the existing Janiform core as the innovation driver, but the option pricer must now evolve continuation values over the deviation dimension instead of pretending the entire observed return remains affine in the old sense.

### Required engine scope
- Price European vanilla options for anchored configurations over one or more pricing intervals.
- Support the same anchor families already introduced by the mean-reversion rollout:
  - disabled mean reversion
  - constant anchor
  - deterministic interval-grid anchor
- Use the state-dependent local pricing measure from the new anchored pricing contracts.
- Expose anchored-pricing diagnostics that make the augmented-state recursion inspectable.

### Required engine behavior
- For one interval and a fixed augmented boundary state, evaluate the anchored pricing expectation using the new local pricing measure and the Janiform core transform for that interval.
- For multi-interval maturities, propagate continuation values across the augmented state rather than collapsing the problem into one old-style Fourier inversion.
- Reuse existing affine-oracle or core-transform components where mathematically valid, but refactor freely if a substantial rewrite is needed to make the anchored engine coherent.
- Do not introduce magic numbers or numerical heuristics unless absolutely necessary. If you do, document them in the code with clear comments explaining why, what the alternative is, and why the alternative is worse.
- The engine must carry and evolve deviation state explicitly through maturity. It must not assume that observed log-price can be reconstructed from the core path alone.
- The old single-measure Fourier runner may remain useful only for the nested non-anchored case if that is still the cleanest implementation after the rewrite.

### Required numerical work
- Introduce one canonical approach for the deviation dimension, such as a documented grid, quadrature, or other deterministic state-discretization rule.
- Document and implement boundary handling, truncation policy, and convergence diagnostics for that deviation-state representation.
- Provide deterministic diagnostics for:
  - local tilt solves used during pricing
  - state-grid or quadrature resolution
  - continuation-value recursion quality
  - any enforced admissibility domain limits

### Files and modules that should be updated
- Pricing engine runner, transform bridge, and numerical engine modules in Rust
- Any affine-oracle or risk-neutral helper modules that must be refactored to support the augmented state
- Pybind pricing bridge if execution contracts change
- High-level `price_option(...)` preparation if the execution input contract changes at the native boundary
- Don't make monoliths. If a file becomes too large (2k lines+) refactor to multiple smaller files (this includes refactoring the entire original file, not just your additions).

### Acceptance Criteria
- Anchored configurations with `kappa > 0` price successfully through the native engine rather than failing at the boundary.
- The nested case `kappa = 0` and flat anchor remains a valid special case after the rewrite.
- Multi-interval anchored prices are computed through explicit augmented-state propagation, not by silently calling the old one-step Fourier runner.
- Engine diagnostics make it possible to inspect deviation-state resolution and local-martingale solve quality.
- No fallback strips the mean-reversion layer before pricing.

### Unit tests
- One-interval anchored pricing regressions for:
  - constant anchor with zero deviation
  - constant anchor with non-zero deviation
  - deterministic interval-grid anchor
- Nested-case regression showing agreement with the non-anchored engine when `kappa = 0` and the anchor is flat.
- Numerical-quality tests covering deviation-grid or quadrature refinement behavior.

### Integration tests
- End-to-end pricing runs for anchored configurations from public pricing entrypoints down to the native engine.
- Monte Carlo versus engine comparison tests on small synthetic anchored scenarios with fixed seeds and documented tolerances.

### Notes/Constraints
- A substantial rewrite of the pricing engine is acceptable if it leads to one coherent anchored implementation.
- Keep the pricing scope to European vanilla options in this issue unless broader payoff support becomes mathematically trivial.
- Remember to not run multiple test suites at the same time when measuring their performance in elapsed time, as there is a physical CPU in the machine, so parallel runs will make them run slower.
