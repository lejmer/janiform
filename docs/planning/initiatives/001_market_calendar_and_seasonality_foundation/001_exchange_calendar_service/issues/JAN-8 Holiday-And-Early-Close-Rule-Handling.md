## Holiday And Early Close Rule Handling

### Description
Add support for exchange holidays and early-close sessions so the open and closed resolver correctly reflects non-regular schedules.

### Acceptance Criteria
- Resolver supports full-day market holidays where all timestamps resolve as `closed`.
- Resolver supports early-close sessions with truncated trading windows.
- Rule precedence between regular sessions, holidays, and early closes is deterministic and documented.
- Exchange artifacts can encode both holiday and early-close schedules.

### Unit tests
- Holiday test dates that resolve as closed during otherwise regular weekdays.
- Early-close test dates showing transition from open to closed at truncated session end.
- Conflict tests validating deterministic precedence behavior.

### Notes/Constraints
- Use shared date utilities to avoid duplicated calendar math.
- Keep rule precedence centralized in one resolver path.
