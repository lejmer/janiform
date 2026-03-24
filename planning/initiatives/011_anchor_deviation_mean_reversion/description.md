## Initiative 011: Anchor Deviation Mean Reversion

Introduce a canonical anchor-deviation mean-reversion layer for Janiform that keeps the existing Hawkes, NIG, and news core intact while allowing observed log-price paths to revert toward a low-dimensional anchor on longer horizons.

### Desired outcome
- Janiform supports an explicit mean-reversion layer on top of the existing core return process without mutating the Hawkes, NIG, or news mechanisms themselves.
- The new layer is represented by stable Rust and Python contracts, persisted in fitted models and calibration artifacts, and executed consistently by seeded and coarse simulation paths.
- Calibration can estimate low-dimensional anchor and mean-reversion parameters in a staged way without destabilizing the existing return-components workflow.
- High-level outputs, exports, and diagnostics distinguish between core Janiform increments and observed mean-reverting log-price state.
- Pricing boundaries remain mathematically correct throughout the rollout. Unsupported anchored pricing paths must fail explicitly rather than silently reusing the old risk-neutral logic.

### Success signals
- A fitted model can persist and reload mean-reversion parameters and runtime state without ad hoc side channels.
- Seeded and coarse simulations produce observed log-price paths with explicit anchor and deviation state when mean reversion is enabled.
- The original Janiform model remains the nested case with `kappa = 0` or an explicitly disabled mean-reversion configuration.
- A dedicated staged calibration step can estimate mean-reversion parameters from lower-frequency data while keeping the existing return-components stage intact.
- The pricing layer either has a mathematically valid anchored implementation or rejects anchored configurations with a structured error at every entrypoint.

### Example projects that might be useful to add
- A project focused on the runtime, contract, and calibration rollout for the anchor-deviation layer.
- A later project focused on anchored pricing and state-dependent risk-neutral measure design if option pricing for mean-reverting spot assets becomes a strict requirement.
