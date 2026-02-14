## Project 002: Seasonality Estimation Pipeline

### Context and requirements
- Implement nonparametric estimation for intraday, day-of-week, and day-of-year seasonality from event timestamps.
- Produce a normalized `s(t)` that preserves identifiability against baseline Hawkes intensity.
- Support scheduled-event overlays that can adjust deterministic seasonality when configured.

### Definition of Done
- Estimation pipeline produces `s(t)` with anchored mean level over the estimation window.
- Pipeline outputs diagnostics for each seasonal component and fit quality.
- Tests verify normalization, reproducibility, and handling of sparse calendar segments.

### Agent implementation notes
- Keep estimator interfaces model-agnostic so calibration workflows can swap methods if needed.
- Ensure output format is stable and machine-readable for later calibration stages.

### Initiative alignment
- Delivers Initiative 001 signal for seasonality support across intraday, weekly, and yearly structure.
- Reduces confounding between deterministic calendar effects and latent excitation dynamics.
