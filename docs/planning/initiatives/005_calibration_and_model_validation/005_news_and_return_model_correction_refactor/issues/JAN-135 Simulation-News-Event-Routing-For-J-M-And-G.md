## Simulation News Event Routing For J M And G

### Description
Refactor simulation-side news event handling so each news arrival can drive activity coupling and return routing through explicit open-window and closed-window channels.

### Implementation details
- Refactor `crates/core/src/simulation.rs` exogenous news event payloads so each news arrival carries both activity-coupling mark inputs and return-jump mark inputs.
- Extend interval scheduling to support known scheduled news arrivals plus surprise-news arrivals in one deterministic ordering pipeline.
- Resolve market-state routing per news arrival and emit explicit open-window jump application versus closed-window gap carry behavior.
- Update transition-trace contracts so news transition diagnostics include all routed components needed for debugging and regression checks.
- Keep deterministic ordering policy intact while extending tie-break behavior for richer news event payloads when needed.
- Ensure the simulator surface remains coherent with updated return decomposition and risk diagnostics contracts.

### Acceptance Criteria
- Simulation news events carry required mark data for activity coupling and return-jump realization.
- Open-window news arrivals produce direct return-jump effects in the open-jump channel.
- Closed-window news arrivals produce reopening-gap effects in the closed-gap channel.
- Event ordering and debug-trace outputs preserve deterministic behavior under the new routing semantics.

### Unit tests
- Event-path regression tests covering open-window and closed-window arrivals.
- Deterministic ordering and trace-output validation under mixed endogenous and news events.
- Guardrail tests for invalid generated marks and routing-state violations.

### Notes/Constraints
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
- Keep activity-coupling updates and return-jump routing explicit in simulation outputs.
