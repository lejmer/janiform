## ISSUE-008: Interval Sensitivity Benchmark Matrix

### Description
Add benchmark and regression matrix across multiple interval grids to track sensitivity of accuracy and performance when `Delta` changes.

### Acceptance Criteria
- Matrix includes hourly default and at least one sub-hour interval profile when compatible inputs exist
- Accuracy and runtime metrics are reported per interval profile
- Interval-profile baselines are versioned and comparable
- Results feed threshold policy and benchmark reporting pipelines

### Unit tests
- Interval-profile matrix configuration validation and interval-specific report generation
- Per-interval benchmark run wiring and output tagging

### Notes/Constraints
- Keep interval-profile scope bounded for CI feasibility
- Reuse shared factor-availability validation from risk and pricing projects
