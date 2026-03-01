## Project 002: Stylized Fact Diagnostics

Compute and compare stylized-fact diagnostics across horizons for empirical and model-generated data.

### Context and requirements
- Implement diagnostics that compare fitted model outputs against empirical stylized facts while treating empirical estimates as noisy measurements, not fixed ground truth targets.
- Use robust primary metrics for calibration governance: quantile-based shape metrics, tail-risk metrics, and distribution-distance diagnostics.
- Report L-moments and classical variance-skew-kurtosis as secondary diagnostics for interpretability.
- Cover activity clustering, heavy tails, skew, volatility persistence, open-window direct-jump behavior, and closed-window gap behavior.
- Include uncertainty-aware comparison outputs using block-bootstrap confidence intervals and minimum sample-sufficiency gates per horizon.
- Produce machine-readable and human-readable summaries for decision making.

### Definition of Done
- Diagnostic toolkit computes horizon-aware metrics, uncertainty-aware comparisons, and comparison plots or tables.
- Outputs can be generated for both historical data and simulated data from fitted parameters.
- Outputs encode sample coverage and explicit `insufficient_data` status when horizon data is too sparse for reliable shape or tail diagnostics.
- Tests verify metric correctness and expected behavior on reference fixtures.

### Agent implementation notes
- Keep diagnostics decoupled from calibration optimizer to support standalone analysis.
- Use consistent naming for metrics across reports and artifacts.
- Keep pass-fail logic based on threshold policy with uncertainty context rather than raw point-estimate deltas.

### Initiative alignment
- Satisfies Initiative 005 signal for comprehensive stylized-fact validation across horizons.
- Enables evidence-based go or no-go decisions for release candidates.
