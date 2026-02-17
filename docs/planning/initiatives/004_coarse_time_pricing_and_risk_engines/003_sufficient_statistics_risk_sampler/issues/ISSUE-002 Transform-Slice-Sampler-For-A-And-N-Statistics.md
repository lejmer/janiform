## ISSUE-002: Transform Slice Sampler For A And N Statistics

### Description
Implement sampling of `A_plus`, `A_minus`, `N_plus`, and `N_minus` from sufficient-statistics transform slices without cluster simulation.

### Acceptance Criteria
- Sampler uses oracle transform slices to generate interval statistics
- Sampling is deterministic under fixed seed and identical inputs
- Sampler API is reusable in single-path and batch-path workflows
- Sampling errors include context for interval and parameter state

### Unit tests
- Sampled-output schema and domain sanity checks
- Failure-path tests for invalid transform inputs
- Deterministic seeded tests for repeated interval-slice sampling

### Notes/Constraints
- Reuse shared random plumbing with the fine-scale simulator where possible
- Keep sampling logic separate from return-composition logic
