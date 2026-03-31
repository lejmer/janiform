# AUDCAD Endogenous Anchor Investigation

## Purpose

This document preserves the main reasoning from the AUDCAD 2025 investigation
before any implementation work starts.

The immediate question is why the mean-reversion stage estimated `kappa = 0.0`
for a dataset that appears empirically mean-reverting. The broader design
question is whether the current endogenous-anchor formulation is too aggressive,
and how to fix it without:

- adding a new randomness source
- making calibration materially slower
- folding mean-reversion into joint refinement
- giving up the self-contained pricing story

## Run Context

- Run directory: `.janiform/AUDCAD_2025_20260331T104853Z/`
- Dataset: `data/AUDCAD_2025.parquet`
- Workflow scope: `full_without_option_surface`
- Selected stages:
  - `seasonality`
  - `hawkes`
  - `return_components`
  - `mean_reversion`
  - `joint_refinement`

Observed fitted output from the run:

- `model.parameters.mean_reversion.fitted_parameters.kappa = 0.0`
- `model.parameters.return_components.fitted_parameters.mean_reversion_fitted_parameters.kappa = 0.0`
- `model.parameters.joint_refinement.baseline_return_components_fitted_parameters.mean_reversion_fitted_parameters.kappa = 0.0`

The mean-reversion stage completed successfully. This was not a runtime fallback.

## What The Current Implementation Does

The current high-level path derives the mean-reversion calibration grid from
retained session-open boundary spacing when available, and otherwise from
observed session duration.

Relevant code:

- `crates/janiform-pybind/src/bindings/calibration.rs`
- `crates/janiform-calibration-runtime/src/stages/mean_reversion.rs`
- `README.md`

Current sequence:

1. Fit seasonality, Hawkes, and return-components.
2. Build an endogenous anchor path from the fitted core model.
3. Compute deviations:

   ```text
   D_h = X_h - A_h
   ```

4. Estimate persistence with:

   ```text
   phi = sum(D_h D_{h+1}) / sum(D_h^2)
   ```

5. Clamp `phi` into `[0, 1]`.
6. Convert to:

   ```text
   kappa = -log(phi) / Δ
   ```

This means the current stage asks:

```text
How much additional mean reversion remains after subtracting the model's own
interval-by-interval predictable anchor?
```

That is a much narrower question than:

```text
Does the observed FX spot series mean-revert around a slower equilibrium level?
```

## What Was Verified On AUDCAD

### Dataset scale

From `data/AUDCAD_2025.parquet`:

- 23,308,269 total ticks
- 22,194,405 open-session ticks
- 1,113,864 closed-session ticks
- 48 detected close-open boundaries

### Actual calibration grid behavior

The high-level translation path compresses the open-session price path onto a
coarse regular grid using the last observed open-session price before each
interval boundary.

For AUDCAD 2025:

- retained boundary count: `48`
- median boundary spacing: about `167.99874527777777` hours
- resulting mean-reversion grid size: `48` observations

So the stage is not fitting `kappa` from rich intraday evidence. It is fitting
`kappa` from a weekly-aligned open-session grid derived from the same tick data.

### Important comparison

If the weekly resampled price series is treated as a plain centered AR(1)-style
series without subtracting the endogenous anchor, the implied persistence is
less than one:

- raw centered weekly `phi ≈ 0.836`

That is consistent with nonzero mean reversion.

But after subtracting the current endogenous anchor, the residual deviations are
slightly more persistent than unit root:

- `denominator = 2.223904864281047`
- `numerator = 2.2744532880572437`
- `phi_raw = 1.0227295801129237`

The implementation then clamps:

- `phi = 1.0`
- `kappa = 0.0`

This is the direct reason the run produced zero mean reversion.

### Finer grids do not solve the core issue

The weekly default makes the problem easier to trigger, but it is not the whole
problem.

Reconstructing the same anchor-based estimator on finer grids still gave
`phi_raw > 1`:

- 24h: `phi_raw ≈ 1.0031`
- 48h: `phi_raw ≈ 1.0063`
- 72h: `phi_raw ≈ 1.0094`
- 168h: `phi_raw ≈ 1.0227`

So the main issue is not only the grid. The main issue is the anchor
specification.

## Core Interpretation

The current endogenous anchor appears to absorb too much of the slow directional
structure that should likely be split between:

- anchor motion
- deviation persistence

In other words, the current anchor is too reactive.

The current design effectively says:

```text
Every model-implied predictable core increment should move the equilibrium level
immediately.
```

For empirical FX spot behavior, this is likely too aggressive.

That makes `kappa` weakly identified because:

- if the anchor tracks too much of the low-frequency movement, little remains
  for `kappa`
- if the anchor overshoots, deviations drift away from anchor and the stage
  forces `kappa = 0`

This does not prove that endogenous anchoring is wrong in principle. It does
strongly suggest that the current one-step core-expectation anchor is a bad
empirical equilibrium definition for this use case.

## Requirements That Should Not Be Lost

Any fix should preserve the following goals:

- realistic simulated price dynamics with a higher-than-normal realism bar
- superior option pricing performance relative to alternatives
- no new randomness source if it can be avoided
- keep mean-reversion estimation as a separate stage
- do not make joint refinement slower
- preserve a self-contained pricing model if possible

## Cross-Asset Generalization

The model should generalize across asset families without introducing
asset-class-specific mean-reversion logic.

That means the design should not hardcode:

- one anchor policy for FX
- another for commodities
- another for equities

The correct unit is not the asset class. The correct unit is the observed
series and the fitted evidence.

Examples:

- some FX spot pairs are visibly mean-reverting
- some commodities are strongly mean-reverting for economic reasons such as
  storage, inventory, or convenience yield effects
- some assets or contracts may show little or no meaningful level
  mean reversion

The mean-reversion layer therefore should not be toggled by asset family. It
should be estimated from the data within one coherent model family.

### Desired model-family behavior

The preferred endogenous-anchor family should be able to represent at least
three empirical regimes:

1. no meaningful mean reversion
2. fast-moving endogenous anchor
3. slower-moving endogenous anchor with nonzero pull toward equilibrium

In practical terms, one family should nest:

- `kappa = 0`
- `psi = 0`
- `psi > 0`

Where:

- `kappa = 0` means no mean-reversion pull
- `psi = 0` means the current immediate endogenous anchor update
- `psi > 0` means the anchor reacts more slowly than the raw predictable
  increment path

This is preferable to asset-class-specific logic because:

- it keeps the semantics of the model uniform
- it lets the data decide whether mean reversion is present
- it preserves a single pricing story across all fitted models

### Cross-asset interpretation

Under this framing:

- FX can fit nonzero `kappa` with a slow endogenous anchor if equilibrium moves
  more slowly than short-horizon predictable flow pressure
- commodities can fit nonzero `kappa` for economically mean-reverting spot-like
  behavior without needing an exogenous anchor
- assets with little level mean reversion can legitimately fit `kappa = 0`
  without requiring a separate model branch

That is the main reason the preferred fix is not an FX-specific patch. It is a
general repair to the equilibrium definition.

## Options Considered

### Option 1: Keep the current anchor and only add diagnostics

Description:

- Preserve the existing endogenous anchor exactly as-is.
- Add better diagnostics so that `phi_raw > 1` is visible instead of silently
  collapsing to `kappa = 0`.

Pros:

- Almost no implementation risk
- No contract redesign yet
- Helpful regardless of future decisions

Cons:

- Does not solve the identification problem
- The stage still treats anchor overshoot as zero mean reversion
- Likely leaves simulation realism unsolved

Assessment:

- Necessary as instrumentation
- Not sufficient as a modeling fix

### Option 2: Switch to an exogenous anchor

Description:

- Replace the endogenous equilibrium definition with an external or partly
  external anchor concept

Pros:

- Easier statistical identification
- More obvious mean-reversion target for empirical fitting

Cons:

- Weakens the self-contained pricing story
- Moves away from the original design motivation
- Risks a calibration/runtime mismatch unless the pricing model is redesigned

Assessment:

- Only a valid option if all variations of endogenous variants fail
- Not a viable option otherwise

### Option 3: Keep the anchor endogenous, but make it slow

Description:

- Keep the anchor driven by model-implied predictable increments
- Do not let the anchor react immediately to the full increment
- Introduce one deterministic anchor-response parameter

Candidate structure:

```text
m_{h+1} = psi m_h + (1 - psi) mu_h
A_{h+1} = A_h + m_{h+1}
D_h = X_h - A_h
D_{h+1} ≈ phi D_h + epsilon_h
```

Where:

- `mu_h` is the current endogenous predictable core increment
- `psi` controls anchor sluggishness
- `phi` is the deviation persistence that maps to `kappa`

Interpretation:

- `psi = 0` means the current anchor behavior
- higher `psi` means a slower equilibrium update
- no new randomness is introduced

Pros:

- Preserves endogenous anchoring
- Preserves self-contained pricing
- Still separate from joint refinement
- Computationally cheap because the stage remains low-dimensional
- Gives a cleaner separation between equilibrium movement and pull-to-equilibrium

Cons:

- Adds one more deterministic parameter
- Requires updating the anchor-model contract
- Still needs good diagnostics to avoid hidden pathologies
- Requires care to keep `psi` and `kappa` identifiable

Assessment:

- Preferred option (the user agrees)

### Option 4: Keep endogenous anchor, but restrict what feeds it

Description:

- Redefine the anchor increment so it only uses slower structural components
- Examples:
  - lower-frequency state summaries
  - baseline intensity rather than short-horizon intensity detail
  - possibly less intraday structure in anchor motion

Pros:

- No extra fitted parameter may be needed
- Keeps anchor endogenous
- Very cheap computationally

Cons:

- Hardcodes economic judgment into what counts as slow
- Less flexible across assets
- Easier to get wrong silently

Assessment:

- Worth keeping as a secondary alternative if option 3 fails completely
- Less attractive than explicit anchor sluggishness

## Preferred Direction

Preferred direction:

- keep the anchor endogenous
- make the anchor slower
- keep `kappa` estimation separate
- keep the stage cheap
- add diagnostics

Why this is preferred:

- It preserves the original design goal of endogenous anchoring
- It preserves a coherent pricing story
- It does not add randomness
- It does not require touching joint refinement
- It directly addresses the identified failure mode, which is that the anchor is
  too reactive

## Important Caveats

### Caveat 1: Better diagnostics are mandatory

The current stage hides the key evidence. The fit artifacts should surface:

- `phi_raw`
- clamped `phi`
- numerator and denominator of the deviation regression
- a small sample of anchor values and deviations
- a clear diagnostic reason when `phi_raw > 1`

Without this, future failures will be hard to interpret.

### Caveat 2: The current stage likely uses an over-simplified structural state path

The anchor materialization currently starts from a zero Hawkes state in the
mean-reversion stage. That may be a secondary source of misspecification.

This does not appear to be the main cause of the AUDCAD result, but it is worth
revisiting after the larger anchor-speed fix.

### Caveat 3: The anchor and `kappa` must remain semantically aligned

It would be a mistake to calibrate `kappa` against one equilibrium notion and
then price or simulate against a different one. The runtime anchor policy and
the calibration anchor policy should stay the same family.

### Caveat 4: `psi` should be parameterized in a grid-stable way

If the slow-anchor family is introduced, the fitted persistence should not be
defined as a raw per-interval number whose meaning changes with the calibration
grid.

The better contract is a continuous-time anchor-response speed such as
`lambda_anchor >= 0`, with interval persistence derived from the grid:

```text
psi_h = exp(-lambda_anchor * Delta_h)
```

This keeps the parameter interpretable across:

- different calibration intervals
- different asset families
- different runtime grids

It also makes the slow-anchor family mathematically parallel to the current
`kappa` parameterization.

## Exact Estimation And Model Selection Logic

The mean-reversion stage should remain a separate cheap stage.

The preferred estimation logic is a profile-likelihood style fit over the
slow-anchor parameter, while solving for deviation persistence conditionally.

### Proposed family

Let `mu_h` denote the current model-implied predictable core increment from the
already fitted core model.

Let the slow endogenous anchor use:

```text
m_{h+1} = psi_h m_h + (1 - psi_h) mu_h
A_{h+1} = A_h + m_{h+1}
D_h = X_h - A_h
D_{h+1} = phi_h D_h + epsilon_h
```

With:

```text
psi_h = exp(-lambda_anchor * Delta_h)
phi_h = exp(-kappa * Delta_h)
```

Parameters:

- `lambda_anchor >= 0`
- `kappa >= 0`

Special cases:

- `lambda_anchor -> +infinity` gives the current immediate-anchor behavior
- `lambda_anchor = 0` gives a maximally sluggish anchor increment state
- `kappa = 0` gives no pull toward anchor

For implementation, the immediate-anchor case does not have to be represented
literally by an infinite value. The contract can either:

- keep the existing `psi = 0` family as a named special case, or
- define an equivalent finite-speed parameterization with the same nesting

The important part is that the semantics are explicit in the contract.

### Cheap profile estimation

Given a candidate anchor-speed parameter:

1. Materialize the core-implied predictable increments `mu_h`.
2. Build the anchor path recursively.
3. Compute deviations `D_h`.
4. Estimate the best constrained deviation persistence from the same closed form
   used today:

   ```text
   phi_raw = sum(D_h D_{h+1}) / sum(D_h^2)
   phi = clamp(phi_raw, 0, 1)
   kappa = -log(phi) / Delta
   ```

5. Compute residuals:

   ```text
   epsilon_h = D_{h+1} - phi D_h
   ```

6. Score the candidate with a proper likelihood or quasi-likelihood on the
   residual sequence.

This keeps the stage cheap because:

- the search is one-dimensional in the new anchor-speed parameter
- conditional `kappa` estimation remains closed form
- no simulation-based inner loop is required
- joint refinement is untouched

### Scoring objective

The current stage effectively uses the persistence estimate only. The improved
stage should score the whole deviation recursion.

The simplest acceptable choice is a Gaussian quasi-log-likelihood:

```text
epsilon_h ~ N(0, sigma^2)
sigma^2_hat = SSE / n
log L = -n/2 * (1 + log(2 * pi * sigma^2_hat))
```

Where:

- `SSE = sum(epsilon_h^2)`
- `n` is the number of regression pairs

This is cheap, deterministic, and mathematically explicit.

If future evidence shows heavy-tail sensitivity matters at this stage, the
residual law can be upgraded later. The initial repair does not require that.

### Model comparison

The model-selection logic should explicitly compare nested cases instead of
implicitly treating clamped `phi = 1` as the answer.

Recommended candidates:

- `M0`: no mean reversion, `kappa = 0`
- `M1`: current immediate endogenous anchor, `psi = 0`, `kappa >= 0`
- `M2`: slow endogenous anchor, `psi > 0`, `kappa >= 0`

Comparison should use a mathematically specified criterion, not an undocumented
heuristic. The cleanest cheap choices are:

- maximized likelihood with an information criterion penalty such as BIC, or
- out-of-sample likelihood if a deterministic holdout protocol is defined

BIC is attractive here because:

- it is cheap
- it is deterministic
- it penalizes the additional anchor-speed parameter in `M2`
- it does not require adding a holdout workflow immediately

This lets the stage legitimately choose:

- no mean reversion for assets that do not need it
- immediate-anchor mean reversion if the current family is sufficient
- slow-anchor mean reversion when the data supports it

### Diagnostics that should be emitted

For each candidate family that is evaluated, the stage should record:

- fitted anchor-speed parameter
- fitted `phi_raw`
- fitted clamped `phi`
- fitted `kappa`
- numerator and denominator of the deviation regression
- residual sum of squares
- log-likelihood or quasi-log-likelihood
- information criterion value if used
- compact samples of:
  - `mu_h`
  - anchor values
  - deviations
  - residuals

This is especially important when:

- `phi_raw > 1`
- the selected model is `kappa = 0`
- `M0` beats `M1` and `M2`
- `M2` wins only narrowly over simpler candidates

### Pricing consistency

The same anchor family used in calibration should be used in runtime pricing and
simulation.

That means:

- under `P`, the anchor uses the physical predictable increment recursion
- under `Q`, the anchor uses the risk-neutral predictable increment recursion
- the slow-anchor state law itself stays the same family across measures

This preserves the self-contained pricing story. It also avoids a semantic bug
where `kappa` is calibrated against one equilibrium definition but priced
against another.

## Recommended Staged Plan

### Phase 1: Instrumentation

Add diagnostics only.

Deliverables:

- expose `phi_raw`
- expose numerator and denominator
- expose compact anchor and deviation samples
- explicitly flag `phi_raw > 1` as anchor misspecification evidence

Goal:

- make future decisions evidence-based and reproducible

### Phase 2: Slow endogenous anchor model

Add a new endogenous anchor family with one deterministic anchor-speed
parameter.

Possible model family name:

- `slow_endogenous_core_expectation`

Goal:

- preserve self-contained pricing
- preserve separate mean-reversion stage
- reduce overreaction of the anchor

### Phase 3: Cheap separate-stage estimation

Keep mean reversion as its own cheap stage.

Proposed fitting pattern:

1. materialize `mu_h`
2. for each candidate `psi`, build anchor path
3. compute deviations
4. estimate optimal constrained `phi`
5. choose `psi` by residual fit score
6. convert `phi` to `kappa`

This should stay cheap because:

- the search is low-dimensional
- the path is short after gridding
- it does not require simulation-based refinement
- nested model comparison can stay deterministic and cheap with a criterion such
  as BIC

### Phase 4: Revisit structural-state fidelity if needed

Only after the anchor-speed change:

- revisit whether the mean-reversion stage should use a better recovered
  structural state path than the current zero-state initialization

This should be treated as a second-order refinement, not the first fix.

## Decision Summary

Current conclusion:

- do not abandon endogenous anchoring yet
- the current endogenous anchor is too aggressive
- the best next model change is a slower endogenous anchor
- the model family should be cross-asset rather than asset-class-specific
- the first implementation change should still be diagnostics

If this direction fails, exogenous or semi-exogenous anchors remain a valid
fallback. They should not be the first move while the slower endogenous option
has not yet been tested.
