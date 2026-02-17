## ISSUE-001: Risk Sampler Run Contract And Path Model

### Description
Define the public run contract and output path model for sufficient-statistics risk simulation, including seeds, interval settings, model state, and emitted path records.

### Acceptance Criteria
- A typed run request model exists for seed, horizon, interval length `Delta`, and required model inputs
- A typed path output model exists for interval-level sufficient statistics and return components
- Input validation rejects malformed or incomplete requests with structured errors
- Contract is documented for both Rust and Python-facing consumers

### Unit tests
- Valid run request and output model construction and serialization tests for run input and output objects
- Invalid request handling and error payload structure

### Notes/Constraints
- Keep model reusable by portfolio-level risk pipelines
- Avoid embedding engine-internal temporary fields in public outputs
