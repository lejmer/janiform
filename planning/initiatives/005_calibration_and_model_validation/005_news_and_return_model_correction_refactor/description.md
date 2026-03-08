## Project 005: News And Return Model Correction Refactor

Refactor core model contracts and calibration pathways so implementation matches the intended known-surprise news architecture with open-window direct jumps and closed-window gap realization.

### Context and requirements
- Align core-domain contracts with the intended model semantics: known scheduled news, surprise news, shared activity coupling, open-window direct return jumps, and closed-window gap jumps.
- Break apart tightly coupled or oversized components where needed so responsibilities are explicit and testable.
- Ensure calibration and validation workflows consume and report the corrected semantics consistently.
- Treat this as intentional pre-v0.1 contract evolution where breaking changes are expected and allowed.

### Definition of Done
- Core model, calibration, and validation planning artifacts define one consistent target behavior for news arrivals, marks, and return-component routing.
- Work is split into independent implementation issues with clear acceptance criteria and test expectations.
- Issue set covers domain contracts, simulation, risk and pricing composition, calibration candidate generation, and regression harness updates.

### Agent implementation notes
- Prioritize canonical shared contracts over compatibility wrappers.
- Keep naming explicit for known, surprise, open-jump, closed-gap, and activity-coupling paths.
- Coordinate schema and policy-identifier updates together to avoid partial semantic migrations.

### Initiative alignment
- Extends Initiative 005 calibration and validation scope so fitted outputs target the intended model architecture.
- Reduces spec-implementation drift risk before broader Python and release-surface hardening in Initiative 006.
