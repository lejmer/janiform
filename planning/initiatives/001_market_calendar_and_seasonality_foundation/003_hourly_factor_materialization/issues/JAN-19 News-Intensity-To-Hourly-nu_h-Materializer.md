## News Intensity To Hourly nu_h Materializer

### Description
Implement hourly materialization for news intensity `nu_news(t)` into `nu_h` values aligned with the same interval grid as `s_h`.

### Acceptance Criteria
- Materializer outputs `nu_h` for every hourly interval in the requested range
- `nu_h` respects closed-window baseline intensity policy from market-state rules, with explicit override hooks for injected open-window scenario studies
- Materialization method and calendar dependencies are explicit in metadata
- Output aligns exactly with the interval grid used by `s_h`

### Unit tests
- Optional injected open-window scenario overrides that must not imply direct gap emission rules, overnight and weekend intervals where closed-state logic dominates, and Alignment tests confirming one-to-one interval matching with `s_h` outputs
- Closed-window baseline behavior across standard weekdays

### Notes/Constraints
- Do not duplicate calendar state logic in this materializer
- Keep method deterministic for consistent pricing and simulation inputs
