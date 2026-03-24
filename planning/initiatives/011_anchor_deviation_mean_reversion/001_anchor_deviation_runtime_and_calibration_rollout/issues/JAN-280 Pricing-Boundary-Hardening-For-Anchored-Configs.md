## Pricing Boundary Hardening For Anchored Configs

### Description
Harden all pricing and risk-neutral entrypoints so anchored mean-reversion configurations cannot silently flow through the old Esscher-only pricing logic.

This issue is required to keep the library mathematically correct while mean reversion is being rolled out through simulation and calibration.

The old pricing stance is:

```latex
\[
\psi_Q(u) = \psi_P(u + i\theta) - \psi_P(i\theta),
\]
```

with martingale validation applied to the Janiform core increment alone.

That is not sufficient once the observed process becomes

```latex
\[
X_{h+1} = a_{h+1} + \phi_h (X_h - a_h) + R_h,
\qquad
\phi_h = e^{-\kappa \Delta_h}.
\]
```

The observed increment is then

```latex
\[
X_{h+1} - X_h
=
(a_{h+1} - a_h)
+
(\phi_h - 1)(X_h - a_h)
+
R_h.
\]
```

Under the old minimal-change pricing stance,

```latex
\[
\mathbb E^Q\!\left[e^{X_{h+1}-X_h}\mid \mathcal F_h\right]
=
\exp\!\Big(
(a_{h+1}-a_h)
+
(\phi_h - 1)(X_h-a_h)
\Big)
\,
\mathbb E^Q\!\left[e^{R_h}\mid \mathcal F_h\right].
\]
```

This cannot equal a state-independent martingale target such as

```latex
\[
e^{(r-q)\Delta_h}
\]
```

for all boundary states unless `kappa = 0` or the pricing measure itself becomes state-dependent.

Therefore the old pricing implementation must not be reused for anchored configurations.

### Required behavior
- Every pricing boundary must detect enabled mean reversion and fail with a structured unsupported-configuration error.
- The old risk-neutral martingale check must reject anchored configurations explicitly.
- High-level `JaniformModel.price_option(...)` must fail before calling the old pricer when mean reversion is enabled.
- Documentation must state clearly that anchored configs are unsupported for pricing until a new pricing measure is defined.

### Files and modules that should be updated
- Pricing contracts and validation in Rust and Python
- High-level model pricing preparation in [`model.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/calibration/fit/model.py)
- Risk-neutral helper utilities
- CLI and examples that expose pricing
- Pricing contract docs

### Acceptance Criteria
- Anchored configurations are rejected at every pricing entrypoint with a structured, deterministic error message.
- No anchored config reaches the old Fourier pricing runner or the old Esscher martingale validator.
- The nested case `kappa = 0` or disabled mean reversion remains supported by the existing pricing workflow.
- Pricing docs and examples no longer imply that anchored configs are already supported.

### Unit tests
- Rust and Python validation tests showing anchored pricing requests fail with the documented error.
- High-level model tests showing `price_option(...)` rejects anchored fitted models before native pricing dispatch.
- Nested-case tests showing disabled mean reversion still routes through the existing pricing engine.

### Notes/Constraints
- This issue is a correctness gate, not the anchored pricing implementation itself.
- Do not add a hidden fallback that strips the mean-reversion block and prices the remaining core.
