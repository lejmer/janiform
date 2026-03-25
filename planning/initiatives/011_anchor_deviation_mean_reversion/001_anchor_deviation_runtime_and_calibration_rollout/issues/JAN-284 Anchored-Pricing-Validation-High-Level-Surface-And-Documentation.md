## Anchored Pricing Validation High Level Surface And Documentation

### Description
Finish the mean-reversion rollout by wiring anchored pricing through the high-level model, validation harnesses, examples, and implementation-facing docs.

After the new measure and engine land, the public workflow must make anchored pricing practically usable. This issue replaces the old documentation-refresh-only endgame with validation and surface work centered on the anchored pricing implementation that actually ships.

### Required validation updates
- Add anchored-pricing validation suites that compare native prices against seeded or coarse simulation estimates on synthetic scenarios where the Monte Carlo error is controlled.
- Extend pricing regression fixtures and governance harnesses so they understand:
  - observed-state boundary input
  - anchor alignment
  - local-martingale diagnostics
  - anchored-engine numeric-quality diagnostics
- Add diagnostic coverage for:
  - price sensitivity to boundary deviation
  - convergence under deviation-state refinement
  - admissibility failures near transform-domain boundaries
  - nested-case agreement when `kappa = 0`

### Required high-level surface updates
- Update high-level `JaniformModel.price_option(...)` and any related Python-facing helpers so anchored fitted models price successfully when the required observed-state boundary input is available.
- Update CLI and examples so anchored pricing requests can provide the needed boundary state explicitly.
- Make the public pricing outputs and run reports include anchored-pricing diagnostics where relevant.
- Ensure high-level surfaces do not silently invent missing observed-state boundary inputs. Required anchored inputs must be explicit.

### Required documentation updates
- Update pricing docs so the anchored workflow is the documented implementation for mean-reverting observed prices.
- Document the augmented pricing state, the local martingale equation, and the numerical approach used for the deviation dimension.
- Update examples so at least one anchored pricing example shows:
  - fitted mean-reversion block
  - observed-state boundary input
  - anchored pricing call
  - returned diagnostics
- Refresh working model docs so simulation, calibration, and pricing all describe the same executable mean-reversion semantics.

### Files and modules that should be updated
- High-level pricing preparation in [`model.py`](/mnt/c/Users/anton/source/repos/janiform/python/janiform/calibration/fit/model.py)
- Python pricing contracts, CLI surfaces, and examples
- Validation tests and governance harnesses under `tests/` and `crates/janiform-validation/`
- README and pricing-related development guides
- `docs/design/janiform_compact_model_spec.tex`

### Acceptance Criteria
- High-level anchored pricing works end to end for supported configurations and required boundary inputs.
- Validation harnesses compare anchored prices against simulation-based references and surface meaningful diagnostics.
- Public docs no longer describe anchored pricing as unsupported or gated behind a structured rejection.
- Examples demonstrate anchored pricing without relying on hidden conversational context.
- All docs and fixtures describe the same augmented-state pricing semantics as the implemented engine.

### Unit tests
- High-level model tests showing anchored fitted models reach the anchored pricer rather than failing at the old gate.
- Python contract tests for anchored pricing request and response payloads.
- Validation adapter tests for new anchored-pricing diagnostic fields and reports.

### Integration tests
- End-to-end anchored pricing workflow examples for:
  - disabled mean reversion
  - constant-anchor mean reversion
  - deterministic interval-grid anchor mean reversion
- Documentation regression tests if command-reference or example snapshots exist for the updated surfaces.

### Notes/Constraints
- Do not defer docs and validation to a later cleanup. They must land in the same change set that makes anchored pricing usable.
- Keep the docs implementation-facing and precise. The target reader must be able to reconstruct the anchored pricing workflow without relying on earlier conversational context.
