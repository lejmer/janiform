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

To make the rollout executable across seeded simulation, coarse simulation, risk generation, calibration, and exports without blocking on a new continuous-time pricing transform, the project must define one canonical interval-grid execution rule:

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

The implementation must preserve the existing Janiform core as the innovation generator. No feedback from `X_t` or `D_t` back into Hawkes intensities, NIG drivers, or news processes is allowed in this project.

Definition of Done (Project-level):
- Canonical mean-reversion contracts exist in Rust, Python, and persisted artifact schemas.
- Seeded and coarse simulation paths execute the same interval-grid anchor-deviation semantics and expose observed state in their artifacts.
- High-level model persistence, runtime preparation, exports, and examples understand the new layer.
- A dedicated staged calibration step estimates anchor and `kappa` without requiring immediate joint refinement support.
- Pricing boundaries are hardened so anchored configurations are never priced under the old Esscher-only logic.
- Tests, fixtures, and docs are updated in the same PRs as the behavior changes.

Agent implementation notes:
- Do not add compatibility shims, legacy aliases, or dual old/new payload support. Update in-repo call sites directly in each issue.
- Keep schema identifiers at `v1`.
- Prefer explicit interval-grid contracts over implicit materialization rules.
- If a runtime cannot yet execute anchored pricing correctly, it must reject the configuration with a structured error rather than ignore mean reversion.

### How it ties into the parent initiative
This project is the concrete rollout plan for the initiative’s desired outcome. It makes the anchor-deviation layer real in contracts, runtime execution, calibration artifacts, and user-facing outputs while keeping correctness ahead of convenience at every boundary.
