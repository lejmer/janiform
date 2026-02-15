## Project 001: NIG Levy Driver Module

Implement reusable NIG exponent and sampling utilities for buy and sell trading-time return components.

### Context and requirements
- Implement NIG exponent and sampling utilities for buy and sell trading-time return components.
- Support shared core parameters with one-parameter sell-side skew shift.
- Expose interfaces required by both characteristic-function pricing and path simulation.

### Definition of Done
- Core NIG module validates parameter constraints and computes stable exponents for complex arguments.
- Buy and sell driver configuration follows shared-plus-shift design with explicit sign conventions.
- Unit tests cover parameter domains, exponent consistency, and numerical sanity checks.

### Agent implementation notes
- Keep NIG math in one canonical module to avoid drift across pricing and simulation.
- Include numerical guardrails for near-boundary parameter settings.

### Initiative alignment
- Delivers Initiative 003 signal that NIG logic is implemented once and reused.
- Establishes the trading-time heavy-tail and skew foundation of the return model.
