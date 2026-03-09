## Coarse Simulation Run Contract And Time Indexed Path Artifact

### Description
Define the public run contract and output artifact for coarse-interval simulation as a simulation workflow rather than as a sampler-only risk helper.

The current sufficient-statistics request contract carries interval counts and factor arrays, but it does not anchor runs to an actual start time or emit path records with interval boundary timestamps and simulation-facing metadata. The coarse simulation workflow needs a contract that represents real interval paths, not only abstract sampler inputs.

### Acceptance Criteria
- A typed coarse simulation run request exists with explicit time anchoring and interval semantics.
- The request contract defines how `Delta`, horizon length, factor alignment, and compatible interval grids are validated.
- The coarse simulation output artifact includes interval boundary timestamps, interval identifiers, sampled sufficient statistics, return components, and terminal state information.
- Path metadata and schema fields are machine-readable and suitable for downstream validation and analytics workflows.
- Existing lower-level sampler contracts remain available for internal and validation use, but the new simulation contract is the primary workflow boundary.

### Unit tests
- Valid request construction and serialization tests for coarse simulation run inputs.
- Output artifact validation and serialization tests for time-indexed coarse path records.
- Failure-path tests for malformed interval settings, incompatible factor alignment, and missing required time anchors.

### Notes/Constraints
- Keep the contract explicit about interval timing and avoid implicit timestamp reconstruction from only `horizon_interval_count`.
- Do not silently repurpose the existing risk path contract if simulation-specific fields are needed. Replace or separate the public workflow contract cleanly.
