## Project 002: CLI Workflows And User Experience

Deliver scriptable CLI workflows with consistent arguments, structured outputs, and predictable error behavior, while reshaping calibration commands around user tasks instead of internal orchestration payloads.

### Context and requirements
- Implement CLI commands for core workflows with consistent argument patterns and outputs.
- Support common use cases for simulation, calibration orchestration, and pricing tasks.
- Ensure CLI behavior is scriptable and suitable for automation.
- Encode `--execution-mode {disk,memory}` for calibration workflows with CLI default `disk` and explicit override support.
- Keep CLI contracts aligned with corrected known-surprise news semantics and open-jump versus closed-gap terminology where surfaced.
- Treat the staged calibration execution JSON contract as an advanced or internal mode rather than the default workflow for end users.
- Align the default calibration UX with how market users think about the task: provide files, session context, and known-news inputs, then run calibration.

### Definition of Done
- CLI exposes primary commands with help text, structured output options, and error codes.
- Calibration CLI contracts document default `execution_mode=disk` and explicit override to `memory`.
- Calibration CLI offers a market-data-first path that accepts user inputs such as `csv` or `parquet` files plus session and known-news configuration.
- Smoke tests validate CLI execution for representative workflows, including mode defaults and overrides.
- Documentation includes command reference and practical examples.

### Agent implementation notes
- Keep command naming aligned with Python API terminology to reduce cognitive load.
- Provide machine-readable output mode for pipeline integration.
- Prefer progressive disclosure: simple task-oriented flags first, internal contract files only for advanced use and debugging.

### Initiative alignment
- Delivers Initiative 006 signal that CLI workflows are available and usable in v0.1.
- Expands accessibility beyond direct Python integration and reduces the gap between engineering-facing contracts and market-user expectations.
