## ISSUE-006: Open Closed Gating Semantics For Gap Application

### Description
Implement and document market-state gating semantics where news events always affect activity through `J_t` but explicit gap increments apply only during closed windows.

### Acceptance Criteria
- Open-window news events update activity coupling path without direct `G_t` contribution.
- Closed-window news events update both `J_t` and `G_t` according to mark construction rules.
- Gating semantics reference canonical market state service outputs.
- Behavior is deterministic at session boundaries and documented.

### Unit tests
- Tests for open-window events confirming no direct gap contribution.
- Tests for closed-window events confirming both activity and gap effects.
- Boundary tests for events at session-open and session-close edges.

### Notes/Constraints
- Reuse market-state resolver from calendar foundation project.
- Keep gating policy centralized to prevent drift across engines.
