# Endogenous Pressure-To-Level Mean Reversion Design

## Purpose

This document records the implementation-ready follow-up to the AUDCAD 2025
investigation in
`docs/design/audcad_endogenous_anchor_investigation.md`.

The conclusion from that investigation is that the current endogenous-anchor
family is not merely too fast. It is structurally wrong for this use case
because it integrates short-horizon expected-return pressure directly into the
equilibrium level.

The replacement proposed here keeps the anchor endogenous, keeps mean reversion
as a separate calibration stage, adds no new randomness source, and remains
cheap enough to avoid touching joint refinement.

## Design decision

Replace the current integrated anchor family:

```text
A_{h+1} = A_h + filtered_expected_increment_h
```

with a pressure-to-level family:

```text
slow endogenous pressure state -> equilibrium level offset -> mean-reverting deviation
```

This preserves the original modeling intent:

- the anchor is still derived from Janiform's own endogenous expected-return
  signal
- no exogenous future curve is introduced
- no additional latent randomness is introduced
- pricing remains self-contained under both `P` and `Q`

What changes is the map from endogenous pressure to equilibrium.

## Why the current family fails

The current immediate and slow integrated families both treat the core expected
increment sequence as if it were already a level-driving signal:

```text
mu_h = expected core increment over interval h

immediate:
A_{h+1} = A_h + mu_h

slow:
m_{h+1} = psi m_h + (1 - psi) mu_h
A_{h+1} = A_h + m_{h+1}
```

Even the slow family still accumulates filtered expected returns into level.

That is what failed on AUDCAD:

- the old immediate family gave `phi_raw = 1.0227295801129237`
- the replacement slow family selected the boundary `lambda_anchor = 0.0`
- even that slowest integrated anchor still gave `phi_raw = 1.0287957415314914`

The optimizer therefore preferred the slowest possible integrated anchor but
still found deviations that drift away from it. This indicates that the missing
piece is not more smoothing. The missing piece is a proper endogenous
equilibrium law.

## Canonical replacement family

### Core endogenous pressure signal

Let `mu_h^star` denote the model-implied expected core increment over interval
`h` under measure `star`, where `star` is either `P` or `Q`:

```text
mu_h^star = E^star[R_h | structural_state_h]
```

This is the same endogenous expected-return signal already produced by the core
model. No new randomness is introduced.

### Deterministic pressure state

Define a deterministic slow pressure state `p_h^star`:

```text
p_0^star = 0

p_{h+1}^star = psi_h p_h^star + (1 - psi_h) mu_h^star
```

where

```text
psi_h = psi_ref ^ (Delta_h / Delta_ref)
```

with:

- `Delta_h` equal to the interval width in hours for interval `h`
- `Delta_ref` equal to the calibration-grid reference interval in hours
- `psi_ref in [0, 1]` equal to the persisted pressure persistence over one
  reference interval

Interpretation:

- `psi_ref = 0` means immediate pressure response
- `psi_ref = 1` means fully frozen pressure state
- intermediate values define a slow endogenous state without adding noise

### Equilibrium level

Define the endogenous equilibrium level:

```text
A_h^star = a_0^star + c p_h^star
```

where:

- `a_0^star` is a boundary-origin nuisance level for the current sample or path
- `c` is the persisted pressure-to-level gain

Equivalent runtime recursion:

```text
A_{h+1}^star = A_h^star + c (p_{h+1}^star - p_h^star)
```

This is the key structural change. The equilibrium level is no longer the
cumulative sum of expected drift. It is instead a level offset induced by a
slow endogenous pressure state.

### Deviation recursion

Let observed log price satisfy:

```text
X_h = A_h^P + D_h^P
```

with mean-reverting deviation:

```text
D_{h+1}^P = phi_h D_h^P + epsilon_{h+1}
phi_h = exp(-kappa Delta_h)
```

where `epsilon_{h+1}` is the one-step innovation implied by the calibrated
stage residual.

The risk-neutral measure uses the same structural family:

```text
X_h = A_h^Q + D_h^Q
```

with:

```text
p_{h+1}^Q = psi_h p_h^Q + (1 - psi_h) mu_h^Q
A_{h+1}^Q = A_h^Q + c (p_{h+1}^Q - p_h^Q)
D_{h+1}^Q = phi_h D_h^Q + centered_Q_innovation_{h+1}
```

This keeps the pricing story self-contained. The pressure signal changes under
the measure. The anchor family does not.

## Persisted fit contract

The current integrated anchor kinds should be replaced, not kept in parallel.
Backward compatibility is explicitly not required in this repository.

### Canonical fitted parameters

Persist exactly one anchor family:

```text
MeanReversionFittedParameters {
    kappa: f64,
    anchor_model: MeanReversionAnchorModel {
        anchor_model_kind: "endogenous_core_pressure_level",
        reference_interval_hours: f64,
        pressure_persistence_per_reference_interval: f64,
        pressure_level_gain: f64,
    }
}
```

Required invariants:

- `kappa >= 0`
- `reference_interval_hours > 0`
- `pressure_persistence_per_reference_interval in [0, 1]`
- `pressure_level_gain` finite

Notes:

- `reference_interval_hours` should equal the mean-reversion calibration grid
  interval used to estimate the parameters
- `pressure_persistence_per_reference_interval` is persisted instead of a rate
  constant so that the immediate-response boundary `0` is represented exactly
  without introducing infinities into the static fit contract
- the calibration-sample origin `a_0^star` is a nuisance state, not a
  persisted model parameter

### Runtime state implications

The executable observed layer should carry:

- current anchor value `A_h^star`
- current pressure state `p_h^star`
- current deviation `D_h^star`

This adds deterministic state, not randomness.

The runtime and pricing layers should evolve anchor value from the current
boundary state using:

```text
p_{h+1}^star = psi_h p_h^star + (1 - psi_h) mu_h^star
A_{h+1}^star = A_h^star + c (p_{h+1}^star - p_h^star)
```

This avoids persisting any sample-specific origin while still allowing current
deviation from equilibrium to matter for pricing.

## Separate-stage calibration objective

### Inputs

The mean-reversion stage keeps the same upstream dependency boundary:

- retained calibration observations `X_0, ..., X_N`
- interval widths `Delta_0, ..., Delta_{N-1}`
- core expected increments `mu_0^P, ..., mu_{N-1}^P`

These are fully determined from:

- seasonality
- Hawkes
- return-components

Joint refinement remains out of scope.

### Full candidate family

For one candidate pair `(psi_ref, kappa)`:

1. Set `p_0 = 0`.
2. Build `psi_h = psi_ref ^ (Delta_h / Delta_ref)`.
3. Recursively materialize `p_h`.
4. Build `phi_h = exp(-kappa Delta_h)`.
5. Estimate the nuisance anchor-origin level `a_0` and gain `c`.
6. Score the residual recursion by Gaussian quasi-likelihood.

### Regression form for fixed `psi_ref` and `kappa`

Using:

```text
X_h = a_0 + c p_h + D_h
D_{h+1} = phi_h D_h + epsilon_{h+1}
```

the one-step prediction equation becomes:

```text
X_{h+1} - phi_h X_h
=
a_0 (1 - phi_h)
+ c (p_{h+1} - phi_h p_h)
+ epsilon_{h+1}
```

Define:

```text
y_h = X_{h+1} - phi_h X_h
u_h = 1 - phi_h
z_h = p_{h+1} - phi_h p_h
```

Then for fixed `(psi_ref, kappa)` the nuisance parameters solve:

```text
y_h = a_0 u_h + c z_h + epsilon_{h+1}
```

This is a two-column linear least-squares problem.

Implementation requirement:

- use the repository's canonical deterministic least-squares routine for the
  two-column design matrix
- do not add ad hoc regularization, epsilons, or undocumented rank thresholds
- if the design is rank-deficient, use the solver's canonical minimum-norm
  least-squares solution and record the rank deficiency in diagnostics

### Residual score

Let residuals be:

```text
r_{h+1} = y_h - a_0 u_h - c z_h
```

with residual sum of squares:

```text
RSS = sum_h r_{h+1}^2
```

and pair count `M = N`.

Use the same Gaussian quasi-likelihood convention already used in the
mean-reversion stage:

```text
ell = -0.5 M [1 + log(2 pi RSS / M)]
```

for `RSS > 0`, with the same explicit infinite-metric handling policy already
used elsewhere in the stage diagnostics.

## Candidate families and model selection

Model selection should compare the following nested candidates by BIC, using the
same quasi-likelihood convention for all of them.

### 1. Full pressure-to-level mean reversion

Free parameters:

- `psi_ref`
- `kappa`
- nuisance `a_0`
- `c`

Parameter count for BIC: `4`

### 2. Pressure-to-level without mean reversion

Fix:

- `kappa = 0`

Free parameters:

- `psi_ref`
- `c`

The anchor-origin nuisance level `a_0` drops out exactly because `phi_h = 1`.

Transition equation:

```text
Delta X_h = c (p_{h+1} - p_h) + epsilon_{h+1}
```

Parameter count for BIC: `2`

### 3. Flat-anchor mean reversion

Fix:

- `c = 0`

Free parameters:

- `kappa`
- nuisance `a_0`

Transition equation:

```text
X_{h+1} - phi_h X_h = a_0 (1 - phi_h) + epsilon_{h+1}
```

Parameter count for BIC: `2`

### 4. Random-walk null

Fix:

- `kappa = 0`
- `c = 0`

Transition equation:

```text
Delta X_h = epsilon_{h+1}
```

Parameter count for BIC: `0`

### Selection rule

Select the candidate with the smallest BIC.

Consequences:

- if the full model wins with `c != 0` and `kappa > 0`, the asset exhibits
  endogenous anchor motion and nonzero pull to equilibrium
- if the no-mean-reversion candidate wins, the series may have endogenous
  directional pressure but no pull toward equilibrium
- if the flat-anchor mean-reversion candidate wins, the series mean-reverts but
  the endogenous pressure signal is not useful as the equilibrium driver
- if the random-walk null wins, the observed series does not support this layer

This supports cross-asset generalization without asset-family switches.

## Optimization policy

The stage should remain cheap by preserving a profile-search structure.

### Recommended search structure

1. Use `psi_ref in [0, 1]` as the bounded search variable for pressure
   persistence.
2. Use `kappa >= 0` as the canonical persisted parameter.
3. For optimization, search over the bounded equivalent
   `phi_ref = exp(-kappa Delta_ref)` in `(0, 1]`.
4. For each fixed `(psi_ref, phi_ref)` pair:
   - build `p_h`
   - build `phi_h = phi_ref ^ (Delta_h / Delta_ref)`
   - solve for `a_0` and `c`
   - score by quasi-likelihood
5. Use nested one-dimensional bounded searches driven by the existing
   mean-reversion-stage solver settings:
   - outer search over `psi_ref`
   - inner search over `phi_ref`

The existing stage already uses a bounded golden-section profile search for the
slow integrated family. This proposal preserves the same computational scale.

### Boundary evaluation

The profiling implementation must explicitly evaluate:

- `psi_ref = 0`
- `psi_ref = 1`
- `phi_ref = 1`

and compare those boundaries against the interior optimum instead of assuming
the optimizer visited them exactly.

No undocumented fallback constants should be introduced.

## Diagnostics requirements

The stage should emit enough information to explain why a candidate won or lost.

At minimum diagnostics should include:

- selected candidate family
- selected `kappa`
- selected `pressure_persistence_per_reference_interval`
- selected `pressure_level_gain`
- calibration-grid metadata
- nuisance `a_0` estimate for candidates where it is identified
- quasi-log-likelihood and BIC for every candidate family
- regression pair count
- residual sum of squares
- pressure-state path sample
- anchor-path sample
- deviation-path sample
- least-squares rank information
- explicit boundary-selection flags when `psi_ref` or `phi_ref` win at a
  boundary

The current silent `phi_raw -> 1 -> kappa = 0` collapse is not acceptable for
this family either.

## Why this fixes the AUDCAD failure mode

The AUDCAD evidence shows that even the slowest integrated anchor still trends
too hard because it keeps accumulating a positive endogenous expected-return
signal into the equilibrium level.

This replacement removes the integrator.

Instead:

- persistent positive endogenous pressure pushes the pressure state upward
- the pressure state maps to a bounded equilibrium-level offset through `c`
- the anchor level only moves when the pressure state itself changes
- the model can therefore represent a slower-moving endogenous equilibrium
  without forcing that equilibrium to inherit the full cumulative drift of the
  one-step expected-return signal

That is the missing structural degree of freedom.

## Implementation plan

### 1. Replace the fitted anchor contract

Update:

- `crates/janiform-model-kernels/src/mean_reversion.rs`
- Python contract mirrors

Actions:

- remove the current integrated anchor kinds
- introduce `endogenous_core_pressure_level`
- persist `reference_interval_hours`
- persist `pressure_persistence_per_reference_interval`
- persist `pressure_level_gain`

### 2. Add deterministic pressure-state runtime helpers

Update:

- `crates/janiform-model-kernels/src/mean_reversion.rs`

Actions:

- add pressure-state evolution from `mu_h` and `Delta_h`
- update runtime anchor materialization to evolve anchor value from current
  `(A_h, p_h)`
- expose enough runtime data for pricing and simulation

### 3. Rewrite the mean-reversion stage estimator

Update:

- `crates/janiform-calibration-runtime/src/stages/mean_reversion.rs`

Actions:

- compute `mu_h` exactly as today from the upstream fitted core
- evaluate the four candidate families defined above
- implement profile search over `psi_ref` and `phi_ref`
- solve nuisance linear coefficients by deterministic least squares
- select by BIC
- emit full diagnostics

### 4. Update pricing and simulation state

Update:

- pricing engine runtime state
- coarse and seeded simulation runtimes
- Python model runtime wrappers

Actions:

- carry current pressure state
- carry current anchor value
- keep deviation state explicit
- use the same endogenous family under `P` and `Q`

### 5. Keep separate-stage iteration cheap

The recalibration helper in `scripts/recalibrate_mean_reversion_from_run_artifacts.py`
should be updated after the runtime contract change so that the new family can
be tested against archived runs without rerunning joint refinement.

## Recommendation

This should replace the current integrated anchor family rather than coexist
with it.

The integrated family answered the wrong question:

```text
Should cumulative expected drift itself be treated as the equilibrium level?
```

The pressure-to-level family answers the better question:

```text
How should slow endogenous directional pressure move the equilibrium level, and
how strongly should observed price pull back toward that equilibrium?
```

That is the version that best matches Janiform's realism and pricing goals.
