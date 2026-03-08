## Project 003: Pybind Boundary Hardening

Harden the PyO3 boundary with strict conversion, exception mapping, and panic containment rules.

### Context and requirements
- Harden Rust-to-Python boundary conversion and error mapping with clear invariants.
- Keep PyO3 layer thin and free of core business logic.
- Ensure panic-safe behavior and consistent exception surfaces.
- Support the higher-level market-data calibration workflows planned above the boundary without pushing file-ingestion or user-workflow policy into `pybind`.

### Definition of Done
- Boundary code converts domain errors into Python exceptions with actionable messages.
- PyO3 interface layer only orchestrates conversion and dispatch to core modules.
- Tests cover conversion edge cases and failure-path exception mapping.
- Boundary responsibilities remain clear even as Python-side workflow orchestration becomes more user-facing and file-driven.

### Agent implementation notes
- Avoid leaking internal Rust structs directly through public Python interfaces.
- Document boundary invariants and error taxonomy.
- Pre-v0.1 contract evolution is expected. Breaking changes are allowed when they improve clarity and correctness of boundary contracts.
- Keep raw file loading, schema inference, and market-data normalization above the PyO3 layer unless a Rust-side implementation provides clear reuse value across surfaces.

### Initiative alignment
- Satisfies Initiative 006 success signal for thin and consistent PyO3 boundary code.
- Improves reliability of the packaged Python experience while preserving a clean separation between user-facing workflow orchestration and Rust execution.
