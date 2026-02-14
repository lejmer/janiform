## ISSUE-007: Composable News Mark Interface And Extension Points

### Description
Create composable interfaces for news-arrival intensity, severity sampler, base-shock sampler, and coupled-mark constructor so alternative distributions can be introduced later without rewiring core flow.

### Acceptance Criteria
- Interface boundaries are explicit for arrival generation, mark sampling, and coupling stages.
- Default v0.1 implementation wires Gamma plus double-exponential components.
- Extension points allow alternative mark families behind stable interfaces.
- Interface documentation includes lifecycle and ownership expectations for generated marks.

### Unit tests
- Tests for default interface implementation wiring.
- Mock-based tests demonstrating pluggable alternative mark components.
- Contract tests for interface-level invariants and required outputs.

### Notes/Constraints
- Keep interfaces minimal to avoid accidental over-abstraction.
- Preserve deterministic behavior regardless of implementation choice.
