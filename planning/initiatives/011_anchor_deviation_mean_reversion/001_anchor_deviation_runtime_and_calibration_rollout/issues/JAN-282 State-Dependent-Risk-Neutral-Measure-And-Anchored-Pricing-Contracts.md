## State Dependent Risk Neutral Measure And Anchored Pricing Contracts

### Description
Replace the old core-only Esscher pricing boundary with the canonical anchored pricing measure and contract surface.

Once mean reversion is enabled, the observed interval return is

```latex
\[
\Delta X_h
=
(a_{h+1} - a_h)
+
(\phi_h - 1)(X_h - a_h)
+
R_h,
\qquad
\phi_h = e^{-\kappa \Delta_h}.
\]
```

That means the martingale target depends on the observed-state boundary input through `D_h = X_h - a_h`. One global Esscher parameter is no longer sufficient.

This issue must define the canonical augmented pricing state

```latex
\[
Z_h = (Y_h, D_h),
\]
```

where `Y_h` is the existing Janiform boundary state and `D_h` is the anchor deviation, and then define the state-dependent local pricing measure on top of the Janiform core contribution `R_h`.

Let

```latex
\[
K_h^P(\xi \mid Z_h)
=
\log \mathbb E^P[e^{\xi R_h} \mid Z_h].
\]
```

The issue must introduce one canonical local tilt `theta_h(Z_h)` satisfying

```latex
\[
K_h^P(1 + \theta_h(Z_h) \mid Z_h)
-
K_h^P(\theta_h(Z_h) \mid Z_h)
=
(r-q)\Delta_h
-
(a_{h+1} - a_h)
-
(\phi_h - 1)D_h.
\]
```

This is the local martingale constraint for the anchored observed return. It must be expressed in executable contracts and diagnostics, not only in narrative docs.

### Required contract scope
- Add canonical Rust and Python contract types for anchored pricing boundary input, including:
  - mean-reversion configuration
  - aligned interval anchors
  - observed-state boundary input
  - any required boundary Hawkes state already used by pricing
- Define canonical diagnostics for one local tilt solve, including:
  - boundary state summary
  - target exponent
  - solved `theta`
  - residual
  - admissibility/domain status
- Replace the old martingale validator contract with an augmented-state validator that operates on anchored observed returns.
- Define structured failure modes for:
  - no admissible local tilt
  - non-finite transform inputs
  - out-of-domain state requests
  - inconsistent anchor alignment or observed-state payloads

### Required implementation rules
- Preserve the existing Janiform core transform machinery where it remains correct for `R_h`.
- Do not pretend that anchored pricing is still a one-parameter global Esscher problem when `kappa > 0`.
- Do not add a fallback that drops `D_h` or strips mean reversion before pricing.
- The nested case `kappa = 0` must appear as a special case of the new contracts rather than as a separate hidden path.

### Files and modules that should be updated
- Pricing contracts and validation in Rust and Python
- Shared risk-neutral helper utilities
- Pricing-side model preparation that currently assumes a state-independent martingale check
- Pricing contract docs and schema docs

### Acceptance Criteria
- There is one canonical anchored pricing request surface that carries the full augmented boundary state needed for pricing.
- The martingale condition for anchored pricing is expressed and validated against the observed return, not only against the Janiform core increment.
- One local state-dependent tilt solve returns deterministic diagnostics for valid inputs.
- Invalid or inadmissible anchored pricing states fail with structured deterministic errors.
- In-repo call sites and docs no longer describe anchored pricing as unsupported at the contract layer.

### Unit tests
- Contract validation tests for anchored pricing boundary input and observed-state payloads.
- Local martingale-solver tests for:
  - `kappa = 0`
  - positive `kappa` with zero deviation
  - positive `kappa` with non-zero deviation
  - inadmissible states where no valid tilt exists
- Rust and Python serialization round-trip tests for the new pricing contracts and diagnostics.

### Integration tests
- Deterministic anchored pricing boundary tests showing the new contract layer accepts valid anchored states and rejects malformed ones before the engine runs.

### Notes/Constraints
- This issue defines the anchored pricing measure and executable contracts. It does not yet implement the full multi-interval option pricer.
- Keep schema identifiers at `v1` while replacing the old pricing contracts directly.
