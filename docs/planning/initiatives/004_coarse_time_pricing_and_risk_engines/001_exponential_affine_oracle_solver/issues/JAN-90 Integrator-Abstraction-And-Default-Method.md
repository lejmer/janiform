## Integrator Abstraction And Default Method

### Description
Create an integrator abstraction for solving the transform ODE system with a default stable method and configurable solver settings.

### Acceptance Criteria
- Solver supports a default integration method with configurable step controls
- Integrator abstraction allows alternative methods without changing equation code
- Solver settings are validated before runtime execution
- Integration result includes status and metadata about solver behavior

### Unit tests
- Tests verifying alternative integrator selection wiring
- Invalid solver settings and expected failures
- Default integrator execution on deterministic fixtures

### Notes/Constraints
- Keep integrator interfaces minimal and explicit
- Avoid embedding equation logic inside integrator implementations
