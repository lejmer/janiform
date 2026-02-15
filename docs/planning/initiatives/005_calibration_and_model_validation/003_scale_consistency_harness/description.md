## Project 003: Scale Consistency Harness

### Context and requirements
- Build a harness that compares direct coarse simulation to aggregated fine-scale simulation.
- Define model-validation tolerance thresholds for key distributional and path statistics across regimes.
- Support repeatable runs across multiple parameter regimes and intervals.

### Definition of Done
- Harness can execute paired experiments and compute pass or fail decisions by threshold.
- Reports include metric deltas for returns, volatility, and event-derived statistics.
- Regression tests ensure harness behavior remains stable over time.

### Agent implementation notes
- Use shared random control to reduce variance in paired experiment comparisons.
- Keep threshold configuration externalized for governance and tuning.
- Build on engine-level numerical sanity outputs from Initiative 004 without duplicating low-level benchmark ownership.

### Initiative alignment
- Directly fulfills Initiative 005 success signal on fine-to-coarse consistency validation.
- Ensures tractability gains do not compromise model coherence.
