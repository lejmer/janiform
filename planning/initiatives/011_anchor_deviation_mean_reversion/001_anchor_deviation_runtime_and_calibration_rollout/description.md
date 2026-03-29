## Anchor Deviation Runtime And Calibration Rollout

### Context and requirements
Janiform currently models the log-return process

```latex
\[
X_t^{\mathrm{JF}} = L^+_{A_t^+} - L^-_{A_t^-} + M_t + G_t.
\]
```

This project adds a separate observed-state layer:

```latex
\[
X_t = A_t + D_t,
\qquad
dD_t = -\kappa D_t\,dt + dX_t^{\mathrm{JF}} - dA_t,
\qquad
\kappa \ge 0.
\]
```

`A_t` is the anchor and `D_t` is deviation from anchor.

To make the rollout executable across seeded simulation, coarse simulation, risk generation, calibration, exports, and anchored pricing, the project must define one canonical interval-grid execution rule:

For interval `h` with boundaries `t_h` and `t_{h+1}`, interval length `Delta_h = t_{h+1} - t_h`, and piecewise-constant anchor value `a_h` held fixed on `[t_h, t_{h+1})`,

```latex
\[
D_{h+1}^{-} = e^{-\kappa \Delta_h} D_h + R_h,
\]
```

where `R_h` is the executable Janiform core contribution used by the runtime on that interval.

After the interval closes, the anchor may jump from `a_h` to `a_{h+1}` at the boundary:

```latex
\[
D_{h+1} = D_{h+1}^{-} + a_h - a_{h+1},
\]
\[
X_{h+1} = a_{h+1} + D_{h+1}.
\]
```

This gives the equivalent observed-state update:

```latex
\[
X_{h+1} = a_{h+1} + e^{-\kappa \Delta_h}(X_h - a_h) + R_h.
\]
```

Because the observed interval return now depends on the boundary deviation state, the project must also replace the old core-only pricing stance with an anchored pricing workflow on an augmented state. For one interval,

```latex
\[
\Delta X_h
=
(a_{h+1} - a_h)
+
\big(e^{-\kappa \Delta_h} - 1\big)(X_h - a_h)
+
R_h.
\]
```

Let `Y_h` denote the existing Janiform boundary state and let `D_h = X_h - a_h`. The anchored pricing workflow must treat `Z_h = (Y_h, D_h)` as the pricing state, define a state-conditional transform for the core contribution `R_h`, and solve a local risk-neutral tilt `theta_h(Z_h)` that enforces

```latex
\[
\mathbb E^Q\!\left[e^{\Delta X_h} \mid Z_h\right] = e^{(r-q)\Delta_h}
\]
```

state by state. That local martingale constraint is no longer equivalent to one global Esscher parameter once `kappa > 0`.

The implementation must preserve the existing Janiform core as the innovation generator. No feedback from `X_t` or `D_t` back into Hawkes intensities, NIG drivers, or news processes is allowed in this project.

Definition of Done (Project-level):
- Canonical mean-reversion contracts exist in Rust, Python, and persisted artifact schemas.
- Seeded and coarse simulation paths execute the same interval-grid anchor-deviation semantics and expose observed state in their artifacts.
- High-level model persistence, runtime preparation, exports, and examples understand the new layer.
- A dedicated staged calibration step estimates anchor and `kappa` without requiring immediate joint refinement support.
- Anchored pricing contracts include observed-state boundary input, augmented-state diagnostics, and state-dependent martingale-solver outputs.
- An anchored pricing engine prices European options across one or more intervals without routing anchored configs through the old single-measure Fourier runner.
- High-level pricing surfaces, validation harnesses, and docs support anchored pricing end to end.
- Tests, fixtures, and docs are updated in the same PRs as the behavior changes.

Agent implementation notes:
- Do not add compatibility shims, legacy aliases, or dual old/new payload support. Update in-repo call sites directly in each issue.
- Keep schema identifiers at `v1`.
- Prefer explicit interval-grid contracts over implicit materialization rules.
- Do not add hidden fallbacks that strip the mean-reversion layer and then price the remaining core under the old logic.
- Temporary intermediate work may fail explicitly while the new pricing path is under construction, but the final project state must support anchored configurations rather than gate them away.

### How it ties into the parent initiative
This project is the concrete rollout plan for the initiative’s desired outcome. It makes the anchor-deviation layer real in contracts, runtime execution, calibration artifacts, anchored pricing, and user-facing outputs while replacing the old core-only pricing logic where it is no longer mathematically valid.
