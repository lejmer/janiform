## Project 004: Open Closed Gap Behavior Validation

Validate end-to-end open and closed gap behavior and prevent leakage of direct gap moves into open windows.

### Context and requirements
- Build targeted validation scenarios for market open and closed shock behavior.
- Confirm that explicit gap jumps are emitted only during closed windows.
- Validate that open-window news changes return behavior via activity and not direct gap increments.
- Own the end-to-end leakage regression suite and CI guardrails for open and closed behavior, without duplicating mark-distribution unit tests from Project 002.

### Definition of Done
- Scenario suite covers overnight and weekend baseline closed-window news cases plus intraday injected-news cases with expected outcomes.
- Regression tests fail when gap components leak into open windows.
- Validation summary artifacts make behavior differences easy to inspect.

### Agent implementation notes
- Reuse calendar fixtures from Initiative 001 to avoid duplicated setup logic.
- Keep scenario definitions data-driven for extension to new exchanges.

### Initiative alignment
- Delivers Initiative 003 success signal for open versus closed coupling semantics.
- Increases confidence in realistic gap modeling for risk and pricing use cases.
