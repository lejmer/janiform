## Seeded Tick Anchor Deviation Runtime

### Description
Update the seeded tick simulator to execute the anchor-deviation mean-reversion layer and emit observed-state artifacts.

The Janiform core remains the innovation driver:

```latex
\[
dX_t^{\mathrm{JF}} = d\big(L^+_{A_t^+} - L^-_{A_t^-} + M_t + G_t\big).
\]
```

The observed state is:

```latex
\[
X_t = A_t + D_t,
\qquad
dD_t = -\kappa D_t\,dt + dX_t^{\mathrm{JF}} - dA_t.
\]
```

For the seeded runtime, use the exact executable semantics on each factor interval:
- the anchor is held fixed within the interval
- deviation decays continuously between transitions
- Janiform core increments from endogenous trades, open-window jumps, and reopening gaps are applied at their existing seeded event times
- the anchor updates only at interval boundaries

If `delta` is the elapsed time in hours from the previous transition to the current transition, the seeded deviation update is:

```latex
\[
D_{\mathrm{new}}^{-}
=
e^{-\kappa \delta} D_{\mathrm{old}}
+
\Delta X^{\mathrm{JF}}_{\mathrm{transition}}.
\]
```

At the right boundary of interval `h`, after all in-interval events are processed:

```latex
\[
D_{h+1} = D_{h+1}^{-} + a_h - a_{h+1},
\qquad
X_{h+1} = a_{h+1} + D_{h+1}.
\]
```

### Required runtime changes
- Extend seeded simulation input contracts to carry the mean-reversion configuration, interval-aligned anchors, and observed-state boundary input.
- Replace the current assumption that `cumulative_log_return` is the whole observed log-price path.
- Keep Janiform core return components separate from observed-state fields.
- Emit observed-state data on every fine-path record and event-tape projection where the current simulator already emits return state.

The seeded runtime artifact needs explicit fields for:
- `core_total_return_increment`
- `observed_log_price`
- `anchor_value`
- `deviation_from_anchor`

If you keep `cumulative_log_return`, it must be renamed or clearly redefined so it cannot be misread as the observed log-price when mean reversion is enabled.

### Required artifact semantics
Each fine-path record must make it possible to reconstruct:
- the Janiform core increment applied at that transition
- the observed state after the transition
- the anchor value in force after the transition

For `kappa = 0` and constant anchor, the seeded runtime must reduce exactly to the old observed-price semantics:

```latex
\[
X_t = X_0 + \sum \Delta X_t^{\mathrm{JF}}.
\]
```

### Files and modules that should be updated
- Seeded runtime config and execution in:
  - [`config.rs`](/mnt/c/Users/anton/source/repos/janiform/crates/janiform-engine-simulation/src/config.rs)
  - [`runtime.rs`](/mnt/c/Users/anton/source/repos/janiform/crates/janiform-engine-simulation/src/runtime.rs)
  - seeded simulation artifact types and tests
- Python seeded contracts and response validation
- Pybind seeded result transport types if record columns are exposed there

### Acceptance Criteria
- Seeded simulation inputs accept validated mean-reversion config, runtime anchors, and observed-state boundary input.
- Every seeded event update applies exponential deviation decay over elapsed time before applying the Janiform core increment for that transition.
- Interval-boundary anchor resets are applied exactly once per interval after all in-interval seeded updates.
- Seeded output artifacts expose observed log-price state and anchor/deviation state explicitly.
- For `kappa = 0` and flat anchor, seeded outputs match the previous no-mean-reversion behavior after in-repo fixtures are migrated.
- No existing seeded return-component semantics are silently redefined. Core increments remain interpretable as Janiform core contributions.

### Unit tests
- Deterministic seeded-path regression with zero core shocks verifying pure exponential decay:
  - `D_{t+\delta} = exp(-kappa * delta) D_t`
- Constant-anchor regression verifying that positive `kappa` pulls the observed path back toward the anchor.
- `kappa = 0` regression verifying nested equivalence to the old runtime after payload migration.
- Boundary test verifying one interval with `a_h != a_{h+1}` applies the anchor reset exactly once at the interval boundary.
- Serialization and validation tests for the new seeded response fields.

### Notes/Constraints
- Do not feed observed price or deviation back into Hawkes state, NIG drivers, or news routing.
- Keep the existing seeded event ordering unchanged.
- Do not hide the new observed-state layer inside exporter-only logic. It must exist in the canonical runtime artifact.
