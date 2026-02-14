## ISSUE-003: News Intensity To Hourly nu_h Materializer

### Description
Implement hourly materialization for news intensity `nu_news(t)` into `nu_h` values aligned with the same interval grid as `s_h`.

### Acceptance Criteria
- Materializer outputs `nu_h` for every hourly interval in the requested range.
- `nu_h` respects market `open` and `closed` regime rules from calendar services.
- Materialization method and calendar dependencies are explicit in metadata.
- Output aligns exactly with the interval grid used by `s_h`.

### Unit tests
- Tests for open-window and closed-window behavior across standard weekdays.
- Tests for overnight and weekend intervals where closed-state logic dominates.
- Alignment tests confirming one-to-one interval matching with `s_h` outputs.

### Notes/Constraints
- Do not duplicate calendar state logic in this materializer.
- Keep method deterministic for consistent pricing and simulation inputs.
