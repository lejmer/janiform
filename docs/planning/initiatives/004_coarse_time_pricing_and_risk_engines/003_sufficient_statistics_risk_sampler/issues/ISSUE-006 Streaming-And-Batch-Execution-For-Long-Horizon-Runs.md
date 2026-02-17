## ISSUE-006: Streaming And Batch Execution For Long Horizon Runs

### Description
Implement streaming-friendly and batch execution modes for long-horizon risk runs so large scenarios can be generated with bounded memory usage.

### Acceptance Criteria
- Sampler supports streaming emission of interval outputs
- Batch mode supports multi-path runs with deterministic seed partitioning
- Memory profile remains bounded for long-horizon execution
- Execution modes share core sampling logic to avoid divergence

### Unit tests
- Consistent outputs between streaming and batch modes
- Streaming output ordering and completeness and batch deterministic behavior under fixed seed settings

### Notes/Constraints
- Keep output chunking policy explicit and configurable
- Avoid duplicated random-stream logic between execution modes
