## Project 002: News Mark Process And Coupling

### Context and requirements
- Implement shared exogenous news arrivals with Gamma severity and double-exponential gap marks.
- Couple news severity into Hawkes-state loading and closed-window gap jump generation.
- Preserve deterministic behavior for open-window news where returns are affected through activity only.

### Definition of Done
- News process supports configurable deterministic intensity and mark distribution parameters.
- Severity marks feed both Hawkes accumulation `J_t` and gap mark scaling without duplicated logic.
- Tests verify coupling behavior for open and closed windows and mark distribution properties.

### Agent implementation notes
- Keep mark-generation interfaces composable to support future alternative distributions.
- Ensure coupling semantics are explicit in code-level documentation.

### Initiative alignment
- Satisfies Initiative 003 signal for shared news coupling across activity and gap components.
- Supports realistic shock transmission across market states.
