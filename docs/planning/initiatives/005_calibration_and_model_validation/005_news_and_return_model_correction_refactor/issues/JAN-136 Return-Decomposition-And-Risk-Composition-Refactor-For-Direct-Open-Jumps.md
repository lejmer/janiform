## Return Decomposition And Risk Composition Refactor For Direct Open Jumps

### Description
Refactor return-decomposition and risk-return composition contracts so open-window direct news jumps and closed-window gap jumps are represented as separate components.

### Implementation details
- Refactor `crates/core/src/return_decomposition.rs` to include explicit open-jump and closed-gap components in the decomposition contract.
- Update edge-mode semantics so component toggles align with the new decomposition shape and naming.
- Refactor `crates/core/src/risk.rs` interval-return generation contracts to accept and compose open-jump and closed-gap inputs explicitly.
- Replace gap-only gating policy fields in risk provenance and diagnostics with component-aware routing policy fields.
- Update risk path record and diagnostics schemas to expose cumulative open-jump and cumulative closed-gap summaries.
- Align transform-slice and risk-run request surfaces with updated decomposition and component naming conventions.

### Acceptance Criteria
- Return-decomposition contracts include explicit fields for trading-time, open-jump, and closed-gap components.
- Risk interval-return generation composes all components with explicit market-state routing policy.
- Risk diagnostics and provenance outputs include component-level cumulative summaries for open-jump and closed-gap paths.
- Edge-mode behavior and component-disable semantics are updated to the new decomposition model.

### Unit tests
- Return-generation correctness for open-window, closed-window, and mixed scenarios.
- Serialization and deserialization coverage for updated decomposition and risk-path record contracts.
- Diagnostics aggregation and provenance-validation tests for component-level totals.

### Notes/Constraints
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
- Keep decomposition naming consistent across simulation, risk, and pricing modules.
