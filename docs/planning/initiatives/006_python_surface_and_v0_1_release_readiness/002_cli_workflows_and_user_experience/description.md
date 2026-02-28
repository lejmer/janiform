## Project 002: CLI Workflows And User Experience

Deliver scriptable CLI workflows with consistent arguments, structured outputs, and predictable error behavior.

### Context and requirements
- Implement CLI commands for core workflows with consistent argument patterns and outputs.
- Support common use cases for simulation, calibration orchestration, and pricing tasks.
- Ensure CLI behavior is scriptable and suitable for automation.
- Encode `--execution-mode {disk,memory}` for calibration workflows with CLI default `disk` and explicit override support.
- Keep CLI contracts aligned with corrected known-surprise news semantics and open-jump versus closed-gap terminology where surfaced.

### Definition of Done
- CLI exposes primary commands with help text, structured output options, and error codes.
- Calibration CLI contracts document default `execution_mode=disk` and explicit override to `memory`.
- Smoke tests validate CLI execution for representative workflows, including mode defaults and overrides.
- Documentation includes command reference and practical examples.

### Agent implementation notes
- Keep command naming aligned with Python API terminology to reduce cognitive load.
- Provide machine-readable output mode for pipeline integration.

### Initiative alignment
- Delivers Initiative 006 signal that CLI workflows are available and usable in v0.1.
- Expands accessibility beyond direct Python integration.
