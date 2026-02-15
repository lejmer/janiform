## Project 002: Stylized Fact Diagnostics

Compute and compare stylized-fact diagnostics across horizons for empirical and model-generated data.

### Context and requirements
- Implement diagnostics that compare fitted model outputs against empirical stylized facts.
- Cover activity clustering, heavy tails, skew, volatility persistence, and gap behavior.
- Produce machine-readable and human-readable summaries for decision making.

### Definition of Done
- Diagnostic toolkit computes horizon-aware metrics and comparison plots or tables.
- Outputs can be generated for both historical data and simulated data from fitted parameters.
- Tests verify metric correctness and expected behavior on reference fixtures.

### Agent implementation notes
- Keep diagnostics decoupled from calibration optimizer to support standalone analysis.
- Use consistent naming for metrics across reports and artifacts.

### Initiative alignment
- Satisfies Initiative 005 signal for comprehensive stylized-fact validation across horizons.
- Enables evidence-based go or no-go decisions for release candidates.
