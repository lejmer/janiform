## Initiative 006: Python Surface And v0.1 Release Readiness

Ship a production-ready Python and CLI surface with stable contracts, hardened bindings, and release-quality process gates.

### Desired outcome
- Janiform ships a usable Python package and CLI backed by the Rust core, with clear boundaries between `core` and `pybind`.
- The v0.1 developer and user experience is documented well enough for internal teams and potential open-source adopters.
- The repository has reliable quality gates for packaging, typing, tests, and build verification.

### Success signals
- Python API and CLI expose simulation, calibration, and pricing workflows with stable user-facing contracts.
- PyO3 boundary code is thin and consistent, with explicit conversion and error mapping.
- CI-level checks pass for core build, Python importability, and repository verification commands.
- Documentation covers install, quickstart, model concepts, and examples for the primary v0.1 workflows.
