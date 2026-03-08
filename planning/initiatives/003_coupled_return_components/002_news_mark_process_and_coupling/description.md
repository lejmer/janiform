## Project 002: News Mark Process And Coupling

Implement shared-news mark generation and coupling into activity and closed-window gap channels.

### Context and requirements
- Implement shared exogenous news arrivals with Gamma severity and double-exponential gap marks.
- Couple news severity into Hawkes-state loading and closed-window gap jump generation.
- Preserve deterministic behavior where explicit gap increments are closed-window only, while open-window injected news scenarios affect returns through activity only.

### Definition of Done
- News process supports configurable deterministic intensity and mark distribution parameters.
- Severity marks feed both Hawkes accumulation `J_t` and gap mark scaling without duplicated logic.
- Tests verify mark and coupling primitives plus gating rules, while end-to-end open and closed leakage regression coverage is owned by Project 004 in this Initiative.

### Agent implementation notes
- Keep mark-generation interfaces composable to support future alternative distributions.
- Ensure coupling semantics are explicit in code-level documentation.

### Initiative alignment
- Satisfies Initiative 003 signal for shared news coupling across activity and gap components.
- Supports realistic shock transmission across market states.
