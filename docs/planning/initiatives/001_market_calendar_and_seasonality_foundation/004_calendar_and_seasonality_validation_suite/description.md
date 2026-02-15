## Project 004: Calendar And Seasonality Validation Suite

Provide shared fixtures and regression checks that lock down calendar and seasonality correctness over time.

### Context and requirements
- Create a dedicated validation and fixture suite for calendar and seasonality behavior.
- Include regression fixtures for historical holiday calendars and known session anomalies.
- Define acceptance thresholds for deterministic outputs and expected factor ranges.

### Definition of Done
- Shared fixtures exist for sessions, holidays, and event windows across Python and Rust tests.
- Regression tests detect unintended changes in `open` or `closed` classification and seasonal factors.
- Validation documentation defines expected behavior and fallback policy for unknown dates.

### Agent implementation notes
- Prefer reusable fixtures over per-test local helper data.
- Keep fixtures versioned to support stable calibration reruns.

### Initiative alignment
- Directly fulfills Initiative 001 signal on deterministic test coverage for normal sessions and closure scenarios.
- Supports reliability goals for downstream Hawkes and pricing components.
