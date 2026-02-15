## Project 003: Sufficient Statistics Risk Sampler

Implement interval sufficient-statistics risk sampling for long-horizon scenario generation without cluster replay.

### Context and requirements
- Implement interval sampling of `A_plus`, `A_minus`, `N_plus`, and `N_minus` from transform slices.
- Update boundary Hawkes state and sample return increments without cluster simulation.
- Support long-horizon scenario generation for risk analysis workloads.
- Support configurable interval grids for non-pricing generation workflows, including sub-hour horizons when inputs are available.

### Definition of Done
- Risk sampler produces reproducible interval paths with configurable seeds.
- State update pipeline is consistent with oracle assumptions and return decomposition modules.
- Tests verify statistical consistency and state evolution across multiple intervals.

### Agent implementation notes
- Keep sampling interfaces streaming-friendly for large portfolio runs.
- Reuse common random plumbing with fine-scale simulator where possible.

### Initiative alignment
- Delivers Initiative 004 success signal for coarse-time risk simulation using sufficient statistics.
- Makes large-scale scenario generation computationally practical.
