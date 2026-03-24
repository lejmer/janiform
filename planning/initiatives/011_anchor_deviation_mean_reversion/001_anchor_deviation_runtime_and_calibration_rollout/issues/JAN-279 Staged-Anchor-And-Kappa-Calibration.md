## Staged Anchor And Kappa Calibration

### Description
Add a dedicated staged calibration step for the mean-reversion layer.

This stage must estimate the low-dimensional anchor and the mean-reversion speed `kappa` without changing the existing Hawkes and return-components stages. It must treat the Janiform core as already fitted and then fit the outer observed-state layer on slower horizons.

The stage targets the model

```latex
\[
X_{h+1} = a_{h+1} + e^{-\kappa \Delta_h}(X_h - a_h) + R_h,
\]
```

where `R_h` is the Janiform core interval contribution implied by the existing fitted core.

The stage is intentionally staged, not fully joint:
1. fit the original Janiform core as before
2. estimate the anchor model
3. estimate `kappa`
4. persist the fitted mean-reversion block

Do not add joint refinement of `kappa` or anchor parameters in this issue unless it is mathematically required. The goal is a stable staged estimator first.

### Required stage scope
Support at least:
- disabled mean reversion
- constant anchor
- deterministic interval-grid anchor estimated on a lower-frequency calibration grid

The stage must operate on aggregated open-session observations, not raw tick-by-tick microstructure noise.

### Required estimation rules
The stage must define one canonical calibration grid `Delta_mr` and document it in the stage contract and diagnostics.

For constant anchor:

```latex
\[
a_h \equiv m.
\]
```

The stage may estimate `m` from robust low-frequency observed log-price levels on the calibration grid.

For `kappa`, estimate the AR(1)-style decay parameter on deviation:

```latex
\[
D_{h+1} = \phi D_h + \varepsilon_h,
\qquad
\phi = e^{-\kappa \Delta_{mr}},
\qquad
\kappa = -\frac{\log \phi}{\Delta_{mr}}.
\]
```

The estimator must enforce:
- `0 <= phi <= 1`
- `kappa >= 0`

The stage diagnostics must include:
- chosen anchor family
- calibration grid
- anchor parameter estimates
- `phi`
- `kappa`
- sample counts and coverage
- stage decision reason when mean reversion is disabled

### Required orchestration changes
- Extend staged calibration run contracts to include the new stage.
- Persist the stage payload in calibration artifacts.
- Make high-level fitted models include the new stage output when present.
- Leave joint refinement untouched unless it already supports freezing external stage outputs cleanly.

### Files and modules that should be updated
- Staged calibration contracts in Rust and Python
- Calibration artifact stage types and persistence
- High-level estimator assembly
- Stage execution bridge
- Relevant docs and run-report tests

### Acceptance Criteria
- There is a dedicated calibration stage for mean reversion with its own execution report and fitted-parameter artifact.
- The stage can emit:
  - disabled mean reversion
  - constant-anchor mean reversion
  - deterministic interval-anchor mean reversion
- The stage persists `kappa` and anchor parameters into the high-level fitted model.
- The stage is fully deterministic for fixed data and seed inputs.
- Existing staged calibration workflows still succeed when mean reversion is disabled.
- No joint refinement of mean reversion parameters is introduced in this issue unless the implementation can prove that freezing is impossible.

### Unit tests
- Stage contract validation tests.
- Deterministic estimation regression on synthetic data with:
  - `kappa = 0`
  - moderate positive `kappa`
  - constant anchor
- Artifact round-trip tests for the new stage output.
- High-level estimator tests verifying the fitted model contains the mean-reversion block when the stage succeeds.

### Integration tests
- Synthetic end-to-end calibration workflow where:
  - the core Janiform stage is fixed or stubbed
  - the mean-reversion stage recovers a known anchor and `kappa`

### Notes/Constraints
- The stage must use slower-horizon data than the Hawkes decay scale. Do not fit `kappa` on raw tick intervals.
- Do not let this stage mutate Hawkes or NIG parameters. It only fits the outer layer.
