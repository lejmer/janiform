## Project 004: Open Closed Gap Behavior Validation

### Context and requirements
- Build targeted validation scenarios for market open and closed shock behavior.
- Confirm that explicit gap jumps are emitted only during closed windows.
- Validate that open-window news changes return behavior via activity and not direct gap increments.

### Definition of Done
- Scenario suite covers overnight, weekend, and intraday news cases with expected outcomes.
- Regression tests fail when gap components leak into open windows.
- Validation summary artifacts make behavior differences easy to inspect.

### Agent implementation notes
- Reuse calendar fixtures from Initiative 001 to avoid duplicated setup logic.
- Keep scenario definitions data-driven for extension to new exchanges.

### Initiative alignment
- Delivers Initiative 003 success signal for open versus closed coupling semantics.
- Increases confidence in realistic gap modeling for risk and pricing use cases.
