# Endogenous Anchor Mean Reversion And Pricing Specification

## Purpose

This document defines the canonical endogenous-anchor version of the Janiform
mean-reversion layer.

It replaces the exogenous-anchor interpretation where the fitted model stores a
future anchor path or requires the caller to supply one. It also replaces the
older integrated endogenous-anchor law that accumulated the one-interval
expected core return directly into the equilibrium level. Janiform must remain
a self-contained generative and pricing model. The anchor therefore has to be
generated from Janiform's own state under both the physical measure `P` and the
risk-neutral measure `Q`.

## Design decision

The anchor is not an externally supplied future curve.

The canonical anchor is instead the model-generated equilibrium level implied by
the current Janiform state, a deterministic endogenous pressure state, and the
relevant measure.

For implementation purposes:

- the fitted model persists the mean-reversion speed `kappa`
- the fitted model persists the anchor-generation policy and its deterministic
  pressure-state parameters
- the runtime materializes interval-aligned anchor values and pressure states
  from the model state
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
- `p_h^\star` is the deterministic endogenous pressure state under measure `star`
- `A_h^\star` is the anchor level under measure `star`
- `D_h^\star` is deviation from anchor under measure `star`
- `star` is either `P` or `Q`

The physical measure uses the structural state `Sigma_h`.

The risk-neutral measure uses the augmented pricing state

```latex
\[
Z_h = (\Sigma_h, p_h^Q, D_h^Q).
\]
```

The deviation enters the `Q` state because the local pricing tilt depends on the
current deviation. The pressure state enters because the anchor recursion now
evolves through a deterministic filtered pressure coordinate rather than from a
direct expected-return integrator.

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

Define the canonical physical expected core increment as the conditional first moment
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

This is the endogenous pressure signal. It is produced by the model itself from
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

### Pressure, anchor, and deviation recursions under `P`

Let `Delta_ref > 0` be the persisted reference interval, let
`psi_ref in [0, 1]` be the persisted pressure persistence per reference
interval, let `c` be the persisted pressure-to-level gain, and define:

```latex
\[
\psi_h = \psi_{\mathrm{ref}}^{\Delta_h / \Delta_{\mathrm{ref}}}.
\]
```

Define the deterministic physical pressure state:

```latex
\[
p_0^P = 0,
\qquad
p_{h+1}^P = \psi_h p_h^P + (1 - \psi_h)\mu_h^P(\Sigma_h).
\]
```

Define the physical equilibrium level:

```latex
\[
A_h^P = a_0^P + c p_h^P.
\]
```

Equivalently, the runtime can evolve the anchor directly from the current
boundary state:

```latex
\[
A_{h+1}^P = A_h^P + c\big(p_{h+1}^P - p_h^P\big).
\]
```

Let

```latex
\[
\phi_h = e^{-\kappa \Delta_h}.
\]
```

Then the canonical physical deviation recursion is:

```latex
\[
D_{h+1}^P
=
\phi_h D_h^P + R_h + A_h^P - A_{h+1}^P
=
e^{-\kappa \Delta_h} D_h^P + R_h - c\big(p_{h+1}^P - p_h^P\big).
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

- the model-implied expected core increment drives a slow deterministic pressure
  state
- the anchor level moves only when that pressure state changes
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

Then define the risk-neutral pressure recursion:

```latex
\[
p_{h+1}^Q = \psi_h p_h^Q + (1 - \psi_h)\mu_h^Q(Z_h).
\]
```

```latex
\[
A_{h+1}^Q = A_h^Q + c\big(p_{h+1}^Q - p_h^Q\big).
\]
```

The risk-neutral deviation recursion is:

```latex
\[
D_{h+1}^Q
=
\phi_h D_h^Q + R_h + A_h^Q - A_{h+1}^Q
=
e^{-\kappa \Delta_h} D_h^Q + R_h - c\big(p_{h+1}^Q - p_h^Q\big).
\]
```

### Local martingale condition

Under the endogenous-anchor specification, the observed return over one interval
is

```latex
\[
\Delta X_h
=
\big(\phi_h - 1\big) D_h^Q + R_h.
\]
```

The anchor increment cancels algebraically in the observed-return identity, so
the exponential moment of the observed return depends on `K_h^Q(1 | Z_h)`
rather than explicitly on the pressure-to-level anchor drift.

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

1. start from a validated boundary state `(X_0, A_0, p_0, D_0, Sigma_0)`
2. compute `mu_h^star` from the current state under the relevant measure
3. update `p_{h+1}^star = psi_h p_h^star + (1 - psi_h) mu_h^star`
4. update `A_{h+1}^star = A_h^star + c (p_{h+1}^star - p_h^star)`
5. record the resulting interval-aligned anchor values and pressure states
6. use those materialized values in seeded, coarse, export, and diagnostic
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
- `anchor_model_kind = endogenous_core_pressure_level`
- `reference_interval_hours`
- `pressure_persistence_per_reference_interval`
- `pressure_level_gain`

Later policies may be added only if they remain fully model-generated. For
example a finite-horizon endogenous anchor based on a multi-interval forward
expectation may be added in a later project, but exogenous future grids must not
appear as fitted-model families at all once this rewrite lands.

### Runtime contract

Runtime contracts may still carry:

- interval-aligned start anchor
- interval-aligned start pressure state
- interval-aligned end anchors by interval
- interval-aligned end pressure states by interval
- boundary anchor value
- boundary pressure state
- boundary deviation

Those are execution artifacts and diagnostics, not the top-level anchor model.
They are produced by the endogenous-anchor recursion during execution. They are
not a legacy alternate source of truth.

### Pricing contract

Anchored pricing should not require the caller to supply the future anchor path.

The pricing engine should instead:

- accept the current observed boundary state
- accept the current structural Janiform state
- generate local `Q` expected core increments from the tilted transform
- evolve pressure state and anchor value internally from that expected
  increment sequence
- materialize any interval-aligned `Q` anchor values internally for diagnostics
  and recursion

## Calibration implications

Calibration for the endogenous-anchor model should proceed in stages.

1. Fit the Janiform core as before.
2. Recover structural states `Sigma_h` on the calibration grid.
3. Compute the physical expected core increments `mu_h^P(Sigma_h)` from the
   fitted core.
4. Build the pressure state path for candidate `psi_ref` values.
5. Fit the nuisance anchor-origin level and pressure-to-level gain for each
   candidate together with the deviation persistence.
6. Select among full pressure-to-level mean reversion, pressure-to-level
   without mean reversion, flat-anchor mean reversion, and random-walk null by
   one shared information criterion.

The calibration stage must not ask the user to provide an external anchor path
for future use. Any boundary anchor needed at runtime must come from the fitted
state and the model's own pressure-to-level anchor recursion.

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

Those are outside the intended Janiform architecture and should never be implemented.
