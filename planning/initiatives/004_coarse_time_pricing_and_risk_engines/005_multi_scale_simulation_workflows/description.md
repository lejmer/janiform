## Project 005: Multi Scale Simulation Workflows

Deliver a first-class simulation workflow that supports both fine-scale tick generation and direct coarse-interval path generation, closing the gap between the sufficient-statistics engine internals and the design requirement for tractable multi-scale simulation.

### Context and requirements
- Add a public coarse-time simulation workflow instead of leaving sufficient-statistics sampling as a set of lower-level primitives used only by tests, validation harnesses, and benchmarks.
- Preserve the existing seeded tick simulator as the canonical fine-scale path generator.
- Support interval-parametric coarse simulation for any explicit `Delta` when compatible interval factors are provided, with hourly remaining the default intraday reference case.
- Make the coarse simulation request calendar-anchored and time-indexed rather than only horizon-count based, so generated paths correspond to actual interval boundaries and market-state routing semantics.
- Generate direct open-window news jumps and closed-window gap jumps inside the coarse simulation workflow from shared model inputs, instead of requiring callers to supply precomputed `M` and `G` increments manually.
- Expose the resulting coarse simulation workflow through the public Python simulation surface once the Rust core contract is stable.

### Definition of Done
- Rust core provides a public end-to-end coarse-time simulation runner that produces reproducible interval paths.
- The coarse simulation contract includes explicit time anchoring, interval semantics, model inputs, and factor alignment rules for interval-parametric execution.
- Coarse path outputs include interval-level timestamps, sampled sufficient statistics, return components, and terminal state information needed for downstream analysis and validation.
- Direct jump generation for open-window `M` and closed-window `G` is part of the workflow and is consistent with the shared news semantics used elsewhere in the model.
- Cross-scale regression coverage demonstrates that coarse-interval generation is meaningfully aligned with aggregated fine-scale seeded-tick behavior where the two paths are intended to agree.
- Public Python exposes the coarse simulation workflow without regressing the existing seeded-tick simulation APIs.

### Agent implementation notes
- Reuse the sufficient-statistics sampler, return generator, Hawkes state update, calendar services, and shared news-process machinery instead of duplicating model logic.
- Prefer a simulation-facing entrypoint over exposing the raw risk-sampler plumbing directly to users.
- Keep the fine-scale and coarse-scale workflows explicitly related but not conflated. Each should have its own typed request and output models.

### Initiative alignment
- Directly satisfies the Initiative 004 outcome that Janiform supports direct coarse-interval simulation without tick replay.
- Completes the missing production simulation path implied by the sufficient-statistics oracle and interval-parametric engine work.
- Strengthens the multi-scale story by making tick generation and coarse-time generation both available as intentional workflows rather than partial internals.
