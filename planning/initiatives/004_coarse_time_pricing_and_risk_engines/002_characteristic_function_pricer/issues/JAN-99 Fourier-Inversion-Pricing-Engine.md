## Fourier Inversion Pricing Engine

### Description
Implement the core Fourier inversion engine that maps characteristic functions to vanilla option prices with configurable integration settings.

### Acceptance Criteria
- Engine supports baseline vanilla payoff pricing via Fourier inversion
- Integration contour and numeric settings are configurable through typed options
- Engine returns price plus integration diagnostics
- Implementation is modular for future payoff extensions

### Unit tests
- Known pricing sanity cases under controlled parameters
- Integration setting validation and boundary handling
- Deterministic output on reference fixtures

### Notes/Constraints
- Keep payoff-specific logic separate from CF assembly
- Avoid hardcoded numeric constants without documentation
