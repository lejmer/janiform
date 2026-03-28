# Endogenous Anchor Mean Reversion And Pricing Specification

## Purpose

This document defines the canonical endogenous-anchor version of the Janiform
mean-reversion layer.

It replaces the exogenous-anchor interpretation where the fitted model stores a
future anchor path or requires the caller to supply one. Janiform must remain a
self-contained generative and pricing model. The anchor therefore has to be
generated from Janiform's own state under both the physical measure `P` and the
risk-neutral measure `Q`.

## Design decision

The anchor is not an externally supplied future curve.

The canonical anchor is instead the model-generated predictable level implied by
the current Janiform state and the relevant measure.

For implementation purposes:

- the fitted model persists the mean-reversion speed `kappa`
- the fitted model persists the anchor-generation policy
- the runtime materializes interval-aligned anchor values from the model state
- the pricing engine materializes risk-neutral anchor values from the
  risk-neutral state recursion

Interval-aligned anchor values remain useful as runtime artifacts. They are not
the canonical top-level model family, and the old exogenous interval-grid anchor
model must not remain as a parallel fitted-model option after this rewrite. Instead,
these are merely the runtime projection of the model.

## State split

Use two layers of state on the executable interval grid.

### Structural Janiform state

Let `Sigma_h` denote the structural Janiform boundary state needed to generate
the next core interval contribution under the physical measure. At minimum this
includes:

- Hawkes boundary state `Y_h`
- calendar or session phase needed to evaluate deterministic seasonality
- known-news schedule state needed to evaluate deterministic scheduled news over
  the next interval
- any other Markov boundary variables required by the core transition law

The observed-state layer must not feed back into `Sigma_h`.

### Observed-state layer

Let

```latex
\[
X_h = A_h^\star + D_h^\star,
\]
```

where:

- `X_h` is observed log price at the interval boundary
- `A_h^\star` is the anchor level under measure `star`
- `D_h^\star` is deviation from anchor under measure `star`
- `star` is either `P` or `Q`

The physical measure uses the structural state `Sigma_h`.

The risk-neutral measure uses the augmented pricing state

```latex
\[
Z_h = (\Sigma_h, D_h^Q).
\]
```

The deviation enters the `Q` state because the local pricing tilt depends on the
current deviation.

## Core interval contribution

Let `R_h` denote the Janiform core interval contribution over interval `h`. This
is the same conceptual core contribution already used in seeded, coarse, and
pricing runtimes:

```latex
\[
R_h = \Delta\!\big(L^+_{A^+} - L^-_{A^-} + M + G\big)_h.
\]
```

Define the one-interval conditional log-moment-generating function of the core
contribution under the physical measure:

```latex
\[
K_h^P(u \mid \Sigma_h)
=
\log \mathbb E^P[e^{u R_h} \mid \Sigma_h].
\]
```

Because the observed layer does not feed back into the Janiform core, the
physical conditional law of `R_h` depends on `Sigma_h` and not on `D_h`.

## Canonical endogenous anchor under `P`

### Predictable core increment

Define the canonical physical anchor increment as the conditional first moment
of the Janiform core contribution:

```latex
\[
\mu_h^P(\Sigma_h)
=
\partial_u K_h^P(u \mid \Sigma_h)\big|_{u=0}
=
\mathbb E^P[R_h \mid \Sigma_h].
\]
```

This is the anchor increment generator. It is produced by the model itself from
the current Janiform state.

If the implementation evaluates the first moment by decomposition rather than by
differentiating the total transform, it must still equal the same quantity:

```latex
\[
\mu_h^P(\Sigma_h)
=
m_+^P\,\mathbb E^P[\Delta A_h^+ \mid \Sigma_h]
-
m_-^P\,\mathbb E^P[\Delta A_h^- \mid \Sigma_h]
+
\mathbb E^P[\Delta M_h + \Delta G_h \mid \Sigma_h],
\]
```

where `m_+^P` and `m_-^P` are the one-unit business-time means of the two NIG
drivers under `P`.

This is where news effects are already baked into the anchor. News contributes:

- directly through `M` and `G`
- indirectly through future Hawkes activity via the `J_t` coupling

### Anchor and deviation recursions under `P`

Define the physical anchor recursion:

```latex
\[
A_{h+1}^P = A_h^P + \mu_h^P(\Sigma_h).
\]
```

Define the centered core innovation:

```latex
\[
\xi_h^P = R_h - \mu_h^P(\Sigma_h),
\qquad
\mathbb E^P[\xi_h^P \mid \Sigma_h] = 0.
\]
```

Then the canonical physical deviation recursion is:

```latex
\[
D_{h+1}^P = \phi_h D_h^P + \xi_h^P,
\qquad
\phi_h = e^{-\kappa \Delta_h}.
\]
```

Equivalently:

```latex
\[
D_{h+1}^P = e^{-\kappa \Delta_h} D_h^P + R_h - \mu_h^P(\Sigma_h).
\]
```

The observed log-price recursion becomes:

```latex
\[
X_{h+1}
=
A_{h+1}^P + D_{h+1}^P
=
X_h + \big(e^{-\kappa \Delta_h} - 1\big) D_h^P + R_h.
\]
```

This preserves the Janiform core as the innovation driver while making the
anchor endogenous.

### Interpretation

Under `P`:

- the anchor level moves by the model-implied predictable core increment
- deviation contains the unpredictable part of the core contribution
- `kappa` governs how aggressively observed price is pulled back toward the
  endogenous anchor

This is the mean-reversion layer. The anchor is not meaningful without this
deviation recursion, and the deviation recursion is not meaningful without the
anchor-generation rule.

## Canonical endogenous anchor under `Q`

### Risk-neutral local transform

Under anchored pricing, the current deviation changes the required local pricing
tilt. The risk-neutral state is therefore

```latex
\[
Z_h = (\Sigma_h, D_h^Q).
\]
```

Define the canonical local tilt `theta_h(Z_h)` and the risk-neutral one-interval
core log-moment-generating function:

```latex
\[
K_h^Q(u \mid Z_h)
=
K_h^P\big(u + \theta_h(Z_h) \mid \Sigma_h\big)
-
K_h^P\big(\theta_h(Z_h) \mid \Sigma_h\big).
\]
```

This is the local state-dependent Esscher transform already implied by the
anchored pricing design, but now it also generates the anchor drift under `Q`.

### Risk-neutral anchor increment

Define the canonical risk-neutral anchor increment by the first moment of the
tilted core law:

```latex
\[
\mu_h^Q(Z_h)
=
\partial_u K_h^Q(u \mid Z_h)\big|_{u=0}
=
\mathbb E^Q[R_h \mid Z_h].
\]
```

Then define:

```latex
\[
A_{h+1}^Q = A_h^Q + \mu_h^Q(Z_h),
\]
```

```latex
\[
\xi_h^Q = R_h - \mu_h^Q(Z_h),
\qquad
\mathbb E^Q[\xi_h^Q \mid Z_h] = 0.
\]
```

The risk-neutral deviation recursion is:

```latex
\[
D_{h+1}^Q = \phi_h D_h^Q + \xi_h^Q.
\]
```

Equivalently:

```latex
\[
D_{h+1}^Q = e^{-\kappa \Delta_h} D_h^Q + R_h - \mu_h^Q(Z_h).
\]
```

### Local martingale condition

Under the endogenous-anchor specification, the observed return over one interval
is

```latex
\[
\Delta X_h
=
\mu_h^Q(Z_h)
+
\big(\phi_h - 1\big) D_h^Q
+
\xi_h^Q.
\]
```

Since `xi_h^Q = R_h - mu_h^Q(Z_h)`, the exponential moment of the observed
return depends on `K_h^Q(1 | Z_h)` rather than explicitly on the anchor drift.

The canonical local martingale equation is therefore:

```latex
\[
K_h^Q(1 \mid Z_h) + \big(\phi_h - 1\big) D_h^Q = (r-q)\Delta_h.
\]
```

This is the risk-neutral condition the pricing engine must satisfy for each
interval state.

The risk-neutral anchor is therefore not an external pricing input. It is a
derived quantity from the same local tilted transform used to price.

## Runtime materialization

The runtime still needs interval-aligned anchor values. The difference is where
they come from.

The canonical runtime materialization rule is:

1. start from a validated boundary state `(X_0, A_0, D_0, Sigma_0)`
2. compute `mu_h^star` from the current state under the relevant measure
3. update `A_{h+1}^star = A_h^star + mu_h^star`
4. record the resulting interval-aligned anchor values
5. use those materialized values in seeded, coarse, export, and diagnostic
   artifacts as needed

This means `MeanReversionRuntimeIntervalAnchors` remains a valid execution
artifact. It is no longer the canonical fitted-model representation of the
anchor.

This distinction is mandatory:

- the model generates the anchor internally from state
- the runtime records the resulting interval-aligned anchor values as concrete
  execution data

It must not be implemented as two parallel model families where either:

- the new endogenous-anchor model generates the anchor from state, or
- the old exogenous interval-grid model survives as a separate fitted-model
  choice

After this rewrite, interval-grid anchors are derived runtime data only.

## Contract implications

### Fitted-model contract

The fitted-model contract should represent an endogenous anchor policy, not a
caller-supplied future path.

The canonical fitted mean-reversion block should therefore contain:

- `kappa`
- an anchor policy selector identifying the endogenous anchor rule
- any additional parameters strictly required by that anchor policy

The canonical physical anchor policy for the first implementation should be:

- `endogenous_core_expectation_one_interval`

This means the anchor increment is the one-interval conditional expected core
increment under `P`.

Later policies may be added only if they remain fully model-generated. For
example a finite-horizon endogenous anchor based on a multi-interval forward
expectation may be added in a later project, but exogenous future grids must not
appear as fitted-model families at all once this rewrite lands.

### Runtime contract

Runtime contracts may still carry:

- interval-aligned start anchor
- interval-aligned end anchors by interval
- boundary anchor value
- boundary deviation

Those are execution artifacts and diagnostics, not the top-level anchor model.
They are produced by the endogenous-anchor recursion during execution. They are
not a legacy alternate source of truth.

### Pricing contract

Anchored pricing should not require the caller to supply the future anchor path.

The pricing engine should instead:

- accept the current observed boundary state
- accept the current structural Janiform state
- generate local `Q` anchor increments from the tilted transform
- materialize any interval-aligned `Q` anchor values internally for diagnostics
  and recursion

## Calibration implications

Calibration for the endogenous-anchor model should proceed in stages.

1. Fit the Janiform core as before.
2. Recover structural states `Sigma_h` on the calibration grid.
3. Compute the physical anchor increments `mu_h^P(Sigma_h)` from the fitted core.
4. Build the endogenous anchor path recursively.
5. Recover the deviation path `D_h^P = X_h - A_h^P`.
6. Fit `kappa` from the deviation recursion

```latex
\[
D_{h+1}^P = e^{-\kappa \Delta_h} D_h^P + \xi_h^P.
\]
```

The calibration stage must not ask the user to provide an external anchor path
for future use. Any boundary anchor needed at runtime must come from the fitted
state and the model's own anchor recursion.

## Implementation requirements

The implementation is not complete unless all of the following are true:

- simulation under `P` can generate future anchor values without external input
- pricing under `Q` can generate future anchor values without external input
- news-driven effects enter the anchor automatically through the Janiform core
- no future anchor grid is required from the caller as the primary workflow
- interval-aligned anchors remain available as native artifacts for debugging,
  exports, and validation
- the old exogenous interval-grid anchor is removed as a fitted-model family

## Non-goals

This document does not define:

- a caller-supplied future anchor path as the primary model contract
- a reduced product story where Janiform only models residuals around an
  external forecast
- feedback from observed price or deviation into Hawkes intensities, NIG drivers,
  or news processes

Those are outside the intended Janiform architecture.
