## Scheduled Event Overlay Framework

### Description
Add support for deterministic scheduled-event overlays `f_special(t)` that adjust baseline seasonality when configured. Overlays should be optional, composable, and transparent in output artifacts.

### Acceptance Criteria
- Overlay definitions can be loaded from configuration and applied to the composed seasonality signal.
- Overlay precedence and merge behavior are deterministic and documented.
- The pipeline can run with and without overlays using the same API.
- Output artifacts include overlay provenance metadata.

### Unit tests
- Tests for single and multiple overlay windows.
- Tests for precedence behavior when overlay windows overlap.
- Tests confirming no-op behavior when overlays are disabled.

### Notes/Constraints
- Keep overlay logic deterministic and side-effect free.
- Avoid embedding exchange-specific scheduled events directly in code.
