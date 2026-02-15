## ISSUE-001: Vanilla Option Pricing Contract And Input Model

### Description
Define the public input and output contract for vanilla option pricing, including model state, market inputs, and requested payoff configuration.

### Acceptance Criteria
- A typed request model exists for strike, maturity interval, rate and dividend inputs, boundary state, and model parameters.
- A typed response model exists for price and key diagnostics.
- Input validation rejects malformed or incomplete pricing requests with actionable errors.
- Contract is documented for both Rust and Python-facing usage.

### Unit tests
- Tests for valid request and response construction.
- Tests for invalid input handling and structured error output.
- Serialization tests for request and response payloads.

### Notes/Constraints
- Keep contract focused on baseline vanilla payoffs for v0.1.
- Avoid embedding engine-specific internals in public request models.
