## News Process Domain Refactor For Known And Surprise Components

### Description
Refactor the core news-process domain model so known scheduled news and surprise news are first-class components with explicit shared and component-level configuration.

### Implementation details
- Refactor `crates/core/src/news.rs` contracts so the top-level process is componentized into known and surprise arrivals instead of one monolithic intensity block.
- Replace the current closed-window-baseline-only intensity contract with component-aware contracts that support both scheduled known-news timing and surprise-news intensity.
- Extend shared-news state and dispatch contracts from `J_t` plus `G_t` to `J_t` plus `M_t` plus `G_t`, with explicit per-event increments recorded for each channel.
- Update dispatch record payloads to expose market-state-resolved routing for open-window direct return jumps and closed-window gap jumps.
- Update policy identifiers and schema versions tied to old closed-window-only assumptions.
- Keep mark coupling explicit so activity coupling and return-jump routing use one coherent per-event mark contract.

### Acceptance Criteria
- News-process configuration supports explicit known and surprise arrival components.
- Domain model defines explicit routing targets for activity coupling and return-jump effects.
- Shared and component-specific mark-law parameters are represented without ambiguity.
- Configuration validation and error taxonomy are updated for the new model structure.

### Unit tests
- Valid and invalid configuration construction for known and surprise components.
- Deterministic serialization and deserialization of the new news-process contract.
- Dispatch tests for open-window `M` increments, closed-window `G` increments, and always-on `J` increments.
- Validation errors for missing required component fields and invalid parameter domains.

### Notes/Constraints
- Pre-v0.1 policy applies: breaking contract and schema changes are allowed when they improve correctness and clarity.
- Keep domain contracts explicit and avoid implicit fallback behavior.
