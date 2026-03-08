## Project 004: Hawkes Invariants And Regression Tests

Create invariant and regression suites that detect behavioral drift in Hawkes dynamics and event outputs.

### Context and requirements
- Build an invariant-driven test suite for Hawkes dynamics and event simulation outputs.
- Define regression scenarios that lock down known-good behavior for state transitions and event statistics.
- Ensure failures quickly reveal whether issues arise from constraints, dynamics, or random sampling.
- Include explicit invariants and regressions for non-grid event-time arrivals in tick simulation paths.
- Include explicit regressions for time-scale consistency between aggregated tick outputs and direct coarse-horizon outputs.

### Definition of Done
- Invariant tests validate non-negativity, deterministic boundary updates, and stable evolution under valid parameters.
- Regression scenarios exist for asymmetry behavior and clustered-event signatures.
- Test diagnostics include enough metadata to support rapid root-cause analysis.
- The suite can detect violations of non-grid event-time semantics and scale-consistency expectations.

### Agent implementation notes
- Keep fixtures reusable and minimal to avoid fragile tests.
- Separate deterministic equation tests from statistical-property tests.

### Initiative alignment
- Reinforces Initiative 002 success signals around deterministic state transitions and robust behavioral guarantees.
- Reduces regression risk as pricing and calibration layers are added.
- Provides quality gates for both realistic tick generation and coarse-horizon consistency requirements.
