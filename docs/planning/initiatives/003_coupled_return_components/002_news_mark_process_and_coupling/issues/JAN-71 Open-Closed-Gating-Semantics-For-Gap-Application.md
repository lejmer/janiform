## Open Closed Gating Semantics For Gap Application

### Description
Implement and document market-state gating semantics where explicit gap increments apply only during closed windows, and any open-window injected news scenarios affect returns through activity coupling only.

### Acceptance Criteria
- Baseline intensity configuration is consistent with closed-window news behavior for gap generation
- Injected open-window news scenarios update activity coupling path without direct `G_t` contribution
- Closed-window news events update both `J_t` and `G_t` according to mark construction rules
- Gating semantics reference canonical market state service outputs
- Behavior is deterministic at session boundaries and documented

### Unit tests
- Baseline closed-window configuration behavior, injected open-window events confirming no direct gap contribution, and closed-window events confirming both activity and gap effects
- Boundary tests for events at session-open and session-close edges

### Notes/Constraints
- See `docs/design/janiform_design_brainstorm_notes.md` and `docs/design/janiform_compact_model_spec.tex` for the model spec itself.
- Reuse market-state resolver from calendar foundation project
- Keep gating policy centralized to prevent drift across engines
