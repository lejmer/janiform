## Coarse Risk And Observed State Integration

### Description
Update the coarse simulation workflow and sufficient-statistics risk path to carry the anchor-deviation observed state.

The coarse engine already samples Janiform core interval statistics and then generates one interval return contribution. This issue must place the mean-reversion layer around that existing interval contribution without changing the underlying Hawkes, NIG, or news sampling contracts.

For interval `h` on the coarse grid:

```latex
\[
X_{h+1} = a_{h+1} + e^{-\kappa \Delta_h}(X_h - a_h) + R_h,
\]
```

where `R_h` is the coarse engine’s sampled Janiform core interval contribution on that grid.

Equivalently,

```latex
\[
D_{h+1} = e^{-\kappa \Delta_h} D_h + R_h + a_h - a_{h+1}.
\]
```

This issue deliberately defines the coarse-time mean-reversion layer on the coarse interval grid. It does not require a new intra-interval weighted sufficient-statistics transform.

### Required runtime changes
- Extend coarse simulation model inputs and risk model inputs to carry:
  - mean-reversion configuration
  - interval-aligned anchor values
  - observed-state boundary input
- Update coarse path records and diagnostics to include observed-state fields.
- Update sufficient-statistics risk path records and diagnostics to include observed-state fields.
- Preserve existing Janiform core interval statistics and return-component fields.

The observed-state update must occur after the Janiform core interval contribution for that interval has been generated and after the interval-end anchor value is known.

### Required artifact semantics
Each coarse interval record must carry both:
- the Janiform core return contribution for the interval
- the observed end-of-interval state after mean reversion and anchor update

The same split is required for the lower-level sufficient-statistics risk path if that path remains public.

Recommended explicit record fields:
- `core_total_return`
- `observed_interval_return`
- `observed_log_price_end`
- `anchor_value_end`
- `deviation_from_anchor_end`

The diagnostics block must add observed-state summaries such as:
- cumulative observed return
- max absolute deviation from anchor
- terminal observed log-price
- terminal deviation from anchor

### Files and modules that should be updated
- Coarse simulation contracts and runtime in:
  - [`coarse.rs`](/mnt/c/Users/anton/source/repos/janiform/crates/janiform-engine-simulation/src/coarse.rs)
  - [`coarse_runtime.rs`](/mnt/c/Users/anton/source/repos/janiform/crates/janiform-engine-simulation/src/coarse_runtime.rs)
- Sufficient-statistics risk contracts and path artifacts
- Python coarse and risk contracts
- Any public serialization helpers and tests for those paths

### Acceptance Criteria
- Coarse simulation request contracts and lower-level risk contracts accept the mean-reversion configuration, interval anchors, and observed-state boundary input.
- The coarse runtime updates observed state with the documented interval-grid formula and records the result explicitly.
- Core Janiform return components remain available and are not overwritten by observed-state fields.
- `kappa = 0` and flat anchor reduce exactly to the previous coarse observed-return semantics after fixture migration.
- Risk-path artifacts and coarse-path artifacts validate deterministically with the new observed-state fields.

### Unit tests
- One-interval coarse regression with zero Janiform core return verifying:
  - pure deviation decay
  - anchor reset handling
- Multi-interval regression verifying cumulative observed return differs from cumulative core return when `kappa > 0`.
- Nested-case regression for `kappa = 0`.
- Risk-path serialization tests covering the new observed-state diagnostics and record fields.

### Integration tests
- Coarse versus seeded scale-consistency harness updated so anchored runs compare observed-state outputs on the coarse grid rather than misreading core return fields as observed price.

### Notes/Constraints
- This issue uses grid-level mean-reversion semantics for the coarse engine. Do not attempt to retrofit a new continuous-time affine transform here.
- Keep Hawkes sufficient statistics, news dispatch, and boundary activity coupling unchanged except for the outer observed-state update.
